# Matriz de Paridade VEGAS Pro — Neoveg

Registro factual dos recursos comparados à semântica e ao fluxo operacional do VEGAS Pro, apontando diretamente os módulos implementados no workspace.

| Área | Recurso / Comportamento | Status | Implementação no Código |
| :--- | :--- | :--- | :--- |
| **Tempo & Régua** | Tempo exato inteiro (`Ticks`), FPS racional e SMPTE *Drop-Frame* (`;`) | Implementado | `crates/neoveg_common/src/time.rs` |
| **Timeline** | Múltiplas trilhas A/V, cabeçalhos (`Mute`, `Solo`, `Volume`, `Pan`, `FX`, `Envelopes`, `Lock`) | Implementado | `crates/neoveg_ui/src/timeline.rs` |
| **Timeline** | Divisão (`S`), marcador (`M`), região (`R`), loop (`Q`/`Shift+Q`), Auto Ripple (`Ctrl+L`), Encaixe (`F8`) | Implementado | `crates/neoveg_engine/src/commands.rs`, `session.rs` |
| **Timeline** | Auto-crossfade ao sobrepor eventos na mesma trilha e alças de fade nos cantos superiores | Implementado | `crates/neoveg_engine/src/engine/{audio,video}.rs`, `crates/neoveg_ui/src/timeline.rs` |
| **Timeline** | Alça de ganho em dB na borda superior de eventos de áudio (`Drag::Gain`) | Implementado | `crates/neoveg_ui/src/timeline.rs` |
| **Timeline** | Playhead físico de `1 px` e controle contínuo de *Playback Rate* (`0.25×`–`4.00×`) | Implementado | `crates/neoveg_ui/src/timeline.rs`, `app.rs` |
| **Timeline** | Invariante de movimentação atômica (sem IDs duplicados) e formato pelo 1º clipe | Implementado | `crates/neoveg_engine/src/session.rs` |
| **Vídeo & FX** | Pan/Crop com keyframes, Track Motion 2D/3D, Chroma Key e efeitos/transições nativos | Implementado | `crates/neoveg_engine/src/engine/video.rs`, `crates/neoveg_ui/src/dialogs.rs` |
| **Vídeo & Prévia** | Matriz 4×3 (*Rascunho/Prévia/Boa/Melhor* × *1/4/1/2/Completa*), áreas de segurança e escopos (Hist/Wave/Vec/Parade) | Implementado | `crates/neoveg_common/src/prefs.rs`, `crates/neoveg_engine/src/scopes.rs` |
| **Hardware Limitado** | Orçamento de cache em RAM/quadros, proxies `.avi`, pré-render seletivo de região e threads proporcionais | Implementado | `crates/neoveg_engine/src/media/mod.rs`, `crates/neoveg/src/main.rs` |
| **Áudio & Mixer** | Mixer multifaixa, envelopes de volume/pan, DSP nativo, medidores de pico e saída `cpal` | Implementado | `crates/neoveg_engine/src/engine/{audio,afx}.rs`, `playback.rs`, `panels.rs` |
| **Áudio Avançado** | Até 26 barramentos auxiliares + roteamento completo de buses | Planejado | Estrutura inicial Master em `crates/neoveg_engine/src/model.rs` |
| **UI & Acessibilidade** | Painéis acopláveis/destacáveis em janelas flutuantes + `.neoveglayout` | Implementado | `crates/neoveg_ui/src/{panels,layout,app}.rs` |
| **UI & Acessibilidade** | *Editar → Propriedades → Layout* (`Sonic Foundry`, `Sony`, `MAGIX`) + posição da timeline/abas/transporte | Implementado | `crates/neoveg_common/src/prefs.rs`, `crates/neoveg_ui/src/{app,dialogs,panels}.rs` |
| **UI & Acessibilidade** | 7 temas (incluindo 2 temas Alto Contraste WCAG AAA >= 7:1), 54 ícones vetoriais coloridos nativos (`icons.rs`) e diálogo Preferências | Implementado | `crates/neoveg_ui/src/{theme,icons,dialogs}.rs` |
| **Comandos & Atalhos** | Tabela Única de Comandos (`COMMANDS`) + editor interativo de atalhos persistido | Implementado | `crates/neoveg_engine/src/commands.rs`, `crates/neoveg_ui/src/dialogs.rs` |
| **Codecs & Containers** | AVI (MJPEG/PCM), Y4M, GIF, WAV/AIFF/FLAC/OGG/MP3 em Rust puro | Implementado | `crates/neoveg_engine/src/media/` |
| **Codecs OS** | Windows Media Foundation (MP4/MOV/MKV/WEBM/M4A/AAC) via FFI isolado em streaming | Implementado (Windows) | `crates/neoveg_engine/src/media/mf.rs` |
| **Codecs OS** | Linux VA-API/GStreamer, macOS VideoToolbox e fallback puro H.264/AAC | Planejado | — |
| **Intercâmbio** | EDL CMX 3600, FCP7 XML, FCPXML, OTIO, AAF, SRT/WebVTT | Planejado | — |
| **Scripting & Plugins** | `--run-script`, `--command`, `--list-commands` (`.NET Host` e VST3/OFX sob demanda) | Parcial (CLI ok; `.NET`/VST3/OFX planejados) | `crates/neoveg_scripting/src/lib.rs`, `crates/neoveg/src/main.rs` |
