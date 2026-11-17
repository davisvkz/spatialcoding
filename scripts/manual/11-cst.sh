#!/usr/bin/env bash
# CST panel with tree-sitter (TASK-11, v2 settings TASK-44.7).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture mixed)
checklist "11 · Árvore sintática (CST)" \
  "Abrir o painel CST sem nada selecionado|mensagem \"Select a file node to see its syntax tree.\" (sem arquivos no canvas: \"Open some files on the canvas…\")" \
  "Clicar no nó rust/src/main.rs|nome do arquivo, \"Rust · N nodes\" e a raiz source_file aberta com os itens do arquivo" \
  "Expandir o function_item de main|filhos com campo (name: identifier \"main\", body: block); os números L batem com as linhas do arquivo (1-based)" \
  "Clicar na engrenagem e ligar \"Show Hidden Types\"|aparecem tokens como fn, (, ), { em cinza" \
  "Desligar \"Semantic Nodes Only\"|aparecem os nós block, parameters, arguments; ligado, os filhos deles sobem um nível" \
  "Max depth 1 e depois 6|a árvore mostra só a raiz e o primeiro nível; o contador (N) acompanha; Performance › CST Depth mostra os mesmos valores" \
  "Desligar \"Single Hover Symbol\" e passar o mouse num nó interno|além do nó, os ancestrais ganham uma faixa leve" \
  "Com main.rs selecionado (tem arestas)|os nós CST nas pontas das arestas ficam contornados em verde; desligar Selected Edge CST Nodes remove" \
  "Desligar o switch do cabeçalho (Toggle CST nodes)|nenhum destaque CST no canvas, mesmo com hover" \
  "⋮ → Reset CST settings|tudo volta ao padrão" \
  "Passar o mouse sobre uma linha da árvore|as linhas daquele nó ficam destacadas em azul no nó do canvas" \
  "Passar o mouse sobre source_file|o destaque cobre o arquivo inteiro, cortado nas linhas visíveis do nó" \
  "Dar zoom out até o nó virar barras (minificado)|o destaque continua alinhado às barras" \
  "Selecionar Button.tsx, legacy.js, cart.py e main.go|cada um mostra sua árvore (\"TSX · …\" e \"JavaScript · …\" com raiz program, Python module, Go source_file)" \
  "Selecionar README.md ou settings.json|\"No syntax tree: <linguagem> is not supported.\"" \
  "Trocar para a aba Layout com o mouse em cima de uma linha|o destaque some do canvas"
launch "$dir" --layout \
  --open rust/src/main.rs --open web/src/components/Button.tsx --open web/src/legacy.js \
  --open python/src/shop/cart.py --open go/main.go --open README.md --open config/settings.json
