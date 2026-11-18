# MCP e servidor local

## Servidor local
`npx code-canvas-server` (pacote `code-canvas-server@2.6.1`). Necessário no browser para: pastas locais, language servers, diffs locais, MCP, file watching.
Na extensão VSCode não é necessário (backend da extensão). Desktop macOS: embutido.
Detalhes técnicos em [arquitetura.md](arquitetura.md).

## MCP
Config (também em Local Server → MCP, com botão de copiar):
```json
{ "mcpServers": { "code-canvas": { "command": "npx", "args": ["-y", "-p", "code-canvas-server", "code-canvas-mcp"] } } }
```
Sempre habilitado; o painel mostra os agentes conectados.

| Tool | Função |
|---|---|
| `list_commands` | descobre comandos de canvas/UI |
| `use_command` | executa um comando (`canvas:openFiles`, `layout:apply`, `viewport:fitNodes`, `folder:collapseAll`, `agentNotes:push`...) |
| `query` | lê estado: canvas, nós, pastas, backwards edges, layout, stores; edges com semântica, categoria, endpoints resolvidos, type-only, ranges e proveniência da regra |
| `search` | código, caminhos, conteúdo, dados do language server |
| `lsp_servers` | detectar, iniciar, recarregar, parar, inspecionar servers |
| `read_docs` | ler o guia, uma seção ou a lista de seções |

Uso típico: "explain this codebase using agent_notes" → o agente abre arquivos, aplica layout e publica notas navegáveis (links `cc://`).

## `.codecanvas/settings.json` (por repositório)
```json
{ "languageServers": { "rust": { "command": ["direnv", "exec", ".", "rust-analyzer"] } } }
```
O app pergunta antes de executar (repo pode ser de terceiros); a resposta vale para aquele comando exato naquela pasta. Override do usuário vale se "Use default command".
