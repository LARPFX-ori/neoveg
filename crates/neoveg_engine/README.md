# `neoveg_engine`

Núcleo de edição, mixer de áudio multifaixa, compositor de vídeo, efeitos/transições, persistência e intercâmbio do **Neoveg**.

## Implementação em Sala Limpa e Especificações Públicas (`AGENTS.md` §2.3)

### OpenTimelineIO (`.otio`) — `src/otio.rs`

- **Edição de referência:** Especificação pública **OpenTimelineIO (OTIO) 0.17** — *OpenTimelineIO JSON Schema* (documentação oficial em `opentimelineio.readthedocs.io`, esquemas versionados `1` e `2`).
- **Método de implementação:** Sala limpa estrita a partir do formato JSON documentado na especificação pública e dos vetores estruturais descritos nela, sem consulta a código-fonte de implementações de referência ou de terceiros.
- **Esquemas suportados:**
  - Contêineres: `Timeline.1`, `Stack.1`, `Track.1` (`Video` / `Audio`)
  - Componíveis de linha do tempo: `Clip.1`, `Clip.2` (`media_reference`, `media_references` + `active_media_reference_key`), `Gap.1`, `Transition.1` (`transition_type`, `in_offset`, `out_offset`)
  - Referências de mídia: `ExternalReference.1` (`target_url`, `available_range`), `MissingReference.1`, `GeneratorReference.1` (`generator_kind`, `parameters`)
  - Tempo e metadados: `RationalTime.1` (`value`, `rate`), `TimeRange.1` (`start_time`, `duration`), `Marker.1` / `Marker.2` (`marked_range`, `name`, `color`), efeitos temporais `LinearTimeWarp.1` (`time_scalar`) e `FreezeFrame.1`
- **Semântica preservada:**
  - Conversão de `RationalTime` para `Ticks` exatos (`1_411_200_000` ticks/s) com aritmética inteira `i128` nas taxas e valores racionais/inteiros, e rejeição/saneamento de taxas não finitas ou `<= 0`.
  - Limitação de tamanho de arquivo (`16 MiB`), profundidade de aninhamento (`32` níveis de `Stack`), número de trilhas/itens e validação final por `Project::sanitize()`.

### Final Cut Pro XML (`.fcpxml`) — `src/fcpxml.rs`

- **Edição de referência:** Especificação pública **Apple Final Cut Pro XML Format Reference / FCPXML DTD 1.8–1.11** (elementos `<fcpxml>`, `<resources>`, `<format>`, `<asset>`, `<media-rep>`, `<effect>`, `<library>`, `<event>`, `<project>`, `<sequence>`, `<spine>`, `<asset-clip>`, `<clip>`, `<ref-clip>`, `<sync-clip>`, `<gap>`, `<title>`, `<transition>`, `<marker>`, `<chapter-marker>`, `<timeMap>`).
- **Método de implementação:** Parser XML limitado próprio em sala limpa sem `unsafe` (máximo `16 MiB`, profundidade `48`, `100_000` nós), conversão racional exata `"N/Ds"` em `i128` para `Ticks` e rejeição segura de XML malformado ou cíclico (`fuzz_fcpxml`).

### Final Cut Pro 7 XML (`.xml`, `xmeml`) — `src/fcp7xml.rs`

- **Edição de referência:** Documento público **Apple *Final Cut Pro XML Interchange Format*** (edição para Final Cut Pro 7, `xmeml` versão 5; também lê a versão 4): elementos `<xmeml>`, `<sequence>`, `<rate>` (`<timebase>` + `<ntsc>`), `<timecode>`/`<displayformat>`, `<media>`/`<video>`/`<audio>`, `<format>`/`<samplecharacteristics>`, `<track>`, `<clipitem>` (`<start>`, `<end>`, `<in>`, `<out>`, `-1` em bordas de transição), `<transitionitem>`, `<file>` (`<pathurl>`, definição única + referências por `id`), `<link>`/`<linkclipref>` e `<marker>`.
- **Método de implementação:** Reusa o parser XML limitado de `fcpxml.rs`; quadros inteiros limitados a `50 000 000` e convertidos para `Ticks` exatos; até `20 000` clipes e `64` trilhas. Na exportação, eventos sobrepostos de uma trilha vão para trilhas extras (o FCP7 não admite sobreposição numa trilha). Fuzzing por mutação em `fcp7xml::tests`.

### AAF (`.aaf`, importação) — `src/aaf.rs`

