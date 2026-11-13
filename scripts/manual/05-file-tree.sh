#!/usr/bin/env bash
# File tree sidebar (TASK-5).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture mixed)
checklist "05 · Árvore de arquivos (sidebar esquerda)" \
  "Ver a aba Workspace|raiz 'mixed (N)' recolhida; o canvas está vazio" \
  "Clicar no nome da raiz e depois no de uma subpasta|a pasta expande/recolhe tanto pela seta quanto pelo nome" \
  "Conferir o conteúdo|pastas antes dos arquivos, ordem alfabética sem diferenciar maiúsculas; contador '(N)' em cada pasta" \
  "Procurar .github, node_modules e target|não aparecem (dot folders ocultas por padrão; build/deps sempre ignorados)" \
  "Conferir ícones|🧱 .rs, 📜 .ts/.js, 🐍 .py, ⚙ .toml, 📝 .md, 📋 .json, 📄 demais" \
  "Clicar em rust/src/main.rs e depois em rust/src/config.rs|cada clique cria um nó no canvas, dentro do quadro da pasta, com aresta main→config" \
  "Clicar em assets/logo.png e docs/latin1.txt|o nó aparece vazio (binário / não UTF-8), sem travar o app"
launch "$dir"
