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
  "Clicar de novo em rust/src/main.rs (e em outros arquivos já abertos)|nenhum nó duplicado: o arquivo continua aparecendo uma única vez no canvas" \
  "Clicar com o botão direito em rust/src|menu mostra Open on canvas e Remove from canvas; não expande/recolhe a árvore" \
  "Escolher Open on canvas duas vezes|abre os arquivos da pasta e subpastas, sem duplicar nós; o menu fecha após cada ação" \
  "Abrir um arquivo fora de rust/src; no menu de rust/src escolher Remove from canvas|remove apenas os nós dessa pasta e subpastas; o arquivo externo permanece e nenhum arquivo é apagado do disco" \
  "Clicar em assets/logo.png e docs/latin1.txt|o nó aparece vazio (binário / não UTF-8), sem travar o app"
launch "$dir"
