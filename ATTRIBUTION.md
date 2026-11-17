# ATTRIBUTION

Índice de assets (imagens, ícones, fontes, sons, presets etc.) presentes no
repositório, conforme o `AGENTS.md` §1. Cada asset `X` tem um arquivo
`X.attribution` ao lado.

Até o momento o repositório **não contém nenhum asset**:

- os ícones da interface são caracteres Unicode desenhados pelas fontes padrão
  do egui (incluídas pela dependência, não copiadas para o repositório);
- os títulos usam fontes do sistema operacional carregadas em tempo de execução
  (`Segoe UI`/`Arial` no Windows, `DejaVu Sans` no Linux), com a fonte padrão
  do egui como alternativa;
- a mídia de teste é gerada pelos próprios testes ou fica fora do controle de
  versão.

## Ícones vetoriais nativos (`crates/neoveg_ui/src/icons.rs`)

- **Recurso:** 69 ícones coloridos de UI (`enum Icon`) desenhados programaticamente via `egui::Painter`
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
