# Neoveg

Editor de vídeo não-linear (NLE) escrito em **Rust puro**, com interface nativa em português (`eframe` + `wgpu` com fallback automático para `glow`), semântica operacional inspirada no VEGAS Pro e política estrita de **zero pânico em produção**.

## Arquitetura do Workspace (5 Crates)

- **`neoveg_common`** (`crates/neoveg_common`): tipos fundamentais, representação exata de tempo (`Ticks`, `FrameRate`, `Rate`, *SMPTE Drop-Frame*), curvas, preferências e utilitários base.
- **`neoveg_engine`** (`crates/neoveg_engine`): estado da timeline, invariantes de edição atômica, mixer de áudio multifaixa, compositor de vídeo, efeitos/transições, escopos, persistência `.neoveg`/`.nvp`, decodificação de áudio em Rust puro via `symphonia` (com leitor WAVE/AIFF próprio e fallbacks), decodificação streaming via Windows Media Foundation (`media/mf.rs`) e **Tabela Única de Comandos**.
- **`neoveg_scripting`** (`crates/neoveg_scripting`): camada de automação headless e ponte prevista para scripts `.NET`/`C#` (`ScriptPortal.Vegas`).
- **`neoveg_ui`** (`crates/neoveg_ui`): interface desktop nativa em `eframe` + `wgpu`, timeline interativa, 7 temas visuais (incluindo 2 temas de Alto Contraste WCAG AAA), gerenciador de layouts `.neoveglayout` com painéis destacáveis e editores de atalhos e preferências.
- **`neoveg`** (`crates/neoveg`): binário principal e interface CLI com inicialização do pool `rayon` proporcional ao hardware.

## Recursos Implementados

- **Timeline e Edição VEGAS Pro:** múltiplas trilhas A/V, auto-crossfade ao sobrepor eventos, alças de fade nos cantos superiores, alça de ganho em dB no topo de eventos de áudio, playhead físico de `1 px`, controle contínuo de taxa (*Playback Rate*), divisão (`S`), marcações (`M`), regiões (`R`), *Auto Ripple* (`Ctrl+L`), encaixe (`F8`) e *scrub* (`J`/`K`/`L`).
- **Tabela Única de Comandos e Atalhos Regraváveis:** menus, barra de ferramentas, atalhos e comandos CLI despacham pelo mesmo `id`; atalhos personalizados podem ser gravados/limpos em *Ajuda → Atalhos de teclado*.
- **Personalização e Acessibilidade:** 7 temas selecionáveis em *Exibir → Tema* ou *Opções → Preferências...* (incluindo *Alto Contraste AAA Escuro* e *Claro* com contraste >= 7:1), **Editar → Propriedades → Layout** com os três tipos clássicos de disposição (*Sonic Foundry*, *Sony* e *MAGIX* padrão VEGAS Pro 21 em 3 colunas com **Prévia de Vídeo**, **Master Bus** dedicado com `Mono`/`Dim -20 dB` e abas na base), **Tela de abertura (Splash Screen)** (`crates/neoveg_ui/src/splash.rs`), **Tela de boas-vindas (Welcome Screen)** com modelos rápidos e **Projetos recentes** persistidos (`crates/neoveg_ui/src/welcome.rs`), painel acoplável **Color Grading** ligado ao `FxKind::ColorCorrector` (`crates/neoveg_ui/src/color_grading.rs`), janela **Renderizar como...** em 2 colunas com modelos e favoritos, 57 ícones vetoriais nativos coloridos (`crates/neoveg_ui/src/icons.rs`), barra inferior de transporte/edição com seleção `In`/`Fim`/`Tam`, painéis acopláveis/destacáveis em janelas nativas do Windows e 4 presets + arquivos `.neoveglayout` salvos pelo usuário.
- **Formatos Suportados:**
  - **Rust puro (multiplataforma):** projetos `.neoveg` (e legado `.nvp`), `AVI (MJPEG/PCM)`, `Y4M`, `GIF`, sequências de imagens (`PNG`, `JPEG`, `BMP`, `TIFF`, `WebP`), áudio `WAV`/`AIFF`/`ADPCM`, `FLAC`, `Ogg Vorbis` (`.ogg`, `.oga`), `MP1`/`MP2`/`MP3`, `AAC`/`ALAC` (`.aac`, `.m4a`), `CAF` e `MKA` via `symphonia` + leitor WAVE/AIFF próprio e fallbacks (`claxon`, `lewton`, `puremp3`).
  - **Windows nativo (Media Foundation):** importação e decodificação em streaming de vídeo e fallback de áudio em `MP4`, `MOV`, `MKV`, `WEBM`, `M4V`, `WMV` e `ASF`.
- **Hardware Limitado:** matriz de prévia 4×3 (*Rascunho / Prévia / Boa / Melhor* × *1/4 / 1/2 / Completa*), geração de proxies `.avi`, pré-renderização seletiva de região, limite configurável de RAM/quadros em cache e compilação portátil sem `-C target-cpu=native`.

## Como Executar

```bash
# Abrir interface gráfica (opcionalmente passando um projeto ou mídia)
cargo run -p neoveg
cargo run -p neoveg -- meu_projeto.neoveg

# Interface de linha de comando (CLI)
cargo run -p neoveg -- --list-commands
cargo run -p neoveg -- --command insert.marker meu_projeto.neoveg atualizado.neoveg
cargo run -p neoveg -- --render meu_projeto.neoveg saida.avi
```

Logs, autosave, preferências (`preferencias.json`), layouts (`.neoveglayout`), proxies e pré-renderizações são gravados ao lado do executável em builds portáteis (ou na pasta temporária do sistema quando executado em diretório somente leitura).

## Empacotamento Portátil e Verificações de Qualidade

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python tools/check_licenses.py
python build_portable.py
```

O script `build_portable.py` gera o pacote portátil em `target/portable/neoveg-portable/` sem dependência de instalador, fontes proprietárias ou binários externos.

## Acompanhamento de Paridade

- [Roadmap](ROADMAP.md)
- [Matriz de Paridade VEGAS Pro](docs_obsidian/ui_references/matriz_de_paridade.md)
- [Atribuição e Licenças de Assets](ATTRIBUTION.md)
