# Roadmap do Neoveg

Este documento acompanha os marcos técnicos do **Neoveg**, editor de vídeo não-linear em Rust puro com semântica e fluxo de trabalho baseados no VEGAS Pro.

## Fase 1 — Fundação de Confiabilidade, Tempo Exato e Licenciamento *(Concluída)*

- **Política Zero Pânico (`AGENTS.md` §0):** proibição de `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` e `unreachable!` fora de testes; entrada externa tratada como hostil com parsers limitados e testes de fuzzing por mutação.
- **Tempo Exato (§Regras Rust 5):** ticks inteiros (`Ticks`, `FrameRate`, `Rate`) e contagem SMPTE *Drop-Frame* sem aritmética `f64` para edições de quadros/amostras.
- **Licenças e Exceções (`docs/licencas-de-dependencias.md`):** dependências em conformidade com a lista SPDX permitida (`AGENTS.md`), auditadas por `python tools/check_licenses.py` (sem dependências `MPL-2.0`/`GPL`/`LGPL`; `ffmpeg`/`ffprobe` restritos a oráculos de teste externos).

## Fase 2 — Arquitetura de 5 Crates, Tabela Única de Comandos e GPU com Fallback *(Concluída)*

- **Workspace de 5 crates (§3.2):** `neoveg_common`, `neoveg_engine`, `neoveg_scripting`, `neoveg_ui` e binário `neoveg`.
- **Renderização UI (`eframe` + `wgpu` com fallback `glow`):** inicialização com `wgpu` e recuo automático para backend `glow`/software caso não exista adaptador compatível.
- **Tabela Única de Comandos (§3.3):** registro unificado de comandos (`neoveg_engine::commands::COMMANDS`) compartilhado entre menus, barra de ferramentas, atalhos regraváveis e CLI (`--list-commands`, `--command`).
- **Interface CLI completa:** modos `--open`, `--render` (AVI MJPEG/PNG/Y4M/WAV), `--command`, `--list-commands` e `--run-script` (`.NET Host`).

## Fase 3 — Paridade Operacional VEGAS Pro, Decodificação Nativa do SO e Hardware Limitado *(Atual)*

- **Invariantes de edição (§3.1):** movimentação atômica rejeitando IDs duplicados (`apply_move_plan` / `build_move_plan_from_selection`), propagação de formato do primeiro arquivo somente se o usuário não configurou o projeto manualmente (`manually_configured`).
- **Hardware limitado (§3.6):**
  - Resoluções escalonáveis de pré-visualização (*Rascunho / Prévia / Boa / Melhor*);
  - Limite configurável de RAM para prévia e cache de quadros em bytes;
  - Geração assíncrona de proxies `.avi` em baixa resolução e pré-renderização seletiva de região;
  - Pool global do `rayon` dimensionado pelos núcleos da máquina (`Preferences.effective_worker_threads()`).
