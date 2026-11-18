# Arquitetura (inferida do bundle e do tráfego de rede)

## Frontend
SPA estática em `v2.codecanvas.app` (bundle único `assets/index-*.js`, ~10 MB), com:
- **React** (UI de painéis/menus, Radix + Tailwind) e **PixiJS** (WebGL, com WebGPU como opção) para o canvas.
  `OffscreenCanvas` e `SharedArrayBuffer` aparecem no bundle.
- **Web workers** separados: `analysis.worker`, `layout.worker`, `search.worker`.
- **web-tree-sitter** (`web-tree-sitter-*.wasm`) com gramáticas wasm: bash, c, cpp, css, go, html, java, json,
  kotlin, objc, php, python, ruby, rust, svelte, swift, tsx, typescript, vue. Fonte do CST e do highlight.
- **TypeScript** embutido (a análise de TS/JS roda no browser; o LSP "bundled" é o TypeScript).
- Persistência local: **IndexedDB**; opção "Use SQLite database" (desabilitada sem servidor) para armazenar a análise.
- Telemetria **PostHog** (`us-assets.i.posthog.com`: session recorder, surveys, exception autocapture); `media.codecanvas.app` serve vídeos de tutorial.
  Auth via serviço tipo Supabase/Firebase (referências no bundle); versão 2.6.1 com "Check for updates".

## Pipeline ao abrir um repo GitHub (observado com reduxjs/redux)
1. `GET api.github.com/repos/{owner}/{repo}` (branch padrão).
2. `GET api.github.com/repos/{owner}/{repo}/git/trees/{branch}` (árvore inteira → painel Workspace, com contagem de arquivos por pasta).
3. `GET raw.githubusercontent.com/{owner}/{repo}/{branch}/{path}` para cada arquivo (busca ao abrir / analisar).
4. **AST analysis** em worker: "Analyzed 283 files in 12.01s (CST depth limited to 6)". Extrai imports/exports, símbolos, árvore sintática.
5. **Resolução de módulos**: tsconfig paths, aliases do Vite, fallback `index.*`, `@/`→`src/`, pacotes npm marcados external (sem edge).
6. **Layout** em worker: das pastas-folha para cima, ordenando por dependência, esq→dir; rank breaking ajusta a proporção.
7. Render no PixiJS: nós de arquivo, labels de pasta, edges (bezier/step/line/PCB), CST como retângulos coloridos que ganham detalhe com o zoom.

## Estratégia de análise / memória
Painel **Performance**: Analysis = Auto | Eager | On Open | Lazy; "Use SQLite database"; "Limit CST depth" (+ Max depth).
Detalhe de CST e edges carrega conforme o zoom e é descartado (mais antigo e fora da tela primeiro) conforme a memória; avisos
informam qual detalhe foi limitado. Diffs acima do limite por arquivo são ignorados (card "Large Diffs").

## Servidor local (`code-canvas-server`)
Pacote npm (bins `code-canvas-server`, `code-canvas`, `code-canvas-mcp`). Dependências: `express`, `ws`, `chokidar`
(file watching), `typescript`, `pyright`, `@vue/language-server`, `svelte-language-server`, `web-tree-sitter`, `diff`, `ignore`,
`zod`, `@modelcontextprotocol/sdk`. O app conecta por WebSocket (`ws://localhost:<porta>/ws`; na extensão `127.0.0.1:<vscodeServerPort>`,
no desktop `<electronServerPort>`). Mensagens LSP no protocolo interno: `lsp:init|detect|status|query|configure|dispose|syncSnapshot|syncBase`
(+ `*Response`). O servidor roda os language servers no ambiente do projeto (login shell + direnv `.envrc` permitido).

## Barramento de comandos
Tudo o que a UI faz é um comando nomeado (`canvas:openFiles`, `layout:apply`, `edges:setStyle`, `folder:collapseAll`...), o que permite
a paleta Ctrl+K, atalhos e MCP (`use_command`) compartilharem a mesma superfície. Lista em [atalhos-e-comandos.md](atalhos-e-comandos.md).
