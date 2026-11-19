# Método e limites

**Método**: `agent-browser` (Chrome headless, viewport 1600×1000) no app hospedado; docs embutidos; todos os painéis; screenshots por nível de zoom; leitura do bundle `assets/index-*.js` (10 MB, sem executar) com busca por identificadores e constantes; pacote npm `code-canvas-server` (dependências, health, portas); servidor local real para o LSP.

**Limites**
- Valores [bundle] não foram medidos em pixels; `endpointDotSizePx` parece sem uso; a ordem exata (acima dos nós) das edges selecionadas é inferida.
- LSP: só TypeScript rodou (redux). Observados na 2ª rodada: type definition, definition, pin múltiplo, Highlight Top-Level Symbols, Open Importers e Open imported files. Não observados: Rust/Python/Vue/Svelte.
- Não exercitados: login/Pro, pasta local, PRs/commits privados, MCP conectado a um agente, diff em canvas.
- Para o LSP foi necessário `--disable-features=LocalNetworkAccessChecks` no Chrome de teste (sessão descartável, só para v2.codecanvas.app). O servidor local foi parado ao final.
