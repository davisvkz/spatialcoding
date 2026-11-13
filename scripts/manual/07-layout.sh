#!/usr/bin/env bash
# Hierarchical layout (TASK-7).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture rust)
checklist "07 · Layout hierárquico" \
  "Ao abrir, os nós estão numa grade de 3 colunas (ordem de abertura)|nenhum layout aplicado ainda" \
  "Apertar 1 (fora de campos de texto) e depois Shift+F|colunas por dependência: main.rs sozinho na 1ª coluna, quem ele importa à direita" \
  "Painel direito (◀) → Layout → mudar File Spacing e clicar Apply Layout|a distância entre colunas e entre nós muda" \
  "Sort Mode Asc / Desc / Alt / AltRev + Apply Layout|a ordem vertical dos nós dentro de cada coluna muda" \
  "Vertical Align Top / Center / Bottom + Apply Layout|as colunas alinham pelo topo, centro ou base" \
  "Arquivos sem relação entre si|ficam todos na primeira coluna (rank 0)"
launch "$dir" --open-all
