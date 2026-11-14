# AGENTS.md — Regras de Engenharia e Governança do Neoveg

  

Estas regras se aplicam a todo contribuidor, humano ou agente de IA, em qualquer repositório do projeto **Neoveg**. Elas vêm em três camadas: as Regras da Casa (todo repositório), as regras de Repositórios Rust do Craft e as regras específicas do Neoveg (§0 a §3). Uma camada posterior pode ser mais rígida que a anterior, nunca mais branda. `CLAUDE.md` contém as instruções de trabalho; este arquivo contém as regras que nunca devem ser quebradas. Quando os dois divergirem, este arquivo prevalece.

  

---

  

## Regras da Casa

  

Estas regras valem em todo repositório do Neoveg. Uma regra posterior pode ser mais rígida, não mais branda. A fonte é `standards/rules-core.md` em `neoveg/contribution`. Proponha mudanças lá, não em uma cópia.

  

1. **Uma pessoa é responsável.** Quem executa um agente responde por tudo o que ele abre (issues, pull requests, commits) e pelos seus comentários de revisão.

2. **Verifique, depois reivindique.** Antes de escrever código, leia os comentários da issue, pesquise pull requests por `#N` e palavras-chave e confira os commits recentes em `main`. Comente na issue primeiro. Libere uma reivindicação que você não vai concluir.

3. **Confirme, não adivinhe.** Abra a definição de uma função, struct ou comando antes de usá-la. Confirme a causa no código, mesmo quando alguém indicar um local.

4. **Mantenha as correções restritas e testadas.** Corrija apenas o que a issue descreve, onde o bug reside, e adicione um teste que falhe sem a correção.

5. **Revise o diff antes de abrir um pull request.** Os identificadores existem como são usados, o teste falha sem a correção, nada não relacionado foi alterado e a descrição é verdadeira.

6. **Procure um pull request melhor.** Se outro PR corrige a mesma coisa, feche o seu quando o outro for melhor ou equivalente, e diga por quê.

7. **Relate apenas o que foi executado.** Diga o que você testou e o que não conseguiu executar (ex.: build cross-platform). Sem build ou teste local, diga isso e conte com a CI antes de marcar como pronto.

8. **`main` muda por meio de pull request.** Não faça push em `main`. A proteção de branch impõe isso. Não há exceção.

9. **Pare e pergunte apenas sobre:** licenciamento fora da lista de permissões, publicação (site, pacotes), releases, um novo remote, segredos, gastos de dinheiro, `git` destrutivo e qualquer outra coisa difícil de desfazer. Caso contrário, continue trabalhando. `plan/` é um caderno ignorado pelo git, não regras. Sem ele, não peça por ele e não pare.

10. **Mantenha segredos e dados pessoais fora** do repositório, dos logs (`logs/neoveg.log`, `logs/crash.log`) e das requisições.

11. **A CI permanece compartilhada.** Os workflows em `.github/workflows/` chamam `neoveg/common-actions`. Não altere a CI em um pull request de funcionalidade ou de correção. Se a CI não conseguir cobrir uma mudança, abra uma issue lá (veja *Workflows de CI*).

12. **Proteções dizem por quê.** Uma verificação que parece removível (lint, guarda de invariante, checagem de CI) diz o que ela previne. Leia isso antes de alterá-la.

13. **As instruções devem ser legíveis por todos.** Nada de notas sobre uma única máquina (caminhos absolutos, configurações locais) e nada de links para arquivos que o leitor não consegue abrir. Não crie links para ele.

14. **Mantenha os pull requests andando.** Pull requests simultâneos são aceitáveis; feedback acionável sem resposta não é. Responda em 7 dias; lembrete aos 14; elegível para fechamento aos 30, contados a partir do momento em que uma solicitação acionável aguarda o autor. Sugestões e achados de bots não confirmados não iniciam a contagem. Uma correção, explicação ou plano acordado a reinicia; atividade não relacionada, não. Aguardar um revisor ou mantenedor a pausa e não conta contra o autor. Um mantenedor decide o fechamento; ele nunca é automático. Reutilize resultados de busca, não faça polling e pare ao atingir um limite de taxa.

15. **Sem atribuição a agentes.** Um pull request ou mensagem de commit não deve declarar que foi escrito por um agente.

  

---

  

## Regras Rust do Neoveg

  

