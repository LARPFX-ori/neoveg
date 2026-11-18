# Referência visual das janelas — Pan/Crop, Movimento da Trilha, Trimmer e outras

Descrição, com palavras nossas, da disposição e do comportamento das janelas do VEGAS Pro 22, feita a partir da observação do programa aberto pelo mantenedor (novembro de 2026) e conferida com o manual do VEGAS Pro 21. As capturas ficam só na máquina de quem estudou (pasta `plan/`, fora do Git), conforme o AGENTS.md (§1.3, §1.4 e §2.1). Aqui não há imagens, ícones nem textos copiados: só a estrutura, as medidas aproximadas e o que cada controle faz, para o Neoveg desenhar o seu próprio.

Medidas em pixels lógicos numa tela de 1920×1080 com escala de 100%; são aproximadas.

## Padrões comuns às janelas de quadros-chave

As janelas de Pan/Crop e de Movimento da Trilha seguem o mesmo esqueleto, que vale repetir no Neoveg com um único componente:

1. **Linha de predefinição** no alto: rótulo, lista editável com o nome da predefinição (o nome padrão aparece entre parênteses), botão de salvar e botão de excluir à direita.
2. **Barra de ferramentas** da janela, com botões de alternância (aceso = fundo azul).
3. **Grade de propriedades** à esquerda (cerca de 220 px de largura): seções com cabeçalho cinza e um quadradinho `+`/`−` para recolher; cada linha tem o nome alinhado à esquerda com um ponto de recuo e o valor numérico editável à direita (duas casas decimais, separador do idioma).
4. **Área de trabalho** à direita, ocupando o resto: fundo cinza-claro (não escuro), grade de pontos, a moldura de saída do projeto desenhada em linha fina e as alças do objeto editado. Lupa e mão no canto inferior direito (zoom e deslocar a área de trabalho).
5. **Linha do tempo de quadros-chave** embaixo (cerca de 190 px de altura): coluna de nomes das trilhas de animação à esquerda, régua de tempo do evento ou da trilha, losangos dos quadros-chave, cursor vertical, barra de rolagem horizontal com `+`/`−` de zoom e, abaixo, a barra de navegação.
6. **Barra de navegação de quadros-chave**: sincronizar cursor (alternância, acesa por padrão), primeiro, anterior, próximo, último, adicionar e excluir quadro-chave; à direita, o tempo do cursor com o ícone de marcador.

O fundo fora da área útil da linha do tempo de quadros-chave é xadrez cinza (nada a animar ali).

## Pan/Crop do evento

Abre dentro da janela *Efeitos de Vídeo do Evento*, como o primeiro item da cadeia.

- **Cabeçalho**: número/ícone da trilha, o título "Pan/Crop do evento:" seguido do nome do evento; à direita, três botões para escolher o arranjo da janela (só a área de trabalho, propriedades + área, ou dividido).
- **Cadeia de efeitos**: uma linha com a cadeia em pílulas ligadas por um fio (`o—[Pan/Crop]—o`), com a pílula do Pan/Crop sempre primeira; à direita, adicionar e remover efeito.
- **Barra de ferramentas vertical** à esquerda da grade (uma coluna de ícones): mostrar propriedades, ferramenta normal (seta), zoom (lupa), encaixar na grade (ímã), travar proporção, dimensionar em torno do centro (sem ele, o lado oposto fica ancorado ao arrastar uma borda) e o modo de movimento, que alterna entre livre, só em X e só em Y (manual do VEGAS Pro 21, pp. 250–251).
- **Grade de propriedades**:
  - *Posição*: Largura, Altura, Centro X, Centro Y (em pixels da mídia; padrão é o tamanho do quadro e o centro dele).
  - *Rotação*: Ângulo, Centro X, Centro Y.
  - *Interpolação de quadros-chave* (recolhida): suavidade e tipo.
  - *Origem* (recolhida): manter proporção e esticar para preencher o quadro (Sim/Não; com Não, a mídia mantém o tamanho e o que fica fora do retângulo é cortado).
  - *Área de trabalho*: Zoom (%), Deslocamento X, Deslocamento Y, Espaçamento da grade.
