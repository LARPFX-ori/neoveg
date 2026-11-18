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

## Fase 3 — Paridade Operacional VEGAS Pro, Decodificação Nativa do SO e Hardware Limitado *(Concluída)*

- **Invariantes de edição (§3.1):** movimentação atômica rejeitando IDs duplicados (`apply_move_plan` / `build_move_plan_from_selection`), propagação de formato do primeiro arquivo somente se o usuário não configurou o projeto manualmente (`manually_configured`).
- **Hardware limitado (§3.6):**
  - Resoluções escalonáveis de pré-visualização (*Rascunho / Prévia / Boa / Melhor*);
  - Limite configurável de RAM para prévia e cache de quadros em bytes;
  - Geração assíncrona de proxies `.avi` em baixa resolução e pré-renderização seletiva de região;
  - Pool global do `rayon` dimensionado pelos núcleos da máquina (`Preferences.effective_worker_threads()`).
- **Áudio em Rust puro com licenças SPDX permitidas (`media/{audio,audio_extra,wav}.rs`):** leitores/escritores próprios para `WAV`/`BWF`/`RF64` (PCM, Float32, G.711 µ-law/A-law, IMA ADPCM), `AIFF`/`AIFC`, `AU`/`SND`, `CAF`, `Wave64` (`.w64`), `VOC`, `IFF 8SVX` e `QOA`; além de `FLAC` (`claxon`, `Apache-2.0`), Ogg Vorbis (`lewton`, `MIT OR Apache-2.0`) e `MP3` (`puremp3`, `MIT OR CC0-1.0`), com verificação opcional via oráculo externo `ffprobe` em `target/fixtures/` (sem dependências `MPL-2.0`/`GPL`/`LGPL`).
- **Imagens e sequências em Rust puro (`media/img_extra.rs`, `image` 0.25):** importação de 31 extensões incluindo `DPX`, `Cineon`, `OpenEXR`, `QOI`, `TGA`, `PNM`, `HDR`, `DDS`, `ICO`/`CUR`, `Farbfeld`, `WBMP`, `XBM`, `XPM`, `IFF/ILBM` e `WebP`/`GIF` animados; exportação de `GIF` animado, 10 sequências de imagem e capturas multi-formato.
- **Contrato de decodificador do sistema (`media/system.rs`, `SystemVideoReader` / `VideoSource::System`):** abstração comum em `neoveg_engine::media` desacoplada do Windows para leitores nativos e fallback multiplataforma, implementada no Windows pelo backend Media Foundation (`media/mf/mod.rs` e `encode.rs`, fronteira FFI isolada com RAII e `// SAFETY:`) para leitura em streaming de `MP4`, `MOV`, `MKV`, `WEBM`, `M4V`, `M4A`, `AAC`, `WMV`, `ASF`, `WMA`, `3GP`, `3G2`, `M2TS`/`MTS`/`TS`, `MPEG`/`VOB`, `MP2`/`MPA`, `M4B` e `ALAC`, além de codificação via `IMFSinkWriter` para `MP4 (H.264/HEVC)`, `MOV`, `3GP`, `WMV`, `M4A`, `AAC`, `WMA` e `MP3`.
- **Experiência VEGAS Pro em `neoveg_ui`:**
  - **Fontes do sistema na interface** como no VEGAS Pro (Segoe UI no Windows, Consolas nos campos monoespaçados), lidas em tempo de execução e com opção de voltar às embutidas (`crates/neoveg_ui/src/fonts.rs`), e **Escala da interface** de 50% a 300% em *Exibir* e nas Preferências;
  - **7 temas visuais** incluindo 2 temas de **Alto Contraste WCAG AAA** (>= 7:1) e **107 ícones vetoriais coloridos nativos** (`crates/neoveg_ui/src/icons.rs`) desenhados em `egui::Painter` com adaptação automática ao Alto Contraste;
  - Janelas **Trimmer** (`Alt+2`, pré-visualização A/V de mídia, marcação `In`/`Out` com navegação quadro a quadro, seleção *Áudio + Vídeo* / *Apenas Vídeo* / *Apenas Áudio* e envio com Desfazer) e **Detalhes da Edição (*Edit Details*, `Alt+3`)** tabular com edição em linha (*Eventos*, *Marcadores*, *Regiões*, *Trilhas*, *Barramentos*), filtro e cópia TSV;
  - Playhead físico de **1 px** contrastado ao tema e ao Auto Ripple, **Linha do Playhead** com cabeçalho no topo, **retângulos nos números das faixas**, **Visor de Tempo** dedicado acima da lista de faixas (alinhado à régua e marcadores), controle **Taxa (`Playback Rate`)** + botões de trilha abaixo das faixas, **barras de rolagem clássicas com setas (`◀`/`▶`, `▲`/`▼`), botões de zoom horizontal e altura de trilhas (`−`/`+`) e ajuste ao projeto** na timeline e **Barra de Status de Renderização** na base da timeline com faixa indicadora de pré-render no topo;
  - Painéis da área acoplável, **Prévia de Vídeo** e **Master Bus** destacáveis em **janelas nativas do SO/Windows** (`show_viewport_immediate`, com fallback embutido em testes headless) e layouts `.neoveglayout` (4 presets + salvamento/carregamento);
  - **Editar > Propriedades > Layout** responsivo (adaptável a monitores pequenos/grandes) com seção **Tipos de layout** (*Clássico*, com a timeline no topo; *Intermediário*, com a área acoplável no topo; e *Atual*, o padrão: área acoplável em 3 colunas no topo com abas na base — *Painéis | Prévia de Vídeo com transporte central | Master Bus dedicado com `Mono`/`Dim -20 dB`/Mute/FX* —, timeline central, barra de transporte/edição + caixas `In`/`Fim`/`Tam` e barra de status);
  - **Tela de abertura (Splash Screen)** nativa com arte vetorial original e spinner compartilhado (`crates/neoveg_ui/src/splash.rs`), acompanhamento da inicialização em janela própria sem bordas (a janela do programa só aparece depois), atalho *Ajuda → Tela de abertura* (`help.splash`) e opção persistida em *Preferências...* (`show_splash`);
  - **Tela de boas-vindas (Welcome Screen)** (`crates/neoveg_ui/src/welcome.rs`) com navegação lateral (*Novo Projeto*, *Abrir Projeto* com lista persistida de `recent_projects` e recuperação, *Primeiros Passos*), banner vetorial original de câmera, 5 botões de proporção (*Widescreen 16:9*, *Fullscreen 4:3*, *Portrait 9:16*, *Square 1:1*, *Scope 2.4:1*), combos de resolução, taxa de quadros e *Drop Frame*, *Configurações Avançadas...*, *Ajuda → Tela de boas-vindas* (`help.welcome`) e opção `show_welcome`;
  - Painel **Color Grading** (`crates/neoveg_ui/src/color_grading.rs`) acoplável/flutuante com rodas tonais vetoriais (*Lift*, *Gamma*, *Gain*), pad *Temp × Tinta*, sliders e predefinições ligados ao `FxKind::ColorCorrector`;
  - Janela **Renderizar como... em 2 colunas** com pesquisa, favoritos ★ persistidos, modelos de formato e progresso com spinner vetorial original;
  - **Neoveg Encoder (`neoveg_engine::encoder`, `encoder_panel.rs`):** fila em lote persistente com codificação paralela de múltiplas saídas por origem, painel *Codificação* ao vivo com miniatura e ETA, *Navegador de Predefinições* (`.neovegpreset`, grupos e apelidos), *Pastas Monitoradas*, *Configurações de Exportação* com Corte/Escala em 5 modos, LUT `.cube` 1D/3D em sala limpa, sobreposições (Imagem, Nome e Timecode) e Ajustador de Tempo;
  - Janela dinâmica de **Atalhos de teclado** (regravação, remoção e filtro ligados à Tabela Única) e diálogo **Preferências...** em 2 painéis (barra lateral com 9 categorias: *Geral*, *Aparência e Layout*, *Edição e Timeline*, *Vídeo e Prévia*, *Áudio*, *Memória e Desempenho*, *Salvamento Automático*, *Atalhos e Controles* e *Aplicativos externos*, com rolagem contínua/por página, comportamento da roda do mouse, guias de Área Segura configuráveis e parâmetros persistidos com sanitização).
