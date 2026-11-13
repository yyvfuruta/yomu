//! A UI nunca espera rasterização, redimensionamento ou codificação.
use crate::{
    document::Document,
    process,
    terminal::Geometry,
    viewer::{scaled_dimensions, View},
};
use anyhow::{Context, Result};
use image::{imageops::FilterType, DynamicImage};
use std::{
    io::Write,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

struct Request {
    id: u64,
    view: View,
    geo: Geometry,
    cancel: Arc<AtomicBool>,
}
pub struct Frame {
    pub view: View,
    pub width: u32,
    pub sixel: Vec<u8>,
}
type Response = (u64, Result<Frame>);

#[derive(Default)]
struct Cache {
    source: Option<DynamicImage>,
    source_key: Option<(usize, u32)>,
    scaled: Option<DynamicImage>,
    scaled_key: Option<(usize, u32, u32, u32)>,
}
impl Cache {
    fn image(&mut self, doc: &Document, view: &mut View, geo: Geometry) -> Result<DynamicImage> {
        process::check_cancelled()?;
        let target = (geo.page_width() as f64 * view.zoom)
            .round()
            .clamp(1.0, 8192.0) as u32;
        let key = (view.page, target);
        if self.source_key != Some(key) {
            self.scaled = None;
            self.scaled_key = None;
            self.source = Some(doc.page(view.page, target)?);
            self.source_key = Some(key);
        }
        process::check_cancelled()?;
        let source = self.source.as_ref().unwrap();
        let (w, h, width) = scaled_dimensions(source, view, geo)?;
        let key = (view.page, target, w, h);
        if self.scaled_key != Some(key) {
            self.scaled = None;
            // Triangle is significantly cheaper than Lanczos3 for interactive
            // page fitting; the scaled page is retained across scroll commands.
            self.scaled = Some(source.resize_exact(w, h, FilterType::Triangle));
            self.scaled_key = Some(key);
        }
        process::check_cancelled()?;
        Ok(self
            .scaled
            .as_ref()
            .unwrap()
            .crop_imm(view.x, view.y, width.min(w), view.height.min(h)))
    }

    fn prepare(&mut self, doc: &Document, mut view: View, geo: Geometry) -> Result<Frame> {
        let image = self.image(doc, &mut view, geo)?.to_rgb8();
        let width = image.width();
        // PPM avoids PNG compression and decompression between Rust and libsixel.
        let mut file = tempfile::NamedTempFile::new()?;
        write!(file, "P6\n{} {}\n255\n", image.width(), image.height())?;
        file.write_all(image.as_raw())?;
        process::check_cancelled()?;
        let sixel = process::run(
            Command::new("img2sixel")
                .args(["-p", "256"])
                .arg(file.path()),
        )?;
        Ok(Frame { view, width, sixel })
    }
}

pub struct Renderer {
    requests: Option<Sender<Request>>,
    responses: Receiver<Response>,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Renderer {
    pub fn new(doc: Arc<Document>) -> Self {
        let (tx, rx) = mpsc::channel::<Request>();
        let (done, responses) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut cache = Cache::default();
            while let Ok(mut request) = rx.recv() {
                // Only the latest requested frame matters.
                for newer in rx.try_iter() {
                    request = newer;
                }
                if request.cancel.load(Ordering::Relaxed) {
                    continue;
                }
                process::set_cancel(request.cancel.clone());
                let result = cache.prepare(&doc, request.view, request.geo);
                if !request.cancel.load(Ordering::Relaxed)
                    && done.send((request.id, result)).is_err()
                {
                    break;
                }
            }
        });
        Self {
            requests: Some(tx),
            responses,
            cancel: Arc::new(AtomicBool::new(false)),
            worker: Some(worker),
        }
    }
    pub fn submit(&mut self, id: u64, view: View, geo: Geometry) -> Result<()> {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.requests
            .as_ref()
            .unwrap()
            .send(Request {
                id,
                view,
                geo,
                cancel: self.cancel.clone(),
            })
            .context("Thread de renderização encerrada")
    }
    pub fn ready(&self) -> Result<Option<Response>> {
        match self.responses.try_recv() {
            Ok(frame) => Ok(Some(frame)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => anyhow::bail!("Thread de renderização encerrada"),
        }
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.requests.take();
        if let Some(worker) = self.worker.take() {
            // Subprocesses observe cancellation every 10 ms. A CPU-bound image
            // operation may still finish later; it never holds up terminal exit.
            let start = Instant::now();
            while !worker.is_finished() && start.elapsed() < Duration::from_millis(100) {
                thread::sleep(Duration::from_millis(2));
            }
            if worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::Fit;

    #[test]
    fn scroll_reuses_decoded_and_scaled_page_but_zoom_invalidates_scale() {
        let temp = tempfile::tempdir().unwrap();
        DynamicImage::new_rgb8(80, 400)
            .save(temp.path().join("1.png"))
            .unwrap();
        let doc = Document::open(temp.path()).unwrap();
        let geo = Geometry {
            cols: 20,
            rows: 10,
            cw: 8,
            ch: 16,
        };
        let mut view = View::new(0);
        view.fit = Fit::Width;
        let mut cache = Cache::default();
        let first = cache.image(&doc, &mut view, geo).unwrap();
        assert_eq!((first.width(), first.height()), (144, 96));
        let source = cache.source.as_ref().unwrap().as_bytes().as_ptr();
        let scaled = cache.scaled.as_ref().unwrap().as_bytes().as_ptr();
        // Removing the original proves scroll doesn't reopen or decode it.
        std::fs::remove_file(temp.path().join("1.png")).unwrap();
        view.y = 100;
        cache.image(&doc, &mut view, geo).unwrap();
        assert_eq!(cache.source.as_ref().unwrap().as_bytes().as_ptr(), source);
        assert_eq!(cache.scaled.as_ref().unwrap().as_bytes().as_ptr(), scaled);
        assert_eq!(view.y, 100);
        // Width fitting at a different height changes the viewport, not scale.
        cache
            .image(&doc, &mut view, Geometry { rows: 8, ..geo })
            .unwrap();
        assert_eq!(cache.scaled.as_ref().unwrap().as_bytes().as_ptr(), scaled);
        view.fit = Fit::Area;
        cache.image(&doc, &mut view, geo).unwrap();
        assert_eq!(cache.source.as_ref().unwrap().as_bytes().as_ptr(), source);
        assert_eq!(cache.scaled.as_ref().unwrap().height(), 96);
        DynamicImage::new_rgb8(80, 400)
            .save(temp.path().join("1.png"))
            .unwrap();
        view.zoom = 2.0;
        cache.image(&doc, &mut view, geo).unwrap();
        assert_eq!(cache.scaled.as_ref().unwrap().height(), 192);
    }

    #[test]
    fn real_libsixel_accepts_the_uncompressed_viewport() {
        if Command::new("img2sixel").arg("--version").output().is_err() {
            eprintln!("SKIP: img2sixel não instalado");
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        DynamicImage::new_rgb8(64, 96)
            .save(temp.path().join("1.png"))
            .unwrap();
        let doc = Document::open(temp.path()).unwrap();
        let mut cache = Cache::default();
        let frame = cache
            .prepare(
                &doc,
                View::new(0),
                Geometry {
                    cols: 20,
                    rows: 10,
                    cw: 8,
                    ch: 16,
                },
            )
            .unwrap();
        assert!(frame.sixel.starts_with(b"\x1bP"));
        assert!(frame.sixel.ends_with(b"\x1b\\"));
        assert_eq!(frame.view.height, 96);
    }
}
