# ATTRIBUTION

Índice de assets (imagens, ícones, fontes, sons, presets etc.) presentes no
repositório, conforme o `AGENTS.md` §1. Cada asset `X` tem um arquivo
`X.attribution` ao lado.

Os arquivos de asset versionados no repositório ficam em `assets/fonts/` (fontes
tipográficas `OFL-1.1` embutidas na interface via `include_bytes!`) e
`docs/screenshots/` (capturas de tela do próprio Neoveg). Fora eles:

- os ícones vetoriais e elementos gráficos da interface são desenhados
  programaticamente via `egui::Painter` ou utilizam as fontes abertas de
  `assets/fonts/` com fallback das fontes padrão embutidas pelo `egui`
  (`Hack`, `Ubuntu-Light`, `NotoEmoji-Regular`, `emoji-icon-font`);
- os títulos, legendas em vídeo (`neoveg_engine::media::generate`) e glifos
  CJK (japonês) carregam fontes do sistema operacional em tempo de execução
  (`Segoe UI`/`Tahoma`/`Arial` e `YuGothic`/`Meiryo`/`MS Gothic` no Windows;
  `Hiragino`/`Osaka` no macOS; `Liberation Sans`/`Noto Sans`/`DejaVu Sans` e
  `Noto Sans CJK` no Linux), sem copiá-las para o repositório (`AGENTS.md` §1.5);
- a mídia de teste é gerada pelos próprios testes ou fica fora do controle de
  versão.

## Fontes tipográficas (`assets/fonts/`)

Todas as fontes distribuídas no repositório e no pacote portátil seguem a licença
`OFL-1.1` (SIL Open Font License 1.1, permitida em `AGENTS.md`), com arquivo
`.attribution` ao lado de cada `.ttf` e texto integral da licença em
`assets/fonts/`:

- **Selawik Regular & SemiBold (`selawk.ttf`, `selawksb.ttf`):**
  - **Recurso:** `assets/fonts/selawk.ttf`, `assets/fonts/selawksb.ttf`
  - **Autor / Origem:** Microsoft Corporation / Aaron Bell (`microsoft/Selawik` v1.01, `Selawik_Release.zip`)
  - **Licença:** `OFL-1.1` (`assets/fonts/Selawik.LICENSE.txt`)
  - **Observações:** Fonte humanista sem serifa metricamente compatível com `Segoe UI`, usada como família proporcional primária da interface (`Selawik-Regular`) e nos destaques/cabeçalhos (`Selawik-SemiBold`).
- **Inter Regular & SemiBold (`Inter-Regular.ttf`, `Inter-SemiBold.ttf`):**
  - **Recurso:** `assets/fonts/Inter-Regular.ttf`, `assets/fonts/Inter-SemiBold.ttf`
  - **Autor / Origem:** The Inter Project Authors / Rasmus Andersson (`rsms/inter` v4.1, `Inter-4.1.zip`)
  - **Licença:** `OFL-1.1` (`assets/fonts/Inter.LICENSE.txt`)
  - **Observações:** Fonte de alta legibilidade numérica usada nas famílias `"visor"` (Visor de tempo, chips de transporte e leitores do Trimmer) e `"ruler"` (Régua SMPTE da timeline), com renderização tabular fixa (`LayoutJob`) para evitar oscilação horizontal, além de fornecer setas e marcadores geométricos (`◀ ▶ ▼ ★ → • ×`).
- **Noto Sans Regular (`NotoSans-Regular.ttf`):**
  - **Recurso:** `assets/fonts/NotoSans-Regular.ttf`
  - **Autor / Origem:** The Noto Project Authors / Monotype Design Team (`notofonts/latin-greek-cyrillic` v2.015, `NotoSans-v2.015.zip`)
  - **Licença:** `OFL-1.1` (`assets/fonts/NotoSans.LICENSE.txt`)
  - **Observações:** Fallback multilíngue e de diacríticos/símbolos (`º`, `ℹ`, acentuação latina, grego e cirílico) atrás de `Selawik` e `Inter`.
