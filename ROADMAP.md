# Roadmap do Neoveg

Este documento acompanha os marcos técnicos do **Neoveg**, editor de vídeo não-linear em Rust puro com semântica e fluxo de trabalho baseados no VEGAS Pro.

## Fase 1 — Fundação de Confiabilidade, Tempo Exato e Licenciamento *(Concluída)*

- **Política Zero Pânico (`AGENTS.md` §0):** proibição de `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` e `unreachable!` fora de testes; entrada externa tratada como hostil com parsers limitados e testes de fuzzing por mutação.
- **Tempo Exato (§Regras Rust 5):** ticks inteiros (`Ticks`, `FrameRate`, `Rate`) e contagem SMPTE *Drop-Frame* sem aritmética `f64` para edições de quadros/amostras.
- **Licenças e Exceções (`docs/licencas-de-dependencias.md`):** dependências na lista SPDX permitida, com exceção autorizada de `MPL-2.0` para o `symphonia`, auditadas por `python tools/check_licenses.py`.

## Fase 2 — Arquitetura de 5 Crates, Tabela Única de Comandos e GPU com Fallback *(Concluída)*

- **Workspace de 5 crates (§3.2):** `neoveg_common`, `neoveg_engine`, `neoveg_scripting`, `neoveg_ui` e binário `neoveg`.
- **Renderização UI (`eframe` + `wgpu` com fallback `glow`):** inicialização com `wgpu` e recuo automático para backend `glow`/software caso não exista adaptador compatível.
- **Tabela Única de Comandos (§3.3):** registro unificado de comandos (`neoveg_engine::commands::COMMANDS`) compartilhado entre menus, barra de ferramentas, atalhos regraváveis e CLI (`--list-commands`, `--command`).
- **Interface CLI completa:** modos `--open`, `--render` (AVI MJPEG/PNG/Y4M/WAV), `--command`, `--list-commands` e `--run-script` (`.NET Host`).

## Fase 3 — Paridade Operacional VEGAS Pro, Decodificação Nativa do SO e Hardware Limitado *(Atual)*

- **Invariantes de edição (§3.1):** movimentação atômica rejeitando IDs duplicados (`apply_move_plan` / `build_move_plan_from_selection`), propagação de formato do primeiro arquivo somente se o usuário não configurou o projeto manualmente (`manually_configured`).
- **Hardware limitado (§3.6):**
  - Matriz completa de pré-visualização 4×3 (*Rascunho / Prévia / Boa / Melhor* × *1/4 / 1/2 / Completa*);
  - Limite configurável de RAM para prévia e cache de quadros em bytes;
  - Geração assíncrona de proxies `.avi` em baixa resolução e pré-renderização seletiva de região;
  - Pool global do `rayon` dimensionado pelos núcleos da máquina (`Preferences.effective_worker_threads()`).
