use crate::{
    archive,
    document::{natural_cmp, Document},
    state::History,
    terminal::Geometry,
    viewer::{self, Action, Fit, View},
};
use image::{DynamicImage, ImageFormat};
use std::{fs, io::Cursor, path::Path, process::Command};

fn image_bytes(format: ImageFormat) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::new_rgb8(80, 120)
        .write_to(&mut bytes, format)
        .unwrap();
    bytes.into_inner()
}

// A stored ZIP fixture with central directory; no ZIP production code is duplicated.
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    fn crc(data: &[u8]) -> u32 {
        let mut value = !0u32;
        for &byte in data {
            value ^= byte as u32;
            for _ in 0..8 {
                value = (value >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(value & 1)));
            }
        }
        !value
    }
    fn u16(out: &mut Vec<u8>, value: u16) {
        out.extend(value.to_le_bytes());
    }
    fn u32(out: &mut Vec<u8>, value: u32) {
        out.extend(value.to_le_bytes());
    }
    let mut data = Vec::new();
    let mut central = Vec::new();
    for (name, body) in entries {
        let offset = data.len() as u32;
        let checksum = crc(body);
        let size = body.len() as u32;
        u32(&mut data, 0x04034b50);
        u16(&mut data, 20);
        for _ in 0..4 {
            u16(&mut data, 0);
        }
        u32(&mut data, checksum);
        u32(&mut data, size);
        u32(&mut data, size);
        u16(&mut data, name.len() as u16);
        u16(&mut data, 0);
        data.extend(name.as_bytes());
        data.extend(*body);
        u32(&mut central, 0x02014b50);
        u16(&mut central, 20);
        u16(&mut central, 20);
        for _ in 0..4 {
            u16(&mut central, 0);
        }
        u32(&mut central, checksum);
        u32(&mut central, size);
        u32(&mut central, size);
        u16(&mut central, name.len() as u16);
        for _ in 0..4 {
            u16(&mut central, 0);
        }
        u32(&mut central, 0);
        u32(&mut central, offset);
        central.extend(name.as_bytes());
    }
    let offset = data.len() as u32;
    let size = central.len() as u32;
    data.extend(central);
    u32(&mut data, 0x06054b50);
    u16(&mut data, 0);
    u16(&mut data, 0);
    u16(&mut data, entries.len() as u16);
    u16(&mut data, entries.len() as u16);
    u32(&mut data, size);
    u32(&mut data, offset);
    u16(&mut data, 0);
    data
}

#[test]
fn natural_order_handles_large_numbers_and_case() {
    let mut names = [
        "10.png",
        "2.PNG",
        "1.webp",
        "99999999999999999999999999999999.jpg",
    ];
    names.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(
        names,
        [
            "1.webp",
            "2.PNG",
            "10.png",
            "99999999999999999999999999999999.jpg"
        ]
    );
    assert!(natural_cmp("0002.png", "10.png").is_lt());
}

#[test]
fn folder_decodes_png_jpeg_and_webp() {
    let temp = tempfile::tempdir().unwrap();
    assert!(Document::open(temp.path()).is_err());
    fs::write(temp.path().join("10.webp"), image_bytes(ImageFormat::WebP)).unwrap();
    fs::write(temp.path().join("2.jpg"), image_bytes(ImageFormat::Jpeg)).unwrap();
    fs::write(temp.path().join("1.png"), image_bytes(ImageFormat::Png)).unwrap();
    fs::write(temp.path().join("ignored.txt"), b"ignore").unwrap();
    let document = Document::open(temp.path()).unwrap();
    assert_eq!(document.count, 3);
    for index in 0..3 {
        let page = document.page(index, 640).unwrap();
        assert_eq!((page.width(), page.height()), (80, 120));
    }
}

