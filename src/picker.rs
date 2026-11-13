//! Navegação de documentos sem carregar ou rasterizar o arquivo selecionado.
use crate::{
    document::{is_image, natural_cmp},
    process,
    terminal::Terminal,
    ui::fit,
};
use anyhow::Result;
use std::{
    fs,
    path::{Path, PathBuf},
};

struct Entry {
    path: PathBuf,
    label: String,
    directory: bool,
    chapter: bool,
}
fn document(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "pdf" | "cbz" | "zip"))
}

pub fn should_pick(path: &Path) -> Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }
    let mut images = false;
    for entry in fs::read_dir(path)? {
        let p = entry?.path();
        if p.is_file() {
            if document(&p) {
                return Ok(true);
            }
            images |= is_image(&p);
        }
    }
    Ok(!images)
}

fn entries(path: &Path) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut images = false;
    for entry in fs::read_dir(path)? {
        let p = entry?.path();
        let name = p.file_name().unwrap_or_default().to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if p.is_file() && is_image(&p) {
            images = true;
        }
        let directory = p.is_dir();
        if directory || (p.is_file() && document(&p)) {
            let label = format!("{name}{}", if directory { "/" } else { "" });
            entries.push(Entry {
                path: p,
                label,
                directory,
                chapter: false,
            });
        }
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| natural_cmp(&a.label, &b.label))
    });
    if images {
        entries.insert(
            0,
            Entry {
                path: path.to_owned(),
                label: "Imagens desta pasta/".into(),
                directory: true,
                chapter: true,
            },
        );
    }
    Ok(entries)
}

pub fn select(term: &mut Terminal, initial: &Path) -> Result<Option<PathBuf>> {
    let mut path = initial.canonicalize()?;
    let mut items = entries(&path)?;
    let mut selected = 0usize;
    let mut top = 0usize;
    let mut search: Option<String> = None;
    let mut filter = String::new();
    let mut pending_g = false;
    let mut redraw = true;
    let mut previous_size = (0, 0);
    let mut message = String::new();
    while !process::stopping() {
        let (cols, rows) = crossterm::terminal::size()?;
        if cols < 20 || rows < 6 {
            anyhow::bail!("Terminal muito pequeno: mínimo de 20 colunas e 6 linhas");
        }
        if (cols, rows) != previous_size {
            redraw = true;
            previous_size = (cols, rows);
        }
        let visible: Vec<_> = items
            .iter()
            .enumerate()
            .filter(|(_, e)| e.label.to_lowercase().contains(&filter.to_lowercase()))
            .map(|(i, _)| i)
            .collect();
        selected = selected.min(visible.len().saturating_sub(1));
        let height = rows as usize - 4;
        if selected < top {
            top = selected;
        }
        if selected >= top + height {
            top = selected + 1 - height;
        }
        if redraw {
            let mut output = format!(
                "\x1b[0m\x1b[2J\x1b[H\x1b[38;2;187;187;187m {}\x1b[0m",
                fit(&path.to_string_lossy(), cols as usize - 2)
            );
            for (line, &index) in visible.iter().skip(top).take(height).enumerate() {
                let label = fit(&items[index].label, cols as usize - 4);
                output.push_str(&format!(
                    "\x1b[{};2H{} {}{}\x1b[0m",
                    line + 3,
                    if top + line == selected {
                        "\x1b[38;2;238;238;238;48;2;51;51;51m›"
                    } else {
                        " "
                    },
                    label,
                    " ".repeat(
                        (cols as usize - 4)
                            .saturating_sub(unicode_width::UnicodeWidthStr::width(label.as_str()))
                    )
                ));
            }
            if visible.is_empty() {
                output.push_str("\x1b[3;3HNenhum documento encontrado");
            }
            let bottom = if let Some(query) = &search {
                format!("/{query}")
            } else if !message.is_empty() {
                message.clone()
            } else {
                "j/k escolher · Enter/l abrir · h voltar · / filtrar · q sair".into()
            };
            output.push_str(&format!(
                "\x1b[{};1H\x1b[0;38;2;187;187;187m {}\x1b[0m\x1b[?25l",
                rows,
                fit(&bottom, cols as usize - 2)
            ));
            term.write(output.as_bytes())?;
            redraw = false;
        }
        let Some(key) = term.key()? else {
            continue;
        };
        redraw = true;
        if let Some(query) = search.as_mut() {
            match key {
                27 => {
                    search = None;
                    filter.clear();
                }
                13 | 10 => {
                    search = None;
                }
                8 | 127 => {
                    query.pop();
                    filter = query.clone();
                }
                21 => {
                    query.clear();
                    filter.clear();
                }
                3 => return Ok(None),
                32..=126 if query.len() < 128 => {
                    query.push(key as char);
                    filter = query.clone();
                }
                _ => {}
            }
            selected = 0;
            top = 0;
            continue;
        }
        let was_g = pending_g;
        pending_g = false;
        match key {
            b'q' | 3 | 27 => return Ok(None),
            b'j' => selected = (selected + 1).min(visible.len().saturating_sub(1)),
            b'k' => selected = selected.saturating_sub(1),
            4 | 6 => {
                selected = selected
                    .saturating_add(if key == 4 { height / 2 } else { height })
                    .min(visible.len().saturating_sub(1))
            }
            21 | 2 => {
                selected = selected.saturating_sub(if key == 21 { height / 2 } else { height })
            }
            b'g' if was_g => selected = 0,
            b'g' => pending_g = true,
            b'G' => selected = visible.len().saturating_sub(1),
            b'/' => {
                search = Some(String::new());
                filter.clear();
            }
            b'h' => {
                if let Some(parent) = path.parent() {
                    match entries(parent) {
                        Ok(next) => {
                            path = parent.to_owned();
                            items = next;
                            selected = 0;
                            top = 0;
                            filter.clear();
                            message.clear();
                        }
                        Err(error) => message = error.to_string(),
                    }
                }
            }
            b'l' | 13 | 10 => {
                if let Some(&index) = visible.get(selected) {
                    let entry = &items[index];
                    if !entry.directory || entry.chapter || !should_pick(&entry.path)? {
                        return Ok(Some(entry.path.clone()));
                    }
                    match entries(&entry.path) {
                        Ok(next) => {
                            path = entry.path.clone();
                            items = next;
                            selected = 0;
                            top = 0;
                            filter.clear();
                            message.clear();
                        }
                        Err(error) => message = error.to_string(),
                    }
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directories_choose_documents_naturally_and_keep_image_chapters() {
        let temp = tempfile::tempdir().unwrap();
        assert!(should_pick(temp.path()).unwrap());
        fs::write(temp.path().join("1.png"), []).unwrap();
        assert!(!should_pick(temp.path()).unwrap());
        for name in ["10.cbz", "2.PDF", "1.zip", "ignore.txt"] {
            fs::write(temp.path().join(name), []).unwrap();
        }
        fs::create_dir(temp.path().join("chapters")).unwrap();
        assert!(should_pick(temp.path()).unwrap());
        let listed = entries(temp.path()).unwrap();
        assert_eq!(
            listed.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
            [
                "Imagens desta pasta/",
                "chapters/",
                "1.zip",
                "2.PDF",
                "10.cbz"
            ]
        );
        assert!(listed[0].chapter);
    }
}