1. **Nunca travar (Zero Panic em Produção).** Código fora de `#[cfg(test)]` não entra em pânico e não usa `unsafe`, exceto nas fronteiras FFI que este arquivo nomeia (§0, regra 3). Retorne `Result<T, E>`, trate toda entrada como hostil e limite recursão e alocações. Novas raízes de crate de produção copiam o atributo `deny` de um crate existente: `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, `clippy::unimplemented`, `clippy::todo`, `clippy::unreachable`. Toda correção de travamento vem com um teste que entrava em pânico antes da correção.

2. **Sala limpa (Clean Room) e Semântica VEGAS Pro.** Aprenda a partir do comportamento observado do VEGAS Pro, dos manuais oficiais de usuário e de especificações técnicas públicas. Não copie código, assets, ícones, binários, strings ou artes de outro produto. Verifique a licença de uma dependência antes de adicioná-la.

3. **Assets e fontes.** Todo asset é original ou devidamente licenciado sob a lista SPDX permitida, acompanhado de arquivo `.attribution` e registrado em `ATTRIBUTION.md`. Não faça commit de arquivos de fonte proprietários do sistema operacional (como `segoeui.ttf` ou `tahoma.ttf`).

4. **Apenas Rust nativo para o editor desktop.** Nada de JavaScript ou TypeScript escrito à mão, e nada de Tauri, Electron ou shell de webview, na aplicação desktop (`neoveg`). Toda a interface gráfica é construída nativamente em Rust (`eframe` + `wgpu`). Ferramentas compartilhadas seguem seu escopo técnico documentado e as exceções aprovadas pelos mantenedores; nenhuma delas flexibiliza esta regra para o app. Preserve qualquer build web existente.

5. **Tempo exato.** Todo cálculo de timeline, régua SMPTE, quadros e áudio usa inteiros exatos (ticks/amostras). Nunca use `f64` em segundos para aritmética de edição. Datas de calendário passam por `chrono`.

6. **Créditos de contribuidores.** Adicione ou edite apenas a entrada da sua própria pessoa em `contributors/people.toml`, e somente depois de perguntar a ela. Nunca adivinhe um nome nem edite `contributors/contributors.json` à mão (veja *Créditos de Contribuidores*).

  

---

  

## Lista de Licenças Permitidas (SPDX)

  

Dependências (`Cargo.toml`) e assets devem usar identificadores SPDX desta lista (o projeto **Neoveg** é distribuído sob `MIT OR Apache-2.0`):

  

- `MIT`

- `Apache-2.0`

- `BSD-2-Clause`

- `BSD-3-Clause`

- `ISC`

- `OFL-1.1`

- `Ubuntu-font-1.0`

- `Bitstream-Vera`

- `CC0-1.0`

- `CC-BY-4.0`

- `CC-BY-SA-4.0`

- `LicenseRef-Public-Domain`

  

---

  

## 0. Nunca Travar (Precedência Máxima)

  

Editores de vídeo gerenciam horas de trabalho do usuário. Um arquivo de mídia corrompido, um projeto `.neoveg` malformado, um script `.cs` defeituoso, um dispositivo de áudio desconectado ou disco cheio devem gerar um erro recuperável, **nunca um travamento (`panic`) e nunca perda silenciosa de dados**. **Esta regra tem precedência sobre qualquer entrega de funcionalidade:** não entregue uma funcionalidade adicionando um caminho de pânico, e corrija um travamento antes de construir em cima dele.

  

1. **Falhe com `Result<T, E>`.** Propague erros com `?` adicionando contexto relevante. Na UI (`neoveg_ui`), exiba o erro na barra de status ou diálogo padrão e mantenha a sessão ativa.

2. **Proibição de macros de pânico em produção:** É proibido usar `unwrap()`, `expect()`, `panic!()`, `unreachable!()`, `todo!()` ou `unimplemented!()` em código não-teste. Use `?`, `ok_or(...)?`, `let Some(x) = ... else { return Err(...) }` ou fallbacks seguros que não corrompam o estado do projeto. Funcionalidades inacabadas retornam erro explícito de não suportado. A única exceção é uma chamada comprovadamente infalível apenas pelo código local (ex.: um literal que sempre é analisado com sucesso): um único `#[allow(clippy::expect_used)]` no item, com o motivo.