#[test]
fn zip_reads_nested_entries_without_extracting() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("chapter.cbz");
    let png = image_bytes(ImageFormat::Png);
    fs::write(
        &path,
        zip(&[
            ("pages/10.png", &png),
            ("pages/2.png", &png),
            ("pages/1.png", &png),
            ("__MACOSX/junk.png", &png),
            ("notes.txt", b"notes"),
        ]),
    )
    .unwrap();
    let entries = archive::list(&path).unwrap();
    assert_eq!(
        entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["pages/1.png", "pages/2.png", "pages/10.png"]
    );
    let document = Document::open(&path).unwrap();
    assert_eq!(document.count, 3);
    for index in 0..3 {
        assert_eq!(document.page(index, 640).unwrap().width(), 80);
    }
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    let bad = temp.path().join("bad.zip");
    fs::write(&bad, b"invalid").unwrap();
    assert!(Document::open(&bad).is_err());
}

fn keys(view: &mut View, input: &[u8]) {
    for &key in input {
        view.key(key, 20);
    }
}
#[test]
fn vim_pages_counts_bounds_and_quit() {
    let mut view = View::new(0);
    keys(&mut view, b"12G");
    assert_eq!(view.page, 11);
    keys(&mut view, b"3h");
    assert_eq!(view.page, 8);
    keys(&mut view, b"Gllll");
    assert_eq!(view.page, 19);
    keys(&mut view, b"ggh");
    assert_eq!(view.page, 0);
    keys(&mut view, b"999999999999999999999999G");
    assert_eq!(view.page, 19);
    keys(&mut view, b"2xG");
    assert_eq!(view.page, 19);
    assert_eq!(view.key(b'q', 20), Action::Quit);
}
#[test]
fn vim_scroll_zoom_and_fit() {
    let mut view = View::new(0);
    view.height = 600;
    view.max_y = 3000;
    view.awaiting_layout = false;
    keys(&mut view, &[4, 6]);
    assert_eq!(view.y, 900);
    keys(&mut view, &[21, 2]);
    assert_eq!(view.y, 0);
    keys(&mut view, b"3j");
    assert_eq!(view.y, 144);
    keys(&mut view, b"zi");
    assert_eq!(view.zoom, 1.25);
    assert_eq!(view.y, 0);
    keys(&mut view, b"zo");
    assert_eq!(view.zoom, 1.0);
    keys(&mut view, b"w");
    assert_eq!(view.fit, Fit::Width);
    keys(&mut view, b"a");
    assert_eq!(view.fit, Fit::Area);
}
#[test]
fn viewport_crops_before_encoding_and_reflows() {
    let source = DynamicImage::new_rgb8(800, 2000);
    let mut view = View::new(0);
    view.fit = Fit::Width;
    let geo = Geometry {
        cols: 80,
        rows: 24,
        cw: 8,
        ch: 16,
    };
    let image = viewer::viewport(&source, &mut view, geo).unwrap();
    assert_eq!((image.width(), image.height()), (624, 318));
    assert_eq!(view.max_y, 1242);
    view.y = 9000;
    viewer::viewport(&source, &mut view, geo).unwrap();
    assert_eq!(view.y, view.max_y);
    view.zoom = 2.0;
    let image = viewer::viewport(&source, &mut view, geo).unwrap();
    assert_eq!(image.width(), 624);
    assert_eq!(view.max_x, 624);
    keys(&mut view, b"L");
    assert_eq!(view.x, 80);
    let smaller = Geometry {
        cols: 40,
        rows: 12,
        ..geo
    };
    let image = viewer::viewport(&source, &mut view, smaller).unwrap();
    assert_eq!((image.width(), image.height()), (304, 126));
}
#[test]
fn state_is_atomic_clamped_and_tolerates_corruption() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state/page");
    let state = History::at(path.clone());
    assert_eq!(state.read(20), 0);
    state.write(11).unwrap();
    assert_eq!(state.read(20), 11);
    assert_eq!(state.read(3), 2);
    fs::write(path, b"invalid").unwrap();
    assert_eq!(state.read(20), 0);
}
#[test]
fn status_sanitizes_controls_and_counts_wide_characters() {
    assert_eq!(viewer::status_text("x\x1b[2J", 6), "x?[2J ");
    assert_eq!(viewer::status_text("漫画", 3), "漫 ");
}

