#!/usr/bin/env bash
# Paleta de comandos (Ctrl+K) e painel Search (TASK-16, TASK-17).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture showcase)
checklist "14 · Paleta de comandos e busca" \
  "Ctrl+K, digitar 'fit' (sem nada aberto)|a lista fica vazia: 'Fit view to nodes' só existe com nós no canvas; Esc fecha" \
  "Ctrl+K, digitar 'toggle cst', Enter|o painel CST abre na direita; Ctrl+K de novo e 'toggle cst' fecha" \
  "Ctrl+K, digitar 'price.rs'|aparecem arquivos da pasta (server/src/domain/price.rs); as letras que casam ficam coloridas; ↑/↓ movem a seleção; Enter abre o arquivo no canvas, selecionado" \
  "Com a paleta aberta, digitar f, 1 e Shift+X|viram texto na caixa; não centralizam, não aplicam layout nem limpam o canvas" \
  "Clicar fora da paleta|ela fecha sem executar nada" \
  "Painel Search → Files, digitar 'prod'|lista arquivos por nome (Product.ts, product.rs, ...) com o nome em destaque e a pasta em cinza; clicar abre e centraliza o nó" \
  "Search → Content, digitar 'InMemoryRepository'|após uma pausa curta aparece 'N matches' com arquivo:linha e a linha com o trecho em destaque; clicar abre o arquivo e centraliza essa linha" \
  "Search → Symbols, digitar 'order'|funções, classes e structs de topo (Order, ...) com o tipo e arquivo:linha; clicar salta para a declaração" \
  "Search com uma consulta sem resultado|'No files found' (ou matches/symbols)" \
  "Painel Search sem pasta aberta (rodar o app sem argumentos)|'Open a folder to search'"
launch "$dir"
