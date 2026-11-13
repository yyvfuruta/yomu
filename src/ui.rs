//! Informações de leitura discretas e comandos sob demanda.
use crate::{
    terminal::Geometry,
    viewer::{Fit, View},
};
use std::{fmt::Write as _, path::Path};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const MUTED: &str = "\x1b[0;38;2;187;187;187m";
const RESET: &str = "\x1b[0m";

pub enum Event {
    View(u8),
    Chrome,
    Redraw,
    Quit,
    Page(usize),
    Fit(Fit),
    Files,
}
#[derive(Default)]
pub struct Ui {
    pub help: bool,
    pub loading: bool,
    pub message: String,
    command: Option<String>,
    help_scroll: usize,
}

pub fn fit(text: &str, columns: usize) -> String {
    let safe: String = text
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    if safe.width() <= columns {
        return safe;
    }
    if columns == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut width = 0;
    for c in safe.chars() {
        let w = c.width().unwrap_or(0);
        if width + w > columns - 1 {
            break;
        }
        result.push(c);
        width += w;
    }
    result.push('…');
    result
}
fn row(out: &mut String, y: u16, columns: usize, text: &str, style: &str) {
    let text = fit(text, columns);
    let _ = write!(
        out,
        "\x1b[{y};1H{style}{text}{}{RESET}",
        " ".repeat(columns.saturating_sub(text.width()))
    );
}
fn put(out: &mut String, y: u16, x: usize, text: &str, style: &str) {
    let _ = write!(out, "\x1b[{y};{}H{style}{text}{RESET}", x + 1);
}

const HELP: &[&str] = &[
    "YOMU / leitura no terminal",
    "",
    "NAVEGAÇÃO",
    "  j / k          descer / subir; continua entre páginas",
    "  h / l          página anterior / próxima",
    "  Ctrl+d / u     meia tela",
    "  Ctrl+f / b     uma tela",
    "  gg / G         primeira / última página",
    "  12G ou :12     ir para a página 12",
    "",
    "LEITURA",
    "  w / a          ajustar à largura / área",
    "  zi / zo / zz   ampliar / reduzir / restaurar zoom",
    "  H / L          rolar horizontalmente",
    "",
    "COMANDOS",
    "  :q             sair",
    "  :fit width     ajustar à largura",
    "  :fit area      ajustar à área",
    "  ? ou :help     esta ajuda",
    "  Ctrl+p / :files  escolher outro documento",
    "",
    "  Esc ou ?       voltar à leitura",
    "  j / k          rolar esta ajuda",
    "  q              sair do Yomu",
];

