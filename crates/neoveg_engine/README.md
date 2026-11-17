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

### AAF (`.aaf`, importação e exportação) — `src/aaf.rs`, `src/aaf_metadict.rs`

- **Edição de referência:** Especificações públicas da AAF Association / AMWA — **MS-03 AAF Low-Level Container Specification v1.0.1** (contêiner Compound File Binary, lido e gravado pelo `cfb`), **MS-02 AAF Stored Format Specification v1.0.1** (stream `properties`: ordem de bytes, contagem, entradas `pid/forma/tamanho`; formas de dado, referência forte, vetor e conjunto de referências fortes com stream `index` ordenado por chave; referência fraca e `/referenced properties`; assinatura de raiz §1.6 `aafSignature_Aaf_SSBinary`), **MS-01 AAF Object Specification v1.1** (§5/§27 `MetaDictionary` com `ClassDefinitions` e `TypeDefinitions`; `Header` → `Identification` / `Dictionary` / `ContentStorage` → Mobs; `CompositionMob`, `MasterMob`, `SourceMob`; `TimelineMobSlot` com `EditRate` e `PhysicalTrackNumber`; `Sequence`, `SourceClip`, `Filler`, `Transition` + `OperationGroup`; `EssenceDescriptor` → `ImportDescriptor` → `NetworkLocator` `URLString`) e **AMWA AS-01 *AAF Edit Protocol*** (2005-04-08, §12.1 `OperationDef_VideoDissolve` e §15 `OpEditProtocol`). Os AUIDs, tipos, rótulos (`PictureEssenceTrack`, `SoundEssenceTrack`, `AAFEditProtocol`) e `LocalIdentification` (*pids*) vêm dos registros públicos da **SMPTE** (`registry.smpte-ra.org`: ST 395, ST 335, ST 2003 e ST 400); o AUID de `Video Dissolve` (`0c3bea40-fc05-11d2-8a29-0050040ef7d2`) vem ainda do vetor binário gerado por `tools/gen_aaf_fixtures.py`.
- **Método de implementação:** Contêiner Compound File lido e gravado pelo crate `cfb` (MIT); na leitura, só os streams de metadados são lidos (até `4 MiB` cada, `200 000` objetos, profundidade `32`, `20 000` clipes) e toda a operação roda sob `catch_unwind`. Na escrita (`export_aaf_bytes` / `export_aaf_file` com gravação atômica via `.part`), o arquivo inclui `/MetaDictionary-1` transitivamente fechado para o submodelo emitido, dissoluções de vídeo viram `Transition` + `OperationGroup` (`Video Dissolve`), lacunas viram `Filler`, e sobreposições de áudio são distribuídas em slots paralelos (sem transição de áudio, pois seu AUID numérico não consta das especificações MS-01/MS-02/MS-03 nem dos registros SMPTE citados).
- **Fixtures:** `tools/gen_aaf_fixtures.py` gera `target/fixtures/oracle_*.aaf` com o pacote externo `pyaaf2` (MIT), usado só como oráculo/gerador externo (como o `ffmpeg`): `PYTHONPATH=<pasta do pyaaf2> python tools/gen_aaf_fixtures.py target/fixtures`. Sem as fixtures, os testes que dependem delas pulam.

### EDL CMX 3600 (`.edl`) — `src/edl.rs`

- **Edição de referência:** Especificação pública **CMX 3600 Edit Decision List (SMPTE 258M / ASCII EDL)**: cabeçalhos `TITLE:` e `FCM: DROP FRAME | NON-DROP FRAME`, registros `<NUM> <REEL> <CANAL> <MODO> [DUR_QUADROS] <SRC_IN> <SRC_OUT> <REC_IN> <REC_OUT>`, `M2` (*motion/speed*), rolo preto `BL` e comentários `* FROM CLIP NAME:`, `* SOURCE FILE:`, `* LOC:`.
- **Método de implementação:** Sala limpa com suporte a Drop Frame (`;`) / Non-Drop (`:`), normalização opcional da convenção `01:00:00:00`, reconstrução de crossfades em sobreposição e testes de fuzzing por mutação (`fuzz_edl_cmx3600`).

