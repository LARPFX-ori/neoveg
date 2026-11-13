# NVOV — editor de vídeo não-linear em Rust puro

Um NLE no estilo do **VEGAS Pro**, construído a partir dos conceitos, da terminologia e dos atalhos descritos nos manuais em [`manuais/`](manuais/).
Só usa crates Rust, sem bindings para bibliotecas C (nada de ffmpeg-sys, GTK ou Qt).

| Camada | Crates |
|---|---|
| Interface | `eframe`/`egui` (OpenGL via `glow`), `rfd` (diálogos de arquivo) |
| Áudio | `symphonia` (decodificação), `cpal` (saída WASAPI) |
| Imagem/vídeo | `image` (PNG/JPEG/GIF/BMP/TIFF/WebP), leitor/gravador AVI e Y4M próprios |
| Texto | `ab_glyph` |
| Paralelismo | `rayon` |

## Compilar e executar

```powershell
cargo run --release                       # interface gráfica
cargo run --release -- projeto.nvp        # abre um projeto
cargo run --release -- --render projeto.nvp saida.avi   # renderiza sem interface (.avi/.png/.y4m/.wav)
cargo test                                # testes (inclui teste de fumaça da interface)
```

> Os artefatos de build vão para `C:/Users/paulomatos/.cargo-targets/nvov` (veja `.cargo/config.toml`),
> para não sincronizar gigabytes de arquivos temporários no Google Drive.

## Recursos (mapeados aos capítulos do manual)

**Janela principal / timeline**
- Lista de trilhas de vídeo e áudio com nome, mudo, solo, nível de composição ou volume, pan, modo de composição, FX e Track Motion; altura ajustável; reordenar, duplicar e excluir.
- Régua com timecode SMPTE, tempo ou quadros; barra de marcadores e regiões; região de loop; cursor de edição e cursor de reprodução.
- Eventos: mover (inclusive entre trilhas), copiar com Ctrl+arrastar, aparar bordas, slip (Alt), slip-trim, fades com 5 curvas, crossfade automático ao sobrepor, agrupamento A/V, travar, loop, reverso, velocidade, congelar quadro.
- Encaixe (snapping) em cursor, marcadores, regiões e bordas; quantizar para quadros; ripple automático.
- Edição: dividir (S), recortar, copiar, colar, colar inserindo, excluir, aparar para a seleção, excluir ou inserir intervalo, fechar lacunas, desfazer e refazer (200 níveis).
- Envelopes de trilha: volume, pan, nível de composição e fade para cor, com pontos e curvas.

**Janelas acopláveis**
- Mídia do Projeto (miniaturas, arrastar para a timeline), Explorer, Transições, Efeitos de Vídeo, Efeitos de Áudio, Geradores de Mídia, Mixer com medidores de pico, Escopos (histograma, forma de onda, vectorscope, RGB parade) e Notas.
- Prévia de vídeo em três qualidades (1/4, 1/2 e completa), áreas de segurança e snapshot para PNG.

**Animação e composição**
- Pan/Crop por evento com keyframes (posição, zoom, rotação) e predefinições Ken Burns.
- Track Motion com keyframes (posição, escala, rotação) e predefinição de picture-in-picture.
- 10 modos de composição: alfa, adicionar, subtrair, multiplicar, screen, overlay, luz forte, clarear, escurecer e diferença.

**Efeitos** (todos os parâmetros aceitam keyframes)
- Vídeo (18): brilho e contraste, HSL, corretor de cor, níveis, P&B, sépia, inverter, desfoque gaussiano, nitidez, vinheta, chroma key, pixelizar, espelhar, glow, granulação, relevo, posterizar, colorir.
- Áudio (10): ganho, EQ paramétrico de 3 bandas, passa-altas, passa-baixas, compressor, noise gate, delay, reverb (Freeverb), chorus, distorção.
- Os efeitos podem ser aplicados no evento, na trilha, no master bus (áudio) e na saída de vídeo.
- Transições (16): dissolver, fade pelo preto, wipes, push, slide, íris, relógio, porta de celeiro, zoom, persianas e tabuleiro, com borda suave.

**Geradores de mídia**
- Texto e títulos com fonte, contorno, sombra, fundo e alinhamento, e animações de créditos rolando, letreiro, máquina de escrever e fade.
- Cor sólida, gradiente linear ou radial, xadrez, barras de cor e ruído.

**Renderizar como**
- AVI Motion-JPEG com áudio PCM 16 bits, sequência PNG (+WAV), Y4M (+WAV) e WAV de 16 bits, 24 bits ou 32 bits float.
- Projeto inteiro ou região de loop, resolução personalizada, barra de progresso com cancelamento.

## Formatos de entrada

| Tipo | Formatos |
|---|---|
| Vídeo | AVI (MJPEG ou RGB não comprimido, com áudio PCM), Y4M, GIF animado, sequências numeradas de imagens |
| Imagem | PNG, JPEG, BMP, TIFF, WebP |
| Áudio | WAV, MP3, FLAC, OGG/Vorbis, AAC/M4A, ALAC, AIFF, CAF |
| MP4/MOV/MKV/WebM | **somente o áudio** |

> [!IMPORTANT]
> Não existe decodificador H.264/H.265/VP9 maduro em Rust puro. Converta antes de importar:
> `ffmpeg -i entrada.mp4 -c:v mjpeg -q:v 3 -c:a pcm_s16le saida.avi`

## Estrutura do código

```
src/
  main.rs            ponto de entrada (GUI ou --render)
  model.rs           projeto, trilhas, eventos, envelopes, keyframes, efeitos (serde -> .nvp JSON)
  media/             importação e cache: AVI, Y4M, GIF, sequências, áudio (symphonia), geradores e texto
  engine/            compositor de vídeo, pan/crop, track motion, modos de mistura, efeitos, transições, mixer de áudio
  render.rs          renderização final (AVI/PNG/Y4M/WAV)
  playback.rs        saída de áudio em tempo real (relógio mestre) e worker da prévia
  scopes.rs          escopos de vídeo
  ui/                interface: timeline, painéis, diálogos, operações de edição, atalhos
```

Os atalhos de teclado seguem a tabela padrão do VEGAS Pro 18 (veja **Ajuda > Atalhos de teclado**).
