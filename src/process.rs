//! Subprocessos com saída em arquivos temporários: sem deadlock de pipes.
use anyhow::{bail, Context, Result};
use std::{
    cell::RefCell,
    fs,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

pub static STOP: OnceLock<Arc<AtomicBool>> = OnceLock::new();
thread_local! {
    static CANCEL: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

pub fn set_cancel(cancel: Arc<AtomicBool>) {
    CANCEL.with(|slot| *slot.borrow_mut() = Some(cancel));
}

pub fn cancelled() -> bool {
    stopping()
        || CANCEL.with(|slot| {
            slot.borrow()
                .as_ref()
                .is_some_and(|c| c.load(Ordering::Relaxed))
        })
}

pub fn check_cancelled() -> Result<()> {
    if cancelled() {
        bail!("Renderização cancelada");
    }
    Ok(())
}

pub fn stopping() -> bool {
    STOP.get().is_some_and(|s| s.load(Ordering::Relaxed))
}

pub fn run(command: &mut Command) -> Result<Vec<u8>> {
    check_cancelled()?;
    let output = tempfile::NamedTempFile::new()?;
    let error = tempfile::NamedTempFile::new()?;
    let mut child = command
        .stdout(Stdio::from(output.reopen()?))
        .stderr(Stdio::from(error.reopen()?))
        .stdin(Stdio::null())
        .spawn()
        .with_context(|| {
            format!(
                "Não foi possível executar {:?}; consulte as dependências no README",
                command.get_program()
            )
        })?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!(
                    "{}: {}",
                    command.get_program().to_string_lossy(),
                    fs::read_to_string(error.path())?.trim()
                );
            }
            return Ok(fs::read(output.path())?);
        }
        if cancelled() || start.elapsed() > Duration::from_secs(90) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Operação interrompida ou excedeu o limite de 90 segundos");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