- **Noto Sans Math Regular (`NotoSansMath-Regular.ttf`):**
  - **Recurso:** `assets/fonts/NotoSansMath-Regular.ttf`
  - **Autor / Origem:** The Noto Project Authors / Monotype Design Team, Delve Withrington, Jeff Kellem (`notofonts/math` v3.000, `NotoSansMath-v3.000.zip`)
  - **Licença:** `OFL-1.1` (`assets/fonts/NotoSansMath.LICENSE.txt`)
  - **Observações:** Fallback técnico para operadores e símbolos de UI ausentes nas demais fontes (`⋮` `U+22EE`, `─` `U+2500`).

### Estudo comparativo Clean-Room (tipografia inspirada no VEGAS Pro)

Para preservar a densidade visual e legibilidade do VEGAS Pro sem copiar ou redistribuir fontes proprietárias do sistema operacional (`AGENTS.md` §§1.5 e 2.1), foram avaliadas cinco famílias de licença aberta frente às convenções públicas de UI do Windows/VEGAS Pro (`Segoe UI` na interface moderna e `Tahoma` em diálogos/inspetores legados):

| Fonte | Licença SPDX | Equivalência com VEGAS Pro (`Segoe UI` / `Tahoma`) | Papel adotado no Neoveg |
|---|---|---|---|
| **Selawik** | `OFL-1.1` | Criada especificamente como substituta open-source **metricamente compatível com `Segoe UI`**; preserva larguras de linha e altura-x em cabeçalhos de faixa compactos e menus densos. | **Primária da UI** (`selawk.ttf` e `selawksb.ttf`). |
| **Inter** | `OFL-1.1` | Desenhada para interfaces digitais com altura-x elevada, algarismos bem diferenciados e símbolos de UI (`◀ ▶ ▼ ★ →`), superando `Segoe UI`/`Tahoma` em leitura rápida de timecodes; seus dígitos padrão do `cmap` têm avanço proporcional, resolvido por células tabulares em `neoveg_ui::fonts`. | **Visor de tempo e Régua** (`Inter-Regular.ttf`; `Inter-SemiBold.ttf` incluída no conjunto). |
| **Noto Sans** | `OFL-1.1` | Humanista neutra com ampla cobertura Unicode (latino estendido, diacríticos `pt-BR`, grego, cirílico) e métricas próximas às de `Segoe UI`, sem alterar a densidade da interface. | **Fallback multilíngue** (`NotoSans-Regular.ttf`, complementado por `NotoSansMath-Regular.ttf` para `⋮`/`─`). |
| **DejaVu Sans** | `Bitstream-Vera` *(derivada)* | Baseada na Bitstream Vera com vasto repertório de símbolos técnicos, porém com largura de avanço sensivelmente maior que `Segoe UI`/`Tahoma`, causando aperto e quebras em painéis compactos estilo VEGAS. | Avaliada, não adotada como fonte primária. |
| **PT Sans** | `OFL-1.1` | Humanista com boa cobertura latina/cirílica e abertura próxima à família `Tahoma`/`Myriad`, porém sem compatibilidade métrica com `Segoe UI` nem o repertório de setas/símbolos UI de `Inter`. | Avaliada, não adotada como fonte primária. |

## Ícones vetoriais nativos (`crates/neoveg_ui/src/icons.rs`)

- **Recurso:** 146 ícones coloridos/adaptáveis de UI (`enum Icon`) desenhados programaticamente via `egui::Painter`
- **Autor / Origem:** `original work` (contribuidores do projeto Neoveg, criados do zero em código Rust sem cópia ou traçado de terceiros)
- **Licença:** `MIT OR Apache-2.0`
- **Observações:** Nenhuma imagem ou SVG foi importada de bancos com licença restritiva/não-SPDX; os ícones suportam estilos por geração (*Automático*, *Colorido*, *Moderno*, *Clássico* e *Monocromático*), saturação configurável e adaptação automática aos 11 temas de cor (incluindo os 2 temas de Alto Contraste WCAG AAA).

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