- **Área de trabalho**: a imagem de origem em tamanho real; sobre ela, o retângulo de recorte tracejado com 8 alças (cantos e meios dos lados) e um círculo no centro; uma letra "F" grande tracejada no meio indica a orientação (de cabeça para baixo ou espelhada se girada/invertida); um círculo tracejado maior em volta é a alça de rotação. Linhas finas mostram as bordas do quadro de saída.
- **Linha do tempo de quadros-chave**: trilhas "Posição" e "Máscara" (esta com caixa de seleção para ligar a máscara).

Comportamento: arrastar dentro do retângulo move; arrastar alça redimensiona (com travar proporção ligado, mantém a proporção); arrastar fora do retângulo, perto do círculo grande, gira. Clicar com o botão direito na área abre: Restaurar, Centralizar, Inverter horizontal/vertical, Combinar proporção de saída, Combinar proporção de origem.

## Movimento da Trilha (2D)

Janela própria, com o título "Movimento da Trilha — " e o nome da trilha.

- **Linha de predefinição** (igual ao padrão).
- **Barra de ferramentas** (manual do VEGAS Pro 21, pp. 412–413): lista "Alfa da origem" (modo de composição da trilha), depois alternâncias: habilitar rotação, encaixar na grade, editar no espaço do objeto; impedir movimento em X, impedir movimento em Y; travar proporção, dimensionar em torno do centro; impedir dimensionar em X, impedir dimensionar em Y.
- **Grade de propriedades**:
  - *Posição*: X, Y, Largura, Altura (X e Y são deslocamento do centro em pixels do projeto; 0,0 é o centro).
  - *Orientação*: Ângulo.
  - *Rotação*: Ângulo, Deslocamento X, Deslocamento Y.
  - *Quadro-chave*: Suavidade, Tipo (Linear por padrão).
  - *Área de trabalho*: Zoom (%) — 50% por padrão —, Deslocamento X, Deslocamento Y.
  - *Encaixe*: Espaçamento da grade (10), Rotação (passo de 5°).
- **Área de trabalho**: a moldura do quadro de saída é um retângulo fino; o objeto é mostrado como um círculo com dois eixos (X para a direita e Y para cima, cada um com a letra na ponta) e um pequeno círculo no centro.
- **Linha do tempo de quadros-chave**: trilhas "Posição", "Sombra 2D" e "Brilho 2D"; as duas últimas têm caixa de seleção que liga o efeito.

## Movimento da Trilha (3D)

Quando o modo de composição da trilha é 3D, a mesma janela muda:

- A lista da barra passa a ser "Alfa da origem 3D" e aparece a lista **Layout** (o número de vistas, por padrão 6 combinações), mais uma alternância de impedir movimento em Z e de impedir dimensionar em Z.
- A grade ganha Z e Profundidade em *Posição*, X/Y/Z em *Orientação*, X/Y/Z e Deslocamento Z em *Rotação* e Deslocamento Z em *Área de trabalho*.
- A área de trabalho se divide em **quatro vistas**: três pequenas em cima (Topo, Esquerda, Frente, cada uma com o nome no canto superior esquerdo e lupa/mão no canto inferior direito) e a **Perspectiva** grande embaixo. Cada vista mostra o plano do objeto com os eixos daquela vista.
- A linha do tempo fica só com "Posição".

## Trimmer

- **Linha do alto**: lista do histórico de mídias abertas no Trimmer ("(Nenhum)" quando vazio) e, à direita, ordenar histórico, opções de exibição, limpar histórico e destacar.
- **Área de vídeo** ocupando quase toda a janela, fundo quase preto; acima dela, uma régua fina com o marcador de cursor amarelo no início.
- **Barra de transporte** centralizada embaixo: loop, tocar desde o início, tocar, pausar, parar, ir ao início, ir ao fim, quadro anterior, próximo quadro; depois os botões do Trimmer: sobrescrever a timeline, adicionar à timeline a partir do cursor, adicionar até o cursor, ajustar para preencher, criar subclip, marcar entrada, marcar saída, inserir marcador, inserir região, salvar marcadores/regiões, detectar cenas e adicionar a partir do cursor, detecção de batida; por fim, o botão "…" (mais), que abre a edição do conjunto de botões visíveis.
- **Linha de baixo**: tempo do cursor, com o ícone de marcador, e duas caixas de tempo (entrada e saída da seleção).
- **Editar conjunto de botões visíveis**: diálogo com a lista de todos os botões acima, cada um com caixa de seleção, mais "Mostrar todos" e "Ocultar todos", OK e Cancelar.