- **Dock `egui_tiles` e testes de UI `egui_kittest`:** árvore de painéis 3 colunas com faixa de abas inferior em linha única rolável (`◀`/`▶` + barra horizontal), painéis com barras de rolagem clássicas com setas (`▲`/`▼` / `◀`/`▶`), arraste de abas com 5 alvos de encaixe (meio junta, lados dividem; soltar fora destaca), *Maximizar/Restaurar* painel, *Fechar painel* no menu de qualquer aba (volta pelo menu Exibir), layout novo com só *Mídia do Projeto*, *Transições*, *Efeitos de Vídeo*, *Geradores de Mídia* e *Plug-In Manager* à esquerda e o *Trimmer* no grupo da Prévia, `F11` / `Shift+F11` / `Ctrl+F11` (`dock_drop.rs`), destaque em janelas nativas e suíte automatizada de interação (`kittest_tests.rs`).
- **Áudio avançado (`fundsp` + `rubato` + `ebur128` + `dasp`):** até 26 barramentos auxiliares (`Bus A`..`Bus Z`) com envios *pre/post-fader*, roteamento acíclico no cabeçalho e Mixer, envelopes Volume/Pan, trilhas de barramentos na timeline (`Ctrl+B`), DSP nativo via `fundsp`, reamostragem *Sinc* para *Playback Rate* (`0.25x`–`4.00x`) com `rubato` e **Medidores de Loudness EBU R128 / ITU-R BS.1770-4** (`Ctrl+Alt+7`, tempo real + varredura offline com metas R128/Streaming/ATSC/Podcast).
- **Intercâmbio aberto em Sala Limpa (`neoveg_engine/src/{otio,fcpxml,edl,subtitles}.rs`):** importação e exportação via **OpenTimelineIO (`.otio`)**, **Final Cut Pro XML (`.fcpxml`)**, **EDL CMX 3600 (`.edl`)** e **legendas SubRip (`.srt`) / W3C WebVTT (`.vtt`)**, integrados à Tabela Única de Comandos (`Arquivo → Intercâmbio & Legendas`) e arrastar-e-soltar, com suíte de fuzzing por mutação (`AGENTS.md` §0.8).
- **Scripting `.NET` sob demanda (`netcorehost`):** carregamento isolado do host `.NET`/CLR (`Hostfxr`) em `neoveg_scripting` com ponte *Clean Room* `ScriptPortal.Vegas` (`NeovegBridge`), hospedagem de assemblies `.dll` externos em `AssemblyLoadContext` coletável (sem bloqueio de arquivo no Windows) com mutação atômica em passo único de Desfazer, execução via *Ferramentas → Executar script (.cs/.dll)...* e CLI `--run-script`. A API cobre projeto, trilhas, eventos, marcadores, cursor, seleção, mídia do projeto (`MediaPool`, `Media`) e takes (`Takes`, `ActiveTake`, `AddTake`), cria eventos com `AddVideoEvent`/`AddAudioEvent` e importa mídia nova pelo script (`MediaPool.AddMedia`, `new Media(path)`).
- **Empacotamento portátil (`build_portable.py`):** build `--release` portátil sem `target-cpu=native` gravada em `dist/` na raiz, com auditoria contra assets/fontes/ferramentas não redistribuíveis e os catálogos `i18n/*.json`; depois de conferir a cópia, a pasta de build do cargo é apagada (mantida se algo falhar, `AGENTS.md` §3.4).
- **Internacionalização completa (`i18n`):** suporte a 6 idiomas (`pt-BR` padrão, `en`, `es`, `fr`, `de`, `ja`) em `i18n/*.json` (carregamento embutido com sobreposição externa opcional via `NEOVEG_I18N_DIR` / `i18n/`), seletor ao vivo em **Preferências → Geral** e **Boas-vindas**, `--lang <código>` / `NEOVEG_LANG` na CLI e carregamento seguro de fallback CJK do sistema operacional em tempo de execução.

