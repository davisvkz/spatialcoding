#!/usr/bin/env bash
# File nodes and level of detail (TASK-2).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture large)
checklist "02 · Nós de arquivo e LOD" \
  "Abre com ~150 arquivos enquadrados (zoom bem baixo)|cada nó é um bloco sólido com o nome do arquivo centralizado (LOD TitleOnly, zoom < 0,12)" \
  "Ctrl+roda até ~0,2x|o código vira barras coloridas por token, estilo minimap (LOD Minified, 0,12–0,5)" \
  "Ctrl+roda acima de 0,5x|texto real com syntax highlighting (LOD Full)" \
  "Entre os níveis, observar a borda e posição dos nós|a geometria não muda: só o conteúdo desenhado troca" \
  "Abrir um nó com mais de 50 linhas (ex.: m002.rs)|o nó mostra no máximo 50 linhas, mesmo o arquivo sendo maior" \
  "Em zoom intermediário, olhar o título|se o título no header ficar ilegível, aparece um rótulo flutuando acima do nó" \
  "Clicar num nó|borda de seleção destacada; Shift+clique adiciona/remove outros nós" \
  "Com um nó selecionado, arrastar com o botão esquerdo|o nó selecionado acompanha o mouse"
launch "$dir" --open-all --layout
