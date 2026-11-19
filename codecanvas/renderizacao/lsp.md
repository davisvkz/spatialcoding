# LSP no canvas (observado ao vivo)

Teste em `reduxjs/redux` / `src`, app v2.6.1 + `code-canvas-server@2.6.1` na porta 9315 (TypeScript embutido, status **Running**).

## 1. Pré-requisito: servidor local
- Sem servidor, o click mostra o toast "Local server required… before LSP symbol graph results can run" (`img/18`).
- Com o servidor (`npx code-canvas-server`, WebSocket `ws://127.0.0.1:9315/ws`, health em `/__code_canvas/health`) o ponto laranja some do rail e o LSP funciona.
- Painel **Language Servers** (menu ⋮ do LSP Symbols, `img/20`): barra de arquivos do repo por linguagem (320 arquivos: 139 TS/JS, 181 outros; abas Repository | Canvas); linhas: TypeScript/JavaScript Built-in **Running**; Svelte e Vue (Volar) Bundled; Python (Pyright) Bundled; Rust (rust-analyzer) Installed; PHP, Go, Java, C, C++, Swift (SourceKit-LSP, com Setup) Not installed + botão Install; "Load project environment".
- Nota de ambiente de teste: o Chrome headless bloqueou o site de falar com `127.0.0.1` (Local Network Access). Foi preciso lançar a sessão com `--disable-features=LocalNetworkAccessChecks`; num Chrome normal aparece o prompt de permissão.

## 2. Interação [obs]
- **Hover** num token: destaca o token (retângulo azul claro) e a linha do cursor (`img/21`). Com *Query On Hover* **desligado** (padrão), o hover só destaca; a consulta ao servidor acontece no **click**.
- **Click**: fixa (pin) o grafo do símbolo; toast "Shift+click to pin multiple symbols. Shift+C to clear all." (`img/22`). Resultado em ~1–2 s.
- **Shift+Click** fixa vários; **Shift+C** ou o botão "Clear N Symbol(s)" limpa.
- Em zoom afastado, os alvos ganham **pills de label** com o nome do símbolo: `kindOf`, `miniKindOf`, `getUnexpectedStateShapeWarningMessage` (o chamador) (`img/24`).

## 3. Resultado para `kindOf` (src/utils/kindOf.ts:62)
Painel "LSP Symbols → Symbol Graph" (`img/22`, `23`):
- **Pinned Symbols (1)**: `src/utils/kindOf.ts:62`
- **Files (4)** com contagem: bindActionCreators.ts (4), combineReducers.ts (4), createStore.ts (18), kindOf.ts (4)
- **Results (30) / Edges (10)**:
  - **Implementations (1)**: `impl ctx` kindOf.ts:62
  - **Incoming Calls (23)**: pares `in callsite` / `caller` / `caller sel` (ex. createStore.ts:99, :124, :86…, combineReducers.ts:35/14, bindActionCreators.ts:68/58)
  - **Outgoing Calls (3)**: `out callsite` kindOf.ts:66, `callee` kindOf.ts:1, `callee sel` kindOf.ts:2 (miniKindOf)
  - **References (3)**: createStore.ts:13, combineReducers.ts:12, bindActionCreators.ts:7 (os imports)

## 4. Cores no canvas: observado vs. bundle
| Tipo | Cor [bundle] | Visto ao vivo |
|---|---|---|
| reference | `#FFC465` âmbar α .28 | **sim**: pontos âmbar na lista e **edges âmbar** do uso até cada referência em outros arquivos (`img/27`, `28`, `29`) |
| definition (sel / ctx) | `#FF5CBF` rosa | **sim**: bloco rosa preenchido sobre `Unsubscribe` (def sel) e entradas `def sel`/`def ctx` (`img/27`) |
| type definition | `#FF5CBF` + traço | **sim**: entrada `type def` + retângulo rosa com contorno em volta da interface (`img/27`) |
| implementation ctx | `#00FF65` verde α .05 + traço α .55 | **sim**: bloco verde cobrindo o corpo de `kindOf` (`img/22`) |
| incoming (callsite/caller/caller sel) | `#C4A3FF` roxo | **sim**: marcadores roxos e **edges roxas** de createStore/combineReducers/bindActionCreators até a linha 62 (`img/22`) |
| outgoing (callsite/callee/callee sel) | `#FF6B24` laranja | **sim**: bloco laranja sobre `miniKindOf` (linhas 1–38) e edge laranja do callsite (linha 66) até a função (`img/22`, `24`) |
| token de origem | branco α .35 | **sim**: token `kindOf` com fundo claro |

Alphas e larguras: ver [bundle]: callsite .26, caller/callee .16, selection .30, traço .55; token edges de largura 1.25, bezier ancorado na coluna do caractere; alpha reference .5 (cai a .1 com incoming ou >5 refs), typeDef .55, impl .52/.38, incoming callsite .8; limites 300 overlays / 400 edges por tipo.

## 5. Efeito sobre as dependency edges
As edges de dependência ficam muito mais apagadas enquanto há símbolo fixado [obs `img/22`]: *Dim Dependency Edges* ligado, 80% (alpha ×0.2; volta com atraso de 500 ms) [bundle].

## 6. LSP Advanced Settings (`img/25`) [obs]
Query: **Query On Hover** off · **Auto Open Matches** on, **Max 25 files** · Navigation: **Keep Zoom** off, **Smooth Transition** on · Symbol Edges: **Dim Dependency Edges** on, **Dim % 80** · **Reveal Through Collapse** off · **Out of View** off.
**Out of View** ligado: uma tag com o nome do arquivo conectado e fora da tela aparece na borda da viewport (ex. `bindActionCreators.ts` na borda inferior, `img/26`); clique faz pan [obs + bundle].
Auto Open Matches: abre os arquivos com resultados (até 25) — createStore/combineReducers/bindActionCreators já estavam abertos por isso.

