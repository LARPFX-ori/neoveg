# NVOV â€” editor de vÃ­deo nÃ£o-linear em Rust puro

Um NLE no estilo do **VEGAS Pro**, construÃ­do a partir dos conceitos, da terminologia e dos atalhos descritos nos manuais em [`manuais/`](manuais/).
SÃ³ usa crates Rust, sem bindings para bibliotecas C (nada de ffmpeg-sys, GTK ou Qt).

| Camada | Crates |
|---|---|
| Interface | `eframe`/`egui` (OpenGL via `glow`), `rfd` (diÃ¡logos de arquivo) |
| Ãudio | `symphonia` (decodificaÃ§Ã£o), `cpal` (saÃ­da WASAPI) |
| Imagem/vÃ­deo | `image` (PNG/JPEG/GIF/BMP/TIFF/WebP), leitor/gravador AVI e Y4M prÃ³prios |
| Texto | `ab_glyph` |
| Paralelismo | `rayon` |

## Compilar e executar

```powershell
cargo run --release                       # interface grÃ¡fica
cargo run --release -- projeto.nvp        # abre um projeto
cargo run --release -- --render projeto.nvp saida.avi   # renderiza sem interface (.avi/.png/.y4m/.wav)
cargo test                                # testes (inclui teste de fumaÃ§a da interface)
```

> Os artefatos de build vÃ£o para `C:/Users/paulomatos/.cargo-targets/nvov` (veja `.cargo/config.toml`),
> para nÃ£o sincronizar gigabytes de arquivos temporÃ¡rios no Google Drive.

## Recursos (mapeados aos capÃ­tulos do manual)

**Janela principal / timeline**
- Lista de trilhas de vÃ­deo e Ã¡udio com nome, mudo, solo, nÃ­vel de composiÃ§Ã£o ou volume, pan, modo de composiÃ§Ã£o, FX e Track Motion; altura ajustÃ¡vel; reordenar, duplicar e excluir.
- RÃ©gua com timecode SMPTE, tempo ou quadros; barra de marcadores e regiÃµes; regiÃ£o de loop; cursor de ediÃ§Ã£o e cursor de reproduÃ§Ã£o.
- Eventos: mover (inclusive entre trilhas), copiar com Ctrl+arrastar, aparar bordas, slip (Alt), slip-trim, fades com 5 curvas, crossfade automÃ¡tico ao sobrepor, agrupamento A/V, travar, loop, reverso, velocidade, congelar quadro.
- Encaixe (snapping) em cursor, marcadores, regiÃµes e bordas; quantizar para quadros; ripple automÃ¡tico.
- EdiÃ§Ã£o: dividir (S), recortar, copiar, colar, colar inserindo, excluir, aparar para a seleÃ§Ã£o, excluir ou inserir intervalo, fechar lacunas, desfazer e refazer (200 nÃ­veis).
- Envelopes de trilha: volume, pan, nÃ­vel de composiÃ§Ã£o e fade para cor, com pontos e curvas.

**Janelas acoplÃ¡veis**
- MÃ­dia do Projeto (miniaturas, arrastar para a timeline), Explorer, TransiÃ§Ãµes, Efeitos de VÃ­deo, Efeitos de Ãudio, Geradores de MÃ­dia, Mixer com medidores de pico, Escopos (histograma, forma de onda, vectorscope, RGB parade) e Notas.
- PrÃ©via de vÃ­deo em trÃªs qualidades (1/4, 1/2 e completa), Ã¡reas de seguranÃ§a e snapshot para PNG.

**AnimaÃ§Ã£o e composiÃ§Ã£o**
- Pan/Crop por evento com keyframes (posiÃ§Ã£o, zoom, rotaÃ§Ã£o) e predefiniÃ§Ãµes Ken Burns.
- Track Motion com keyframes (posiÃ§Ã£o, escala, rotaÃ§Ã£o) e predefiniÃ§Ã£o de picture-in-picture.
- 10 modos de composiÃ§Ã£o: alfa, adicionar, subtrair, multiplicar, screen, overlay, luz forte, clarear, escurecer e diferenÃ§a.

**Efeitos** (todos os parÃ¢metros aceitam keyframes)
- VÃ­deo (18): brilho e contraste, HSL, corretor de cor, nÃ­veis, P&B, sÃ©pia, inverter, desfoque gaussiano, nitidez, vinheta, chroma key, pixelizar, espelhar, glow, granulaÃ§Ã£o, relevo, posterizar, colorir.
- Ãudio (10): ganho, EQ paramÃ©trico de 3 bandas, passa-altas, passa-baixas, compressor, noise gate, delay, reverb (Freeverb), chorus, distorÃ§Ã£o.
- Os efeitos podem ser aplicados no evento, na trilha, no master bus (Ã¡udio) e na saÃ­da de vÃ­deo.
- TransiÃ§Ãµes (16): dissolver, fade pelo preto, wipes, push, slide, Ã­ris, relÃ³gio, porta de celeiro, zoom, persianas e tabuleiro, com borda suave.

**Geradores de mÃ­dia**
- Texto e tÃ­tulos com fonte, contorno, sombra, fundo e alinhamento, e animaÃ§Ãµes de crÃ©ditos rolando, letreiro, mÃ¡quina de escrever e fade.
- Cor sÃ³lida, gradiente linear ou radial, xadrez, barras de cor e ruÃ­do.

**Renderizar como**
- AVI Motion-JPEG com Ã¡udio PCM 16 bits, sequÃªncia PNG (+WAV), Y4M (+WAV) e WAV de 16 bits, 24 bits ou 32 bits float.
- Projeto inteiro ou regiÃ£o de loop, resoluÃ§Ã£o personalizada, barra de progresso com cancelamento.

## Formatos de entrada

| Tipo | Formatos |
|---|---|
| VÃ­deo | AVI (MJPEG ou RGB nÃ£o comprimido, com Ã¡udio PCM), Y4M, GIF animado, sequÃªncias numeradas de imagens |
| Imagem | PNG, JPEG, BMP, TIFF, WebP |
| Ãudio | WAV, MP3, FLAC, OGG/Vorbis, AAC/M4A, ALAC, AIFF, CAF |
| MP4/MOV/MKV/WebM | **somente o Ã¡udio** |

> [!IMPORTANT]
> NÃ£o existe decodificador H.264/H.265/VP9 maduro em Rust puro. Converta antes de importar:
> `ffmpeg -i entrada.mp4 -c:v mjpeg -q:v 3 -c:a pcm_s16le saida.avi`

## Estrutura do cÃ³digo

```
src/
  main.rs            ponto de entrada (GUI ou --render)
  model.rs           projeto, trilhas, eventos, envelopes, keyframes, efeitos (serde -> .nvp JSON)
  media/             importaÃ§Ã£o e cache: AVI, Y4M, GIF, sequÃªncias, Ã¡udio (symphonia), geradores e texto
  engine/            compositor de vÃ­deo, pan/crop, track motion, modos de mistura, efeitos, transiÃ§Ãµes, mixer de Ã¡udio
  render.rs          renderizaÃ§Ã£o final (AVI/PNG/Y4M/WAV)
  playback.rs        saÃ­da de Ã¡udio em tempo real (relÃ³gio mestre) e worker da prÃ©via
  scopes.rs          escopos de vÃ­deo
  ui/                interface: timeline, painÃ©is, diÃ¡logos, operaÃ§Ãµes de ediÃ§Ã£o, atalhos
```

Os atalhos de teclado seguem a tabela padrÃ£o do VEGAS Pro 18 (veja **Ajuda > Atalhos de teclado**).
