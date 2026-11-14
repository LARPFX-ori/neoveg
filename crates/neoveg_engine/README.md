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

### EDL CMX 3600 (`.edl`) — `src/edl.rs`

- **Edição de referência:** Especificação pública **CMX 3600 Edit Decision List (SMPTE 258M / ASCII EDL)**: cabeçalhos `TITLE:` e `FCM: DROP FRAME | NON-DROP FRAME`, registros `<NUM> <REEL> <CANAL> <MODO> [DUR_QUADROS] <SRC_IN> <SRC_OUT> <REC_IN> <REC_OUT>`, `M2` (*motion/speed*), rolo preto `BL` e comentários `* FROM CLIP NAME:`, `* SOURCE FILE:`, `* LOC:`.
- **Método de implementação:** Sala limpa com suporte a Drop Frame (`;`) / Non-Drop (`:`), normalização opcional da convenção `01:00:00:00`, reconstrução de crossfades em sobreposição e testes de fuzzing por mutação (`fuzz_edl_cmx3600`).

### Legendas SubRip (`.srt`) e W3C WebVTT (`.vtt`) — `src/subtitles.rs`

- **Edição de referência:** Especificações públicas **SubRip Text (`.srt`)** (`HH:MM:SS,mmm --> HH:MM:SS,mmm`) e **W3C WebVTT — *Web Video Text Tracks Format*** (cabeçalho `WEBVTT`, blocos `NOTE`/`STYLE`/`REGION`, deixas `(HH:)?MM:SS.mmm --> ...`, ajustes `align:` / `line:`).
- **Método de implementação:** Sala limpa com aritmética exata de milissegundos em `Ticks`, sanitização linear de tags inline sem retrocesso e criação de faixas superiores `GenSpec::Text` compatíveis com Desfazer e exportação bidirecional (`fuzz_subtitles_srt_and_vtt`).
