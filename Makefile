.DEFAULT_GOAL := build
CARGO ?= cargo
export FILE

.PHONY: build run install test check fmt clean help

build:
	$(CARGO) build --release --locked

run:
	@test -n "$$FILE" || { printf '%s\n' 'Uso: make run FILE="arquivo.cbz"' >&2; exit 2; }
	$(CARGO) run --release --locked -- -- "$$FILE"

install:
	$(CARGO) install --path . --locked

test:
	$(CARGO) test --locked

check:
	$(CARGO) fmt --check
	$(CARGO) clippy --locked --all-targets -- -D warnings
	$(CARGO) test --locked

fmt:
	$(CARGO) fmt

clean:
	$(CARGO) clean

help:
	@printf '%s\n' \
	  'make                         Compila em release: target/release/yomu' \
	  'make run FILE="arquivo.cbz"   Compila e abre o documento' \
	  'make install                 Instala em ~/.cargo/bin' \
	  'make test                    Executa os testes' \
	  'make check                   Verifica formato, Clippy e testes' \
	  'make fmt                     Formata o código Rust' \
	  'make clean                   Remove os artefatos de compilação'
