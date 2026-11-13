# Roadmap do Neoveg

Este documento acompanha os marcos técnicos do **Neoveg**, editor de vídeo não-linear em Rust puro com semântica e fluxo de trabalho baseados no VEGAS Pro.

## Fase 1 — Fundação de Confiabilidade, Tempo Exato e Licenciamento *(Concluída)*

- **Política Zero Pânico (`AGENTS.md` §0):** proibição de `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` e `unreachable!` fora de testes; entrada externa tratada como hostil com parsers limitados e testes de fuzzing por mutação.
- **Tempo Exato (§Regras Rust 5):** ticks inteiros (`Ticks`, `FrameRate`, `Rate`) e contagem SMPTE *Drop-Frame* sem aritmética `f64` para edições de quadros/amostras.
- **Licenças Permissivas:** substituição integral de dependências por crates com licenças na lista SPDX permitida, auditados por `python tools/check_licenses.py`.

## Fase 2 — Arquitetura de 5 Crates, Tabela Única de Comandos e GPU com Fallback *(Concluída)*

- **Workspace de 5 crates (§3.2):** `neoveg_common`, `neoveg_engine`, `neoveg_scripting`, `neoveg_ui` e binário `neoveg`.
- **Renderização UI (`eframe` + `wgpu` com fallback `glow`):** inicialização com `wgpu` e recuo automático para backend `glow`/software caso não exista adaptador compatível.
- **Tabela Única de Comandos (§3.3):** registro unificado de comandos (`neoveg_engine::commands::COMMANDS`) compartilhado entre menus, barra de ferramentas, atalhos regraváveis e CLI (`--list-commands`, `--command`).
- **Interface CLI completa:** modos `--open`, `--render` (AVI MJPEG/PNG/Y4M/WAV), `--command`, `--list-commands` e ponto de entrada `--run-script`.

## Fase 3 — Paridade Operacional VEGAS Pro, Decodificação Nativa do SO e Hardware Limitado *(Atual)*

- **Invariantes de edição (§3.1):** movimentação atômica rejeitando IDs duplicados (`apply_move_plan` / `build_move_plan_from_selection`), propagação de formato do primeiro arquivo somente se o usuário não configurou o projeto manualmente (`manually_configured`).
- **Hardware limitado (§3.6):**
  - Matriz completa de pré-visualização 4×3 (*Rascunho / Prévia / Boa / Melhor* × *1/4 / 1/2 / Completa*);
  - Limite configurável de RAM para prévia e cache de quadros em bytes;
  - Geração assíncrona de proxies `.avi` em baixa resolução e pré-renderização seletiva de região;
  - Pool global do `rayon` dimensionado pelos núcleos da máquina (`Preferences.effective_worker_threads()`).
- **Media Foundation no Windows (`media/mf.rs`):** fronteira FFI isolada com RAII e `// SAFETY:` para leitura em streaming de MP4, MOV, MKV, WEBM, M4V, M4A, AAC, WMV e ASF.
- **Experiência VEGAS Pro em `neoveg_ui`:**
  - **7 temas visuais** incluindo 2 temas de **Alto Contraste WCAG AAA** (>= 7:1) e **54 ícones vetoriais coloridos nativos** (`crates/neoveg_ui/src/icons.rs`) desenhados em `egui::Painter` com adaptação automática ao Alto Contraste;
  - Playhead físico de **1 px**, alça de ganho em dB na borda superior de eventos de áudio, controle contínuo de taxa (*Playback Rate*);
  - Painéis acopláveis/destacáveis em janelas flutuantes + layouts `.neoveglayout` (4 presets + salvamento/carregamento em arquivo);
  - **Editar > Propriedades > Layout** com seção **Tipos de layout** (*Sonic Foundry*, *Sony* e *MAGIX*) e opções de disposição da janela (timeline superior/inferior, abas no topo/base, barra de transporte e prévia à esquerda);
  - Janela dinâmica de **Atalhos de teclado** (regravação, remoção e filtro ligados à Tabela Única) e janela **Preferências...**.
- **Empacotamento portátil (`build_portable.py`):** build `--release` portátil sem `target-cpu=native` e auditoria contra assets/fontes/ferramentas não redistribuíveis.

## Fase 4 — Próximos Marcos *(Planejado)*

- **Backends nativos Linux e macOS:** bridges FFI isoladas para VA-API/GStreamer (Linux) e VideoToolbox/AVFoundation (macOS) + decodificador fallback em Rust puro sem dependência do SO.
- **Scripting `.NET` (`neoveg_scripting`):** carregamento sob demanda do host .NET Core/CLR para execução de scripts `.cs` compatíveis com `ScriptPortal.Vegas`.
- **Intercâmbio aberto:** importação e exportação de EDL CMX 3600, FCP7 XML, FCPXML, OpenTimelineIO (OTIO) e legendas SRT/WebVTT a partir de especificações públicas.
- **Áudio avançado e plug-ins:** roteamento completo de até 26 barramentos auxiliares + Master e hospedagem isolada de plug-ins VST3 e OpenFX (OFX).
