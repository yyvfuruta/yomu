use crate::{
    document::Document,
    process,
    state::History,
    terminal::{Geometry, Terminal},
};
use anyhow::{bail, Result};
use image::DynamicImage;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
#[cfg(test)]
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fit {
    Area,
    Width,
}
#[derive(Debug, Clone)]
pub struct View {
    pub page: usize,
    pub zoom: f64,
    pub fit: Fit,
    pub x: u32,
    pub y: u32,
    pub max_x: u32,
    pub max_y: u32,
    pub height: u32,
    pub cell_h: u32,
    pub awaiting_layout: bool,
    prefix: u32,
    has_prefix: bool,
    g: bool,
    z: bool,
}
#[derive(Debug, PartialEq)]
pub enum Action {
    None,
    Render,
    Page,
    Quit,
}
impl View {
    pub fn clear_pending_keys(&mut self) {
        self.g = false;
        self.z = false;
        self.has_prefix = false;
        self.prefix = 0;
    }
    pub fn pending_keys(&self) -> String {
        format!(
            "{}{}{}",
            if self.has_prefix {
                self.prefix.to_string()
            } else {
                String::new()
            },
            if self.g { "g" } else { "" },
            if self.z { "z" } else { "" }
        )
    }
    pub fn jump(&mut self, page: usize, total: usize) -> Action {
        self.page = page.min(total - 1);
        self.reset_scroll();
        self.g = false;
        self.z = false;
        self.has_prefix = false;
        self.prefix = 0;
        Action::Page
    }
    pub fn new(page: usize) -> Self {
        Self {
            page,
            zoom: 1.0,
            fit: Fit::Width,
            x: 0,
            y: 0,
            max_x: 0,
            max_y: 0,
            height: 1,
            cell_h: 16,
            awaiting_layout: true,
            prefix: 0,
            has_prefix: false,
            g: false,
            z: false,
        }
    }
    fn reset_scroll(&mut self) {
        self.x = 0;
        self.y = 0;
        self.awaiting_layout = true;
    }
    pub fn key(&mut self, key: u8, total: usize) -> Action {
        if matches!(key, b'q' | 3) {
            return Action::Quit;
        }
        if self.z {
            self.z = false;
            match key {
                b'i' => self.zoom = (self.zoom * 1.25).min(4.0),
                b'o' => self.zoom = (self.zoom / 1.25).max(0.25),
                b'z' => self.zoom = 1.0,
                _ => return Action::None,
            }
            self.reset_scroll();
            return Action::Render;
        }
        if key.is_ascii_digit() {
            self.prefix = self
                .prefix
                .saturating_mul(10)
                .saturating_add((key - b'0') as u32)
                .min(9_999_999);
            self.has_prefix = true;
            return Action::None;
        }
        let count = if self.has_prefix {
            self.prefix.max(1)
        } else {
            1
        };
        let had_count = self.has_prefix;
        self.prefix = 0;
        self.has_prefix = false;
        let previous_g = self.g;
        self.g = false;
        let old_page = self.page;
        match key {
            b'g' if previous_g => self.page = 0,
            b'g' => {
                self.g = true;
                return Action::None;
            }
            b'G' => {
                self.page = if had_count {
                    count as usize - 1
                } else {
                    total - 1
                }
            }
            b'h' => self.page = self.page.saturating_sub(count as usize),
            b'l' => self.page = self.page.saturating_add(count as usize),
            b'z' => {
                self.z = true;
                return Action::None;
            }
            b'w' | b'a' => {
                self.fit = if key == b'w' { Fit::Width } else { Fit::Area };
                self.zoom = 1.0;
                self.reset_scroll();
                return Action::Render;
            }
            b'H' | b'L' => {
                let delta = 80u32.saturating_mul(count);
                self.x = if key == b'H' {
                    self.x.saturating_sub(delta)
                } else {
                    self.x.saturating_add(delta).min(self.max_x)
                };
                return Action::Render;
            }
            b'j' | b'k' | 4 | 21 | 6 | 2 => {
                // A new page has unknown bounds until its frame is prepared.
                // Repeated keys must not skip pages while it is loading.
                if self.awaiting_layout {
                    return Action::None;
                }
                let up = matches!(key, b'k' | 21 | 2);
                if !up && self.y == self.max_y && self.page + 1 < total {
                    self.page += 1;
                    self.reset_scroll();
                    return Action::Page;
                }
                if up && self.y == 0 && self.page > 0 {
                    self.page -= 1;
                    self.reset_scroll();
                    // Resolve the previous page's bottom using its own height.
                    self.y = u32::MAX;
                    return Action::Page;
                }
                let step = match key {
                    b'j' | b'k' => self.cell_h * 3,
                    4 | 21 => self.height / 2,
                    _ => self.height,
                };
                let step = step.saturating_mul(count);
                let previous_y = self.y;
                self.y = if up {
                    self.y.saturating_sub(step)
                } else {
                    self.y.saturating_add(step).min(self.max_y)
                };
                return if self.y == previous_y {
                    Action::None
                } else {
                    Action::Render
                };
            }
            _ => return Action::None,
        }
        self.page = self.page.min(total - 1);
        if old_page != self.page {
            self.reset_scroll();
            Action::Page
        } else {
            Action::None
        }
    }
}