- **Áudio em Rust puro com `symphonia` (`media/audio.rs`):** integração via `https://github.com/pdeljanov/Symphonia.git` (AAC, ALAC, ADPCM, CAF, FLAC, MP1/MP2/MP3, PCM, Vorbis, ISOMP4, MKV, Ogg, RIFF), mantendo leitor WAVE/AIFF próprio, `claxon`/`lewton`/`puremp3` e Windows Media Foundation como fallbacks/complemento.
- **Media Foundation no Windows (`media/mf.rs`):** fronteira FFI isolada com RAII e `// SAFETY:` para leitura em streaming de vídeo/áudio em MP4, MOV, MKV, WEBM, M4V, WMV e ASF (com complemento de áudio via `symphonia`).
- **Experiência VEGAS Pro em `neoveg_ui`:**
  - **7 temas visuais** incluindo 2 temas de **Alto Contraste WCAG AAA** (>= 7:1) e **57 ícones vetoriais coloridos nativos** (`crates/neoveg_ui/src/icons.rs`) desenhados em `egui::Painter` com adaptação automática ao Alto Contraste;
  - Playhead físico de **1 px** contrastado ao tema e ao Auto Ripple, **Linha do Playhead** com cabeçalho no topo, **retângulos nos números das faixas**, **Visor de Tempo** dedicado acima da lista de faixas (alinhado à régua e marcadores), controle **Taxa (`Playback Rate`)** + botões de trilha abaixo das faixas e **Barra de Status de Renderização** na base da timeline com faixa indicadora de pré-render no topo;
  - Painéis da área acoplável, **Prévia de Vídeo** e **Master Bus** destacáveis em **janelas nativas do SO/Windows** (`show_viewport_immediate`, com fallback embutido em testes headless) e layouts `.neoveglayout` (4 presets + salvamento/carregamento);
  - **Editar > Propriedades > Layout** responsivo (adaptável a monitores pequenos/grandes) com seção **Tipos de layout** (*Sonic Foundry* clássico superior, *Sony* clássico inferior e *MAGIX* padrão VEGAS Pro 21: área acoplável em 3 colunas no topo com abas na base — *Painéis | Prévia de Vídeo com transporte central | Master Bus dedicado com `Mono`/`Dim -20 dB`/Mute/FX* —, timeline central, barra de transporte/edição + caixas `In`/`Fim`/`Tam` e barra de status);
  - **Tela de abertura (Splash Screen)** nativa com arte vetorial original e spinner compartilhado (`crates/neoveg_ui/src/splash.rs`), acompanhamento da inicialização, atalho *Ajuda → Tela de abertura* (`help.splash`) e opção persistida em *Preferências...* (`show_splash`);
  - **Tela de boas-vindas (Welcome Screen)** (`crates/neoveg_ui/src/welcome.rs`) com navegação lateral (*Novo Projeto*, *Abrir Projeto* com lista persistida de `recent_projects` e recuperação, *Primeiros Passos*), banner vetorial original de câmera, 5 botões de proporção (*Widescreen 16:9*, *Fullscreen 4:3*, *Portrait 9:16*, *Square 1:1*, *Scope 2.4:1*), combos de resolução, taxa de quadros e *Drop Frame*, *Configurações Avançadas...*, *Ajuda → Tela de boas-vindas* (`help.welcome`) e opção `show_welcome`;
  - Painel **Color Grading** (`crates/neoveg_ui/src/color_grading.rs`) acoplável/flutuante com rodas tonais vetoriais (*Lift*, *Gamma*, *Gain*), pad *Temp × Tinta*, sliders e predefinições ligados ao `FxKind::ColorCorrector`;
  - Janela **Renderizar como... em 2 colunas** com pesquisa, favoritos ★ persistidos, modelos de formato e progresso com spinner vetorial original;
  - Janela dinâmica de **Atalhos de teclado** (regravação, remoção e filtro ligados à Tabela Única) e diálogo **Preferências...** em 2 painéis (barra lateral com 8 categorias: *Geral*, *Aparência e Layout*, *Edição e Timeline*, *Vídeo e Prévia*, *Áudio*, *Memória e Desempenho*, *Salvamento Automático* e *Atalhos e Controles*, com rolagem contínua/por página, comportamento da roda do mouse, guias de Área Segura configuráveis e parâmetros persistidos com sanitização).
- **Dock `egui_tiles` e testes de UI `egui_kittest`:** árvore de painéis 3 colunas com destaque em janelas nativas e suíte automatizada de interação (`kittest_tests.rs`).
- **Áudio avançado (`fundsp` + `rubato`):** até 26 barramentos auxiliares (`Bus A`..`Bus Z`) com envios *pre/post-fader* e roteamento acíclico para o *Master Bus*, DSP nativo via `fundsp` e reamostragem *Sinc* para *Playback Rate* (`0.25x`–`4.00x`) com `rubato`.
- **Intercâmbio OpenTimelineIO (`.otio`):** parser e exportador *Clean Room* (esquema OTIO 0.17) em `neoveg_engine/src/otio.rs`, comandos `file.import_otio`/`file.export_otio` e importação via arrastar-e-soltar.
- **Scripting `.NET` sob demanda (`netcorehost`):** carregamento isolado do host `.NET`/CLR (`Hostfxr`) em `neoveg_scripting` com ponte *Clean Room* `ScriptPortal.Vegas` (`NeovegBridge`), hospedagem de assemblies `.dll` externos em `AssemblyLoadContext` coletável (sem bloqueio de arquivo no Windows) com mutação atômica em passo único de Desfazer, execução via *Ferramentas → Executar script (.cs/.dll)...* e CLI `--run-script`.
- **Empacotamento portátil (`build_portable.py`):** build `--release` portátil sem `target-cpu=native` e auditoria contra assets/fontes/ferramentas não redistribuíveis.

## Fase 4 — Próximos Marcos *(Planejado)*

- **Backends nativos Linux e macOS:** bridges FFI isoladas para VA-API (Linux; sem GStreamer, que é LGPL — `AGENTS.md` §2.3) e VideoToolbox/AVFoundation (macOS) + decodificador fallback em Rust puro sem dependência do SO.
- **Intercâmbio aberto restante:** importação e exportação de EDL CMX 3600, FCP7 XML, FCPXML, AAF e legendas SRT/WebVTT a partir de especificações públicas.
- **Áudio espacial, medição e plug-ins:** *Surround Panner* 5.1, *Loudness Meters* e hospedagem isolada de plug-ins VST3 e OpenFX (OFX).
