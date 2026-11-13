//! Exercise the actual binary on a PTY, not just its command parser.
use image::{DynamicImage, ImageFormat};
use std::{
    fs::{self, File},
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[test]
fn q_exits_while_sixel_encoder_is_busy_and_restores_terminal() {
    busy_encoder_quit(b"q");
}

#[test]
fn help_escape_and_command_quit_work_while_encoder_is_busy() {
    busy_encoder_quit(b"?\x1b:q\r");
}

fn busy_encoder_quit(keys: &[u8]) {
    let temp = tempfile::tempdir().unwrap();
    let images = temp.path().join("images");
    fs::create_dir(&images).unwrap();
    DynamicImage::new_rgb8(64, 96)
        .save_with_format(images.join("1.png"), ImageFormat::Png)
        .unwrap();
    let encoder = temp.path().join("img2sixel");
    let marker = temp.path().join("encoding-started");
    let pid = temp.path().join("encoder-pid");
    fs::write(
        &encoder,
        format!(
            "#!/bin/sh\necho $$ > '{}'\n: > '{}'\nexec sleep 30\n",
            pid.display(),
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&encoder, fs::Permissions::from_mode(0o755)).unwrap();
    let mut master = -1;
    let mut slave = -1;
    let size = libc::winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 640,
        ws_ypixel: 384,
    };
    // SAFETY: initialized output pointers, valid window size; no name buffer.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                &size,
            )
        },
        0
    );
    // SAFETY: openpty returned two unique owned descriptors.
    let mut master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    let mut before = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: valid slave descriptor and output storage.
    assert_eq!(
        unsafe { libc::tcgetattr(slave.as_raw_fd(), before.as_mut_ptr()) },
        0
    );
    let before = unsafe { before.assume_init() };
    let path = format!(
        "{}:{}",
        temp.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_yomu"));
    command
        .arg("--force-sixel")
        .arg(&images)
        .env("PATH", path)
        .env("XDG_STATE_HOME", temp.path().join("state"))
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave.try_clone().unwrap()));
    // SAFETY: only async-signal-safe system calls execute between fork and exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    while !marker.exists() && start.elapsed() < Duration::from_secs(5) {
        assert!(
            child.try_wait().unwrap().is_none(),
            "viewer exited before invoking encoder"
        );
        thread::sleep(Duration::from_millis(5));
    }
    if !marker.exists() {
        let _ = child.kill();
        panic!("encoder did not start");
    }
    let start = Instant::now();
    master.write_all(keys).unwrap();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(1) {
            let _ = child.kill();
            let _ = child.wait();
            // Clean up the deliberately slow fake encoder on regression failures.
            if let Ok(pid) = fs::read_to_string(&pid)
                .unwrap_or_default()
                .trim()
                .parse::<i32>()
            {
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
            panic!("q waited for the encoder instead of exiting within one second");
        }
        thread::sleep(Duration::from_millis(5));
    };
    assert!(status.success());
    let encoder_pid: i32 = fs::read_to_string(&pid).unwrap().trim().parse().unwrap();
    // SAFETY: signal zero only checks whether the recorded child still exists.
    assert_eq!(
        unsafe { libc::kill(encoder_pid, 0) },
        -1,
        "encoder was not stopped/reaped"
    );
    let mut after = std::mem::MaybeUninit::<libc::termios>::uninit();
    assert_eq!(
        unsafe { libc::tcgetattr(slave.as_raw_fd(), after.as_mut_ptr()) },
        0
    );
    let after = unsafe { after.assume_init() };
    assert_eq!(
        before.c_lflag, after.c_lflag,
        "terminal raw mode was not restored"
    );
    assert_eq!(before.c_iflag, after.c_iflag);
    assert_eq!(before.c_oflag, after.c_oflag);
    eprintln!(
        "q closed the viewer in {:?} while encoder was busy",
        start.elapsed()
    );
}