3. **Política estrita de `unsafe`:** O workspace adota `unsafe_code = "forbid"` por padrão. As únicas exceções permitidas são módulos isolados de fronteira FFI (`#[allow(unsafe_code)]`) devidamente encapsulados em APIs seguras que retornam `Result`:

   - Bindings de mídia e decodificação/codificação por hardware (`ffmpeg-next` / GPU);

   - Ponte de interoperabilidade `.NET Host` (`neoveg_scripting` / `ScriptPortal.Vegas`);

   - Handles de sistema operacional para áudio/janela (`cpal` / Win32 /Wayland) e hosts isolados de plug-ins (VST3 / OFX).

   - Todo bloco `unsafe` exige comentário `// SAFETY:` justificando as pré-condições.

4. **Toda entrada externa é hostil.** Arquivos de mídia, contêineres `.neoveg` (ZIP/JSON), arquivos de intercâmbio (EDL CMX 3600, FCP7 XML, FCPXML, OTIO, AAF, SRT/WebVTT), presets, scripts C#, parâmetros CLI e estado de UI não são confiáveis:

   - Indexe e fatie slices com `.get(...)` e strings apenas em limites válidos de caractere;

   - Use aritmética `checked_*` e `saturating_*` para offsets, comprimentos e contagens;

   - Nunca divida por valores potencialmente nulos (FPS, sample rate, largura de régua);

   - Nunca converta `NaN`, infinito ou negativos para inteiros sem validação;

   - Não chame `clamp` com limites que podem se cruzar;

   - Limite alocações de memória dimensionadas por cabeçalhos de arquivos ou por qualquer outra entrada.

5. **Limite recursão e laços.** Projetos aninhados (*nested projects*), árvores pai/filho de faixas e roteamento de barramentos (*buses*) podem conter ciclos: valide aciclicidade na inserção e imponha limites explícitos de profundidade na travessia.

6. **Isolamento de threads e locks.** Envenenamento de `Mutex`/`RwLock` não é fatal: use `.lock().unwrap_or_else(PoisonError::into_inner)`. Workers de áudio, decodificação de vídeo, relink de mídia em segundo plano e renderização devem rodar protegidos por `std::panic::catch_unwind`, reportando falhas como erro sem travar a UI: um worker morto deixa monitores em branco ou jobs “em execução” para sempre.

7. **Rede de segurança de último recurso.** Mantenha `panic = "unwind"`. O hook de pânico registra stack traces em `logs/crash.log` e preserva o autosave/recuperação do projeto, mas não substitui as regras 1–6.

8. **Prove com testes.** Toda correção de travamento exige um teste de regressão sintético que falhava antes da correção. Parsers de projeto, intercâmbio e mídia recebem testes de fuzzing por mutação (truncamento, inversão de bits, tamanhos corrompidos) executados sob `catch_unwind`; novos comandos recebem testes com parâmetros hostis. Achados do fuzzer viram testes.

9. **Lints obrigatórios:** Crates de produção declaram `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`. `clippy.toml` os permite em testes: um teste que falha deve falhar ruidosamente. Migre para `[workspace.lints.clippy]` quando todos os crates estiverem limpos.

  

---

  

## 1. Assets: Nenhuma Arte Proprietária, Todo Asset Licenciado e Atribuído

  

Esta regra é absoluta. Quebrá-la é o erro mais grave que um contribuidor pode cometer neste projeto. **Asset** é qualquer imagem, ícone, cursor, logotipo, ilustração, captura de tela, fonte, LUT, preset, template, som, música, vídeo ou outra mídia que não seja código, incluindo bytes embutidos no código (`include_bytes!`, base64, URIs de dados) e dados fixos que reproduzem uma imagem (ex.: listas de pontos traçadas do ícone de outra pessoa).

  

1. **Zero assets proprietários (MAGIX / Sony VEGAS Pro, Adobe ou terceiros):**

   - Nunca copie, extraia, trace ou redistribua ícones, cursores, telas de abertura (*splash screens*), bitmaps de interface, LUTs, presets, efeitos sonoros ou capturas de tela do VEGAS Pro ou de qualquer software proprietário.

   - Os ícones podem seguir convenções universais de edição (play, corte, marcador, envelope, cadeado), mas devem ser desenhados do zero em código vetorial/SVG ou BMP (`neoveg_ui`), sem referência à arte do VEGAS Pro: nada de cópias traçadas, redesenhadas, recoloridas ou “inspiradas em”.

   - Marcas de terceiros (logotipos da MAGIX, VEGAS, Adobe, Discord, GitHub etc.) ficam de fora; desenhe um ícone genérico no lugar.

   - Se a licença de um asset for desconhecida ou pouco clara, ele não entra. Na dúvida, deixe de fora e desenhe ou gere você mesmo.

