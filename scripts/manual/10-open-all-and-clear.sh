#!/usr/bin/env bash
# Open all files (Ctrl+Shift+O) and clearing the canvas.
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture mixed)
checklist "10 · Abrir tudo e limpar" \
  "Canvas vazio ao abrir; apertar Ctrl+Shift+O|todos os arquivos de texto viram nós; logo.png e latin1.txt ficam de fora" \
  "Apertar Ctrl+Shift+O de novo|nada é duplicado" \
  "Apertar 1 e Shift+F|layout aplicado e tudo enquadrado" \
  "Selecionar rust/src/util.rs e clicar Clear Selected|o nó some junto com as arestas dele; nenhuma aresta fica pendurada" \
  "Clicar Clear All|canvas vazio: nós, pastas e arestas" \
  "Ctrl+Shift+O e depois Shift+X|mesmo efeito do Clear All"
launch "$dir"
