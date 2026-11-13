#!/usr/bin/env bash
# Folder container nodes (TASK-3).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture nested)
checklist "03 · Pastas no canvas" \
  "Observar o canvas ao abrir|cada pasta com arquivos abertos tem um quadro com rótulo 'nome (N)' envolvendo seus nós" \
  "Comparar frontend/src e backend/src|são dois quadros distintos, cada um só com os arquivos da própria pasta" \
  "Selecionar um nó e arrastá-lo para longe|o quadro da pasta cresce para continuar envolvendo o nó" \
  "Layout (painel direito) → mudar Folder Padding|a margem entre os nós e o quadro da pasta muda" \
  "Clicar num nó e usar Clear Selected na toolbar|o nó some e o contador da pasta diminui; pasta que fica vazia desaparece"
launch "$dir" --open-all --layout
