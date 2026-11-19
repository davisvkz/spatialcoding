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
  "Painel Search → Files & folders, digitar 'prod'|lista arquivos e pastas por nome (Product.ts, product.rs, ...) com o nome em destaque e a pasta em cinza; clicar abre e centraliza o nó; uma pasta abre seus arquivos" \
  "No campo, Tab|alterna entre Code e Files & folders sem sair do campo" \
  "Search → Code, digitar 'InMemoryRepository'|após uma pausa curta aparece 'N matches' com arquivo:linha e a linha com o trecho em destaque, em grupos On canvas / Off canvas; clicar abre o arquivo e centraliza essa linha" \
  "Code: ligar .* e digitar 'Repo.*y'; depois digitar '(' |regex casa; o padrão inválido mostra 'Invalid pattern' em vez de resultados; Aa e Word restringem os resultados" \
  "Filtros Ignored e Canvas|Ignored inclui arquivos ignorados; Canvas só procura nos arquivos abertos no canvas" \
  "Setas ↑/↓ e Enter nos resultados|a linha ativa muda e Enter abre o resultado; Keep Zoom e Smooth Transition são switches do painel" \
  "Ctrl+Z e Ctrl+Shift+Z (ou Ctrl+Y) no canvas|desfazem e refazem abrir arquivos, mover nós e recolher pastas; um arrasto é um passo; com o foco em um campo de texto, Ctrl+Z desfaz o texto" \
  "Search com uma consulta sem resultado|'No files found' (ou matches/symbols)" \
  "Painel Search sem pasta aberta (rodar o app sem argumentos)|'Open a folder to search'"
launch "$dir"