### Legendas SubRip (`.srt`) e W3C WebVTT (`.vtt`) — `src/subtitles.rs`

- **Edição de referência:** Especificações públicas **SubRip Text (`.srt`)** (`HH:MM:SS,mmm --> HH:MM:SS,mmm`) e **W3C WebVTT — *Web Video Text Tracks Format*** (cabeçalho `WEBVTT`, blocos `NOTE`/`STYLE`/`REGION`, deixas `(HH:)?MM:SS.mmm --> ...`, ajustes `align:` / `line:`).
- **Método de implementação:** Sala limpa com aritmética exata de milissegundos em `Ticks`, sanitização linear de tags inline sem retrocesso e criação de faixas superiores `GenSpec::Text` compatíveis com Desfazer e exportação bidirecional (`fuzz_subtitles_srt_and_vtt`).

### AVI (`.avi`, MJPEG / RGB24 / YUV + PCM / G.711) — `src/media/avi.rs`

- **Edição de referência:** AVI RIFF (documentação pública da Microsoft: `avih`, `strh`, `strf`, `idx1`, bitmaps `BI_RGB`/`DIB `, `YUY2`/`UYVY`, `I420`/`IYUV`) e **OpenDML AVI File Format Extensions v1.02** (OpenDML AVI M-JPEG File Format Subcommittee, 28/02/1996): segmentos `RIFF 'AVIX'`, super-índice `indx`, índices padrão `ix##`, `LIST 'odml'` / `dmlh`.
- **Método de implementação:** A leitura percorre todas as listas `movi` (de `RIFF 'AVI '` e `RIFF 'AVIX'`), decodifica `MJPEG`, `BI_RGB`/`DIB ` (24/32 bpp) e YUV (`YUY2`/`UYVY` 4:2:2 e `I420`/`IYUV` 4:2:0) e áudio PCM/G.711. A escrita grava `MJPEG` ou `AVI Não Comprimido RGB24` (`AviVideoCodec::UncompressedRgb24`) em segmentos de até 1 GB: o primeiro é um AVI 1.0 completo com `idx1`, e os seguintes são `AVIX`. Cada stream ganha um `indx` com até 256 entradas (cerca de 256 GB) e um `ix##` por segmento; passar desse limite é um erro, nunca um arquivo corrompido. Áudio grande é dividido em chunks limitados. O tamanho de cada entrada do super-índice inclui os 8 bytes de cabeçalho do `ix##`, um ponto que a especificação deixa em aberto. Os testes percorrem `indx` → `ix##` → chunk e usam o `ffprobe` como oráculo externo.

### Integrações ArtCraft & Blender — `src/integrations/`

- **Protocolo de Controle ArtCraft (`craft_rpc.rs`, `discovery.rs`):** implementado em Sala Limpa estrita a partir das especificações públicas `docs/control-protocol.md` dos aplicativos da suíte (`PhotoCraft`, `VectorCraft`, `FilmCraft`, `EffectCraft`, `SoundCraft`), mais CLI headless do `AniCraft` (`anicraft-cli new` / `render-seq`):
  - Descoberta por caminhos padrão de instalação, `PATH`, preferência explícita do usuário (`ExternalAppsPrefs`) e arquivos de anúncio em loopback `agent-connection.json` + `agent-control.token` (leituras limitadas a `64 KiB` / `256 B`).
  - Cliente TCP restrito a `127.0.0.1` com limite de linha de `4 MiB`, desafio HMAC-SHA256 em Rust puro (`RFC 2104` / `RFC 6234`, `photocraft-control-v1 server`/`client`) e token por requisição no FilmCraft.