- **Áudio em Rust puro com licenças SPDX permitidas (`media/audio.rs`):** leitor WAVE/AIFF próprio, FLAC (`claxon`, `Apache-2.0`), Ogg Vorbis (`lewton`, `MIT OR Apache-2.0`) e MP3 (`puremp3`, `MIT OR CC0-1.0`), com verificação opcional via oráculo externo `ffprobe` em `target/fixtures/` (sem dependências `MPL-2.0`/`GPL`/`LGPL`).
- **Media Foundation no Windows (`media/mf.rs`):** fronteira FFI isolada com RAII e `// SAFETY:` para leitura em streaming de vídeo/áudio em MP4, MOV, MKV, WEBM, M4V, M4A, AAC, WMV e ASF.
- **Experiência VEGAS Pro em `neoveg_ui`:**
  - **7 temas visuais** incluindo 2 temas de **Alto Contraste WCAG AAA** (>= 7:1) e **61 ícones vetoriais coloridos nativos** (`crates/neoveg_ui/src/icons.rs`) desenhados em `egui::Painter` com adaptação automática ao Alto Contraste;
  - Janelas **Trimmer** (`Alt+2`, pré-visualização A/V de mídia, marcação `In`/`Out` com navegação quadro a quadro, seleção *Áudio + Vídeo* / *Apenas Vídeo* / *Apenas Áudio* e envio com Desfazer) e **Detalhes da Edição (*Edit Details*, `Alt+3`)** tabular com edição em linha (*Eventos*, *Marcadores*, *Regiões*, *Trilhas*, *Barramentos*), filtro e cópia TSV;
  - Playhead físico de **1 px** contrastado ao tema e ao Auto Ripple, **Linha do Playhead** com cabeçalho no topo, **retângulos nos números das faixas**, **Visor de Tempo** dedicado acima da lista de faixas (alinhado à régua e marcadores), controle **Taxa (`Playback Rate`)** + botões de trilha abaixo das faixas, **barras de rolagem clássicas com setas (`◀`/`▶`, `▲`/`▼`), botões de zoom horizontal e altura de trilhas (`−`/`+`) e ajuste ao projeto** na timeline e **Barra de Status de Renderização** na base da timeline com faixa indicadora de pré-render no topo;
  - Painéis da área acoplável, **Prévia de Vídeo** e **Master Bus** destacáveis em **janelas nativas do SO/Windows** (`show_viewport_immediate`, com fallback embutido em testes headless) e layouts `.neoveglayout` (4 presets + salvamento/carregamento);
  - **Editar > Propriedades > Layout** responsivo (adaptável a monitores pequenos/grandes) com seção **Tipos de layout** (*Sonic Foundry* clássico superior, *Sony* clássico inferior e *MAGIX* padrão VEGAS Pro 21: área acoplável em 3 colunas no topo com abas na base — *Painéis | Prévia de Vídeo com transporte central | Master Bus dedicado com `Mono`/`Dim -20 dB`/Mute/FX* —, timeline central, barra de transporte/edição + caixas `In`/`Fim`/`Tam` e barra de status);
  - **Tela de abertura (Splash Screen)** nativa com arte vetorial original e spinner compartilhado (`crates/neoveg_ui/src/splash.rs`), acompanhamento da inicialização, atalho *Ajuda → Tela de abertura* (`help.splash`) e opção persistida em *Preferências...* (`show_splash`);
  - **Tela de boas-vindas (Welcome Screen)** (`crates/neoveg_ui/src/welcome.rs`) com navegação lateral (*Novo Projeto*, *Abrir Projeto* com lista persistida de `recent_projects` e recuperação, *Primeiros Passos*), banner vetorial original de câmera, 5 botões de proporção (*Widescreen 16:9*, *Fullscreen 4:3*, *Portrait 9:16*, *Square 1:1*, *Scope 2.4:1*), combos de resolução, taxa de quadros e *Drop Frame*, *Configurações Avançadas...*, *Ajuda → Tela de boas-vindas* (`help.welcome`) e opção `show_welcome`;
  - Painel **Color Grading** (`crates/neoveg_ui/src/color_grading.rs`) acoplável/flutuante com rodas tonais vetoriais (*Lift*, *Gamma*, *Gain*), pad *Temp × Tinta*, sliders e predefinições ligados ao `FxKind::ColorCorrector`;
  - Janela **Renderizar como... em 2 colunas** com pesquisa, favoritos ★ persistidos, modelos de formato e progresso com spinner vetorial original;
  - Janela dinâmica de **Atalhos de teclado** (regravação, remoção e filtro ligados à Tabela Única) e diálogo **Preferências...** em 2 painéis (barra lateral com 8 categorias: *Geral*, *Aparência e Layout*, *Edição e Timeline*, *Vídeo e Prévia*, *Áudio*, *Memória e Desempenho*, *Salvamento Automático* e *Atalhos e Controles*, com rolagem contínua/por página, comportamento da roda do mouse, guias de Área Segura configuráveis e parâmetros persistidos com sanitização).
