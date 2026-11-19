# Scripts de teste manual

Cada script testa uma feature. Ele gera um projeto de exemplo (fixture) do zero,
compila o app, imprime um roteiro com **o que fazer → o que deve acontecer** e abre o
SpatialCoding já com a pasta e os arquivos certos no canvas.

```sh
scripts/manual/01-canvas.sh
scripts/manual/04-edges.sh python
```

| Script | Feature | Fixture | Abre com |
|---|---|---|---|
| `01-canvas.sh` | Canvas infinito: pan, zoom, F, Shift+F (TASK-1) | `rust` | todos os arquivos + layout |
| `02-file-nodes-lod.sh` | Nós de arquivo e níveis de detalhe (TASK-2) | `large` | ~150 arquivos + layout |
| `03-folders.sh` | Quadros de pasta (TASK-3) | `nested` | todos os arquivos + layout |
| `04-edges.sh [rust\|ts\|python\|go\|mixed]` | Arestas de dependência via servidores LSP (TASK-4, TASK-30, TASK-31) | a escolhida (padrão `rust`) | todos os arquivos + layout |
| `05-file-tree.sh` | Árvore de arquivos na sidebar (TASK-5) | `mixed` | só a pasta (canvas vazio) |
| `06-highlighting.sh` | Syntax highlighting (TASK-6) | `mixed` | um arquivo por linguagem |
| `07-layout.sh` | Layout hierárquico (TASK-7) | `rust` | todos os arquivos, **sem** layout |
| `08-ui-panels.sh` | Toolbar, sidebars, Settings, atalhos (TASK-8) | `rust` | todos os arquivos + layout |
| `09-minimap.sh` | Minimap (TASK-9) | `large` | ~150 arquivos + layout |
| `10-open-all-and-clear.sh` | Ctrl+Shift+O, Clear All/Selected, Shift+X | `mixed` | só a pasta (canvas vazio) |
| `11-cst.sh` | Aba CST: árvore tree-sitter e destaque de linhas (TASK-11) | `mixed` | um arquivo por linguagem + README e JSON |
| `12-lsp.sh` | Aba LSP: símbolos, referências entre arquivos, pular para a linha (TASK-12; precisa do rust-analyzer) | `rust` | `util.rs` e `main.rs` + layout |
| `13-showcase.sh` | Workspace de exemplo com tudo: todas as relações, barrels, Git, arquivo longo (TASK-46; os arquivos estão em `examples/showcase`) | `showcase` | todos os arquivos + layout |
| `14-palette-search.sh` | Paleta Ctrl+K e painel Search: arquivos, texto e símbolos (TASK-16, TASK-17) | `showcase` | só a pasta (canvas vazio) |
| `16-mcp-notes-ignore.sh` | Servidor MCP, Agent notes com links cc://, lista de ignorados e .codecanvas/settings.json (TASK-55.4) | `rust` | todos os arquivos + layout |
| `15-git-diff-watcher.sh` | Branch na barra, painel Changes, janela de diff e file watcher (TASK-14, TASK-18, TASK-13) | `showcase` | só a pasta (canvas vazio) |

## Fixtures

Geradas por `fixtures.sh` em `target/manual-fixtures/<nome>`, que já é ignorado pelo git.
Cada execução apaga e recria a fixture, então pode editar os arquivos à vontade durante o teste.

- `rust`: crate com `mod`, `#[path]`, `use crate/super`, `pub use`.
- `ts`: imports relativos, `index.ts`, alias do `tsconfig` (`@lib/*`), `.js`→`.ts`, `require`, `import()`.
- `python`: layout `src/`, imports absolutos e relativos.
- `go`: `go.mod` com pacotes locais.
- `mixed`: as quatro acima, mais `.github/`, `node_modules/`, `target/`, um PNG e um arquivo latin-1.
- `large`: ~150 arquivos Rust longos com uma árvore de dependências.
- `nested`: pastas profundas e duas pastas `src/` diferentes.
- `showcase`: cópia de `examples/showcase` (loja em TS/TSX, Rust, Python, Go, Markdown e JSON) como repositório Git com um commit e mudanças no working tree.

As arestas vêm dos servidores de linguagem: só aparecem para linguagens com
servidor instalado (veja `12-lsp.sh`). As da fixture `rust` que o `04-edges.sh`
promete são conferidas por `tests/e2e/manual_fixtures.rs`, com o servidor falso
e, num teste ignorado, com o rust-analyzer de verdade. Se mudar a fixture
`rust`, atualize os três juntos.

## Variáveis de ambiente

| Variável | Efeito |
|---|---|
| `SPATIALCODING_RELEASE=1` | compila e roda em release (bom para `02` e `09`, que têm muitos arquivos) |
| `SPATIALCODING_FIXTURES=DIR` | gera as fixtures em `DIR` em vez de `target/manual-fixtures` |
| `DRY_RUN=1` | só gera a fixture e imprime o comando, sem compilar nem abrir o app |

## Abrir o app à mão

Os scripts usam a linha de comando do app, que também funciona direto:

```sh
cargo run -- PASTA [--open ARQUIVO]... [--open-all] [--layout]
cargo run -- --help
```

## Testes automáticos

```sh
scripts/test.sh          # unitários + e2e
scripts/test.sh unit
scripts/test.sh e2e canvas::
```