- **Broadcast Wave Format / `bext` (`workflows.rs`):** gravação WAV BWF segundo **EBU Tech 3285 v2** (bloco `bext` de 602 bytes com `Originator = "Neoveg"` e deslocamento exato de amostras em `TimeReference`) para intercâmbio com precisão de amostra no SoundCraft, além de cópias editáveis vinculadas como *Take* (`Add As Takes`) e observador de modificações no disco.
- **Blender fora do processo (`blender.rs`):** invocação em segundo plano via `blender -b ... --factory-startup --python ...` sem linkar código GPL (`AGENTS.md` §2.3), renderização direta de sequências PNG para a timeline (`Fra:<N>`), geração de scripts `bpy` com escape JSON seguro e esquema limitado `neoveg.blender.camera.v1` para importar marcadores e keyframes de *Track Motion* / *Pan/Crop* (`16 MiB`, máx. `100 000` quadros).

### Demuxer MP4 / MOV / MKV / WebM — `src/media/demux/`

- **Edição de referência:**
  - **ISO/IEC 14496-12** (*Information technology — Coding of audio-visual objects — Part 12: ISO Base Media File Format*) e **Apple *QuickTime File Format Specification*** (`ftyp`, `moov`/`mvhd`/`tkhd`/`edts`/`elst`/`mdia`/`mdhd`/`hdlr`/`minf`/`stbl` com `stsd`/`stts`/`ctts`/`stsc`/`stsz`/`stz2`/`stco`/`co64`/`stss`, extensões de fragmentos `mvex`/`trex`/`moof`/`mfhd`/`traf`/`tfhd`/`tfdt`/`trun`, e descriptores `avcC`/`hvcC`/`av1C`/`vpcC`/`esds`/`wave`);
  - **RFC 8794** (*Extensible Binary Meta Language — EBML*) e **RFC 9559** (*Matroska Media Container Format*) / WebM Container Guidelines (`EBML`, `Segment`, `Info`/`TimestampScale`, `Tracks`/`TrackEntry`/`CodecPrivate`, `Cluster`/`Timestamp`/`SimpleBlock`/`BlockGroup`/`Block`/`BlockDuration`/`ReferenceBlock`, e modos de *lacing* Xiph, Fixed-size e EBML).
- **Método de implementação:** Leitores em Rust puro (`#[forbid(unsafe_code)]`) sobre `Read + Seek`, limitando profundidade (`32`), trilhas (`64`), tabelas de metadados em memória (`64 MiB`), extradata (`1 MiB`) e número de amostras (`2 000 000`) sem carregar `mdat` ou payloads de mídia para a RAM, com conversão racional exata para `Ticks` em `i128` (`AGENTS.md` §0.4, §3.6 e Regra Rust 5). Validado contra oráculo externo `ffprobe -show_packets` e testes de fuzzing por mutação em `fuzz_demux_mp4_e_mkv`.


### Formatos Extras de Áudio e Extensões RIFF/AIFF — `src/media/wav.rs` e `src/media/audio_extra.rs`

