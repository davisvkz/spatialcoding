#!/usr/bin/env bash
# Hierarchical layout (TASK-7, folder clusters TASK-37, Layout panel TASK-44.2).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture rust)
checklist "07 · Layout hierárquico" \
  "Ao abrir, esperar as arestas chegarem|Auto Layout (ligado por padrão) organiza os nós em colunas por dependência sozinho" \
  "Layout → ⋮ → Disable Auto Layout; abrir mais um arquivo pela árvore|o arquivo novo entra na grade sem reorganizar os outros; o painel avisa 'Auto Layout is off'; ⋮ → Enable Auto Layout religa" \
  "Com Auto Layout ligado, selecionar um nó e abrir outro arquivo|o layout muda, mas o nó selecionado continua no mesmo lugar da tela" \
  "Apertar 1 (fora de campos de texto) e depois Shift+F|colunas por dependência: main.rs sozinho na 1ª coluna, quem ele importa à direita" \
  "Olhar as caixas de pasta|cada pasta é um contorno fino sem preenchimento, com o nome acima à esquerda (sem contagem); pastas irmãs nunca se sobrepõem e uma subpasta fica dentro da caixa da pasta-mãe" \
  "Arquivos que se importam em ciclo|ficam em colunas diferentes, não empilhados na mesma" \
  "Arrastar um nó para fora|a caixa da pasta dele (e das pastas acima) cresce junto" \
  "Painel direito → Layout → Advanced Settings: mudar File Spacing e clicar Apply Layout|a distância entre colunas e entre nós muda; o valor aparece à direita do rótulo (padrões 500 / 500 / 80)" \
  "Sort Off / Desc / Asc / Alt / Alt Rev + Apply Layout|a ordem vertical dos nós dentro de cada coluna muda; Desc põe no topo o arquivo mais ligado aos outros (pontuação: entradas×4 + saídas×2 + entradas de irmãos×3 + saídas para irmãos)" \
  "Switch Barycenter + Apply Layout (desligado por padrão)|ligado, cada coluna segue a posição dos arquivos a que se liga nas colunas anteriores; Reset Config desliga de novo" \
  "V Align Top / Center / Bottom / Fill + Apply Layout|as colunas alinham pelo topo, centro ou base; Fill espalha os nós de cada coluna pela altura da coluna mais alta" \
  "H Align Left / Center / Right + Apply Layout numa coluna com pasta e arquivo|o arquivo mais estreito encosta à esquerda, centraliza ou encosta à direita da coluna" \
  "Rank Breaking: passar o mouse; ir de 8 para 1 e Apply Layout com --open-all|tooltip explica; valores menores cortam a coluna mais alta de cada pasta (perto do meio, sem separar quem se importa) até a pasta não passar de N vezes a largura das colunas, no máximo 10 cortes; 0 mostra Off e mantém uma coluna por rank" \
  "Reset Config e ⋮ → Reset Layout settings|voltam aos padrões; Reset Config não mexe no Auto Layout" \
  "Arquivos sem relação entre si no nível de cima|ficam todos na primeira coluna (rank 0)" \
  "Dentro de uma pasta: arquivos sem aresta, ou que só se ligam entre si|ficam em colunas depois das que se ligam ao resto do projeto"
launch "$dir" --open-all
