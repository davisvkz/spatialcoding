#!/usr/bin/env bash
# Minimap overview (TASK-9).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture large)
checklist "09 · Minimap" \
  "Olhar o canto inferior direito do canvas|minimap 200×150 com todos os nós em azul" \
  "Dar zoom e pan|o retângulo branco (área visível) acompanha a câmera" \
  "Arrastar um nó para longe|o minimap reenquadra para incluir o nó" \
  "Settings → desmarcar Minimap|o minimap some; marcar de novo traz de volta" \
  "Shift+X (limpar canvas)|o minimap some quando não há nós"
launch "$dir" --open-all --layout
