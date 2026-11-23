#!/usr/bin/env bash
# MCP, Agent notes, caminhos ignorados e .codecanvas/settings.json (TASK-55.4).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture rust)
mkdir -p "$dir/.codecanvas"
cat > "$dir/.codecanvas/settings.json" <<'JSON'
{ "languageServers": { "rust": { "command": ["rust-analyzer"] } } }
JSON
# TASK-86: um padrão embutido (dist) e um .gitignore aninhado (gen/) escondem pastas.
mkdir -p "$dir/dist" "$dir/gen"
echo 'console.log(1);' > "$dir/dist/out.js"
echo 'pub fn generated() {}' > "$dir/gen/generated.rs"
echo 'gen/' > "$dir/.gitignore"
checklist "16 · MCP, Agent notes, ignorados e .codecanvas" \
  "Painel Local Server (rail esquerda)|mostra 'MCP bridge listening' com o caminho do socket e 'No agent connected'; o snippet JSON usa spatialcoding --mcp" \
  "Em outro terminal: printf '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"clientInfo\":{\"name\":\"manual\"}}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n' | spatialcoding --mcp|duas respostas JSON: serverInfo spatialcoding e as 6 tools list_commands, use_command, query, search, lsp_servers e read_docs" \
  "Mesmo comando com tools/call: {\"name\":\"use_command\",\"arguments\":{\"id\":\"canvas:openAll\"}}|o app abre todos os arquivos na hora; depois {\"name\":\"use_command\",\"arguments\":{\"id\":\"layout:apply\"}} aplica o layout" \
  "tools/call query com {\"what\":\"edges\"} e {\"what\":\"backwards\"}|lista as arestas com origem, destino, relação, tipo, símbolo e se vão para trás" \
  "tools/call use_command agentNotes:push com {\"title\":\"Tour\",\"markdown\":\"Comece por [main](cc://file/src/main.rs) e veja [parse_port](cc://symbol/src/util.rs:6:8).\"}|o painel Agent notes abre com o cartão; passar o mouse num link só mostra o destino; clicar abre o arquivo; o link de símbolo também fixa o símbolo" \
  "Cartão: Open files, ✕ e o ⋮ (Smooth Transition, Clear Agent notes)|Open files abre os arquivos linkados sem duplicar; ✕ remove o cartão; Clear limpa todos" \
  "Fechar o agente (Ctrl+C no terminal) e olhar Local Server|a lista de agentes conectados volta a vazio" \
  "Workspace → botão direito numa pasta → Add to ignore list…|um toast 'Ignoring /pasta/' e a pasta some da árvore, de Open All, da paleta e da busca" \
  "Workspace → ⋮ → Show ignored files|as pastas ignoradas voltam a aparecer na árvore; ⋮ → Ignored file settings lista os padrões com ✕ e um campo para adicionar (sintaxe gitignore: dist, /build/, *.log, !keep.js)" \
  "Workspace → ⋮ → Ignored file settings, sem adicionar nada|o texto avisa que ficam escondidos, além da lista, os nomes embutidos (dist, build, out, vendor, lockfiles…) e o que cada .gitignore lista" \
  "Com Show ignored files desligado, olhar a árvore|dist/ e gen/ não aparecem (dist por padrão, gen por causa do .gitignore); ligar Show ignored files traz os dois de volta" \
  "Adicionar o padrão !dist na lista de ignorados|dist/ volta para a árvore mesmo com Show ignored files desligado; gen/ continua escondida" \
  "Editar o .gitignore da pasta (apagar a linha gen/) com o app aberto|a árvore se atualiza sozinha e gen/ aparece, sem reabrir a pasta" \
  "Fechar e reabrir o app com a mesma pasta|a lista de ignorados continua; outra pasta começa com a lista vazia" \
  "Ao abrir um arquivo .rs com LSP, aparece o diálogo 'Run a language server command?' com o comando do .codecanvas/settings.json|Allow guarda a resposta e reinicia os servidores; Never for this command mantém o servidor padrão; a mesma pergunta não volta para o mesmo comando, linguagem e pasta; SPATIALCODING_LSP_RUST sempre vence"
launch "$dir" --open-all --layout