impl Ui {
    pub fn key(&mut self, key: u8, total: usize) -> Event {
        if let Some(command) = self.command.as_mut() {
            match key {
                27 | 3 => {
                    self.command = None;
                    self.message.clear();
                }
                8 | 127 => {
                    command.pop();
                }
                21 => command.clear(),
                10 | 13 => {
                    let command = self.command.take().unwrap();
                    let command = command.trim();
                    self.message.clear();
                    return match command {
                        "" => Event::Chrome,
                        "q" | "quit" | "q!" => Event::Quit,
                        "help" => {
                            self.help = true;
                            self.help_scroll = 0;
                            Event::Redraw
                        }
                        "fit width" => Event::Fit(Fit::Width),
                        "fit area" => Event::Fit(Fit::Area),
                        "files" => Event::Files,
                        _ => match command.parse::<usize>() {
                            Ok(page) if page > 0 => Event::Page((page - 1).min(total - 1)),
                            _ => {
                                self.message = format!("Comando desconhecido: {command} · :help");
                                Event::Chrome
                            }
                        },
                    };
                }
                32..=126 if command.len() < 128 => command.push(key as char),
                _ => {}
            }
            return Event::Chrome;
        }
        if self.help {
            return match key {
                b'q' | 3 => Event::Quit,
                27 | b'?' => {
                    self.help = false;
                    Event::Redraw
                }
                b'j' | 4 | 6 => {
                    self.help_scroll = (self.help_scroll + 1).min(HELP.len() - 1);
                    Event::Redraw
                }
                b'k' | 21 | 2 => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                    Event::Redraw
                }
                b'g' => {
                    self.help_scroll = 0;
                    Event::Redraw
                }
                b'G' => {
                    self.help_scroll = HELP.len() - 1;
                    Event::Redraw
                }
                _ => Event::Chrome,
            };
        }
        match key {
            16 => Event::Files,
            b':' => {
                self.command = Some(String::new());
                self.message.clear();
                Event::Chrome
            }
            b'?' => {
                self.help = true;
                self.help_scroll = 0;
                Event::Redraw
            }
            _ => {
                self.message.clear();
                Event::View(key)
            }
        }
    }

    pub fn chrome(
        &self,
        path: &Path,
        total: usize,
        view: &View,
        geo: Geometry,
        phase: usize,
        prefix: &str,
    ) -> String {
        let columns = geo.cols as usize;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let mut out = String::new();
        row(&mut out, 1, columns, "", MUTED);
        let position = format!(" {} / {total} ", view.page + 1);
        let right = columns.saturating_sub(position.width());
        let title = fit(&name, right.saturating_sub(2));
        put(&mut out, 1, 1, &title, MUTED);
        put(&mut out, 1, right, &position, MUTED);

        row(&mut out, geo.rows - 1, columns, "", MUTED);
        let fit_mode = if view.fit == Fit::Width {
            "largura"
        } else {
            "área"
        };
        let location = if view.awaiting_layout {
            "…".into()
        } else if view.max_y == 0 {
            "Tudo".into()
        } else if view.y == 0 {
            "Topo".into()
        } else if view.y >= view.max_y {
            "Fim".into()
        } else {
            format!("{}%", view.y as u64 * 100 / view.max_y as u64)
        };
        let info = format!(" {:.0}%  {fit_mode}  {location} ", view.zoom * 100.0);
        let info_x = columns.saturating_sub(info.width());
        put(&mut out, geo.rows - 1, info_x, &fit(&info, columns), MUTED);
        let bottom = if let Some(command) = &self.command {
            format!(":{command}")
        } else if !self.message.is_empty() {
            format!(" {}", self.message)
        } else if self.loading {
            format!(
                " {} Carregando…",
                ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][phase % 10],
            )
        } else if self.help {
            " Esc voltar · j/k rolar · q sair".into()
        } else {
            String::new()
        };
        row(
            &mut out,
            geo.rows,
            columns.saturating_sub(1),
            &bottom,
            MUTED,
        );
        if self.command.is_none() && !prefix.is_empty() {
            let prefix = fit(prefix, columns.saturating_sub(2));
            put(
                &mut out,
                geo.rows,
                columns.saturating_sub(prefix.width() + 1),
                &prefix,
                MUTED,
            );
        }
        if let Some(command) = &self.command {
            let _ = write!(
                out,
                "\x1b[{};{}H\x1b[?25h",
                geo.rows,
                (command.len() + 2).min(columns)
            );
        } else {
            out.push_str("\x1b[?25l");
        }
        out
    }

    pub fn help_body(&self, geo: Geometry) -> String {
        let mut out = String::from("\x1b[0m\x1b[2J\x1b[H");
        let available = geo.rows.saturating_sub(4) as usize;
        let start = self.help_scroll.min(HELP.len().saturating_sub(available));
        let left = (geo.cols as usize).saturating_sub(60) / 2;
        for (index, line) in HELP.iter().skip(start).take(available).enumerate() {
            put(
                &mut out,
                index as u16 + 2,
                left.max(1),
                &fit(line, geo.cols as usize - left.max(1) - 1),
                MUTED,
            );
        }
        out
    }

    pub fn scrollbar(&self, view: &View, geo: Geometry) -> String {
        let mut out = String::new();
        let rows = geo.rows.saturating_sub(4) as u32;
        let full = view.height.saturating_add(view.max_y).max(1);
        let thumb = ((rows as u64 * view.height as u64 / full as u64) as u32).clamp(1, rows.max(1));
        let top = if view.max_y == 0 {
            0
        } else {
            (view.y as u64 * rows.saturating_sub(thumb) as u64 / view.max_y as u64) as u32
        };
        for index in 0..rows {
            put(
                &mut out,
                index as u16 + 2,
                geo.cols as usize - 1,
                if (top..top + thumb).contains(&index) {
                    "┃"
                } else {
                    "│"
                },
                MUTED,
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(ui: &mut Ui, text: &[u8], total: usize) -> Event {
        ui.key(b':', total);
        for &key in text {
            ui.key(key, total);
        }
        ui.key(13, total)
    }

    #[test]
    fn vim_command_mode_edits_cancels_and_submits() {
        let mut ui = Ui::default();
        assert!(matches!(command(&mut ui, b"12", 20), Event::Page(11)));
        assert!(matches!(command(&mut ui, b"999", 20), Event::Page(19)));
        assert!(matches!(
            command(&mut ui, b"fit area", 20),
            Event::Fit(Fit::Area)
        ));
        assert!(matches!(command(&mut ui, b"q", 20), Event::Quit));
        ui.key(b':', 20);
        ui.key(b'q', 20);
        ui.key(127, 20);
        ui.key(27, 20);
        assert!(ui.command.is_none());
        assert!(matches!(ui.key(b'j', 20), Event::View(b'j')));
        command(&mut ui, b"0", 20);
        assert!(ui.message.contains("Comando desconhecido"));
        assert!(matches!(command(&mut ui, b"help", 20), Event::Redraw));
        assert!(ui.help);
        assert!(matches!(ui.key(27, 20), Event::Redraw));
        assert!(!ui.help);
    }

    #[test]
    fn long_and_wide_titles_do_not_hide_page_or_zoom_indicators() {
        let ui = Ui::default();
        let view = View::new(11);
        let geo = Geometry {
            cols: 80,
            rows: 24,
            cw: 8,
            ch: 16,
        };
        let output = ui.chrome(
            Path::new(
                "/manga/漫画-um-nome-de-arquivo-extremamente-longo-que-precisa-ser-abreviado.cbz",
            ),
            24,
            &view,
            geo,
            0,
            "12",
        );
        assert!(!output.contains(" NORMAL "));
        assert!(!output.contains("48;2;"));
        assert!(output.contains(" 12 / 24 "));
        assert!(output.contains("100%  largura"));
        assert!(output.contains("漫画"));
        assert!(output.contains('…'));
        assert!(!fit("file\x1b[2J", 30).contains('\x1b'));
        assert_eq!(fit("漫画", 3), "漫…");
    }

    #[test]
    fn loading_and_commands_have_immediate_feedback_without_an_image() {
        let mut ui = Ui {
            loading: true,
            ..Ui::default()
        };
        let view = View::new(0);
        let geo = Geometry {
            cols: 80,
            rows: 24,
            cw: 8,
            ch: 16,
        };
        let busy = ui.chrome(Path::new("a.cbz"), 3, &view, geo, 0, "");
        assert!(busy.contains("Carregando…"));
        ui.key(b':', 3);
        ui.key(b'1', 3);
        ui.key(b'2', 3);
        let prompt = ui.chrome(Path::new("a.cbz"), 3, &view, geo, 0, "");
        assert!(!prompt.contains(" COMMAND "));
        assert!(prompt.contains(":12"));
        assert!(prompt.contains("\x1b[?25h"));
    }

    #[test]
    fn image_area_reserves_title_scrollbar_status_and_command_rows() {
        let geo = Geometry {
            cols: 80,
            rows: 24,
            cw: 8,
            ch: 16,
        };
        assert_eq!(geo.page_width(), 624);
        assert_eq!(geo.page_height(), 318);
        // The sixel bands, including rounding, stay above the status bar.
        assert!(16 + geo.page_height().div_ceil(6) * 6 <= (geo.rows as u32 - 3) * geo.ch);
        let ui = Ui::default();
        let mut view = View::new(0);
        view.height = 318;
        view.max_y = 1000;
        view.y = 1000;
        let scrollbar = ui.scrollbar(&view, geo);
        assert!(scrollbar.contains("\x1b[21;80H"));
        assert!(!scrollbar.contains("\x1b[23;80H"));
        assert!(!scrollbar.contains("\x1b[24;80H"));
    }
}