## Fase 4 — Próximos Marcos *(Em andamento)*

- **Concluído:**
  - **Acoplar arrastando** uma janela destacada para a área de janelas, com contorno de destino e a preferência *Permitir que janelas flutuantes acoplem* (`Ctrl` inverte, manual do VEGAS Pro 21, pp. 42-43; `float_dock.rs`), e os atalhos alternativos ``Alt+` `` / ``Ctrl+Alt+` `` / ``Shift+Alt+` `` (= `F11` / `Ctrl+F11` / `Shift+F11`, também pela tecla física no ABNT2);
  - **Efeitos e envelopes na API de scripts** (`ScriptPortal.Vegas`, nomes da *VEGAS Pro Scripting API Summary*): `Track.Effects`, `VideoEvent.Effects`/`AudioEvent.Effects`, `Effect` (`Bypass`, `PlugIn`, `Description`, `Index`), `Effects.AddEffect`, árvores `Vegas.VideoFX`/`AudioFX`/`PlugIns` com `PlugInNode.FindChildByName`/`FindChildByUniqueID`, `Track.Envelopes` (`HasEnvelope`, `FindByType`), `Envelope` (`Points`, `Min`/`Max`/`Neutral`, `ValueAt`), `EnvelopePoint` e `CurveType`; validados ao aplicar (protocolo 5 da ponte, `vegas_cs.rs`);
  - **Janelas destacadas lembram posição e tamanho** (Prévia, Master Bus e painéis): a janela reaberta volta ao último lugar na sessão e o `.neoveglayout` salvo leva e traz as posições, saneadas contra valores não finitos e fora de faixa (`layout.rs`, `panels.rs`);
  - **Travar envelopes nos eventos** (*Opções → Travar envelopes nos eventos*, `options.lock_envelopes`, pp. 389, 710 e 1014): arrastar eventos leva junto os pontos de envelope da trilha e os quadros-chave de Track Motion, de movimento do pai e dos efeitos da trilha no trecho de cada evento, recalculados a partir de uma foto do começo do arraste (`env_lock.rs`);
  - *Surround Panner* 5.1 (`engine/surround.rs`, painel `surround_panel.rs`): modo do Master estéreo/5.1, posição 2D por trilha e barramento no anel ITU-R BS.775, LFE, downmix para monitoração e render WAV de 6 canais (`WAVE_FORMAT_EXTENSIBLE`);
  - **Takes múltiplos por evento** (`takes.rs`): adicionar como take, próximo/anterior (`T`/`Shift+T`), escolher/excluir o ativo e *Slip em todos os takes*, com divisão e aparo sincronizados;
  - Importação e exportação de **Final Cut Pro 7 XML** (`xmeml`, `fcp7xml.rs`) e **AAF** (`.aaf`, `aaf.rs`, `aaf_metadict.rs`, `file.import_aaf` / `file.export_aaf` em Sala Limpa);
  - *Adjustment Track*, menu `More (...)`, painéis *Plug-In Manager* e *Compositores*;
  - Descoberta de plug-ins VST3/OFX nas pastas padrão e em **pastas mapeadas pelo usuário** (ex.: uma pasta do Google Drive, salvas nas Preferências e varridas de novo ao reabrir; cada pasta procura VST3 e OFX, com estado por pasta e *Localizar...* quando ela some; varredura fora da thread da interface) (`plugins.rs`, `plugin_manager.rs`) e efeitos de vídeo OFX aplicados a eventos pelo host isolado `neoveg --plugin-host` (`plugin_host.rs`, `plugin_host/ofx.rs`);
  - Efeitos OFX com **keyframes de parâmetros** (linear; Boolean/Choice seguram o valor; Integer arredonda), avaliação completa de curvas no host (`getValueAtTime`, `getDerivative`, `getIntegral`, consultas `getNumKeys`/`getKeyTime`/`getKeyIndex`), clipes opcionais desconectados no contexto *General* e tempo na **taxa de quadros do projeto**;
  - **Host VST3 para efeitos de áudio** (`plugin_host/vst3.rs`, VST 3 SDK 3.8 sob MIT pelos bindings `vst3`): efeitos aplicados a eventos, trilhas, barramentos e Master pelo subprocesso `neoveg --plugin-host serve-vst3`, uma instância por mixer e por efeito, abertura assíncrona na reprodução (o sinal passa seco até o plug-in abrir) e síncrona no render, tempo limite de 2 s por bloco com recriação do host, **persistência do estado completo (`IComponent::getState`/`setState`)**, **compensação automática de latência (PDC)** entre ramos paralelos e pré-roll no render offline/loudness, e editor de parâmetros normalizados no diálogo de FX;
  - **Integração com a suíte ArtCraft e o Blender** (`integrations/` e `integrations_ui.rs`): descoberta automática e caminhos manuais (`Preferências > Aplicativos externos`) para **PhotoCraft**, **VectorCraft**, **FilmCraft**, **EffectCraft**, **AniCraft**, **SoundCraft** e **Blender**, canal de controle JSON-lines em loopback (`127.0.0.1`) com desafio HMAC-SHA256 em Rust puro, envio de cópias editáveis (*Add As Takes* + BWF `bext` com `TimeReference` + SVG limpo + OTIO), renderização headless de `.anicraft`, `.ecproj` e `.blend` em sequências PNG direto para a timeline e ida-e-volta de câmera/marcadores Blender (`.py` / `.json`);
- **Pendente:**
  - **Backends nativos Linux e macOS:** contrato comum `SystemVideoReader` (`media/system.rs`) e leitor/demuxer de contêineres MP4/MOV/MKV/WEBM em Rust puro (`media/demux/`, ISO/IEC 14496-12, QTFF, RFC 8794, RFC 9559) concluídos; faltam as bridges FFI isoladas para VA-API (Linux; sem GStreamer, que é LGPL — `AGENTS.md` §2.3) e VideoToolbox/AVFoundation (macOS) + decodificador fallback em Rust puro sem dependência do SO. Os portões de CI em macOS e Linux estão no PR #12.
  - **Plug-ins externos restantes:** OFX com tiles e múltiplas entradas em contexto *General*; VST3 com editor gráfico do plug-in (`IPlugView`), automação de parâmetros em tempo real e instrumentos (MIDI).
  - **API de scripts:** parâmetros e quadros-chave dos efeitos (`Effect.Keyframes`, presets), efeitos OFX/VST3 criados por script e envelopes de evento.
  - **Quadros-chave de trilha:** curva por quadro-chave nos parâmetros de efeito (manual do VEGAS Pro 21, p. 389).
  - **Área de janelas:** docas flutuantes com várias abas na mesma janela (arrastar abas entre janelas nativas não existe no egui).

## Paridade de UI/UX com o manual do VEGAS Pro 21

Levantamento feito capítulo por capítulo no manual do usuário do VEGAS Pro 21 e conferido no código do `main`. As páginas são as do manual. Cada item entrou pela interface do Neoveg, com nomes e desenho próprios (`AGENTS.md` §1 e §2). Todos os itens abaixo estão implementados; o que ainda falta está em *Fase 4 → Pendente*.

### Mídia do Projeto

- **Pastas de mídia (*bins*)** com hierarquia, arrastar-e-soltar e busca + **pastas inteligentes** (*smart bins* embutidas e personalizáveis, pp. 144 e 172) implementadas em `model.rs`, `session.rs` e `project_media.rs`.
- **Modos de exibição** (**Miniaturas**, **Lista**, **Detalhes** ordenáveis com contagem de uso e **Storyboard**, p. 142) e **scrub ao passar o mouse** (`thumbnail_at`) com linha de tempo e timecode ao vivo (p. 135).
- **Substituir e trocar mídia** mantendo eventos, takes e propriedades (`replace_media_item` e `swap_media_references`, pp. 137 e 144).
- **Selecionar todos os eventos que usam uma mídia** (`media.select_events`, p. 144).
- **Storyboards** com sequência ordenável, cálculo de duração e **Inserir storyboard na timeline** em passo único de Desfazer (`media.insert_storyboard`, p. 155).

### Prévia de Vídeo

- **Prévia em tela dividida** (`PreviewSplitMode::FxBypass` e `Clipboard`, linha divisória arrastável horizontal/vertical, p. 203).
- **Prévia dinâmica em RAM** (`Shift+B`, `RamPreviewCache` limitado ao orçamento de memória das Preferências, p. 207).
- **Zoom e deslocamento dentro da prévia** (roda do mouse em passos de 10% e `Ctrl`+roda em 1%, menu de níveis `Ajustar`–`400%`, `+5%`/`-5%` e *Ignorar zoom* — `preview.zoom_step_in`/`zoom_step_out`/`zoom_bypass` —, arraste de enquadramento, pp. 208–209).
- **Sobreposições** (botão + seta ▾, pp. 197–198): áreas de ação e de título, **grade** com divisões horizontais e verticais nas Preferências, guias (cruz, terços, recortes 4:3/9:16/1:1), **réguas** em pixels do projeto e **isolamento de canal** (vermelho, verde, azul, alfa em cinza, opção em escala de cinza; os escopos continuam com a imagem real) (`crates/neoveg_ui/src/preview_overlays.rs`).
- **Barra de transporte da prévia** completa (loop, tocar do início, tocar/pausar, parar, início, fim, quadro anterior/seguinte, pp. 201–202; em largura estreita só o essencial) e **fps exibido** na linha *Exibição* durante a reprodução (p. 197).
- **Copiar o quadro atual para a área de transferência** (`preview.copy_frame`, imagem RGBA para SO e comparação dividida, p. 200), além de salvar em PNG (`file.snapshot`).
- **Silenciar todo o áudio ou todo o vídeo** da prévia (`preview.mute_audio` / `preview.mute_video`, p. 200).
- **Monitor externo** em janela secundária destacada (`Alt+Shift+4`, tela cheia opcional, p. 210).
- **Soltar efeitos na prévia** aplicando-os à saída mestre de vídeo (`MasterVideo` Video Output FX, p. 55).

### Edição na linha do tempo

- **Seletor de ferramenta de edição** na barra de transporte (p. 236):
  - ferramenta **Normal** com os 7 modos na interface e no motor (**Normal**, **Shuffle**, **Slip**, **Slide**, **Stretch**, **Split Trim** e **Roll**), ícone dinâmico no seletor e modificadores diretos (`Alt`, `Ctrl+Alt`, `Ctrl+borda`, `Ctrl+Alt+Shift`);
  - ferramentas **Envelope**, **Seleção** (caixa) e **Zoom** (caixa e clique), além de `D`/`Shift+D` e seletor de **eixo Livre / Horizontal / Vertical** na barra, no menu e no menu de contexto (pp. 239–240).
- **Smart Split (`Alt+S`)**, **Juntar eventos (`Heal`)**, **Inserir evento vazio** (`insert.empty_event`, p. 243) e **Exibição da duração nos cabeçalhos dos eventos** (p. 329) implementados.
- **Vínculos de sincronia (*Sync Links*, p. 351):** criar vínculo unidirecional (`event.sync_link_create`), selecionar rede (`event.sync_link_select`), desvincular (`event.sync_link_unlink`), realce visual e proteção contra ciclos implementados.
- **Colar atributos do evento** (`Ctrl+Alt+V`) e **Colar atributos seletivamente...** (p. 369) implementados (inclusive **Tom / Pitch**).
- **Esticar o tempo e mudar o tom (`Time Stretch` / `Pitch Shift`, p. 363):** transposição por semitons (`=`/`-`) e oitavas (`Ctrl+=`/`Ctrl+-`) em `-24..+24 st`, opção *Preservar tom ao esticar* com compensador **WSOLA** em tempo de execução no mixer e coluna *Tom* em *Detalhes da Edição* e *Propriedades do evento* implementados.
- **Diálogo *Take Chooser*** com prévia de quadro, listagem, renomeação, reordenação, adição e exclusão de takes (`event.take_chooser`, p. 347) implementado.
- **Detecção de cenas/cortes** (`edit.detect_scenes`, análise BT.601 + histograma L1 em `engine/scene_detect.rs` para dividir eventos ou inserir marcadores, p. 300) implementada.
- **Modo de edição expandido** (`Ctrl+Shift+E` / duplo clique na borda, prévia dupla A/B Saída | Entrada, deslocamento quadro a quadro `Ctrl+[`/`Ctrl+]` e ripple Out/In, p. 329) implementado.
- **Estabilização de vídeo** (`FxKind::Stabilization`, estimativa de movimento global em sala limpa + zoom de borda sem bordas pretas, p. 341) implementada.
- **Gravação de áudio** nas trilhas armadas (`track.record_arm`, `transport.record`, captura `cpal` em WAV PCM 16 bits + medidor de pico no cabeçalho, p. 821) implementada.

### Trilhas

- **Cabeçalho de trilha** com *Minimizar*/*Maximizar* a altura (clicar de novo volta à altura anterior, p. 388), *Tornar filho de composição*/*Promover* nas trilhas de vídeo e *Armar para gravação* nas de áudio, todos no conjunto de botões visíveis e no botão *Mais* (p. 69).
- **Agrupar trilhas** em pastas recolhíveis (`insert.track_folder` / `track.toggle_folder`, indentação hierárquica e cascata de mudo na timeline e no mixer, p. 382) implementado.
- **Propriedades padrão de trilha** (`track.set_defaults` persistido nas Preferências, p. 414) e **Renderizar para uma nova trilha** (`track.render_to_new_track`, p. 415) implementados.
- **Composição pai/filho e máscaras** (`parent_track_id`, `parent_motion` de grupo no *Movimento da trilha* e modos de máscara por luminância/alfa, p. 740) implementados.
- **Linhas do tempo aninhadas** (`MediaSource::NestedProject` / `insert.nested_timeline` com detecção de ciclos em profundidade limitada e pré-mixagem de áudio, p. 114) implementadas.
- **Edição multicâmera** (`tools.multicam_create`, `tools.multicam_mode` / `Ctrl+Shift+D`, grade ao vivo na Prévia e corte por clique ou teclas `1..9`, p. 416) implementada.
- **Edição 3D estereoscópica** (`Stereo3dMode` — Anáglifo, Lado a lado e Acima/Abaixo — mais disposição e convergência por evento nas Propriedades, p. 426) implementada.

### Efeitos, cor e legendas

- **Pacotes e cadeias de efeitos salvos** para reutilizar (salvar/carregar/exportar `.neovegfxchain`, p. 514) implementado.
- **Presets próprios por efeito** e **miniatura animada ao passar o mouse** nas janelas *Efeitos de Vídeo* e *Geradores de Mídia* (pp. 54–56) implementados.
- **Rastreamento de movimento** planar quadro a quadro para *Pan/Crop* ou *Track Motion* (`tools.motion_track`, p. 713) implementado.
- ***Color Grading* (pp. 769–781):** LUTs integradas e arquivos `.cube` externos com controle de intensidade, **curvas RGB** interativas (*Master/R/G/B*) e **curvas HSL** em 6 setores de matiz, além das rodas tonais, implementados.
- **Inserir legendas a partir das regiões** do projeto (`insert.subtitles_from_regions`, p. 722) implementado.
- **Efeitos de áudio fora de tempo real** (`tools.apply_audio_fx`, p. 940) e **ignorar todos os efeitos de áudio** de uma vez (`options.bypass_all_audio_fx` + botão no Mixer, p. 946) implementados.

### Janela, Automação e Preferências

- **Barra de ferramentas personalizável** (`options.customize_toolbar`, adicionar/remover/reordenar comandos da Tabela Única e separadores persistidos nas Preferências, p. 75) implementado.
- **Grade da régua e Encaixar na grade** (`RulerGridSpacing` — Automático, Quadro, Subdivisões em segundos e Batidas/BPM — mais `options.snap_to_grid` / `Ctrl+F8`, pp. 86, 232) implementado.
- **Marcadores de CD (faixa/índice + exportação `.cue`) e Marcadores de comando** (`insert.marker_cd_track` / `N`, `insert.marker_cd_index`, `insert.marker_command`, pp. 131, 495, 725) implementados.
- **Modos de automação dos envelopes** (*Off / Read / Touch / Latch*, escrita em tempo real durante a reprodução + *bypass* em *Off*, p. 1008) e **borda redimensionável dos cabeçalhos de trilha** (arraste progressivo e duplo clique para recolher/expandir, p. 69) implementados.
- **Área de janelas acopláveis** (pp. 41–44): arrastar uma aba mostra 5 alvos sobre o grupo sob o ponteiro (meio junta, lados dividem) com contorno de onde a janela vai ficar; soltar fora da área destaca a aba numa janela própria; Esc cancela. **Maximizar/Restaurar** um painel na área de janelas (duplo clique na aba ou menu da aba). **`F11`** minimiza/restaura a área de janelas, **`Shift+F11`** a lista de trilhas e **`Ctrl+F11`** maximiza/restaura a timeline (p. 80), pela Tabela Única (`view.toggle_dock_area`, `view.toggle_track_list`, `view.maximize_timeline`). Implementado.
- **Região de loop com setas** nas duas pontas (p. 207) e **guias extras das áreas seguras** (cruz central, grade dos terços e recortes centrais 4:3, 9:16 e 1:1, no botão direito do botão *Áreas de segurança* e nas Preferências) implementadas. O zoom da interface pelo teclado do egui foi desligado: `Ctrl+=` / `Ctrl+-` ficam só para o tom em oitavas.
- **Abas restantes das Preferências** (*Dispositivo de Prévia*, *Dispositivo de Áudio*, *MIDI / Sync / Controle Externo* e *Entrada/Saída File I/O*, pp. 1061–1115) implementadas.
