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

Qualquer nova licença fora da lista exige nova decisão antes de entrar no
`Cargo.lock` (Regra da Casa 9).

## Substituições feitas

| Antes | Licença | Depois |
|---|---|---|
| `symphonia` | `MPL-2.0` | leitor WAVE/AIFF próprio, `claxon` (FLAC, `Apache-2.0`), `lewton` (Ogg Vorbis, `MIT OR Apache-2.0`) e `puremp3` (MP3, `MIT OR CC0-1.0`) |

AAC/M4A e o vídeo de MP4/MOV/MKV ficam sem suporte até a integração com a API
de mídia do sistema operacional (fronteira FFI permitida no §0.3).
