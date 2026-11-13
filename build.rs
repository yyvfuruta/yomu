fn main() {
    if let Err(error) = pkg_config::Config::new().probe("libarchive") {
        panic!(
            "Não foi possível localizar a libarchive para compilar o Yomu.\n\
             Debian/Ubuntu: sudo apt install pkg-config libarchive-dev\n\
             Arch Linux: sudo pacman -S pkgconf libarchive\n\
             Homebrew: brew install pkg-config libarchive\n\
             Se necessário, use PKG_CONFIG_PATH=\"$(brew --prefix libarchive)/lib/pkgconfig\" make\n\
             Detalhes: {error}"
        );
    }
}