- **Edição de referência:** Especificações públicas da AAF Association / AMWA — **MS-03 AAF Low-Level Container Specification v1.0.1** (o contêiner Compound File Binary, lido pelo `cfb`), **MS-02 AAF Stored Format Specification v1.0.1** (stream `properties`: ordem de bytes, contagem, entradas `pid/forma/tamanho`; formas de dado, referência forte, vetor e conjunto de referências fortes com stream `index`; referência fraca por chave) e **MS-01 AAF Object Specification v1.1** (Header → ContentStorage → Mobs; CompositionMob, MasterMob, SourceMob; TimelineMobSlot com EditRate; Sequence, SourceClip, Filler, Transition; EssenceDescriptor → NetworkLocator `URLString`; definições de dados *Picture*/*Sound*).
- **Método de implementação:** Contêiner Compound File lido pelo crate `cfb` (MIT); só os streams de metadados são lidos (até `4 MiB` cada, `200 000` objetos, profundidade `32`, `20 000` clipes) e toda a leitura roda sob `catch_unwind`. Transições viram sobreposição com crossfade. A exportação AAF ainda não existe.
- **Fixtures:** `tools/gen_aaf_fixtures.py` gera `target/fixtures/oracle_*.aaf` com o pacote externo `pyaaf2` (MIT), usado só como gerador (como o `ffmpeg`): `PYTHONPATH=<pasta do pyaaf2> python tools/gen_aaf_fixtures.py target/fixtures`. Sem as fixtures, os testes que dependem delas pulam.

### EDL CMX 3600 (`.edl`) — `src/edl.rs`

- **Edição de referência:** Especificação pública **CMX 3600 Edit Decision List (SMPTE 258M / ASCII EDL)**: cabeçalhos `TITLE:` e `FCM: DROP FRAME | NON-DROP FRAME`, registros `<NUM> <REEL> <CANAL> <MODO> [DUR_QUADROS] <SRC_IN> <SRC_OUT> <REC_IN> <REC_OUT>`, `M2` (*motion/speed*), rolo preto `BL` e comentários `* FROM CLIP NAME:`, `* SOURCE FILE:`, `* LOC:`.
- **Método de implementação:** Sala limpa com suporte a Drop Frame (`;`) / Non-Drop (`:`), normalização opcional da convenção `01:00:00:00`, reconstrução de crossfades em sobreposição e testes de fuzzing por mutação (`fuzz_edl_cmx3600`).

### Legendas SubRip (`.srt`) e W3C WebVTT (`.vtt`) — `src/subtitles.rs`

- **Edição de referência:** Especificações públicas **SubRip Text (`.srt`)** (`HH:MM:SS,mmm --> HH:MM:SS,mmm`) e **W3C WebVTT — *Web Video Text Tracks Format*** (cabeçalho `WEBVTT`, blocos `NOTE`/`STYLE`/`REGION`, deixas `(HH:)?MM:SS.mmm --> ...`, ajustes `align:` / `line:`).
- **Método de implementação:** Sala limpa com aritmética exata de milissegundos em `Ticks`, sanitização linear de tags inline sem retrocesso e criação de faixas superiores `GenSpec::Text` compatíveis com Desfazer e exportação bidirecional (`fuzz_subtitles_srt_and_vtt`).

### AVI (`.avi`, MJPEG + PCM) — `src/media/avi.rs`

- **Edição de referência:** AVI RIFF (documentação pública da Microsoft: `avih`, `strh`, `strf`, `idx1`) e **OpenDML AVI File Format Extensions v1.02** (OpenDML AVI M-JPEG File Format Subcommittee, 28/02/1996): segmentos `RIFF 'AVIX'`, super-índice `indx`, índices padrão `ix##`, `LIST 'odml'` / `dmlh`.
- **Método de implementação:** A leitura percorre todas as listas `movi` (de `RIFF 'AVI '` e `RIFF 'AVIX'`). A escrita grava segmentos de até 1 GB: o primeiro é um AVI 1.0 completo com `idx1`, e os seguintes são `AVIX`. Cada stream ganha um `indx` com até 256 entradas (cerca de 256 GB) e um `ix##` por segmento; passar desse limite é um erro, nunca um arquivo corrompido. Áudio grande é dividido em chunks limitados. O tamanho de cada entrada do super-índice inclui os 8 bytes de cabeçalho do `ix##`, um ponto que a especificação deixa em aberto. Os testes percorrem `indx` → `ix##` → chunk e usam o `ffprobe` como oráculo externo.

### Integrações ArtCraft & Blender — `src/integrations/`

- **Protocolo de Controle ArtCraft (`craft_rpc.rs`, `discovery.rs`):** implementado em Sala Limpa estrita a partir das especificações públicas `docs/control-protocol.md` dos aplicativos da suíte (`PhotoCraft`, `VectorCraft`, `FilmCraft`, `EffectCraft`, `SoundCraft`), mais CLI headless do `AniCraft` (`anicraft-cli new` / `render-seq`):
  - Descoberta por caminhos padrão de instalação, `PATH`, preferência explícita do usuário (`ExternalAppsPrefs`) e arquivos de anúncio em loopback `agent-connection.json` + `agent-control.token` (leituras limitadas a `64 KiB` / `256 B`).
  - Cliente TCP restrito a `127.0.0.1` com limite de linha de `4 MiB`, desafio HMAC-SHA256 em Rust puro (`RFC 2104` / `RFC 6234`, `photocraft-control-v1 server`/`client`) e token por requisição no FilmCraft.
- **Broadcast Wave Format / `bext` (`workflows.rs`):** gravação WAV BWF segundo **EBU Tech 3285 v2** (bloco `bext` de 602 bytes com `Originator = "Neoveg"` e deslocamento exato de amostras em `TimeReference`) para intercâmbio com precisão de amostra no SoundCraft, além de cópias editáveis vinculadas como *Take* (`Add As Takes`) e observador de modificações no disco.
- **Blender fora do processo (`blender.rs`):** invocação em segundo plano via `blender -b ... --factory-startup --python ...` sem linkar código GPL (`AGENTS.md` §2.3), renderização direta de sequências PNG para a timeline (`Fra:<N>`), geração de scripts `bpy` com escape JSON seguro e esquema limitado `neoveg.blender.camera.v1` para importar marcadores e keyframes de *Track Motion* / *Pan/Crop* (`16 MiB`, máx. `100 000` quadros).
