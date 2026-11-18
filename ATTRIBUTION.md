# ATTRIBUTION

Índice de assets (imagens, ícones, fontes, sons, presets etc.) presentes no
repositório, conforme o `AGENTS.md` §1. Cada asset `X` tem um arquivo
`X.attribution` ao lado.

Os únicos arquivos de asset do repositório são as capturas de tela da
interface do próprio Neoveg em `docs/screenshots/` (seção abaixo). Fora elas:

- os ícones da interface são caracteres Unicode desenhados pelas fontes padrão
  do egui (incluídas pela dependência, não copiadas para o repositório);
- os títulos e glifos CJK (japonês) usam fontes do sistema operacional carregadas
  em tempo de execução (`Segoe UI`/`Arial` e `YuGothic`/`Meiryo`/`MS Gothic` no Windows;
  `Hiragino`/`Osaka` no macOS; `DejaVu Sans` e `Noto Sans CJK` no Linux), com a fonte padrão
  do egui como alternativa;
- a mídia de teste é gerada pelos próprios testes ou fica fora do controle de
  versão.

## Ícones vetoriais nativos (`crates/neoveg_ui/src/icons.rs`)

- **Recurso:** 74 ícones coloridos de UI (`enum Icon`) desenhados programaticamente via `egui::Painter`
- **Autor / Origem:** `original work` (contribuidores do projeto Neoveg, criados do zero em código Rust sem cópia ou traçado de terceiros)
- **Licença:** `MIT OR Apache-2.0`
- **Observações:** Nenhuma imagem ou SVG foi importada de bancos com licença restritiva/não-SPDX; os ícones se adaptam automaticamente aos 7 temas de cor (incluindo Alto Contraste WCAG AAA).

## Arte vetorial da Tela de abertura / Splash Screen (`crates/neoveg_ui/src/splash.rs`)

- **Recurso:** Emblema `"N"`, marcador/playhead, onda sonora estilizada e indicador animado de carregamento (`draw_original_spinner` / `spinner_inline`) desenhados programaticamente via `egui::Painter`
- **Autor / Origem:** `original work` (contribuidores do projeto Neoveg, criados do zero em código Rust sem cópia, extração ou traçado de arte proprietária ou referências de terceiros)
- **Licença:** `MIT OR Apache-2.0`
- **Observações:** Adapta-se automaticamente aos 7 temas do Neoveg (incluindo Alto Contraste WCAG AAA) e funciona em hardware limitado (`wgpu` ou fallback `glow`/software).

## Controles vetoriais de Color Grading e Tela de boas-vindas (`crates/neoveg_ui/src/{color_grading,welcome}.rs`)

- **Recurso:** Rodas tonais vetoriais (`Lift`, `Gamma`, `Gain`) e pad bidimensional `Temperatura × Tonalidade` desenhados programaticamente via `egui::Painter`, além da Tela de boas-vindas em duas colunas utilizando o emblema original do projeto
- **Autor / Origem:** `original work` (contribuidores do projeto Neoveg, sem cópia ou importação de bitmaps ou SVG de terceiros)
- **Licença:** `MIT OR Apache-2.0`
- **Observações:** Desenho procedural puro em Rust com adaptação automática aos 7 temas de cor e Alto Contraste WCAG AAA.

## Capturas de tela da interface (`docs/screenshots/`)

- **Recurso:** `janela_principal.png`, `boas_vindas.png`, `efeitos_de_video.png`, `geradores_de_midia.png`, `color_grading.png`, `escopos_de_video.png`, `mixer_e_barramentos.png`, `surround_panner.png`, `medidores_loudness.png`, `propriedades_do_evento.png`, `renderizar_como.png` e `neoveg_encoder.png`, usados no `README.md`
- **Autor / Origem:** `original work` (contribuidores do projeto Neoveg); capturas da interface do próprio Neoveg, renderizadas fora da tela (`1600×900`)
- **Licença:** `MIT OR Apache-2.0`
- **Observações:** Mostram só o Neoveg, com mídia gerada localmente pelo próprio programa ou sintética de teste (gradiente, barras de cor, texto e WAV senoidal), conforme o `AGENTS.md` §1.3; cada imagem tem o seu `.attribution` ao lado.
