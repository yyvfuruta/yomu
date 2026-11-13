//! Camada mínima sobre a API estável de leitura da libarchive.
//! O handle e as strings emprestadas nunca escapam deste módulo.
use anyhow::{bail, Context, Result};
use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    os::unix::ffi::OsStrExt,
    path::Path,
};

const LIMIT: usize = 128 * 1024 * 1024;
// build.rs discovers and links libarchive through pkg-config.
extern "C" {
    fn archive_read_new() -> *mut c_void;
    fn archive_read_support_format_zip(a: *mut c_void) -> c_int;
    fn archive_read_open_filename(a: *mut c_void, path: *const c_char, block: usize) -> c_int;
    fn archive_read_next_header(a: *mut c_void, entry: *mut *mut c_void) -> c_int;
    fn archive_entry_pathname(entry: *mut c_void) -> *const c_char;
    fn archive_entry_size(entry: *mut c_void) -> i64;
    fn archive_read_data(a: *mut c_void, buffer: *mut c_void, len: usize) -> isize;
    fn archive_error_string(a: *mut c_void) -> *const c_char;
    fn archive_read_free(a: *mut c_void) -> c_int;
}

struct Reader(*mut c_void);
impl Drop for Reader {
    fn drop(&mut self) {
        // SAFETY: this unique handle was allocated by archive_read_new.
        unsafe {
            archive_read_free(self.0);
        }
    }
}
impl Reader {
    fn error(&self) -> String {
        // SAFETY: error strings belong to the live handle; copy before next call.
        unsafe {
            let ptr = archive_error_string(self.0);
            if ptr.is_null() {
                "Erro de leitura ZIP".into()
            } else {
                CStr::from_ptr(ptr).to_string_lossy().into_owned()
            }
        }
    }
    fn open(path: &Path) -> Result<Self> {
        let path = CString::new(path.as_os_str().as_bytes())?;
        // SAFETY: valid C pathname; null handle is checked before any use.
        unsafe {
            let handle = archive_read_new();
            if handle.is_null() {
                bail!("Falha ao criar leitor ZIP");
            }
            let reader = Self(handle);
            if archive_read_support_format_zip(handle) != 0
                || archive_read_open_filename(handle, path.as_ptr(), 10240) != 0
            {
                bail!("ZIP: {}", reader.error());
            }
            Ok(reader)
        }
    }
    fn next(&self) -> Result<Option<(Vec<u8>, i64)>> {
        let mut entry = std::ptr::null_mut();
        // SAFETY: handle is live; entry is written by libarchive, used only on success.
        unsafe {
            let result = archive_read_next_header(self.0, &mut entry);
            if result == 1 {
                return Ok(None);
            } // ARCHIVE_EOF
            if result != 0 {
                bail!("ZIP: {}", self.error());
            }
            let name = archive_entry_pathname(entry);
            if name.is_null() {
                bail!("ZIP sem nome de entrada");
            }
            Ok(Some((
                CStr::from_ptr(name).to_bytes().to_vec(),
                archive_entry_size(entry),
            )))
        }
    }
}

#[derive(Debug)]
pub struct Entry {
    pub index: usize,
    pub name: String,
}

pub fn list(path: &Path) -> Result<Vec<Entry>> {
    let reader = Reader::open(path)?;
    let mut entries = Vec::new();
    let mut index = 0;
    while let Some((name, _)) = reader.next()? {
        let name = String::from_utf8_lossy(&name).into_owned();
        if !name.starts_with("__MACOSX/")
            && crate::document::is_image(Path::new(&name))
            && !name.ends_with('/')
        {
            entries.push(Entry { index, name });
        }
        index += 1;
    }
    entries.sort_by(|a, b| crate::document::natural_cmp(&a.name, &b.name));
    Ok(entries)
}

pub fn read(path: &Path, index: usize) -> Result<Vec<u8>> {
    let reader = Reader::open(path)?;
    for current in 0..=index {
        let (_, size) = reader.next()?.context("Entrada ZIP não encontrada")?;
        if current != index {
            continue;
        }
        if size > LIMIT as i64 {
            bail!("Imagem ZIP excede 128 MiB");
        }
        let mut data = Vec::new();
        let mut buffer = [0u8; 65536];
        loop {
            // SAFETY: writable buffer of exactly the advertised length; handle is live.
            let len =
                unsafe { archive_read_data(reader.0, buffer.as_mut_ptr().cast(), buffer.len()) };
            if len < 0 {
                bail!("ZIP: {}", reader.error());
            }
            if len == 0 {
                return Ok(data);
            }
            if data.len() + len as usize > LIMIT {
                bail!("Imagem ZIP excede 128 MiB");
            }
            data.extend_from_slice(&buffer[..len as usize]);
        }
    }
    bail!("Entrada ZIP não encontrada")
}
