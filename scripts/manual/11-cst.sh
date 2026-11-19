#!/usr/bin/env bash
# CST panel with tree-sitter (TASK-11, TASK-44.7, Symbol Mapping TASK-50).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture mixed)
checklist "11 · Árvore sintática (CST)" \
  "Abrir CST com arquivos no canvas, sem selecionar|Symbol Mapping lista tipos em ordem alfabética, cores por família e contadores 0/total; (N) conta tipos, não ocorrências" \
  "Selecionar um arquivo e depois vários com Shift|o numerador selecionados/total acompanha as ocorrências nos arquivos selecionados; o denominador não muda" \
  "Clicar na amostra de cor de function item e editar|a cor do tipo muda e persiste após reiniciar; hover usa a mesma cor no canvas" \
  "Hover em function item com Single Hover Symbol ligado e desligado|ligado destaca uma ocorrência (preferindo a seleção); desligado destaca todas as ocorrências daquele tipo" \
  "Fechar ou modificar um arquivo aberto|os tipos e contadores se atualizam sem manter ocorrências do arquivo removido" \
  "Abrir o painel CST sem nada selecionado|mensagem \"Select a file node to see its syntax tree.\" (sem arquivos no canvas: \"Open some files on the canvas…\")" \
  "Clicar no nó rust/src/main.rs|nome do arquivo, \"Rust · N nodes\" e a raiz source_file aberta com os itens do arquivo" \
  "Expandir o function_item de main|filhos com campo (name: identifier \"main\", body: block); os números L batem com as linhas do arquivo (1-based)" \
  "Clicar na engrenagem e ligar \"Show Hidden Types\"|aparecem tokens como fn, (, ), { em cinza" \
  "Desligar \"Semantic Nodes Only\"|aparecem os nós block, parameters, arguments; ligado, os filhos deles sobem um nível" \
  "Max depth 1 e depois 6|a árvore e Symbol Mapping respeitam o limite; (N) conta tipos e tree rows conta nós; Performance › CST Depth mostra os mesmos valores" \
  "Desligar \"Single Hover Symbol\" e passar o mouse num nó interno|além do nó, os ancestrais ganham uma faixa leve" \
  "Com main.rs selecionado (tem arestas)|os nós CST nas pontas das arestas ficam contornados em verde; desligar Selected Edge CST Nodes remove" \
  "Desligar o switch do cabeçalho (Toggle CST nodes)|nenhum destaque CST no canvas, mesmo com hover" \
  "⋮ → Reset CST settings|tudo volta ao padrão, incluindo cores dos tipos" \
  "Passar o mouse sobre uma linha da árvore|as linhas daquele nó ficam destacadas em azul no nó do canvas" \
  "Passar o mouse sobre source_file|o destaque cobre o arquivo inteiro, cortado nas linhas visíveis do nó" \
  "Dar zoom out até o nó virar barras (minificado)|o destaque continua alinhado às barras" \
  "Selecionar Button.tsx, legacy.js, cart.py e main.go|cada um mostra sua árvore (\"TSX · …\" e \"JavaScript · …\" com raiz program, Python module, Go source_file)" \
  "Selecionar README.md ou settings.json|\"No syntax tree: <linguagem> is not supported.\"" \
  "Trocar para a aba Layout com o mouse em cima de uma linha|o destaque some do canvas" \
  "Dar zoom out para 0,2x e depois 0,06x|cada nó mostra retângulos tintados pelo papel (função azul, classe roxa, variável verde, import laranja, export lima); alpha 0,04 em 0,2x e 0,10 abaixo de 0,10x; if/else/switch não são tintados" \
  "Ligar e desligar o switch do cabeçalho (Toggle CST nodes) em zoom afastado|a tinta de todos os nós aparece e some"
launch "$dir" --layout \
  --open rust/src/main.rs --open web/src/components/Button.tsx --open web/src/legacy.js \
  --open python/src/shop/cart.py --open go/main.go --open README.md --open config/settings.json
