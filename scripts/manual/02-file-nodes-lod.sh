#!/usr/bin/env bash
# File nodes and level of detail (TASK-2, File nodes panel TASK-44.4).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture large)
checklist "02 · Nós de arquivo e LOD" \
  "Abre com ~150 arquivos enquadrados (zoom bem baixo)|cada nó é um bloco com o nome do arquivo e a tinta da CST (L3-L5, zoom < 0,10); abaixo de 0,01 o nome some e o nó selecionado ou alterado é preenchido (L5)" \
  "Ctrl+roda até ~0,2x|o código vira retângulos coloridos por token, estilo minimap, com alpha 0,4 (L2, 0,10–0,30), mais a tinta da CST em alpha 0,04" \
  "Ctrl+roda acima de 0,30x|texto real com syntax highlighting, sem tinta base da CST (L1); o zoom máximo é 100%" \
  "Pressionar F com o mouse sobre um nó|o zoom vai a 60% e o ponto sob o mouse fica no centro" \
  "Ctrl+roda de 0,04 até 0,01|o nome do arquivo some aos poucos (fade) e some de vez abaixo de 0,01" \
  "Entre os níveis, observar a borda e posição dos nós|a geometria não muda: só o conteúdo desenhado troca" \
  "Abrir um nó com mais de 50 linhas (ex.: m002.rs) e navegar até o final|o bloco inclui todas as linhas do arquivo; pan/zoom apenas recorta o que está fora da tela, sem truncar o conteúdo" \
  "Em zoom intermediário, olhar o título|se o título no header ficar ilegível, aparece um rótulo flutuando acima do nó" \
  "Clicar num nó|borda de seleção destacada; Shift+clique adiciona/remove outros nós" \
  "Com um nó selecionado, arrastar com o botão esquerdo|o nó selecionado acompanha o mouse" \
  "Painel File nodes → passar o mouse nos ícones de Actions sem seleção|ações que dependem de seleção ou de Git ficam cinza com tooltip explicando o motivo" \
  "Select all nodes, Unselect all, Remove selected nodes, Fit to nodes, Clear all nodes|cada ação faz o que diz; Fit enquadra todos os nós" \
  "Selecionar um arquivo e clicar Highlight top-level symbols|faixas amarelas marcam as declarações de topo; clicar de novo limpa; Skip imports tira os imports" \
  "Long Files: Max lines 20, abrir um arquivo maior pela árvore|abre recolhido (só a moldura com 'N lines · collapsed'); Expand all 1 collapsed files expande" \
  "File Names desligado; depois Min Size 100% com zoom baixo|os nomes acima dos nós somem; com Min Size alto somem mais cedo ao afastar o zoom" \
  "Numa pasta com git: editar um arquivo e esperar ~10 s; Open all changed files / Close files without changes|abre os arquivos alterados; fecha os que não mudaram; Details mostra Changed (Git)"
launch "$dir" --open-all --layout
