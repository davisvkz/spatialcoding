#!/usr/bin/env bash
# Infinite canvas: pan and zoom (TASK-1).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture rust)
checklist "01 · Canvas infinito: pan e zoom" \
  "Ctrl + roda do mouse sobre um nó|zoom suave centrado no cursor: o ponto sob o mouse não sai do lugar" \
  "Ctrl + roda até o limite nos dois sentidos|o zoom para em 5x (perto) e 0,05x (longe), sem pular" \
  "Roda do mouse / dois dedos no trackpad, sem Ctrl|a câmera desliza (pan) na vertical/horizontal" \
  "Arrastar com o botão direito|a câmera acompanha o mouse" \
  "Segurar Espaço e arrastar com o botão esquerdo|a câmera acompanha o mouse" \
  "Selecionar um nó (clique) e repetir os dois pans acima|só a câmera se move; o nó selecionado fica onde estava" \
  "Afastar tudo e apertar Shift+F|todos os nós cabem na tela" \
  "Mouse sobre um ponto e apertar F|a visão recentraliza no ponto sob o mouse"
launch "$dir" --open-all --layout