- **Dock `egui_tiles` e testes de UI `egui_kittest`:** árvore de painéis 3 colunas com faixa de abas inferior em linha única rolável (`◀`/`▶` + barra horizontal), painéis com barras de rolagem clássicas com setas (`▲`/`▼` / `◀`/`▶`), destaque em janelas nativas e suíte automatizada de interação (`kittest_tests.rs`).
- **Áudio avançado (`fundsp` + `rubato` + `ebur128` + `dasp`):** até 26 barramentos auxiliares (`Bus A`..`Bus Z`) com envios *pre/post-fader*, roteamento acíclico no cabeçalho e Mixer, envelopes Volume/Pan, trilhas de barramentos na timeline (`Ctrl+B`), DSP nativo via `fundsp`, reamostragem *Sinc* para *Playback Rate* (`0.25x`–`4.00x`) com `rubato` e **Medidores de Loudness EBU R128 / ITU-R BS.1770-4** (`Ctrl+Alt+7`, tempo real + varredura offline com metas R128/Streaming/ATSC/Podcast).
- **Intercâmbio aberto em Sala Limpa (`neoveg_engine/src/{otio,fcpxml,edl,subtitles}.rs`):** importação e exportação via **OpenTimelineIO (`.otio`)**, **Final Cut Pro XML (`.fcpxml`)**, **EDL CMX 3600 (`.edl`)** e **legendas SubRip (`.srt`) / W3C WebVTT (`.vtt`)**, integrados à Tabela Única de Comandos (`Arquivo → Intercâmbio & Legendas`) e arrastar-e-soltar, com suíte de fuzzing por mutação (`AGENTS.md` §0.8).
- **Scripting `.NET` sob demanda (`netcorehost`):** carregamento isolado do host `.NET`/CLR (`Hostfxr`) em `neoveg_scripting` com ponte *Clean Room* `ScriptPortal.Vegas` (`NeovegBridge`), hospedagem de assemblies `.dll` externos em `AssemblyLoadContext` coletável (sem bloqueio de arquivo no Windows) com mutação atômica em passo único de Desfazer, execução via *Ferramentas → Executar script (.cs/.dll)...* e CLI `--run-script`.
- **Empacotamento portátil (`build_portable.py`):** build `--release` portátil sem `target-cpu=native` e auditoria contra assets/fontes/ferramentas não redistribuíveis.

## Fase 4 — Próximos Marcos *(Em andamento)*

- **Concluído:**
  - *Surround Panner* 5.1 (`engine/surround.rs`, painel `surround_panel.rs`): modo do Master estéreo/5.1, posição 2D por trilha e barramento no anel ITU-R BS.775, LFE, downmix para monitoração e render WAV de 6 canais (`WAVE_FORMAT_EXTENSIBLE`);
  - Importação e exportação de **Final Cut Pro 7 XML** (`xmeml`, `fcp7xml.rs`) e importação de **AAF** (`aaf.rs`);
  - *Adjustment Track*, menu `More (...)`, painéis *Plug-In Manager* e *Compositores*;
  - Descoberta de plug-ins VST3/OFX nas pastas padrão (`plugins.rs`) e efeitos de vídeo OFX aplicados a eventos pelo host isolado `neoveg --plugin-host` (`plugin_host.rs`, `plugin_host/ofx.rs`);
  - **Host VST3 para efeitos de áudio** (`plugin_host/vst3.rs`, VST 3 SDK 3.8 sob MIT pelos bindings `vst3`): efeitos aplicados a eventos, trilhas, barramentos e Master pelo subprocesso `neoveg --plugin-host serve-vst3`, uma instância por mixer e por efeito, abertura assíncrona na reprodução (o sinal passa seco até o plug-in abrir) e síncrona no render, tempo limite de 2 s por bloco com recriação do host, e editor de parâmetros normalizados no diálogo de FX;
- **Backends nativos Linux e macOS:** bridges FFI isoladas para VA-API (Linux; sem GStreamer, que é LGPL — `AGENTS.md` §2.3) e VideoToolbox/AVFoundation (macOS) + decodificador fallback em Rust puro sem dependência do SO.
- **Intercâmbio aberto restante:** exportação AAF a partir da especificação pública.
- **Hospedagem de plug-ins externos:** OFX com keyframes de parâmetros, tiles e contexto General completo; VST3 com editor gráfico do plug-in (`IPlugView`), automação de parâmetros, instrumentos (MIDI), estado completo salvo no projeto e compensação de latência.
