# Neoveg

**Português (BR)** | [English](README.en.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [日本語](README.ja.md)

Editor de vídeo não-linear (NLE) escrito em **Rust puro**, com interface nativa multilíngue (`pt-BR` padrão, `en`, `es`, `fr`, `de` e `ja`; `eframe` + `wgpu` com fallback automático para `glow`), semântica operacional inspirada no VEGAS Pro e política estrita de **zero pânico em produção**.

## Testou o Neoveg? Conte como foi

Se você instalou ou usou o Neoveg, responda o **[formulário para quem testou o Neoveg](https://docs.google.com/forms/d/e/1FAIpQLScyRCZRUxQ48eokch70fh8RRVbK2zXDYoDH9zeRM06bm7OyjA/viewform)**. As respostas ajudam a decidir o que corrigir e o que fazer primeiro. Erros com passos para reproduzir também podem ir para as *Issues* do repositório.

Outros idiomas: [English](https://docs.google.com/forms/d/e/1FAIpQLScsDpUJgDH_hfs2KsD5iO-TcgOLnCiZQvpi2jMNGabmddLCUA/viewform) | [Español](https://docs.google.com/forms/d/e/1FAIpQLSdpY1-si_e8qwGT9Q3eOkcn9LvvqxQaY1S_HsXk5wJewJi6bw/viewform) | [Français](https://docs.google.com/forms/d/e/1FAIpQLSdXkSbZPAGymZRSHzz1gRUAaw-kc9IjmlFVU7pFQCYtLvv-7w/viewform) | [Deutsch](https://docs.google.com/forms/d/e/1FAIpQLSf3El0zy7Tc-5EKrlebeuC3KN6BATtO9KnJyTr6F5PzwAn_BQ/viewform) | [日本語](https://docs.google.com/forms/d/e/1FAIpQLSdW8rLZc9LrAnchxxNdaXQgfW57kqqrI00fQ0grnuRtRfrdDA/viewform)

## Imagens

![Janela principal do Neoveg com projeto demonstrativo: Mídia do Projeto, Prévia de Vídeo, Master Bus, botão Encoder e linha do tempo com trilhas de vídeo, título, áudio, auto-crossfade, marcadores e região](docs/screenshots/janela_principal.png)

| Tela de Boas-Vindas | Renderizar como... |
|---|---|
| ![Tela de boas-vindas do Neoveg com banner original, escolha de 5 proporções, resolução e taxa de quadros](docs/screenshots/boas_vindas.png) | ![Janela Renderizar como com pesquisa, favoritos, lista de modelos e botão Enviar para o Encoder](docs/screenshots/renderizar_como.png) |

| Efeitos de Vídeo | Geradores de Mídia |
|---|---|
| ![Janela Efeitos de Vídeo com categorias, busca, lista de efeitos e miniatura do preset](docs/screenshots/efeitos_de_video.png) | ![Janela Geradores de Mídia com categorias, tipos de gerador e miniaturas dos presets](docs/screenshots/geradores_de_midia.png) |

| Color Grading | Escopos de Vídeo |
|---|---|
| ![Painel Color Grading com rodas tonais vetoriais e predefinições ao vivo na prévia](docs/screenshots/color_grading.png) | ![Escopos de Vídeo exibindo Histograma RGB + Luma e Forma de Onda](docs/screenshots/escopos_de_video.png) |

| Mixer e Barramentos Auxiliares | Surround Panner 5.1 |
|---|---|
| ![Mixer multifaixa com faders de trilha, dois barramentos auxiliares e Master Bus](docs/screenshots/mixer_e_barramentos.png) | ![Surround Panner 5.1 no anel de caixas ITU-R BS.775 e nível LFE](docs/screenshots/surround_panner.png) |

| Medidores de Loudness (EBU R128) | Propriedades do Evento |
|---|---|
| ![Medidores de Loudness EBU R128 com barras M/S/I/True Peak e varredura offline](docs/screenshots/medidores_loudness.png) | ![Propriedades do evento: nome, início, duração, deslocamento na mídia, velocidade, fades e opções](docs/screenshots/propriedades_do_evento.png) |

![Janela destacada do Neoveg Encoder exibindo Fila de Codificação, Painel Codificação ao Vivo, Navegador de Predefinições e Pastas Monitoradas](docs/screenshots/neoveg_encoder.png)

As imagens mostram só a interface do Neoveg, com mídia sintética gerada pelo próprio programa (gradiente, barras de cor, texto e áudio senoidal).

## Arquitetura do Workspace (5 Crates)

- **`neoveg_common`** (`crates/neoveg_common`): tipos fundamentais, representação exata de tempo (`Ticks`, `FrameRate`, `Rate`, *SMPTE Drop-Frame*), curvas, preferências e utilitários base.
- **`neoveg_engine`** (`crates/neoveg_engine`): estado da timeline, invariantes de edição atômica, mixer multifaixa com até 26 barramentos auxiliares (`Bus A`..`Bus Z`), processamento DSP via `fundsp`, *varispeed* com `rubato`, compositor de vídeo 2D/3D (Pan/Crop com máscara Bézier animável, Track Motion 2D/3D com Sombra/Brilho, hierarquia Pai/Filho 3D, ordenação Z e câmera estereoscópica 3D *off-axis*), efeitos/transições, escopos, sequências internas em abas estilo Premiere Pro (`Project.sequences` e `.nvegseq` avulso), persistência `.neoveg`/`.nvp` com recuperação de projetos aninhados, intercâmbio em Sala Limpa (OpenTimelineIO, FCPXML, FCP7 XML, EDL CMX 3600, AAF, SRT/WebVTT), *Surround* 5.1, host isolado de plug-ins OFX (tiles, *General* com entradas auxiliares e geradores) e VST3 (`IPlugView`, automação por amostra e instrumentos MIDI com parser SMF `.mid`/`.midi` em sala limpa), decodificação de áudio e vídeo em Rust puro (`WAV`/`AIFF`/`FLAC`/`Vorbis`/`MP3`, `FFV1 v3`, `media/fallback.rs`), contrato unificado de decodificação de sistema (`media/system.rs`) com backends nativos **Windows Media Foundation**, **Linux VA-API** e **macOS VideoToolbox/AudioToolbox**, e **Tabela Única de Comandos**.
- **`neoveg_scripting`** (`crates/neoveg_scripting`): automação headless (`--command`, `--list-commands`) e ponte sob demanda para scripts `.NET`/`C#` (`.cs`/`.dll`) compatíveis com `ScriptPortal.Vegas` (Clean Room, Protocolo 6) via `netcorehost`.
- **`neoveg_ui`** (`crates/neoveg_ui`): interface desktop nativa em `eframe` + `wgpu`, timeline interativa com 22 ponteiros vetoriais de edição (`cursors.rs`), dock flexível em `egui_tiles` com janelas destacáveis multi-abas, acoplamento persistente de diálogos e testes de interação `egui_kittest`, 7 temas visuais (incluindo 2 temas de Alto Contraste WCAG AAA), gerenciador de layouts `.neoveglayout`, escopos/prévia com legendas CC1–CC4 e visão 360° equiretangular interativa, e editores de atalhos e preferências.
- **`neoveg`** (`crates/neoveg`): binário principal e interface CLI com inicialização do pool `rayon` proporcional ao hardware.

## Recursos Implementados

- **Timeline, Áudio e Edição VEGAS Pro:** múltiplas trilhas A/V, mixer com até 26 barramentos auxiliares (`Bus A`..`Bus Z`) + *Master Bus* (`Mono`/`Dim -20 dB`, envios *pre/post-fader* e roteamento acíclico), DSP com `fundsp`, auto-crossfade ao sobrepor eventos na mesma faixa, alças de fade nos cantos superiores, alça de ganho em dB no topo de eventos de áudio, playhead físico de `1 px`, controle contínuo de taxa (*Playback Rate* com reamostragem *Sinc* via `rubato`), divisão (`S`), marcações (`M`), regiões (`R`), *Auto Ripple* (`Ctrl+L`), encaixe (`F8`) e *scrub* (`J`/`K`/`L`). *Adjustment Track* (`insert.track_adjustment`), *Surround Panner* 5.1 (Master estéreo/5.1, LFE, downmix ITU e render WAV de 6 canais), medidores de Loudness EBU R128 e **Pastas de Trabalho do Projeto** (*Arquivo → Pastas de trabalho...*, `file.work_folders`, para mapeamento persistente de capturas, gravações, pré-renderizações, proxies e exportações).
- **Quadros-chave de trilha na linha do tempo:** os quadros-chave dos efeitos de vídeo da trilha e do *Track Motion* aparecem em linhas abaixo da trilha de vídeo; arrastar muda a posição (com encaixe e sem passar dos vizinhos), o clique direito adiciona, exclui e muda a interpolação para as 6 curvas clássicas (*Linear, Rápida, Lenta, Suave, Abrupta, Manter* tanto para Track Motion quanto para parâmetros de efeitos), e o botão de quadro-chave no cabeçalho expande ou recolhe a área (manual do VEGAS Pro 21, p. 389).
- **Takes múltiplos por evento:** um evento guarda várias mídias alternativas (takes), cada uma com o seu ponto de início e nome; *Adicionar como take aos eventos selecionados* (menu da Mídia do projeto) torna a nova mídia o take ativo e mantém a anterior, `T`/`Shift+T` trocam para o próximo/anterior, o submenu *Take* do evento escolhe o ativo numa lista e exclui o ativo, e *Opções → Slip em todos os takes* faz o slip deslocar todos os takes. Dividir e aparar mantêm os takes sincronizados; render e exportadores usam o take ativo (manual do VEGAS Pro 21, pp. 346–348).
- **Plug-ins externos (OFX e VST3):** descoberta de VST3/OFX nas pastas padrão e em pastas mapeadas pelo usuário (ex.: Google Drive, *Plug-In Manager → Pastas de plug-ins*) no *Plug-In Manager* e no *Plug-In Chooser* em 2 colunas com cadeia de pílulas; efeitos e geradores de vídeo **OFX** (renderização por tiles `SupportsTiles = 1`, contextos *General* com múltiplas entradas auxiliares, *Filter* e *Generator* em *Geradores de Mídia → Terceiros*, keyframes de parâmetros e avaliação completa no host via `getValueAtTime`/`getDerivative`/`getIntegral`) e efeitos e instrumentos **VST3** (eventos, trilhas, barramentos e Master, interface gráfica nativa `IPlugView`/`IPlugFrame`, automação em tempo real por amostra via `IParameterChanges`, instrumentos MIDI com `IEventList` e parser SMF `.mid`/`.midi`, persistência de estado `IComponent::getState`/`setState` e compensação automática de latência PDC) processados no subprocesso isolado `neoveg --plugin-host`, de modo que uma falha do plug-in não derruba o editor.
- **Tabela Única de Comandos, Atalhos e Scripting `.NET`:** menus, barra de ferramentas, atalhos e comandos CLI despacham pelo mesmo `id`; atalhos personalizados podem ser gravados/limpos em *Ajuda → Atalhos de teclado*, e scripts `.cs`/`.dll` (`ScriptPortal.Vegas` Clean Room, Protocolo 6) podem ser executados sob demanda via *Ferramentas → Executar script (.cs/.dll)...* (`tools.run_script`) ou `--run-script`. A API dos scripts segue os nomes da *VEGAS Pro Scripting API Summary* (documentação pública da MAGIX) e do FAQ do manual, com implementação própria: projeto, trilhas (nome, Mute/Solo, seleção, `Events`, `Effects`, `Envelopes`), eventos (`Start`/`Length`/`End`, nome, Mute, Locked, seleção, `FadeIn`/`FadeOut`, `Effects`, `Envelopes`; mover entre trilhas do mesmo tipo e apagar), parâmetros e keyframes de efeitos (`Effect.Keyframes`, `Effect.Parameters`, `Effect.Preset`) incluindo OFX/VST3 por identificador descoberto, marcadores, `Vegas.Cursor`, `Transport.CursorPosition`/`SelectionStart`/`SelectionLength` e `Timecode` em ticks exatos com operadores, mídia do projeto (`Project.MediaPool`, `Media`, `VideoStream`/`AudioStream`) e takes (`Takes`, `ActiveTake`, `AddTake`, `Take.Offset`/`Name`/`MediaPath`), além de criar eventos com `AddVideoEvent`/`AddAudioEvent` + `AddTake` e importar mídia nova com `MediaPool.AddMedia` ou `new Media(path)` (até 64 por execução). No Windows com runtime `Microsoft.WindowsDesktop.App`, os scripts usam o `System.Windows.Forms` real numa thread STA; no Linux, macOS e em ambientes sem `WindowsDesktop`, os diálogos e formulários (`MessageBox`, `Form.ShowDialog` e seletores de arquivo) funcionam pela camada própria em `crates/neoveg_ui/src/script_forms.rs`.
- **Interface, Ponteiros, Personalização e Acessibilidade:** reforma visual editorial de alta densidade com 7 temas selecionáveis em *Exibir → Tema* ou *Opções → Preferências...* (incluindo *Alto Contraste AAA Escuro* e *Claro* com contraste >= 7:1), **22 ponteiros vetoriais originais de edição** (`cursors.rs`: aparo esquerdo/direito/adjacente, esticar tempo, *Split Trim*, fade, *Slip*, *Slide*, crossfade, *scrub*, rotação, movimento livre/X/Y, mão de envelope, lupa +/-, caixa de seleção e ferramentas de caneta/vértice de máscara Bézier), faixa lateral colorida de 22 px com menu `Mais (…)` nos cabeçalhos de trilha, rótulos em cápsula escura e botões configuráveis (`Pan/Crop`, `fx`, `…`) nos eventos, escala dB central no *Master Bus*, **Pan/Crop** com linha interativa de **Máscara Bézier** animável, presets e pílulas de cadeia FX, **Track Motion 2D/3D** completo (pivô, orientação, sombra/brilho 2D, composição Pai/Filho 3D, ordenação Z e câmera estereoscópica 3D *off-axis*), **Prévia de Vídeo** com decodificação de legendas **CC1–CC4** e **visão 360° equiretangular interativa**, **Editar → Propriedades → Layout** com os tipos clássicos de disposição (*Clássico*, *Intermediário*, *Recente* e *Moderno*), **Tela de abertura (Splash Screen)** e **Tela de boas-vindas**, painel acoplável **Color Grading**, janela **Renderizar como...** em 2 colunas, 123 ícones vetoriais nativos coloridos (`crates/neoveg_ui/src/icons.rs`), barra inferior de transporte/edição na ordem do manual com seleção `In`/`Fim`/`Tam`, docas flutuantes multi-abas e acoplamento persistente de diálogos (`Preferences.docked_dialogs`), `F11`/`Shift+F11`/`Ctrl+F11` e layouts `.neoveglayout`.
- **Neoveg Encoder (Fila de Codificação em Lote e Múltiplas Saídas):** janela própria, fora da área de janelas, como um programa à parte (botão **Encoder** na barra superior, *Exibir → Neoveg Encoder* `Ctrl+Alt+M`, botão **Enviar para o Encoder** em *Renderizar como...* e *Arquivo → Enviar para o Neoveg Encoder...* `Ctrl+Shift+M`) e CLI `--encode` integrados aos 7 temas VEGAS Pro: fila persistente (`encoder/queue.json`) com múltiplas saídas paralelas por origem, painel *Codificação* ao vivo com miniatura e ETA, pausa cooperativa e interrupção seletiva/total, *Navegador de Predefinições* (sistema, personalizadas `.neovegpreset`, grupos e apelidos), *Pastas Monitoradas* (*Watch Folders* com disparo automático e anti-sobrescrita), janela *Configurações de Exportação* (intervalo `In`/`Out`, corte/Crop com bloqueio de proporção, 5 modos de escala de origem e prévia *Origem/Saída*), efeitos de pré-codificação (correção de cor, tabela LUT `.cube` 1D/3D em sala limpa, sobreposição de imagem/nome/timecode SMPTE e Ajustador de Tempo) e logs em `logs/neoveg_encoder*.log`.
- **Integração ArtCraft & Blender (*Ferramentas → Integrações*):** descoberta automática e caminhos manuais (*Preferências → Aplicativos externos*) para **PhotoCraft**, **VectorCraft**, **FilmCraft**, **EffectCraft**, **AniCraft**, **SoundCraft** e **Blender**. Inclui fluxo *Round-Trip* de cópias editáveis como *Takes* (PNG para PhotoCraft, SVG limpo para VectorCraft e BWF WAV com `bext` `TimeReference` para SoundCraft com recarga automática ao salvar), cliente TCP loopback (`127.0.0.1`) com autenticação HMAC-SHA256 para apps ArtCraft em execução, exportação OTIO para FilmCraft, renderização headless de cenas `.anicraft`, `.ecproj` e `.blend` direto para sequências PNG na timeline, geração de script `.py` de sincronização de FPS/marcadores/Pan-Crop e importação de câmera Blender (`.json`) para keyframes de *Track Motion* / *Pan/Crop*.
- **Formatos Suportados:**
  - **Intercâmbio (Rust puro, Sala Limpa):** OpenTimelineIO `.otio` (esquema 0.17), Final Cut Pro XML `.fcpxml`, Final Cut Pro 7 XML `.xml` (`xmeml`), AAF `.aaf`, EDL CMX 3600 `.edl` e legendas SubRip `.srt` / W3C WebVTT `.vtt` (importação e exportação). Disponível em *Arquivo → Intercâmbio & Legendas* e por arrastar-e-soltar; as edições das especificações usadas estão em [`crates/neoveg_engine/README.md`](crates/neoveg_engine/README.md).
  - **Rust puro (multiplataforma):**
    - *Projetos, sequências e vídeo:* `.neoveg` (e legado `.nvp`), sequências avulsas `.nvegseq` (importação/exportação com remapeamento de IDs), `AVI` (importação `MJPEG`/`DIB RGB24/32`/`YUY2`/`UYVY`/`I420` + PCM/G.711; exportação `AVI MJPEG` e `AVI Não Comprimido RGB24`), `Y4M`, `GIF` animado (importação e exportação), demuxer de contêineres `MP4`/`MOV`/`MKV`/`WEBM` (`media/demux/`), decodificador `FFV1 v3` sem perdas (`media/ffv1.rs`) e decodificador fallback multiplataforma (`media/fallback.rs`).
    - *Imagens e sequências:* `PNG`, `JPEG`, `BMP`, `TIFF`, `WebP` (estático e animado), `GIF`, `TGA`/`ICB`/`VDA`/`VST`, `ICO`/`CUR`, `PNM` (`PBM`/`PGM`/`PPM`/`PAM`), `Radiance HDR`, `DDS`, `Farbfeld`, `QOI`, `OpenEXR` (`.exr`), `DPX` (`.dpx`), `Kodak Cineon` (`.cin`), `WBMP`, `XBM`, `XPM` e `Amiga IFF/ILBM` (`.iff`/`.ilbm`/`.lbm`), além de captura e renderização em sequência para 10 formatos (`PNG`, `JPEG`, `TIFF`, `TGA`, `BMP`, `WebP`, `PNM`, `QOI`, `DPX`, `EXR`).
    - *Áudio e MIDI:* `WAV` / `BWF` / `RF64` (PCM, Float32, ITU-T G.711 µ-law/A-law, IMA/DVI ADPCM), `AIFF`/`AIFC` (importação e exportação), `Sun/NeXT AU/SND`, `Apple CAF`, `Sony Wave64` (`.w64`), `Creative VOC`, `Amiga IFF 8SVX` (PCM 8b e Fibonacci Delta), `QOA` (`.qoa`), `FLAC` (`claxon`), `Ogg Vorbis` (`.ogg`, `.oga`, `lewton`), `MP3` (`puremp3`) e **Standard MIDI File** (`.mid`/`.midi`, formatos 0/1/2, parser em sala limpa em `midi.rs` com prévia polifônica de fallback e roteamento para instrumentos VST3).
  - **Backends nativos de sistema (Windows, Linux e macOS):**
    - *Windows (Media Foundation):* importação/decodificação em streaming de `MP4`, `MOV`, `MKV`, `WEBM`, `M4V`, `M4A`, `AAC`, `WMV`, `ASF`, `WMA`, `3GP`, `3G2`, `M2TS`, `MTS`, `TS`, `MPG`, `MPEG`, `MPE`, `VOB`, `MP2`, `MPA`, `M4B` e `ALAC`; exportação nativa via `IMFSinkWriter` para `MP4 (H.264/HEVC + AAC)`, `MOV (H.264 + AAC)`, `3GP (H.264 + AAC)`, `WMV` e áudio `M4A (AAC)`, `AAC (ADTS)`, `WMA` e `MP3`.
    - *Linux (VA-API):* decodificação acelerada via `libva.so.2`/`libva-drm.so.2` carregadas dinamicamente (`media/vaapi.rs`, zero GStreamer/LGPL), com recuo automático para o fallback em Rust puro.
    - *macOS (VideoToolbox / AudioToolbox):* decodificação de vídeo H.264/HEVC via `VTDecompressionSession` e áudio via `ExtAudioFile` (`media/videotoolbox.rs`), com recuo automático para o fallback em Rust puro.
  - **Conversão para formato compatível (Windows):** quando um arquivo não abre diretamente (extensão fora da lista ou codec que os leitores próprios não decodificam, como um AVI que não é MJPEG), o Neoveg oferece convertê-lo com o decodificador do sistema para AVI MJPEG + PCM (vídeo) ou WAV 24 bits (só áudio) e importa o resultado na mesma posição; também em *Arquivo → Converter mídia para formato compatível...*. O destino é escolhido no diálogo, o arquivo é gravado como `.part` até terminar, o AVI é gravado com as extensões OpenDML (sem o limite de 4 GB; até ~250 GB, e a qualidade JPEG cai de 90 até 50 só se for preciso para caber, com aviso), e nada é preenchido ou cortado em silêncio (`media/convert.rs`). (`ffmpeg`/`ffprobe` são utilizados exclusivamente como oráculos de teste externos e geradores de fixtures via `tools/gen_media_fixtures.py`, nunca linkados nem distribuídos.)
- **Relatório quando o programa não fecha normalmente:** cada sessão da interface mantém um marcador travado em `logs/sessoes/`; na abertura seguinte a uma queda (erro interno, fechamento forçado, falta de energia), a janela *O Neoveg não foi fechado normalmente* diz o que aconteceu (o erro e o local, ou que o programa foi fechado sem aviso), lista os erros isolados daquela sessão, oferece a recuperação do projeto salvo automaticamente e permite copiar o relatório ou abrir a pasta de logs. Duas instâncias abertas ao mesmo tempo não contam como queda, e os pânicos de outras instâncias e dos hosts de plug-in não entram no relatório.
- **Propriedades do projeto:** abas *Vídeo*, *Áudio*, *Régua*, *Resumo* e *CD de áudio*, como no VEGAS Pro; o Resumo e o código UPC/EAN entram na folha `.cue` exportada.
- **Windows e Linux:** instalador próprio em Rust e pacote portátil para os dois sistemas; no Linux, atalho no menu de aplicativos, comando `neoveg` e associação dos arquivos `.neoveg` (ver *Instalador para Linux*).
- **Hardware Limitado:** resoluções de prévia (*Rascunho / Prévia / Boa / Melhor*), geração de proxies `.avi`, pré-renderização seletiva de região, limite configurável de RAM/quadros em cache e compilação portátil sem `-C target-cpu=native`.

## Como Executar

```bash
# Abrir interface gráfica (opcionalmente passando um projeto ou mídia)
cargo run -p neoveg
cargo run -p neoveg -- meu_projeto.neoveg

# Interface de linha de comando (CLI)
cargo run -p neoveg -- --list-commands
cargo run -p neoveg -- --probe-integrations
cargo run -p neoveg -- --command insert.marker meu_projeto.neoveg atualizado.neoveg
cargo run -p neoveg -- --run-script meu_script.cs
cargo run -p neoveg -- --render meu_projeto.neoveg saida.avi
cargo run -p neoveg -- --encode entrada.mp4 saida.mp4 sys.web_1080p_hq
```

Logs, autosave, preferências (`preferencias.json`), layouts (`.neoveglayout`), proxies e pré-renderizações são gravados ao lado do executável em builds portáteis (ou na pasta temporária do sistema quando executado em diretório somente leitura).

## Como usar

### Primeiros passos

1. Abra o Neoveg (`cargo run -p neoveg` ou o executável do pacote portátil). Na **Tela de boas-vindas**, escolha uma das 5 proporções (*Widescreen 16:9*, *Fullscreen 4:3*, *Portrait 9:16*, *Square 1:1* ou *Scope 2.4:1*), ajuste resolução, taxa de quadros e *Drop-Frame*, ou reabra projetos em *Abrir Projeto*. A tela pode ser reaberta em *Ajuda → Tela de boas-vindas*.
2. Importe vídeo, áudio, imagens estáticas ou sequências de imagens com *Arquivo → Importar mídia...* (`Ctrl+I`) ou arrastando os arquivos diretamente para a janela. Os clipes aparecem em *Mídia do Projeto*; arraste-os para a linha do tempo ou dê um duplo clique para inseri-los no cursor. Se desejar marcar pontos *In*/*Out* antes de cortar, use o painel **Trimmer** (`Alt+2`).
3. O formato do primeiro arquivo importado define automaticamente as dimensões e a taxa de quadros do projeto se *Propriedades do projeto* (`Alt+Enter`) ainda não tiver sido alterado manualmente.
4. No Windows, arquivos fora dos codecs suportados diretamente exibem opção automática de conversão segura para `AVI MJPEG + PCM` ou `WAV 24 bits` (*Arquivo → Converter mídia para formato compatível...*).
5. Salve com `Ctrl+S` (ou `Ctrl+Shift+S` / `Ctrl+Alt+S` para versões). O projeto é salvo em `.neoveg` com recuperação automática caso o editor seja fechado inesperadamente.

### Editar na linha do tempo

| Ação | Atalho |
|---|---|
| Reproduzir / pausar / do início / parar | `Espaço` / `Shift+Espaço` / `Esc` |
| Scrub reverso, pausar, avanço | `J`, `K`, `L` |
| Quadro anterior / próximo | `Alt+←` / `Alt+→` |
| Próxima / anterior ferramenta de edição | `D` / `Shift+D` |
| Dividir o evento no cursor | `S` |
| Aparar início / fim do evento no cursor | `Alt+[` / `Alt+]` |
| Aparar para a seleção de tempo | `Ctrl+T` |
| Definir entrada / saída da seleção / loop | `I` / `O` / `Q` (`Shift+Q`) |
| Próximo / anterior take | `T` / `Shift+T` |
| Agrupar / desagrupar / selecionar grupo | `G` / `U` / `Shift+G` |
| Inserir marcador / região | `M` / `R` |
| Envelope de Volume / Pan na trilha | `V` / `P` |
| Mudo / solo na trilha selecionada | `Z` / `X` |
| Inserir trilha de vídeo / áudio / barramentos | `Ctrl+Shift+Q` / `Ctrl+Q` / `Ctrl+B` |
| Encaixe (`snapping`) / Ripple automático | `F8` / `Ctrl+L` |
| Zoom horizontal / formas de onda vertical | `↑` / `↓` (`Shift+↑` / `Shift+↓`) |
| Desfazer / refazer | `Ctrl+Z` / `Ctrl+Y` |
| Propriedades do evento | `F2` |
| Neoveg Encoder / Enviar timeline | `Ctrl+Alt+M` / `Ctrl+Shift+M` |
| Renderizar como... | `Ctrl+M` |

- **Ferramentas e modos de edição (`D` / `Shift+D`):** na barra de transporte e em *Editar → Ferramenta de edição*, alterne entre **Normal** (movimentação e aparo nos modos *Normal* ou *Slip*), **Envelope** (move apenas pontos de envelope), **Seleção** (caixa retangular; `Shift`/`Ctrl` somam) e **Zoom** (caixa aproxima; clique único ajusta o projeto à janela).
- **Fades, ganho e quadros-chave:** sobrepor eventos na mesma trilha cria crossfade automático; as alças nos cantos superiores ajustam fades, a barra superior em clipes de áudio regula ganho em dB, e as linhas abaixo da trilha editam keyframes de *Track Motion* e efeitos de trilha.
- **Cabeçalhos, transporte e edição tabular:** a barra inferior segue a ordem do VEGAS Pro com leitura/edição de `In`, `Fim` e `Tam`, controle de *Playback Rate* (`0.25×`–`4.00×`), botão *Mais* (`…`) no cabeçalho de trilha para configurar os botões visíveis, e painel **Detalhes da Edição** (`Alt+3`) para edição em tabela.
- **Sequências, `.nvegseq` avulso e Aninhamento:** abas interativas no topo da timeline para múltiplas sequências no mesmo projeto, importação e exportação de sequências avulsas `.nvegseq` (*Editar → Sequência → Importar/Exportar sequência...* ou arraste para a janela) e recuperação automática de abas de projetos `.neoveg` aninhados modificados (`autosave/recuperacao_aninhados.json`), com propagação ao vivo em memória entre projetos aninhados em vários níveis (incluindo netos). Arraste qualquer sequência de *Mídia do Projeto* para a timeline ativa ou use *Aninhar seleção em nova sequência* (`sequence.nest_selection`); prevenção estrita de ciclos e limite de 32 níveis.

### Takes

Um evento pode guardar múltiplas mídias alternativas (takes). Em *Mídia do Projeto*, clique com o botão direito numa mídia e selecione *Adicionar como take aos eventos selecionados*; alterne o take ativo com `T` / `Shift+T` ou pelo submenu *Take* do evento. Com *Opções → Slip em todos os takes*, ajustes de deslocamento mantêm todas as tomadas sincronizadas.

### Efeitos, geradores, Color Grading e escopos

- **Efeitos de Vídeo, Áudio e Transições:** 46 efeitos de vídeo nativos em 8 categorias (cor, luz, desfoque e nitidez, estilizar, distorcer, composição e bordas, estabilização, recorte) e 33 transições. Filtre por categoria ou busca, clique no item e arraste a miniatura do preset para um evento, trilha ou transição, ou aplique pelos botões *Eventos*, *Trilha* e *Master*. Use `★` para favoritar.
- **Geradores de Mídia:** insira texto animado com contorno/sombra, cores sólidas, gradientes, xadrez, barras de cor, ruído, **padrões de teste** (grade de convergência, rampa de cinza, placa de zona, grade de pixels), **textura de ruído** fractal e geradores de terceiros **OFX** (*Terceiros*) pela guia *Geradores de Mídia* (*Inserir no cursor* ou arrastando).
- **Color Grading e Escopos:** abra *Color Grading* (ou clique em **Cor** no cabeçalho da *Prévia de Vídeo*) para ajustar rodas tonais (*Lift*, *Gamma*, *Gain*), pad *Temperatura × Tonalidade*, saturação e predefinições ligadas ao `FxKind::ColorCorrector`. Acompanhe o sinal em tempo real pela aba **Escopos** (*Histograma RGB + luma*, *Forma de onda*, *Vectorscope* e *RGB Parade*).
- **Plug-ins externos (OFX e VST3):** gerencie caminhos VST3 e OFX em *Plug-In Manager* (`Ctrl+Alt+1`). Todos os plug-ins rodam no subprocesso isolado `--plugin-host`, com suporte OFX a tiles, contexto *General* com entradas auxiliares e geradores, e suporte VST3 a interface gráfica nativa (`IPlugView`), automação em tempo real por amostra, instrumentos MIDI (`.mid`/`.midi`), persistência de estado (`IComponent::getState`/`setState`) e compensação automática de latência (PDC) sem risco ao editor.

### Áudio, Mixer, Surround 5.1 e Loudness

- **Mixer e Barramentos Auxiliares (`Bus A`..`Bus Z`):** clique em **＋ Barramento** no *Mixer* ou use *Inserir → Barramento de áudio* para criar até 26 barramentos com envios *pre/post-fader* e visualização na timeline (`Ctrl+B`).
- **Master Bus:** o painel à direita mantém monitoração estéreo com controles `fx`, `Mono`, `Dim` (`-20 dB`) e `M` (mudo).
- **Surround Panner 5.1:** na aba *Surround Panner* (ou em *Propriedades do projeto*), alterne o *Modo do Master* para `5.1 Surround`, posicione cada trilha ou barramento no anel ITU-R BS.775, ajuste LFE e renderize em WAV multicanal de 6 canais.
- **Medidores de Loudness EBU R128 (`Ctrl+Alt+7`):** acompanhe `M`, `S`, `I`, `LRA` e `True Peak` com metas configuráveis (*EBU R128 -23 LUFS*, streaming, ATSC, podcast) e varredura offline (*Analisar projeto* / *Analisar seleção*).

### Renderizar

*Arquivo → Renderizar como...* (`Ctrl+M`) abre a janela em 2 colunas com busca, ★ favoritos, resumo do intervalo (*Projeto inteiro* ou *Região de loop*) e todos os **31 formatos de saída** suportados: `MP4 (H.264/HEVC + AAC)`, `MOV`, `3GP` e `WMV` (Windows Media Foundation), `AVI MJPEG` e `AVI Não Comprimido RGB24` (com OpenDML), `GIF` animado, `Y4M`, 10 sequências de imagens (`PNG`, `JPEG`, `TIFF`, `TGA`, `BMP`, `WebP`, `PNM`, `QOI`, `DPX`, `EXR`) e áudio (`WAV`, `BWF`, `RF64`, `Wave64`, `AIFF`, `AU`, `CAF`, `QOA`, `M4A`, `AAC`, `WMA`, `MP3`). Pela mesma janela também é possível clicar em **Enviar para o Encoder** ou executar sem interface via `neoveg --render projeto.neoveg saida.ext`.

### Neoveg Encoder (fila em lote e pastas monitoradas)

O **Neoveg Encoder** pode ser acessado diretamente pelo botão **Encoder** na barra superior, em *Exibir → Neoveg Encoder* (`Ctrl+Alt+M`), em *Ferramentas → Neoveg Encoder*, pelo botão **Enviar para o Encoder** na janela *Renderizar como...*, ou com *Arquivo → Enviar para o Neoveg Encoder...* (`Ctrl+Shift+M`, que envia a timeline ou seleção atual para a fila). Clique em **Destacar** na faixa da aba para utilizá-lo em sua própria janela ampla fora da dock.

- **Painéis (4):** *Fila de Codificação* (múltiplas saídas paralelas por origem, duplicação, reordenação e interpretação de mídia), *Codificação (Ao Vivo)* (miniatura, ETA, pausa cooperativa e interrupção por item ou total), *Navegador de Predefinições* (sistema, `.neovegpreset` personalizadas e grupos multi-saída) e *Pastas Monitoradas* (detecção automática em diretórios de entrada).
- **Configurações de Exportação:** recorte de intervalo `In`/`Out`, *Crop* com trava de proporção, 5 modos de escala, correção de cor, tabela LUT `.cube` 1D/3D, sobreposições de imagem/nome/timecode SMPTE e *Ajustador de Tempo* (%). Também disponível em linha de comando: `neoveg --encode entrada.mp4 saida.mp4 sys.web_1080p_hq`.

### Intercâmbio e legendas

Use *Arquivo → Intercâmbio & Legendas* (ou arraste o arquivo diretamente para o Neoveg) para importar e exportar timelines em **OpenTimelineIO (`.otio`)**, **Final Cut Pro XML (`.fcpxml`)**, **Final Cut Pro 7 XML (`.xml`, `xmeml`)**, **AAF (`.aaf`)**, **EDL CMX 3600 (`.edl`)** e legendas **SubRip (`.srt`)** / **WebVTT (`.vtt`)**.

### Integrações (ArtCraft & Blender)

Acesse *Ferramentas → Integrações (ArtCraft & Blender)...* (ou configure caminhos manuais em *Opções → Preferências... → Aplicativos externos*) para trabalhar em conjunto com **PhotoCraft**, **VectorCraft**, **FilmCraft**, **EffectCraft**, **AniCraft**, **SoundCraft** e **Blender**. É possível criar cópias *Round-Trip* como *Takes* (PNG, SVG limpo ou WAV/BWF com recarga automática ao salvar), exportar OTIO para o FilmCraft, renderizar cenas `.anicraft`/`.ecproj`/`.blend` como sequências PNG na timeline, gerar scripts `.py` de sincronização e importar câmeras JSON do Blender.

### Scripts C# (.cs/.dll)

*Ferramentas → Executar script (.cs/.dll)...* (ou `neoveg --run-script meu_script.cs`) executa scripts `.NET` compatíveis com a API `ScriptPortal.Vegas` em um único passo de Desfazer. No Windows (com runtime `Microsoft.WindowsDesktop.App`), os formulários usam `System.Windows.Forms`; no Linux, macOS e em ambientes sem `WindowsDesktop`, as caixas e diálogos rodam pela camada própria em `script_forms.rs`.

### Layout, aparência e atalhos

- **Disposição e janelas destacáveis:** *Editar → Propriedades → Layout* alterna entre *Clássico*, *Intermediário* e *Atual* (3 colunas), além da posição da área acoplável e das abas. Todas as abas, a *Prévia de Vídeo* e o *Master Bus* podem ser reorganizados por arraste ou destacados em janelas nativas e salvos em `.neoveglayout`.
- **Temas e atalhos:** escolha entre 7 temas (incluindo 2 de Alto Contraste AAA) em *Exibir → Tema* e personalize qualquer comando da Tabela Única em *Ajuda → Atalhos de teclado* ou *Opções → Preferências...*.

## Empacotamento Portátil e Verificações de Qualidade

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python tools/check_licenses.py
python build_portable.py
```

A CI do GitHub (`.github/workflows/ci.yml`) roda esses portões em todo pull request, no Windows e no Linux (Ubuntu), com os testes um por vez e as fixtures de mídia geradas na hora.

O script `build_portable.py` compila em modo release e gera o pacote portátil em `dist/` na raiz do repositório, com as fontes abertas `assets/fonts/` (`OFL-1.1`) e os catálogos `i18n/*.json`, sem dependência de instalador, fontes proprietárias ou binários externos. Depois de confirmar e verificar a cópia (`SHA-256`), ele apaga a pasta de build do cargo (`target/` ou `CARGO_TARGET_DIR`); se a compilação, verificação ou a cópia falhar, a pasta é mantida, o erro é informado e — fora do GitHub Actions — o script dispara automaticamente `gh workflow run build-packages.yml -R LARPFX-ori/neoveg` e baixa os pacotes verificados por SHA-256 (use `--no-gh-fallback` para desativar). Use `--keep-target` para manter a pasta de build e `--out` para gravar o pacote em outro lugar. `dist/` e `target/` ficam no `.gitignore`.

### Instalador para Windows

```bash
python build_installer.py
```

O `build_installer.py` gera o pacote portátil com o `build_portable.py`, compacta a pasta `dist/` e anexa o ZIP ao fim do `neoveg-setup.exe`, o instalador próprio escrito em Rust (`crates/neoveg/src/setup/`, licença `MIT OR Apache-2.0`, sem ferramenta de terceiros). O resultado vai para `dist-installer/Neoveg-Setup-<versão>.exe`, também no `.gitignore`. Ele aceita as mesmas opções `--no-build`, `--keep-target` e `--no-gh-fallback` (acionando o mesmo fallback via GitHub Actions quando a compilação local falhar).

O instalador pergunta para quem instalar:

- **Só para mim:** em `%LOCALAPPDATA%\Programs\Neoveg`, sem pedir administrador.
- **Para todos os usuários:** em `%ProgramFiles%\Neoveg`, com a confirmação do UAC.

Ele cria o atalho no menu Iniciar e, se marcado, na Área de Trabalho. Também pode associar os arquivos `.neoveg` ao Neoveg. O Neoveg aparece em *Configurações → Aplicativos* (Adicionar ou remover programas) com o desinstalador `desinstalar.exe`.

Instalado, o Neoveg guarda preferências, layouts, logs e cópias de recuperação em `%APPDATA%\Neoveg`, e não na pasta do programa. A pasta de dados só é apagada se essa opção for marcada no desinstalador. O pacote portátil continua guardando tudo ao lado do executável.

Modo silencioso: `Neoveg-Setup.exe --silent [--user | --all-users] [--dir <pasta>] [--desktop] [--no-assoc]`. Para remover sem janela: `desinstalar.exe --uninstall --silent [--remove-data]`.

### Instalador para Linux

```bash
python3 build_installer.py
```

No Linux, o mesmo `build_installer.py` gera dois arquivos em `dist-installer/`:

- `Neoveg-Setup-<versão>-linux-x86_64`: o mesmo instalador em Rust, com a mesma janela e as mesmas opções. Dê permissão de execução (`chmod +x`) e abra.
- `Neoveg-<versão>-linux-x86_64.tar.gz`: o pacote portátil. Extraia e rode `./neoveg`; os dados ficam ao lado do executável.

O instalador pergunta para quem instalar:

- **Só para mim:** em `~/.local/opt/neoveg`, sem pedir senha. O atalho vai para `~/.local/share/applications` e o comando `neoveg` para `~/.local/bin`.
- **Para todos os usuários:** em `/opt/neoveg`, com a senha de administrador pelo `pkexec`. O atalho e o tipo de arquivo vão para `/usr/local/share` e o comando para `/usr/local/bin`. Sem `pkexec`, rode `sudo ./Neoveg-Setup-<versão>-linux-x86_64 --silent --all-users`.

O atalho segue a *Desktop Entry Specification* do freedesktop.org e aparece no menu de aplicativos dos ambientes que seguem essa especificação (GNOME, KDE Plasma, Xfce e outros) com o ícone do Neoveg. Se marcado, ele também vai para a Área de Trabalho (só na instalação para mim, lida de `~/.config/user-dirs.dirs`). A opção de abrir arquivos `.neoveg` registra o tipo `application/x-neoveg-project` (*Shared MIME-info*). As ferramentas `update-desktop-database`, `update-mime-database`, `xdg-mime` e `gio` são usadas quando existem; sem elas a instalação continua. Para desinstalar, use *Desinstalar o Neoveg* no menu do atalho (botão direito) ou rode `desinstalar --uninstall` na pasta instalada.

Instalado, o Neoveg guarda os dados em `$XDG_DATA_HOME/Neoveg` (normalmente `~/.local/share/Neoveg`). O modo silencioso aceita as mesmas opções do Windows: `./Neoveg-Setup-<versão>-linux-x86_64 --silent [--user | --all-users] [--dir <pasta>] [--desktop] [--no-assoc]` e `desinstalar --uninstall --silent [--remove-data]`.

O executável usa a glibc do sistema em que foi compilado: para rodar em distribuições mais antigas, compile na mais antiga delas.

### Idiomas (`i18n/*.json`)

O Neoveg suporta **português do Brasil (`pt-BR`, padrão)**, **inglês (`en`)**, **espanhol (`es`)**, **francês (`fr`)**, **alemão (`de`)** e **japonês (`ja`)** a partir dos catálogos JSON na pasta `i18n/`. A troca de idioma pode ser feita ao vivo em **Preferências → Geral**, na tela inicial (**Boas-vindas**) ou na inicialização via `--lang <código>` / `NEOVEG_LANG`. Em japonês, glifos CJK são carregados das fontes instaladas no sistema operacional em tempo de execução. A interface usa as fontes do sistema, como o VEGAS Pro: Segoe UI no Windows (Tahoma como alternativa; Noto Sans, Liberation Sans ou DejaVu Sans no Linux) e Consolas ou Cascadia Mono nos campos monoespaçados, lidas da pasta de fontes do sistema e nunca distribuídas; **Preferências → Geral** volta para as fontes embutidas. O tamanho de textos, botões e painéis muda em **Exibir → Escala da interface** (Aumentar, Diminuir, 100%; de 50% a 300%, em passos de 10%) ou no campo *Escala da interface* das Preferências; os comandos vêm sem atalho padrão e podem receber um em *Personalizar teclado*. O *Visor de tempo* usa a sem serifa do sistema em negrito (Segoe UI Bold), com números de largura fixa.

## Acompanhamento de Paridade

- [Roadmap](ROADMAP.md)
- [Matriz de Paridade VEGAS Pro](docs_obsidian/ui_references/matriz_de_paridade.md)
- [Atribuição e Licenças de Assets](ATTRIBUTION.md)

## Licença

O Neoveg é distribuído sob licença dupla, à sua escolha:

- MIT ([LICENSE-MIT](LICENSE-MIT))
- Apache 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

Salvo declaração explícita em contrário, toda contribuição enviada para inclusão no projeto é licenciada
sob esses mesmos termos, sem condições adicionais.
