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
| `04-edges.sh [rust\|ts\|python\|go\|mixed]` | Arestas de dependência e análise por linguagem (TASK-4, TASK-10) | a escolhida (padrão `mixed`) | todos os arquivos + layout |
| `05-file-tree.sh` | Árvore de arquivos na sidebar (TASK-5) | `mixed` | só a pasta (canvas vazio) |
| `06-highlighting.sh` | Syntax highlighting (TASK-6) | `mixed` | um arquivo por linguagem |
| `07-layout.sh` | Layout hierárquico (TASK-7) | `rust` | todos os arquivos, **sem** layout |
| `08-ui-panels.sh` | Toolbar, sidebars, Settings, atalhos (TASK-8) | `rust` | todos os arquivos + layout |
| `09-minimap.sh` | Minimap (TASK-9) | `large` | ~150 arquivos + layout |
| `10-open-all-and-clear.sh` | Ctrl+Shift+O, Clear All/Selected, Shift+X | `mixed` | só a pasta (canvas vazio) |

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

As arestas que o `04-edges.sh` promete são conferidas automaticamente por
`tests/e2e/manual_fixtures.rs`. Se mudar uma fixture, atualize os três juntos.

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
