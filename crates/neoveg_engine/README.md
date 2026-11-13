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
