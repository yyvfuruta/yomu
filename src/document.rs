use crate::{archive, process};
use anyhow::{bail, Context, Result};
use image::{DynamicImage, ImageReader, Limits};
use std::{
    cmp::Ordering,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
};

pub fn is_image(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "webp"
        )
    })
}

// Compare numeric runs by length and digits, without overflowing an integer.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (al, bl) = (a.to_lowercase(), b.to_lowercase());
    let (mut x, mut y) = (al.as_bytes(), bl.as_bytes());
    while !x.is_empty() && !y.is_empty() {
        let ordering;
        if x[0].is_ascii_digit() && y[0].is_ascii_digit() {
            let nx = x.iter().take_while(|c| c.is_ascii_digit()).count();
            let ny = y.iter().take_while(|c| c.is_ascii_digit()).count();
            let dx = &x[..nx];
            let dy = &y[..ny];
            let dx = &dx[dx.iter().take_while(|&&c| c == b'0').count()..];
            let dy = &dy[dy.iter().take_while(|&&c| c == b'0').count()..];
            ordering = dx.len().cmp(&dy.len()).then_with(|| dx.cmp(dy));
            x = &x[nx..];
            y = &y[ny..];
        } else {
            ordering = x[0].cmp(&y[0]);
            x = &x[1..];
            y = &y[1..];
        }
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    x.len().cmp(&y.len()).then_with(|| a.cmp(b))
}

enum Source {
    Pdf,
    Directory(Vec<PathBuf>),
    Zip(Vec<archive::Entry>),
}
pub struct Document {
    pub path: PathBuf,
    pub count: usize,
    source: Source,
}

impl Document {
    pub fn open(path: &Path) -> Result<Self> {
        let path = path.canonicalize().context("Documento não encontrado")?;
        let extension = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        let (source, count) = if path.is_dir() {
            let mut entries = Vec::new();
            for entry in fs::read_dir(&path)? {
                let p = entry?.path();
                if p.is_file() && is_image(&p) {
                    entries.push(p);
                }
            }
            entries.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
            let count = entries.len();
            (Source::Directory(entries), count)
        } else if extension == "pdf" {
            let info = process::run(Command::new("pdfinfo").env("LC_ALL", "C").arg(&path))?;
            let info = String::from_utf8_lossy(&info);
            let count = info
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Pages:")
                        .and_then(|n| n.trim().parse::<usize>().ok())
                })
                .context("Não foi possível contar as páginas do PDF")?;
            (Source::Pdf, count)
        } else if extension == "cbz" || extension == "zip" {
            let entries = archive::list(&path)?;
            let count = entries.len();
            (Source::Zip(entries), count)
        } else {
            bail!("Use PDF, CBZ/ZIP ou pasta de imagens PNG, JPEG e WebP");
        };
        if count == 0 {
            bail!("Documento sem páginas ou imagens compatíveis");
        }
        Ok(Self {
            path,
            count,
            source,
        })
    }

    pub fn page(&self, index: usize, width: u32) -> Result<DynamicImage> {
        if index >= self.count {
            bail!("Página fora do documento");
        }
        let bytes = match &self.source {
            Source::Pdf => process::run(
                Command::new("pdftoppm")
                    .args([
                        "-f",
                        &(index + 1).to_string(),
                        "-l",
                        &(index + 1).to_string(),
                        "-singlefile",
                        "-scale-to-x",
                        &width.to_string(),
                        "-scale-to-y",
                        "-1",
                        "-png",
                    ])
                    .arg(&self.path),
            )?,
            Source::Directory(entries) => {
                if fs::metadata(&entries[index])?.len() > 128 * 1024 * 1024 {
                    bail!("Imagem excede 128 MiB");
                }
                fs::read(&entries[index])?
            }
            Source::Zip(entries) => archive::read(&self.path, entries[index].index)?,
        };
        let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
        let mut limits = Limits::default();
        limits.max_alloc = Some(320 * 1024 * 1024);
        limits.max_image_width = Some(32768);
        limits.max_image_height = Some(65536);
        reader.limits(limits);
        let mut image = reader
            .decode()
            .context("Não foi possível decodificar a página")?
            .into_rgba8();
        for pixel in image.pixels_mut() {
            let alpha = pixel[3] as u16;
            for channel in &mut pixel.0[..3] {
                *channel = ((*channel as u16 * alpha + 255 * (255 - alpha)) / 255) as u8;
            }
            pixel[3] = 255;
        }
        Ok(DynamicImage::ImageRgba8(image))
    }
}