2. **Atribuição obrigatória (Sidecar + Índice):** Para todo arquivo de asset `X` adicionado ao repositório, crie `X.attribution` ao lado e registre-o em `ATTRIBUTION.md`:

   ```text

   asset:        <nome do arquivo>

   title:        <descrição>

   author:       <criador / detentor dos direitos>

   source:       <URL, ou "original work" para assets criados por contribuidores>

   license:      <identificador SPDX ou "MIT OR Apache-2.0">

   license-file: <caminho para o texto da licença, se exigido>

   added:        <YYYY-MM-DD> por <contribuidor>

   notes:        <como foi criado ou adaptado>

   ```

3. **Capturas de tela:** Screenshots no repositório devem exibir exclusivamente a interface do **Neoveg** (ou de outros projetos abertos), com mídia gerada por nós ou licenciada abertamente, de preferência os filmes da Blender Foundation (`CC-BY-4.0`, com atribuição). Nunca faça commit de capturas de tela do VEGAS Pro ou de outro produto proprietário. A mídia de teste não é commitada: busque-a fixada por commit e verificada por sha256 (veja §3.4).

4. **Material de referência local:** Manuais do VEGAS Pro, notas de estudo e capturas de tela de referência ficam exclusivamente em pastas locais ignoradas pelo git (`plan/vegas/`), nunca commitadas, embutidas no binário ou empacotadas no build portátil.

5. **Tipografia:** Use fontes do sistema operacional em tempo de execução (`Segoe UI` / `Tahoma` no Windows; `Liberation Sans` / `Noto Sans` no Linux/macOS) ou fontes abertas (`OFL-1.1` / `Ubuntu-font-1.0`) devidamente atribuídas. Nunca copie arquivos `.ttf` proprietários do Windows para o repositório.

  

---

  

## 2. Sala Limpa e Preservação da Semântica do VEGAS Pro

  

1. **Engenharia reversa proibida:** Nunca descompile, faça engenharia reversa ou inspecione binários/assemblies dentro da instalação do VEGAS Pro. Apenas nomes de arquivos e listagens são permitidos. Toda paridade funcional deve derivar da observação da interface, dos manuais públicos e de especificações abertas; nunca capture a tela inicial, os projetos recentes, as informações de conta ou os navegadores de arquivos dele.

2. **Semântica e UI/UX intocáveis do VEGAS Pro:** Toda funcionalidade nova — inclusive recursos inspirados em outros motores (como FilmCraft, Whisper AI, OTIO ou VST3/OFX) — deve ser obrigatoriamente encapsulada na identidade visual e operacional do VEGAS Pro:

   - **Densidade visual e temas:** Layout compacto de alta densidade, cabeçalhos de faixa com controles diretos (`Mute`, `Solo`, `Level`, `Pan`, `FX`, `Automation`), painéis acopláveis/destacáveis com abas no rodapé e suporte integral aos 7 temas (incluindo *High Contrast AAA*).

   - **Comportamento de Timeline:** Auto-crossfade automático ao sobrepor eventos na mesma faixa, alças de fade nos cantos superiores do evento, alça de ganho no topo, playhead físico de 1 pixel, régua SMPTE com drop-frame e controle de *Playback Rate*.

   - **Mapeamento de Teclado Clássico:** Preservar atalhos canônicos (`S` dividir, `M` marcador, `R` região, `J/K/L` scrub, `G`/`U` agrupar/desagrupar, `Z`/`X` mudo/solo, `Ctrl+L` Auto Ripple, `Alt+[` / `Alt+]` aparo, `Ctrl+T` aparar à seleção).

3. **Codecs e FFmpeg (Conformidade LGPL):**

   - Implemente parsers e formatos de intercâmbio (`.neoveg`, EDL, FCP7 XML, FCPXML, OTIO, SRT/WebVTT) a partir de especificações públicas. Não leia o código-fonte de implementações de referência ou de terceiros enquanto implementa um formato, mesmo as de licença permissiva; use apenas o texto da especificação e vetores de conformidade. Registre a edição da especificação usada no README do crate.

   - `ffmpeg`/`ffprobe` podem ser usados apenas como oráculos de teste externos e geradores de fixtures. Eles nunca são linkados, empacotados ou distribuídos. A decodificação/codificação por hardware passa por bindings Rust das APIs de mídia do sistema operacional, com um decodificador próprio como fallback.

   - Nunca copie código GPL/LGPL/AGPL (FFmpeg, x264, x265, MLT, Kdenlive, Shotcut, Olive, LAME…).

  