pub fn scaled_dimensions(
    source: &DynamicImage,
    view: &mut View,
    geo: Geometry,
) -> Result<(u32, u32, u32)> {
    let width = geo.page_width();
    view.height = geo.page_height();
    if view.height < 6 {
        bail!("Área de leitura menor que seis pixels; aumente o terminal");
    }
    view.cell_h = geo.ch;
    let mut factor = width as f64 / source.width() as f64;
    if view.fit == Fit::Area {
        factor = factor.min(view.height as f64 / source.height() as f64);
    }
    factor *= view.zoom;
    let w = (source.width() as f64 * factor).round().max(1.0) as u32;
    let h = (source.height() as f64 * factor).round().max(1.0) as u32;
    if w as u64 * h as u64 > 80_000_000 {
        bail!("Ampliação excede 80 megapixels; reduza o zoom");
    }
    view.max_x = w.saturating_sub(width);
    view.max_y = h.saturating_sub(view.height);
    view.x = view.x.min(view.max_x);
    view.y = view.y.min(view.max_y);
    view.awaiting_layout = false;
    Ok((w, h, width))
}

#[cfg(test)]
pub fn viewport(source: &DynamicImage, view: &mut View, geo: Geometry) -> Result<DynamicImage> {
    let (w, h, width) = scaled_dimensions(source, view, geo)?;
    let scaled = source.resize_exact(w, h, image::imageops::FilterType::Triangle);
    Ok(scaled.crop_imm(view.x, view.y, width.min(w), view.height.min(h)))
}

#[cfg(test)]
pub fn status_text(text: &str, columns: usize) -> String {
    let mut result = String::new();
    let mut width = 0;
    for c in text.chars() {
        let c = if c.is_control() { '?' } else { c };
        let cw = c.width().unwrap_or(0);
        if width + cw > columns {
            break;
        }
        result.push(c);
        width += cw;
    }
    result.push_str(&" ".repeat(columns - width));
    result
}

fn display(term: &Terminal, geo: Geometry, frame: &crate::renderer::Frame) -> Result<()> {
    if crossterm::terminal::size()? != (geo.cols, geo.rows) {
        return Ok(());
    }
    let column = 1 + ((geo.cols as u32 * geo.cw).saturating_sub(frame.width) / (2 * geo.cw));
    term.write(format!("\x1b[0m\x1b[2J\x1b[H\x1b[2;{column}H").as_bytes())?;
    term.write(&frame.sixel)
}

