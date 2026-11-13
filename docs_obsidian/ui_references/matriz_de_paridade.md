# Matriz de Paridade VEGAS Pro — Neoveg

Registro factual dos recursos comparados à semântica e ao fluxo operacional do VEGAS Pro, apontando diretamente os módulos implementados no workspace.

| Área | Recurso / Comportamento | Status | Implementação no Código |
| :--- | :--- | :--- | :--- |
| **Tempo & Régua** | Tempo exato inteiro (`Ticks`), FPS racional e SMPTE *Drop-Frame* (`;`) | Implementado | `crates/neoveg_common/src/time.rs` |
| **Timeline** | Múltiplas trilhas A/V, cabeçalhos (`Mute`, `Solo`, `Volume`, `Pan`, `FX`, `Envelopes`, `Lock`) | Implementado | `crates/neoveg_ui/src/timeline.rs` |
| **Timeline** | Divisão (`S`), marcador (`M`), região (`R`), loop (`Q`/`Shift+Q`), Auto Ripple (`Ctrl+L`), Encaixe (`F8`) | Implementado | `crates/neoveg_engine/src/commands.rs`, `session.rs` |
| **Timeline** | Auto-crossfade ao sobrepor eventos na mesma trilha e alças de fade nos cantos superiores | Implementado | `crates/neoveg_engine/src/engine/{audio,video}.rs`, `crates/neoveg_ui/src/timeline.rs` |
| **Timeline** | Alça de ganho em dB na borda superior de eventos de áudio (`Drag::Gain`) | Implementado | `crates/neoveg_ui/src/timeline.rs` |
| **Timeline** | Cabeçalhos numerados com retângulos delimitados nas faixas A/V e controle contínuo de *Playback Rate* (`0.25×`–`4.00×`) | Implementado | `crates/neoveg_ui/src/timeline.rs`, `app.rs` |
| **Timeline** | **Linha do Playhead** contrastada (com cabeçalho no topo), **Visor de Tempo** dedicado no topo esquerdo acima das faixas, **Taxa (`Playback Rate`)** abaixo das faixas e **Barra de Status de Renderização** + faixa de pré-render | Implementado | `crates/neoveg_ui/src/timeline.rs`, `crates/neoveg_engine/src/session.rs` |
| **Timeline** | Botão `More (...)` personalizável no cabeçalho de faixa, caixas editáveis de comprimento (*Selection Start/End/Length*) e *Adjustment Track* | Planejado | Estrutura de régua e seleção base em `crates/neoveg_ui/src/timeline.rs` |
| **Timeline** | Invariante de movimentação atômica (sem IDs duplicados) e formato pelo 1º clipe | Implementado | `crates/neoveg_engine/src/session.rs` |
| **Vídeo & FX** | Pan/Crop com keyframes, Track Motion 2D/3D, Chroma Key e efeitos/transições nativos | Implementado | `crates/neoveg_engine/src/engine/video.rs`, `crates/neoveg_ui/src/dialogs.rs` |
| **Vídeo & Prévia** | Matriz 4×3 (*Rascunho/Prévia/Boa/Melhor* × *1/4/1/2/Completa*), áreas de segurança e escopos (Hist/Wave/Vec/Parade) | Implementado | `crates/neoveg_common/src/prefs.rs`, `crates/neoveg_engine/src/scopes.rs` |
| **Hardware Limitado** | Orçamento de cache em RAM/quadros, proxies `.avi`, pré-render seletivo de região e threads proporcionais | Implementado | `crates/neoveg_engine/src/media/mod.rs`, `crates/neoveg/src/main.rs` |
| **Áudio & Mixer** | Mixer multifaixa, envelopes de volume/pan, DSP nativo, medidores de pico e saída `cpal` | Implementado | `crates/neoveg_engine/src/engine/{audio,afx}.rs`, `playback.rs`, `panels.rs` |
| **Áudio Avançado** | Até 26 barramentos auxiliares, *Master Bus Window*, *Surround Panner* 5.1 e *Loudness Meters* | Planejado | Estrutura inicial Master em `crates/neoveg_engine/src/model.rs` |
| **UI & Acessibilidade** | Todas as janelas da dock e *Prévia de Vídeo* destacáveis como janelas nativas do Windows + layouts `.neoveglayout` | Implementado | `crates/neoveg_ui/src/{panels,layout,app}.rs` |
| **UI & Acessibilidade** | Janelas VEGAS Pro 21: *Trimmer*, *Edit Details*, *Master Bus Window*, *Plug-In Manager*, *Compositors* e *Surround Panner* | Planejado | Seleção no menu *Exibir* prevista para Fase 4 |
| **UI & Acessibilidade** | *Editar → Propriedades → Layout* (`Sonic Foundry`, `Sony`, `MAGIX` v14–21) responsivo a qualquer monitor | Implementado | `crates/neoveg_common/src/prefs.rs`, `crates/neoveg_ui/src/{app,dialogs,panels}.rs` |
| **UI & Acessibilidade** | Tela de abertura (*Splash Screen*) e spinner vetorial original compartilhado (`spinner_inline`), *Ajuda → Tela de abertura* (`help.splash`) e opção em *Preferências...* | Implementado | `crates/neoveg_common/src/prefs.rs`, `crates/neoveg_engine/src/commands.rs`, `crates/neoveg_ui/src/{splash,app,dialogs}.rs` |
| **UI & Acessibilidade** | Tela de boas-vindas (*Welcome Screen*) com ações rápidas, modelos e projetos recentes persistidos (`help.welcome`, `show_welcome`) | Implementado | `crates/neoveg_common/src/prefs.rs`, `crates/neoveg_engine/src/commands.rs`, `crates/neoveg_ui/src/{welcome,app,dialogs}.rs` |
| **UI & Acessibilidade** | Painel *Color Grading* acoplável/flutuante com rodas tonais vetoriais e pad Temp×Tinta ligado ao `FxKind::ColorCorrector` (`view.tab.color_grading`) | Implementado | `crates/neoveg_engine/src/commands.rs`, `crates/neoveg_ui/src/{color_grading,panels,layout}.rs` |
| **Renderização & Saída** | Diálogo *Renderizar como...* em 2 colunas (formatos/modelos, pesquisa, favoritos ★, resumo e spinner em *Renderizando...*) | Implementado | `crates/neoveg_common/src/prefs.rs`, `crates/neoveg_ui/src/{dialogs,splash,app}.rs` |
| **UI & Acessibilidade** | 7 temas (contraste auditado em temas claros e 2 temas Alto Contraste WCAG AAA >= 7:1), 55 ícones vetoriais coloridos nativos (`icons.rs`) e diálogo *Preferências...* em 2 painéis com 8 categorias (Geral, Aparência/Layout, Edição, Vídeo/Prévia, Áudio, Memória, Autosave e Atalhos) | Implementado | `crates/neoveg_common/src/prefs.rs`, `crates/neoveg_ui/src/{theme,icons,dialogs,app,timeline,panels}.rs` |
| **Comandos & Atalhos** | Tabela Única de Comandos (`COMMANDS`) + editor interativo de atalhos persistido | Implementado | `crates/neoveg_engine/src/commands.rs`, `crates/neoveg_ui/src/dialogs.rs` |
| **Codecs & Containers** | AVI (MJPEG/PCM), Y4M, GIF, WAV/AIFF/ADPCM, FLAC, Ogg Vorbis, MP1/2/3, AAC/ALAC (M4A/AAC), CAF e MKA em Rust puro (`symphonia` + leitor próprio + fallbacks) | Implementado | `crates/neoveg_engine/src/media/` |
| **Codecs OS** | Windows Media Foundation (MP4/MOV/MKV/WEBM/M4A/AAC) via FFI isolado em streaming | Implementado (Windows) | `crates/neoveg_engine/src/media/mf.rs` |
| **Codecs OS** | Linux VA-API/GStreamer, macOS VideoToolbox e fallback puro H.264/AAC | Planejado | — |
| **Intercâmbio** | EDL CMX 3600, FCP7 XML, FCPXML, OTIO, AAF, SRT/WebVTT | Planejado | — |
| **Scripting & Plugins** | `--run-script`, `--command`, `--list-commands` (`.NET Host` e VST3/OFX sob demanda) | Parcial (CLI ok; `.NET`/VST3/OFX planejados) | `crates/neoveg_scripting/src/lib.rs`, `crates/neoveg/src/main.rs` |
