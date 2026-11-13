# Licenças de dependências

O Neoveg é distribuído sob `MIT OR Apache-2.0`. As dependências devem usar
identificadores SPDX da lista permitida do `AGENTS.md`. A verificação é feita
por `python tools/check_licenses.py` (portão de qualidade 4).

## Exceções autorizadas

Decisão registrada em 2026-11-13 pelo responsável pelo repositório. Estas
licenças chegam de forma transitiva por dependências que o próprio `AGENTS.md`
exige (`eframe`) ou que mantêm a acessibilidade da interface (`accesskit`), e
não podem ser removidas por configuração de features.

| Licença | Pacotes | Caminho | Motivo |
|---|---|---|---|
| `BSL-1.0` | `clipboard-win`, `error-code` | `eframe` → `egui-winit` (feature `clipboard`, sempre ativa no desktop) | área de transferência do sistema |
| `Unicode-3.0` | `icu_*`, `litemap`, `potential_utf`, `tinystr`, `writeable`, `yoke`, `yoke-derive`, `zerofrom`, `zerofrom-derive`, `zerotrie`, `zerovec`, `zerovec-derive` | `eframe` → `egui-winit` (feature `links`) → `webbrowser` → `url` → `idna` | abertura de links |
| `Zlib` | `foldhash` | `accesskit` → `hashbrown` | acessibilidade (leitores de tela) |
| `Zlib` | `const_panic`, `konst`, `typewit` | `neoveg_scripting` → `netcorehost` → `fn-ptr` | ponte `.NET Host` (`Hostfxr`) para scripts C# (autorizada em 2026-11-13) |
| `MPL-2.0` | `symphonia` e sub-crates (`symphonia-core`, `symphonia-common`, `symphonia-metadata`, `symphonia-bundle-*`, `symphonia-codec-*`, `symphonia-format-*`) | `neoveg_engine` → `symphonia` (`https://github.com/pdeljanov/Symphonia.git`, rev `5f879df0bf309246a48d57caae457fd8c0e2a5cb`) | decodificação ampliada de áudio em Rust puro (AAC, ALAC, ADPCM, CAF, MP1/MP2/MP3, FLAC, Vorbis, PCM, ISOMP4, MKV, Ogg), autorizada pelo responsável em 2026-11-13 |

Qualquer nova licença fora da lista exige nova decisão antes de entrar no
`Cargo.lock` (Regra da Casa 9).

## Complementos e fallbacks de áudio

- Leitor WAVE/AIFF próprio permanece ativo como caminho principal para PCM/IEEE float WAVE/AIFF, com fallback para `symphonia` (ex.: WAV ADPCM / tags ID3).
- `claxon` (`Apache-2.0`), `lewton` (`MIT OR Apache-2.0`) e `puremp3` (`MIT OR CC0-1.0`) permanecem como fallback para FLAC, Ogg Vorbis e MP3.
- Windows Media Foundation (`media::mf`) permanece para vídeo de contêineres (MP4/MOV/MKV/WEBM/WMV/ASF) e como fallback/complemento de áudio (ex.: perfis AAC/M4A não cobertos pelo `symphonia`).