pub fn run(doc: Document, term: &mut Terminal, cell: (u32, u32)) -> Result<bool> {
    use crate::ui::{Event, Ui};
    let doc = Arc::new(doc);
    let mut renderer = crate::renderer::Renderer::new(doc.clone());
    let history = History::new(&doc.path);
    let mut view = View::new(history.read(doc.count));
    let mut ui = Ui::default();
    let mut dirty = true;
    let mut previous_size = (0, 0);
    let mut geometry = None;
    let mut generation = 0;
    let mut last_frame = None;
    let mut repaint = true;
    let mut chrome_dirty = true;
    let clock = Instant::now();
    let mut last_phase = 0;
    let result = (|| {
        while !process::stopping() {
            let size = crossterm::terminal::size()?;
            if size != previous_size {
                dirty = true;
                repaint = true;
                previous_size = size;
                geometry = None;
                last_frame = None;
                view.awaiting_layout = true;
            }
            let geo = match geometry {
                Some(geo) => geo,
                None => {
                    let geo = term.geometry(cell)?;
                    geometry = Some(geo);
                    geo
                }
            };
            let mut key = term.key()?;
            for _ in 0..256 {
                let Some(byte) = key else {
                    break;
                };
                chrome_dirty = true;
                let action = match ui.key(byte, doc.count) {
                    Event::View(key) => view.key(key, doc.count),
                    Event::Quit => return Ok(false),
                    Event::Files => return Ok(true),
                    Event::Chrome => {
                        view.clear_pending_keys();
                        Action::None
                    }
                    Event::Redraw => {
                        view.clear_pending_keys();
                        repaint = true;
                        Action::None
                    }
                    Event::Page(page) => view.jump(page, doc.count),
                    Event::Fit(mode) => {
                        view.key(if mode == Fit::Width { b'w' } else { b'a' }, doc.count)
                    }
                };
                match action {
                    Action::Quit => return Ok(false),
                    Action::Page => {
                        if history.write(view.page).is_err() {
                            ui.message = "Não foi possível salvar progresso".into();
                        }
                        dirty = true;
                    }
                    Action::Render => dirty = true,
                    Action::None => {}
                }
                key = term.key_timeout(Duration::ZERO)?;
            }
            if dirty {
                generation += 1;
                renderer.submit(generation, view.clone(), geo)?;
                dirty = false;
                ui.loading = true;
                chrome_dirty = true;
            }
            while let Some((id, result)) = renderer.ready()? {
                if id == generation {
                    let frame = result?;
                    view.max_x = frame.view.max_x;
                    view.max_y = frame.view.max_y;
                    view.height = frame.view.height;
                    view.cell_h = frame.view.cell_h;
                    view.x = frame.view.x;
                    view.y = frame.view.y;
                    view.awaiting_layout = frame.view.awaiting_layout;
                    last_frame = Some((id, frame));
                    ui.loading = false;
                    repaint = true;
                }
            }
            let phase = (clock.elapsed().as_millis() / 100) as usize;
            if ui.loading && phase != last_phase {
                chrome_dirty = true;
            }
            last_phase = phase;
            if repaint {
                if ui.help {
                    term.write(ui.help_body(geo).as_bytes())?;
                } else if let Some((id, frame)) = &last_frame {
                    if *id == generation {
                        display(term, geo, frame)?;
                    } else {
                        term.write(b"\x1b[0m\x1b[2J\x1b[H")?;
                    }
                } else {
                    term.write(b"\x1b[0m\x1b[2J\x1b[H")?;
                }
                repaint = false;
                chrome_dirty = true;
            }
            if chrome_dirty {
                if !ui.help && !view.awaiting_layout {
                    term.write(ui.scrollbar(&view, geo).as_bytes())?;
                }
                term.write(
                    ui.chrome(
                        &doc.path,
                        doc.count,
                        &view,
                        geo,
                        phase,
                        &view.pending_keys(),
                    )
                    .as_bytes(),
                )?;
                chrome_dirty = false;
            }
        }
        Ok(false)
    })();
    if let Err(error) = history.write(view.page) {
        eprintln!("\r\nyomu: falha ao salvar progresso: {error}");
    }
    result
}
