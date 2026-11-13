use anyhow::{bail, Result};
use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    collections::VecDeque,
    io::{self, Write},
    time::{Duration, Instant},
};

pub struct Terminal {
    pub pending: VecDeque<u8>,
}
#[derive(Clone, Copy)]
pub struct Geometry {
    pub rows: u16,
    pub cols: u16,
    pub cw: u32,
    pub ch: u32,
}
impl Geometry {
    pub fn page_width(self) -> u32 {
        (self.cols.saturating_sub(2) as u32 * self.cw).min(8192)
    }
    pub fn page_height(self) -> u32 {
        (self.rows.saturating_sub(4) as u32 * self.ch).min(8190) / 6 * 6
    }
}

impl Terminal {
    pub fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        let terminal = Self {
            pending: VecDeque::new(),
        };
        execute!(io::stdout(), EnterAlternateScreen, Hide)?;
        terminal.write(b"\x1b[?80h")?;
        Ok(terminal)
    }
    pub fn write(&self, data: &[u8]) -> Result<()> {
        let mut out = io::stdout().lock();
        out.write_all(data)?;
        out.flush()?;
        Ok(())
    }
    fn query(&mut self, request: &[u8], kind: u8) -> Result<Option<Vec<u32>>> {
        self.write(request)?;
        let start = Instant::now();
        let mut response = Vec::new();
        while start.elapsed() < Duration::from_millis(350) {
            if let Some(b) = read_byte(Duration::from_millis(10))? {
                response.push(b);
                if let Some(begin) = response.windows(2).position(|s| s == b"\x1b[") {
                    if b == kind {
                        let text =
                            String::from_utf8_lossy(&response[begin + 2..response.len() - 1]);
                        if let Ok(values) = text
                            .trim_start_matches('?')
                            .split(';')
                            .map(str::parse::<u32>)
                            .collect::<std::result::Result<Vec<_>, _>>()
                        {
                            self.pending.extend(&response[..begin]);
                            return Ok(Some(values));
                        }
                    }
                }
            }
        }
        self.pending
            .extend(response.into_iter().take_while(|&b| b != 27));
        Ok(None)
    }
    pub fn supports_sixel(&mut self) -> Result<bool> {
        Ok(self.query(b"\x1b[c", b'c')?.is_some_and(|v| v.contains(&4)))
    }
    pub fn geometry(&mut self, fallback: (u32, u32)) -> Result<Geometry> {
        let (cols, rows) = terminal::size()?;
        if rows < 6 || cols < 20 {
            bail!("Terminal muito pequeno: mínimo de 20 colunas e 6 linhas");
        }
        let (mut cw, mut ch) = fallback;
        if let Ok(size) = terminal::window_size() {
            if size.width > 0 && size.height > 0 {
                cw = (size.width as u32 / cols as u32).max(1);
                ch = (size.height as u32 / rows as u32).max(1);
                return Ok(Geometry { rows, cols, cw, ch });
            }
        }
        if let Some(v) = self.query(b"\x1b[16t", b't')? {
            if v.len() == 3 && v[0] == 6 && v[1] > 0 && v[2] > 0 {
                ch = v[1].min(256);
                cw = v[2].min(256);
            }
        }
        Ok(Geometry { rows, cols, cw, ch })
    }
    pub fn key(&mut self) -> Result<Option<u8>> {
        self.key_timeout(Duration::from_millis(16))
    }
    pub fn key_timeout(&mut self, timeout: Duration) -> Result<Option<u8>> {
        let key = if let Some(b) = self.pending.pop_front() {
            Some(b)
        } else {
            read_byte(timeout)?
        };
        if key == Some(27) {
            let next = match self.pending.pop_front() {
                Some(byte) => Some(byte),
                None => read_byte(Duration::from_millis(15))?,
            };
            if matches!(next, Some(b'[' | b'O')) {
                // Consume CSI/SS3 cursor and function keys only.
                for _ in 0..128 {
                    let byte = match self.pending.pop_front() {
                        Some(byte) => Some(byte),
                        None => read_byte(Duration::from_millis(15))?,
                    };
                    if byte.is_none_or(|b| (0x40..=0x7e).contains(&b)) {
                        break;
                    }
                }
            } else if let Some(byte) = next {
                // Esc followed quickly by :q is two Vim actions, not a CSI.
                self.pending.push_front(byte);
            }
        }
        Ok(key)
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.write(b"\x1b[0m\x1b[?80h\x1b[2J");
        let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

fn read_byte(timeout: Duration) -> Result<Option<u8>> {
    // Use poll/read directly so device-attribute replies are not consumed by an
    // event parser. Linux termios and window geometry remain managed by Crossterm.
    let mut descriptor = libc::pollfd {
        fd: 0,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one initialized pollfd and bounded millisecond timeout.
    let result = unsafe { libc::poll(&mut descriptor, 1, timeout.as_millis() as i32) };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            return Ok(None);
        }
        return Err(error.into());
    }
    if result == 0 {
        return Ok(None);
    }
    let mut byte = [0];
    // Read from the same descriptor that poll observed. Stdin::read buffers
    // bytes ahead: a complete DA reply can disappear into its internal buffer,
    // leaving poll waiting on an empty descriptor after the very first byte.
    // SAFETY: byte is a writable one-byte buffer; fd 0 is borrowed, not closed.
    let count = unsafe { libc::read(0, byte.as_mut_ptr().cast(), byte.len()) };
    if count < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            return Ok(None);
        }
        return Err(error.into());
    }
    if count == 0 {
        bail!("Entrada do terminal encerrada");
    }
    Ok(Some(byte[0]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn probe_child() {
        let Ok(scenario) = std::env::var("YOMU_TEST_TERMINAL_REPLY") else {
            return;
        };
        let mut terminal = Terminal {
            pending: VecDeque::new(),
        };
        // This helper never entered raw mode or the alternate screen.
        // Avoid running Terminal::drop on the test runner's output.
        let supported = terminal.supports_sixel().unwrap();
        assert_eq!(supported, scenario != "no-sixel");
        if scenario == "escape" {
            for key in [27, b':', b'q', 13] {
                assert_eq!(terminal.key().unwrap(), Some(key));
            }
        } else {
            assert_eq!(terminal.key().unwrap(), Some(b'l'));
            assert_eq!(terminal.key().unwrap(), Some(b'q'));
        }
        std::mem::forget(terminal);
    }

    #[test]
    fn complete_device_reply_and_queued_keys_are_read_without_buffering() {
        for (scenario, reply) in [
            ("sixel", b"\x1b[?62;4;22clq".as_slice()),
            ("no-sixel", b"\x1b[?62;22clq".as_slice()),
            ("escape", b"\x1b[?62;4;22c\x1b:q\r".as_slice()),
        ] {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "terminal::tests::probe_child", "--nocapture"])
                .env("YOMU_TEST_TERMINAL_REPLY", scenario)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            // A terminal normally delivers its whole response in one burst.
            let mut input = child.stdin.take().unwrap();
            input.write_all(reply).unwrap();
            let output = child.wait_with_output().unwrap();
            drop(input);
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
