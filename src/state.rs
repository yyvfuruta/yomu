use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::Write,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

pub struct History {
    file: Option<PathBuf>,
}
impl History {
    pub fn new(path: &Path) -> Self {
        let root = env::var_os("XDG_STATE_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")));
        let key = format!("{:x}", Sha256::digest(path.as_os_str().as_bytes()));
        Self {
            file: root.map(|r| r.join("yomu").join(format!("{key}.page"))),
        }
    }
    pub fn read(&self, count: usize) -> usize {
        self.file
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0)
            .min(count.saturating_sub(1))
    }
    pub fn write(&self, page: usize) -> Result<()> {
        let path = self
            .file
            .as_ref()
            .context("HOME e XDG_STATE_HOME não definidos")?;
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        writeln!(file, "{page}")?;
        file.persist(path)?;
        Ok(())
    }
    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { file: Some(path) }
    }
}