## Outras janelas observadas

- **Prévia de Vídeo**: barra no alto com propriedades, tela dividida/externa, ignorar efeitos, sobreposições com seta, qualidade (lista "Melhor (Inteira)"), zoom (lista "100 %"), grade/sobreposições com seta, copiar quadro, salvar quadro e dois botões desativados quando não se aplicam; embaixo a barra de transporte (gravar em vermelho, loop, tocar desde o início, tocar, pausar, parar, início, fim, quadro anterior/seguinte, prévia de RAM e "…"); duas linhas de informação: Projeto/Prévia à esquerda e Quadro/Exibição à direita, a exibição em vermelho quando a prévia é reduzida. O conjunto de botões da barra também é editável.
- **Cabeçalho de trilha de vídeo**: número com cor da trilha à esquerda; linha de botões (minimizar, maximizar, compositor pai, modo de composição, movimento da trilha, efeitos da trilha, automação, mudo, solo); linha "Nível: 100,0 %" com controle deslizante; ícones de compositor filho embaixo.
- **Cabeçalho de trilha de áudio**: botões (minimizar, maximizar, armar gravação, inverter fase, entrada de gravação, automação, efeitos com seta, mudo, solo); linhas "Vol: 0,0 dB" e "Pan: Centro" com controles deslizantes; modo de automação "Toque"; medidor vertical estreito à direita. Conjuntos de botões visíveis editáveis separados para vídeo e áudio.
- **Botões nos eventos**: faixa de ícones no canto inferior direito do evento de vídeo (pan/crop, efeitos, gerador, take ativo, taxa de reprodução, congelar quadro, colar atributos seletivamente, cabeçalhos, duração, correção de cor, efeitos de mídia, alças, rastreamento, detectar cenas e "…") e no de áudio (efeitos, take, cabeçalhos, duração, alças, normalizar, normalizar automático e "…"). Cada um tem seu diálogo de botões visíveis.
- **Escolha de plug-in**: janela grande com a cadeia atual no alto ("Nenhum plug-in na cadeia"), lista de caminho com pastas, árvore de pastas à esquerda e lista em várias colunas à direita; botões à direita: OK, Cancelar, Adicionar, Substituir, Remover, Salvar como, Salvar LUT.
- **Menus de contexto do evento**: abrir no Trimmer, criar linha do tempo aninhada, selecionar na Mídia do projeto; efeitos de evento (áudio) ou efeitos de mídia, pan/crop, efeitos de vídeo e rastreamento (vídeo); recortar, copiar, colar atributos; excluir, aparar início/fim, dividir, detectar cenas (vídeo); criar subclip, inverter; selecionar até o fim; submenus de chaves, take, grupo, stream, canais (áudio), sincronizar, links de sincronia; propriedades.
- **Preferências → Geral**: lista longa de caixas de seleção com rolagem, depois "Lista de projetos recentes" com número, pasta de arquivos temporários com Procurar e espaço livre, e "Padrão para todos"; abas em três fileiras no alto.

## Estrutura dos menus (resumo)

Exibir → Janela lista as janelas com atalhos `Alt+1` a `Alt+9` (Explorer, Trimmer, Master Bus, Prévia de Vídeo, Mídia do Projeto, Notas, Transições, Efeitos de Vídeo, Geradores de Mídia) e `Ctrl+Alt+1` a `Ctrl+Alt+9` (Gerenciador de plug-ins, Escopos, Surround, Detalhes de edição, Console de mixagem, Explorador de dispositivos, Medidores de loudness e duas janelas de serviço on-line que o Neoveg não terá). Layouts de janela usam `Alt+D` seguido de letra ou número. Os submenus de Importar e Exportar trazem os formatos de intercâmbio (FCP7/Resolve XML, FCPXML, EDL; importar também Broadcast Wave e legendas).