---

  

## 3. Regras de Engenharia e Arquitetura (Workspace de 5 Crates)

  

### 3.1 Invariantes de Edição

- **Unicidade de identidade em movimentações:** Todo plano de movimentação lista cada ID de evento/clipe no máximo uma vez. Solicitações explícitas com IDs duplicados são rejeitadas atomicamente antes de mutar a timeline, alocar novos IDs ou tocar o histórico de desfazer/refazer (`history.json`); colocar uma identidade em várias faixas torna ambíguas a seleção e a edição posterior. Cópias recebem novos IDs e mantêm o destino solicitado de cada entrada. A criação da faixa de destino e a edição são confirmadas juntas. Planos derivados da seleção mantêm cada identidade uma única vez, na ordem da primeira ocorrência, antes de expandir eventos agrupados.

- **Imutabilidade do formato do projeto após escolha:** O primeiro arquivo de mídia importado define o formato do projeto apenas se o usuário ainda não tiver configurado as propriedades do projeto manualmente.

  

### 3.2 Separação de Camadas (5 Crates)

1. **`neoveg_common`:** Tipos fundamentais, representação exata de tempo (ticks/amostras), curvas de fade/envelopes, tipos de erro e serialização de preferências. Zero dependência de UI, GPU ou áudio de SO.

2. **`neoveg_engine`:** Estado da timeline, seleção, snapping, regras de edição (*Normal, Slip, Slide, Stretch, Roll, Shuffle, Ripple*), mixer multifaixa, barramentos (até 26 + Master), DSP nativo, grafo de composição de vídeo, persistência `.neoveg` e **Tabela Única de Comandos**. Não depende de `egui`, `eframe` ou `winit`.

3. **`neoveg_scripting`:** Ponte `.NET Host` (`ScriptPortal.Vegas`) para retrocompatibilidade com scripts C# (`.cs`), extensões e automação CLI/headless.

4. **`neoveg_ui`:** Interface gráfica em `eframe` + `wgpu`, timeline, waveform amostra a amostra, escopos de vídeo (*Histogram, Waveform, Vectorscope*), janelas *Pan/Crop* e *Track Motion*, gerenciador de layouts (`.neoveglayout`) e localização (`pt-BR` e demais idiomas).

5. **`neoveg` (Binário Principal & CLI):** Inicialização, configuração de logs (`logs/neoveg.log`, `logs/crash.log`), argumentos de linha de comando (`--open`, `--run-script`) e integração com o empacotamento portátil (`build_portable.py`).

  

### 3.3 Tabela Única de Comandos e Automação

- Toda ação editável deve ser registrada na **Tabela Única de Comandos** (`id`, rótulo localizado, caminho de menu, atalho regravável, `enabled()`, `run()`).

- Menus, barras de ferramentas, atalhos de teclado, scripts `.NET` e comandos CLI despacham exclusivamente pelo `id` do comando, garantindo paridade de comportamento e suporte automático ao histórico de Desfazer/Refazer.

  

### 3.4 Portões de Qualidade (Quality Gates)

Antes de abrir ou atualizar qualquer pull request, verifique localmente:

1. `cargo fmt --all -- --check`

2. `cargo clippy --workspace --all-targets -- -D warnings`

3. `cargo test --workspace`

4. Verificação de licenças e entradas em `ATTRIBUTION.md`

5. Empacotamento portátil (`python build_portable.py`) quando houver alterações de recursos, scripts ou binários.

  

Novos crates do workspace incluem `[lints] workspace = true` no `Cargo.toml`. As raízes de crates de produção declaram os atributos de lint em nível de crate antes dos módulos; a documentação `//!` pode precedê-los. Gere fixtures com `ffmpeg` em `target/fixtures/` e não faça commit de mídia. Corpora de teste com arquivos reais são buscados fixados por commit e verificados por sha256, nunca commitados neste repositório.

  

### 3.5 Fluxo de Branches e Commits

- Crie uma branch descritiva a partir de `main` (`feat/...`, `fix/...`, `docs/...`).

- Faça commits atômicos e focados em uma única tarefa por vez, mantendo a árvore sempre compilável e verde.

- Documente o progresso de paridade em `README.md`, `ROADMAP.md` e `docs_obsidian/ui_references/matriz_de_paridade.md`.

  
**