- **Edições de referência:**
  - **Microsoft WAVE / RIFF** (`WAVEFORMATEX`, tags `0x0001` PCM, `0x0003` IEEE Float, `0x0006` ITU-T G.711 A-law, `0x0007` ITU-T G.711 µ-law, `0x0011` Microsoft IMA/DVI ADPCM com passo de 89 entradas) e `WAVE_FORMAT_EXTENSIBLE` (`0xFFFE`);
  - **EBU Tech 3285** (*Broadcast Wave Format — BWF*, chunk `bext`) e **EBU Tech 3306** (*RF64: An Extended File Format for Audio*, chunk `ds64`);
  - **Apple AIFF 1.3** e **AIFF-C** (`COMM` com `extended80`, `FVER`, `SSND`, codecs `NONE`/`sowt`/`fl32`/`fl64`/`ulaw`/`alaw`);
  - **Sun / NeXT AU / SND** (`.snd` big-endian: PCM 8/16/24/32, Float 32/64 e ITU-T G.711 µ-law/A-law);
  - **Apple Core Audio Format (CAF v1)** (`caff`, chunks de 64 bits `desc`, `data`);
  - **Sony Pictures Digital Wave64** (`.w64`, GUIDs 128-bit `riff`, `wave`, `fmt `, `data`);
  - **Creative Voice (`.voc`)** (v1.10/v1.20: blocos 0, 1, 2, 6, 7, 8, 9);
  - **EA IFF 85 `8SVX`** (`.8svx`, `.svx`: `VHDR`, `BODY`, PCM 8 bits e compressão *Fibonacci Delta*);
  - **Quite OK Audio (`.qoa`, especificação v1)** (cabeçalho `qoaf`, quadros e fatias com preditor *Sign-Sign LMS* de 4 estágios em blocos de até 20 amostras).
- **Método de implementação:** Sala limpa estrita com limites explícitos de alocação/amostras e suíte de fuzzing por mutação (`fuzz_audio_extra`).

### Formatos Adicionais de Imagem e Sequências — `src/media/img_extra.rs`

- **Edições de referência:** **ANSI/SMPTE 268M-2003** (*DPX File Format*, big/little-endian 8/10/16 bits), **Kodak Cineon File Format v4.5** (`.cin`, cabeçalho 1024 B e empacotamento 10 bits), **WAP Wireless Bitmap Type 0** (`.wbmp`, *uintvar* multibyte + 1 bpp), **X Window System XBM & XPM v3** (`.xbm`, `.xpm` ASCII), **EA IFF 85 ILBM / PBM** (`.iff`, `.ilbm`, `.lbm`, `BMHD`/`CMAP`/`CAMG` EHB/HAM/`BODY` com compressão `ByteRun1`), além dos formatos licenciados do crate `image` 0.25 (`TGA`, `PNM`/`PBM`/`PGM`/`PPM`/`PAM`, `ICO`/`CUR`, `Radiance HDR`, `DDS`, `Farbfeld`, `QOI`, `OpenEXR`, `WebP` estático/animado).
- **Método de implementação:** Leitores e escritores em sala limpa protegidos por `MAX_DIM` / orçamento de 512 MiB e testados sob mutação (`fuzz_imagens_extras`) e oráculo `ffmpeg`/`ffprobe` (`dpx`).

### Media Foundation no Windows (Decodificação e Codificação) — `src/media/mf/{mod,encode}.rs`

- **Edição de referência:** Documentação pública da Microsoft Win32 Media Foundation (`IMFSourceReader`, `IMFSinkWriter`, `MFTranscodeGetAudioOutputAvailableTypes`, formato de entrada `MFVideoFormat_NV12`).
- **Método de implementação:** Fronteira FFI isolada com RAII e `// SAFETY:`, gravação atômica em arquivo temporário com `drop` explícito antes da renomeação e testes locais com oráculo `ffprobe`.

### Neoveg Encoder e LUTs `.cube` 1D/3D — `src/encoder/`

- **Edição de referência:** Especificação pública do formato **Cube LUT v1.0** (`TITLE`, `LUT_1D_SIZE`, `LUT_3D_SIZE`, `DOMAIN_MIN`, `DOMAIN_MAX`) e fluxos de codificação em lote (*Fila*, *Codificação* paralela, *Predefinições* `.neovegpreset`/grupos/apelidos, *Pastas Monitoradas*, Corte/5 modos de escala, sobreposições e registros).
- **Método de implementação:** 100% Rust seguro (`#[forbid(unsafe_code)]`) com limites rígidos em JSON e tabelas `.cube` (`LUT_1D_SIZE <= 4096`, `LUT_3D_SIZE <= 65`), codificação paralela protegida por `catch_unwind` e testes de fuzzing por mutação (`fuzz_encoder_parsers`).
