# Neoveg

Editor de vídeo não-linear (NLE) escrito em **Rust puro**, com interface nativa em português (`eframe` + `wgpu` com fallback automático para `glow`), semântica operacional inspirada no VEGAS Pro e política estrita de **zero pânico em produção**.

## Arquitetura do Workspace (5 Crates)

- **`neoveg_common`** (`crates/neoveg_common`): tipos fundamentais, representação exata de tempo (`Ticks`, `FrameRate`, `Rate`, *SMPTE Drop-Frame*), curvas, preferências e utilitários base.
- **`neoveg_engine`** (`crates/neoveg_engine`): estado da timeline, invariantes de edição atômica, mixer multifaixa com até 26 barramentos auxiliares (`Bus A`..`Bus Z`), processamento DSP via `fundsp`, *varispeed* com `rubato`, compositor de vídeo, efeitos/transições, escopos, persistência `.neoveg`/`.nvp`, intercâmbio OpenTimelineIO (`.otio`), decodificação de áudio em Rust puro (leitor WAVE/AIFF próprio, `claxon`, `lewton`, `puremp3`), decodificação streaming via Windows Media Foundation (`media/mf.rs`) e **Tabela Única de Comandos**.
- **`neoveg_scripting`** (`crates/neoveg_scripting`): automação headless (`--command`, `--list-commands`) e ponte sob demanda para scripts `.NET`/`C#` (`.cs`/`.dll`) compatíveis com `ScriptPortal.Vegas` (Clean Room) via `netcorehost`.
- **`neoveg_ui`** (`crates/neoveg_ui`): interface desktop nativa em `eframe` + `wgpu`, timeline interativa, dock flexível em `egui_tiles` com janelas destacáveis e testes de interação `egui_kittest`, 7 temas visuais (incluindo 2 temas de Alto Contraste WCAG AAA), gerenciador de layouts `.neoveglayout` e editores de atalhos e preferências.
- **`neoveg`** (`crates/neoveg`): binário principal e interface CLI com inicialização do pool `rayon` proporcional ao hardware.

## Recursos Implementados

- **Timeline, Áudio e Edição VEGAS Pro:** múltiplas trilhas A/V, mixer com até 26 barramentos auxiliares (`Bus A`..`Bus Z`) + *Master Bus* (`Mono`/`Dim -20 dB`, envios *pre/post-fader* e roteamento acíclico), DSP com `fundsp`, auto-crossfade ao sobrepor eventos na mesma faixa, alças de fade nos cantos superiores, alça de ganho em dB no topo de eventos de áudio, playhead físico de `1 px`, controle contínuo de taxa (*Playback Rate* com reamostragem *Sinc* via `rubato`), divisão (`S`), marcações (`M`), regiões (`R`), *Auto Ripple* (`Ctrl+L`), encaixe (`F8`) e *scrub* (`J`/`K`/`L`).
- **Tabela Única de Comandos, Atalhos e Scripting `.NET`:** menus, barra de ferramentas, atalhos e comandos CLI despacham pelo mesmo `id`; atalhos personalizados podem ser gravados/limpos em *Ajuda → Atalhos de teclado*, e scripts `.cs`/`.dll` (`ScriptPortal.Vegas` Clean Room) podem ser executados sob demanda via *Ferramentas → Executar script (.cs/.dll)...* (`tools.run_script`) ou `--run-script`.
- **Personalização e Acessibilidade:** 7 temas selecionáveis em *Exibir → Tema* ou *Opções → Preferências...* (incluindo *Alto Contraste AAA Escuro* e *Claro* com contraste >= 7:1), **Editar → Propriedades → Layout** com os três tipos clássicos de disposição (*Sonic Foundry*, *Sony* e *MAGIX* padrão VEGAS Pro 21 em 3 colunas com **Prévia de Vídeo**, **Master Bus** dedicado e abas na base via `egui_tiles`), **Tela de abertura (Splash Screen)** (`crates/neoveg_ui/src/splash.rs`), **Tela de boas-vindas (Welcome Screen)** estilo VEGAS com navegação lateral (*Novo Projeto*, *Abrir Projeto* com **Projetos recentes** persistidos, *Primeiros Passos*), banner vetorial original de câmera, seletor de 5 proporções (`16:9`, `4:3`, `9:16`, `1:1`, `2.4:1`) e combos de resolução/taxa de quadros + *Drop Frame* (`crates/neoveg_ui/src/welcome.rs`), painel acoplável **Color Grading** ligado ao `FxKind::ColorCorrector` (`crates/neoveg_ui/src/color_grading.rs`), janela **Renderizar como...** em 2 colunas com modelos e favoritos, 57 ícones vetoriais nativos coloridos (`crates/neoveg_ui/src/icons.rs`), barra inferior de transporte/edição com seleção `In`/`Fim`/`Tam`, painéis acopláveis/destacáveis em janelas nativas e 4 presets + arquivos `.neoveglayout` salvos pelo usuário.
- **Formatos Suportados:**
  - **Rust puro (multiplataforma):** projetos `.neoveg` (e legado `.nvp`), intercâmbio OpenTimelineIO `.otio` (Clean Room, esquema 0.17: *Arquivo → Importar/Exportar OpenTimelineIO (.otio)...* e arrastar-e-soltar), `AVI (MJPEG/PCM)`, `Y4M`, `GIF`, sequências de imagens (`PNG`, `JPEG`, `BMP`, `TIFF`, `WebP`), áudio `WAV`/`AIFF` (leitor próprio), `FLAC` (`claxon`), `Ogg Vorbis` (`.ogg`, `.oga`, `lewton`) e `MP3` (`puremp3`).
  - **Windows nativo (Media Foundation):** importação e decodificação em streaming de vídeo e áudio em `MP4`, `MOV`, `MKV`, `WEBM`, `M4V`, `M4A`, `AAC`, `WMV` e `ASF`. (`ffmpeg`/`ffprobe` são utilizados exclusivamente como oráculos de teste externos e geradores de fixtures em `target/fixtures/`, nunca linkados nem distribuídos.)
- **Hardware Limitado:** matriz de prévia 4×3 (*Rascunho / Prévia / Boa / Melhor* × *1/4 / 1/2 / Completa*), geração de proxies `.avi`, pré-renderização seletiva de região, limite configurável de RAM/quadros em cache e compilação portátil sem `-C target-cpu=native`.

## Como Executar

```bash
# Abrir interface gráfica (opcionalmente passando um projeto ou mídia)
cargo run -p neoveg
cargo run -p neoveg -- meu_projeto.neoveg

# Interface de linha de comando (CLI)
cargo run -p neoveg -- --list-commands
cargo run -p neoveg -- --command insert.marker meu_projeto.neoveg atualizado.neoveg
cargo run -p neoveg -- --run-script meu_script.cs
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
