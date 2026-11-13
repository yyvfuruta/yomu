# Yomu

Leitor de PDFs e mangás no terminal Linux, escrito em **Rust**, com imagens Sixel e controles Vim.

O Poppler rasteriza páginas completas de PDFs (texto, vetores e imagens). A libsixel codifica as imagens por meio de `img2sixel`. A libarchive lê CBZ/ZIP sem extrair arquivos, e a biblioteca Rust `image` decodifica PNG, JPEG e WebP e realiza zoom e recorte. A interface usa Crossterm. Não há código ou dependência de Python.

## Instalar no Linux

É necessário Rust **1.88 ou superior**, Cargo, um compilador/linker C e as dependências abaixo. Se o Rust da distribuição for antigo, instale a versão estável seguindo [rustup.rs](https://rustup.rs/).

Debian / Ubuntu:

```sh
sudo apt update
sudo apt install build-essential pkg-config libarchive-dev poppler-utils libsixel-bin
```

Arch Linux:

```sh
sudo pacman -S --needed base-devel rust pkgconf libarchive poppler libsixel
```

Na raiz deste repositório, compile com:

```sh
make
./target/release/yomu documento.pdf
```

Ou compile e abra o arquivo em um único comando (aspas preservam espaços no nome):

```sh
make run FILE="/tmp/manga/One Piece.cbz"
```

Para instalar o executável em `~/.cargo/bin/yomu`:

```sh
make install
```

Inclua `~/.cargo/bin` no `PATH`. `make help` lista os comandos. O Makefile usa o Cargo por baixo: `make` equivale a `cargo build --release --locked`, e `make install` a `cargo install --path . --locked`. Todos os comandos de execução usam a versão otimizada.

A libarchive é vinculada dinamicamente e descoberta pelo `pkg-config` durante a compilação. Se faltar seu pacote de desenvolvimento, a compilação informa como instalá-lo. Para uma instalação Homebrew que não esteja no caminho do `pkg-config`, use `PKG_CONFIG_PATH="$(brew --prefix libarchive)/lib/pkgconfig" make`. Poppler (`pdfinfo`, `pdftoppm`) e libsixel (`img2sixel`) são programas externos e precisam estar no `PATH`. A primeira compilação precisa de internet para baixar as bibliotecas Rust, ou de um cache Cargo já preenchido.

## Executar

```sh
yomu .
yomu /tmp/manga/
yomu documento.pdf
yomu capitulo.cbz
yomu pasta-de-imagens/
yomu --force-sixel documento.pdf
yomu --cell-size 10x20 capitulo.cbz
```

`yomu` sem argumentos e `yomu .` abrem o seletor no diretório atual. Diretórios com PDFs, CBZs ou ZIPs mostram os documentos e subpastas para escolher; pastas de imagens continuam abrindo como capítulos. Em pastas mistas, selecione `Imagens desta pasta/` para ler as imagens diretamente.

No seletor, `j`/`k` escolhem, `Enter` ou `l` abrem, `h` sobe um diretório, `gg`/`G` vão ao início/fim e `/` filtra pelo nome. `Enter` confirma o filtro e `Esc` limpa; fora do filtro, `q` ou `Esc` saem. Durante a leitura, `Ctrl+p` ou `:files` abrem o seletor na pasta do documento. Arquivos ocultos e formatos incompatíveis não aparecem na lista.

Use um terminal com Sixel habilitado, como foot ou xterm compilado com esse recurso. Yomu consulta os atributos do terminal para detectar Sixel; sem confirmação, encerra com uma mensagem explicativa. `--force-sixel` permite ignorar a detecção, mas o terminal ainda precisa suportar imagens Sixel. Multiplexadores podem bloquear as consultas e as imagens; teste diretamente no terminal primeiro.

As dimensões físicas vêm do terminal via ioctl ou consulta de tamanho de célula. Se não houver resposta, usa células de 8×16 pixels. Ajuste `--cell-size LARGURAxALTURA` conforme a fonte do terminal quando necessário. A opção só é usada como fallback.

## Controles

Não há controles por setas, mouse ou menus. Prefixos numéricos funcionam em navegação e rolagem, por exemplo `3l` e `5j`.

| Tecla | Ação |
| --- | --- |
| `h` / `l` | Página anterior / próxima |
| `j` / `k` | Rolar para baixo / cima; no limite, continuar na próxima página / anterior |
| `Ctrl+d` / `Ctrl+u` | Rolar meia tela para baixo / cima, atravessando páginas no limite |
| `Ctrl+f` / `Ctrl+b` | Rolar uma tela para baixo / cima, atravessando páginas no limite |
| `gg` / `G` | Primeira / última página |
| `{n}G` | Ir à página n, contando a partir de 1, como `12G` |
| `w` | Ajustar à largura (padrão) |
| `a` | Ajustar à área disponível |
| `zi` / `zo` | Ampliar / reduzir entre 25% e 400% |
| `zz` | Restaurar o zoom para 100% |
| `H` / `L` | Rolar horizontalmente para esquerda / direita |
| `?` / `:help` | Abrir a ajuda; `Esc` ou `?` volta à leitura |
| `:` | Abrir a linha de comando; `Esc` cancela |
| `:12` | Ir para a página 12 |
| `:fit width` / `:fit area` | Ajustar à largura / área pela linha de comando |
| `:q` | Sair pela linha de comando |
| `Ctrl+p` / `:files` | Abrir o seletor de documentos |
| `q` | Sair |

O leitor abre ajustado à largura para leitura vertical. Role até o fim da página e continue descendo para abrir o topo da próxima; subindo a partir do topo, abre o fim da anterior. Páginas que cabem inteiras na tela avançam diretamente com `j`. A primeira e a última página limitam a rolagem, sem dar a volta no documento. Teclas repetidas durante o carregamento não pulam páginas ainda não exibidas. `h`/`l` continuam disponíveis para trocar diretamente de página.

O zoom é relativo ao ajuste escolhido. Navegar diretamente com `h`/`l` volta ao topo; trocar de modo restaura 100%. O layout é recalculado ao redimensionar o terminal. A página mantém sua proporção e é recortada antes da codificação Sixel, reservando espaço para as barras e uma linha de margem. A tela anterior é limpa a cada atualização. O modo de entrada e o cursor são restaurados ao sair, inclusive após erros e sinais de término tratados pelo aplicativo.

## Interface

A interface usa o fundo do terminal e informações discretas, com foco na leitura.

- Cabeçalho com nome do documento e página atual/total à direita.
- Imagem centralizada com margens laterais e indicador vertical de rolagem à direita.
- Rodapé sem blocos de cor ou rótulos de modo, com zoom, ajuste e posição (`Topo`, porcentagem, `Fim` ou `Tudo`).
- Linha de interação vazia durante a leitura, usada apenas para prefixos (`12`, `g`, `z`), comandos, mensagens e um pequeno indicador de carregamento. Os atalhos ficam na ajuda. Alterar essas informações não recodifica a imagem.
- Ajuda acessível por `?` e `:help`, com rolagem por `j`/`k`, inclusive enquanto a página carrega. `q` continua saindo do aplicativo; `Esc` retorna à leitura.

Na linha de comando, `Backspace` apaga, `Ctrl+u` limpa e `Enter` executa. O tamanho mínimo do terminal é de 20 colunas por 6 linhas; nomes longos são abreviados sem esconder o contador de páginas.

## Formatos e progresso

- PDF: rasterização sob demanda da página completa com Poppler. PDFs que exigem senha exibem o diagnóstico do Poppler; não há entrada de senha.
- CBZ/ZIP: PNG, JPEG e WebP, inclusive dentro de subpastas. Entradas `__MACOSX/` são ignoradas. A leitura não extrai arquivos para o disco.
- Pastas: PNG, JPEG e WebP diretamente dentro da pasta, sem recursão.
- Ordem natural por nome/caminho: `1`, `2`, `10`, com extensões sem distinção de maiúsculas e minúsculas.

A última página é salva ao navegar e ao sair em `${XDG_STATE_HOME:-~/.local/state}/yomu/`, em um arquivo `.page` por caminho absoluto. O arquivo contém o índice da página começando em zero e é atualizado por substituição atômica. Histórico inválido é ignorado; documentos encurtados retomam na última página válida. Renomear ou mover um documento inicia outro histórico.

A página atual fica em cache nas versões decodificada e redimensionada; rolar reaproveita o redimensionamento. Rasterização, zoom e codificação rodam em uma thread separada, mantendo a leitura de teclas ativa. Novos comandos substituem os quadros pendentes e cancelam subprocessos de quadros antigos; `q` sai sem esperar uma página terminar. O recorte é enviado à libsixel como PPM sem compressão intermediária. Subprocessos têm limite de 90 segundos. Imagens de entrada têm limite de 128 MiB, a decodificação usa limites de memória e dimensões, e a ampliação aceita até 80 megapixels. CBR/RAR, EPUB e arquivos protegidos por senha não são suportados.

## Desenvolvimento

```sh
make test    # Testes
make check   # Formatação, Clippy e testes
make fmt     # Formatar o código
```

Os testes cobrem ordem natural, PNG/JPEG/WebP, ZIPs com subpastas, rasterização real de texto e vetores com Poppler, comandos Vim, zoom, recorte, redimensionamento, reutilização do cache e histórico. Há um teste de integração em pseudo-terminal que mantém o encoder ocupado por 30 segundos e verifica que `q` encerra em menos de um segundo, cancela o subprocesso e restaura o terminal. Os testes Poppler/libsixel informam quando as ferramentas não estão instaladas.

Para conferir a integração visual, abra os três formatos em um terminal Sixel, alterne páginas de tamanhos diferentes, amplie, role e redimensione a janela. Verifique se a barra continua visível e se as páginas anteriores não deixam resíduos. A validação visual depende de um terminal Sixel real.

## Organização

- `src/document.rs`: fontes de páginas, decodificação e ordenação natural.
- `src/archive.rs`: wrapper da API de leitura ZIP da libarchive.
- `src/terminal.rs`: tela alternativa, raw mode, consultas e entrada.
- `src/viewer.rs`: comandos, layout, zoom e renderização Sixel.
- `src/ui.rs`: cabeçalho, status, indicador de rolagem, ajuda e linha de comando.
- `src/picker.rs`: escolha de documentos, navegação de diretórios e filtro.
- `src/state.rs`: persistência atômica por documento.
- `src/process.rs`: subprocessos, cancelamento e timeouts.
- `src/renderer.rs`: processamento em segundo plano, cache e descarte de quadros antigos.
- `src/main.rs`: argumentos, sinais e diagnósticos.
- `src/tests.rs`: testes automatizados.