fn pdf_fixture(path: &Path) {
    let content =
        b"1 0 0 rg 20 20 100 100 re f\n0 0 0 rg\nBT /F1 14 Tf 20 200 Td (Yomu text) Tj ET\n";
    let objects = [b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_vec(),
        [format!("<< /Length {} >>\nstream\n", content.len()).into_bytes(), content.to_vec(), b"endstream".to_vec()].concat(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()];
    let mut data = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(data.len());
        data.extend(format!("{} 0 obj\n", index + 1).as_bytes());
        data.extend(object);
        data.extend(b"\nendobj\n");
    }
    let xref = data.len();
    data.extend(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        data.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    data.extend(
        format!("trailer\n<< /Root 1 0 R /Size 6 >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    fs::write(path, data).unwrap();
}
#[test]
fn real_poppler_rasterizes_text_and_vectors() {
    if Command::new("pdfinfo").arg("-v").output().is_err() {
        eprintln!("SKIP: Poppler não instalado");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vector.pdf");
    pdf_fixture(&path);
    let document = Document::open(&path).unwrap();
    assert_eq!(document.count, 1);
    let image = document.page(0, 100).unwrap().to_rgb8();
    assert_eq!(image.dimensions(), (100, 150));
    assert!(image.pixels().any(|p| p[0] > 200 && p[1] < 50 && p[2] < 50));
    assert!(image
        .pixels()
        .any(|p| p[0] < 250 && p[1] == p[0] && p[2] == p[0]));
}

#[test]
fn vertical_reading_crosses_page_boundaries_in_both_directions() {
    let mut view = View::new(0);
    assert_eq!(view.fit, Fit::Width);
    // Input during initial loading doesn't skip a page with unknown height.
    assert_eq!(view.key(b'j', 3), Action::None);
    let geo = Geometry {
        cols: 20,
        rows: 10,
        cw: 8,
        ch: 16,
    };
    let tall = DynamicImage::new_rgb8(160, 400);
    viewer::scaled_dimensions(&tall, &mut view, geo).unwrap();
    assert_eq!(view.max_y, 264);
    assert_eq!(view.key(b'j', 3), Action::Render);
    assert_eq!(view.page, 0);
    view.y = view.max_y - 1;
    view.key(b'j', 3);
    assert_eq!(view.y, 264); // Show the final strip before changing pages.
    assert_eq!(view.key(b'j', 3), Action::Page);
    assert_eq!((view.page, view.y), (1, 0));
    assert_eq!(view.key(b'j', 3), Action::None); // Await new page dimensions.
    let shorter = DynamicImage::new_rgb8(160, 200);
    viewer::scaled_dimensions(&shorter, &mut view, geo).unwrap();
    assert_eq!(view.key(b'k', 3), Action::Page);
    assert_eq!(view.page, 0);
    viewer::scaled_dimensions(&tall, &mut view, geo).unwrap();
    assert_eq!(view.y, 264); // Previous page opens at its bottom, not its top.
    view.y = 0;
    assert_eq!(view.key(b'k', 3), Action::None);
    view.page = 2;
    view.y = view.max_y;
    assert_eq!(view.key(b'j', 3), Action::None);
}

#[test]
fn half_and_full_screen_scrolls_cross_boundaries_and_fit_pages_advance() {
    let geo = Geometry {
        cols: 20,
        rows: 10,
        cw: 8,
        ch: 16,
    };
    let image = DynamicImage::new_rgb8(80, 40);
    for (down, up) in [(4, 21), (6, 2), (b'j', b'k')] {
        let mut view = View::new(0);
        viewer::scaled_dimensions(&image, &mut view, geo).unwrap();
        assert_eq!(view.max_y, 0);
        assert_eq!(view.key(down, 3), Action::Page);
        assert_eq!(view.page, 1);
        viewer::scaled_dimensions(&image, &mut view, geo).unwrap();
        assert_eq!(view.key(up, 3), Action::Page);
        assert_eq!(view.page, 0);
        viewer::scaled_dimensions(&image, &mut view, geo).unwrap();
        assert_eq!(view.y, 0);
    }
}
