mod archive;
mod document;
mod picker;
mod process;
mod renderer;
mod state;
mod terminal;
#[cfg(test)]
mod tests;
mod ui;
mod viewer;

use anyhow::{bail, Context, Result};
use std::{
    env,
    io::{self, IsTerminal},
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};

const HELP: &str = "Yomu — PDFs e mangás no terminal via Sixel\n\nUso: yomu [--force-sixel] [--cell-size 8x16] [DOCUMENTO]\n\nFormatos: PDF, CBZ/ZIP e pasta de PNG/JPEG/WebP.\nTeclas: h/l páginas, j/k rolagem, Ctrl+d/u meia tela, Ctrl+f/b tela,\ngg/G primeira/última, 12G página 12, w/a ajuste, zi/zo zoom, H/L pan.\nComandos: :12 página 12, ? ou :help ajuda, q ou :q sair.\n\nDependências: Poppler (pdfinfo, pdftoppm), libsixel (img2sixel) e libarchive.\n";

struct Args {
    path: PathBuf,
    force: bool,
    cell: (u32, u32),
}
fn arguments() -> Result<Option<Args>> {
    let mut args = env::args_os().skip(1);
    let mut path = None;
    let mut force = false;
    let mut cell = (8, 16);
    let mut literal = false;
    while let Some(arg) = args.next() {
        if !literal && arg == "--" {
            literal = true;
            continue;
        }
        if !literal && (arg == "--help" || arg == "-h") {
            print!("{HELP}");
            return Ok(None);
        }
        if !literal && arg == "--version" {
            println!("yomu {}", env!("CARGO_PKG_VERSION"));
            return Ok(None);
        }
        if !literal && arg == "--force-sixel" {
            force = true;
            continue;
        }
        if !literal && arg == "--cell-size" {
            let value = args
                .next()
                .context("--cell-size exige LARGURAxALTURA, por exemplo 8x16")?;
            let value = value.to_string_lossy();
            let (w, h) = value.split_once('x').context("Use --cell-size 8x16")?;
            cell = (w.parse()?, h.parse()?);
            if !(1..=256).contains(&cell.0) || !(1..=256).contains(&cell.1) {
                bail!("Células devem ter entre 1 e 256 pixels");
            }
            continue;
        }
        if !literal && arg.to_string_lossy().starts_with('-') {
            bail!("Opção desconhecida: {}", arg.to_string_lossy());
        }
        if path.replace(PathBuf::from(arg)).is_some() {
            bail!("Informe somente um documento");
        }
    }
    Ok(Some(Args {
        path: path.unwrap_or_else(|| PathBuf::from(".")),
        force,
        cell,
    }))
}
fn run() -> Result<()> {
    let Some(args) = arguments()? else {
        return Ok(());
    };
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("Execute em um terminal interativo com suporte a Sixel");
    }
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGINT,
    ] {
        signal_hook::flag::register(signal, stop.clone())?;
    }
    let _ = process::STOP.set(stop);
    let mut terminal = terminal::Terminal::enter()?;
    let mut path = args.path;
    let mut choose = picker::should_pick(&path)?;
    let mut sixel_checked = args.force;
    loop {
        if choose {
            let Some(selected) = picker::select(&mut terminal, &path)? else {
                return Ok(());
            };
            path = selected;
        }
        let document = document::Document::open(&path)?;
        let parent = document.path.parent().unwrap_or(&document.path).to_owned();
        if !sixel_checked && !terminal.supports_sixel()? {
            bail!("O terminal não anunciou Sixel. Use um terminal compatível, como foot, ou tente --force-sixel se já houver suporte.");
        }
        sixel_checked = true;
        if !viewer::run(document, &mut terminal, args.cell)? {
            return Ok(());
        }
        path = parent;
        choose = true;
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("yomu: {error:#}");
        std::process::exit(1);
    }
}