## 7. Symbol Mapping [bundle]
Quando o hover cai numa região grande do CST, escolhe-se o token a consultar: corpo de função → nome; corpo de arrow → `=>`; corpo de classe → nome; inicializador → nome da variável; for-in/of → iterável; argumentos de chamada → expressão chamada.

## 8. Estados [bundle]
`hoverTokenAtom` (hover), `tokensAtom` (seleção), `pinnedTokenGraphsAtom` (lista de `{sourceToken, overlays, edges}`); `collectTokenEdges` une os pins com a seleção.


## 9. Complemento: casos antes não observados (segunda rodada)

### 9.1 Type definition e definition (`img/27`)
Click em `Unsubscribe` (uso em `Observable`, src/types/store.ts:60). Painel: **Pinned Symbols (1)** `store.ts:60`; **Files (3)** index.ts (1), types/store.ts (4), test/typescript/store.test-d.ts (2); **Results (7) / Edges (7)**:
- **Definitions (2)**: `def sel` e `def ctx` em store.ts:34 (a interface `Unsubscribe`)
- **Type Definitions (1)**: `type def` store.ts:34
- **References (4)**: store.ts:145, index.ts:15, store.test-d.ts:7 e :202
No canvas: edge **rosa** (bezier) do token clicado até a interface, que ganha retângulo rosa com contorno cobrindo o bloco da declaração (linhas 34–36); edges **âmbar** para as referências em outros arquivos.

### 9.2 Shift+Click, pin múltiplo (`img/28`)
Segundo pin em `Observer` (store.ts:68): botão passa a **"Clear 2 Symbols"**, **Pinned Symbols (2)** (60 e 68), Files (4), **Results (15) / Edges (14)**: Definitions (3), Type Definitions (1), **Implementations (1)** `impl sel` store.test-d.ts:220 (edge **verde**), References (10). Os grafos se somam; cada pin mantém suas cores. O `x` ao lado de cada pin remove só aquele.

### 9.3 Click em zoom afastado e Symbol Mapping (`img/29`)
Com a visão geral (~0.04x) um click dentro do corpo da interface `Store` fixou o símbolo **`Store`** (label pill "Store" sobre o bloco rosa): o Symbol Mapping escolheu o token do nome mesmo sem clicar nele. Efeito colateral de **Auto Open Matches**: arquivos com referências foram abertos automaticamente (Files 18 → 21; specs de `test/` entraram no canvas) e dezenas de edges âmbar ligaram `Store` aos usos.

### 9.4 Highlight Top-Level Symbols (`img/30`)
Com `kindOf.ts` selecionado (clique no label do nó), o botão **Highlight top-level symbols** (tooltip: "Highlight all 5 top-level symbols in the selected file (imports skipped)") fixou **5 símbolos em sequência**: `kindOf.ts:2` (miniKindOf), `:40` (ctorName), `:44` (isError), `:53` (isDate), `:62` (kindOf). Resultado: **Results (50) / Edges (20)**, Implementations (5), Incoming Calls (30), Files (4: bindActionCreators 4, combineReducers 4, createStore 18, kindOf 24). No canvas, **label pills** nomeiam os chamadores: `createStore`, `subscribe`, `dispatch`, `replaceReducer`, `getUnexpectedStateShapeWarningMessage`, `bindActionCreators`, `kindOf`; edges roxas convergem para cada função.
Os botões Open Importers / Open Imports / Highlight ficam **desabilitados sem seleção**; a seleção de um nó por click funcionou no label do arquivo (em zoom afastado o click no corpo do nó fixa símbolo em vez de selecionar).

### 9.5 Open files that import the selection (`img/31`)
Com todos os 21 nós selecionados (Ctrl+A), o botão abriu os importadores que faltavam: **Files 21 → 44** (specs de `test/`, `test/typescript/*.test-d.ts`, `examples/…`). Para `kindOf.ts` sozinho nada novo apareceu porque seus importadores já estavam abertos. Durante a seleção as edges dos nós selecionados ficam em azul claro (`#AED6FF`), mais grossas e com setas (`Arrow Heads` / only selected), as demais esmaecem [obs, bate com `edge.selected` do bundle].

### 9.5b Open imported files (`img/32`)
Partindo de um canvas com **só** `test/combineReducers.spec.ts` (aberto pela árvore, selecionado com Ctrl+A), o botão **Open imported files** levou os arquivos de **1 → 7** (`Files 1 / 7 / 320`): `src/index.ts`, `src/combineReducers.ts` e os arquivos de `types/` e `utils/` que o spec importa, posicionados à direita do spec pelo layout. As edges saem do spec em azul claro (selecionado) com **label pills** nos símbolos importados (`Reducer`, `Action`, `DO_NOT_USE__ActionTypes`). Nesse estado o botão "Open files that import the selection" ficou **desabilitado** (nenhum arquivo analisado importa o spec). Confirma o doc oficial: a lista vem do grafo de dependências, então o arquivo precisa já ter sido analisado.

### 9.6 Ordem observada das fases ao clicar
1) hover destaca token; 2) click → "TypeScript language server loading" na barra inferior na 1ª consulta (~8 s a frio, 1–2 s depois); 3) overlays + edges aparecem; 4) painel lista os resultados agrupados (Implementations, Incoming Calls, Outgoing Calls, Type Definitions, Definitions, References).

## Não verificado
Out-of-view com Shift+pin; outras linguagens (só TypeScript rodou); alpha/largura exatos medidos em pixel.
