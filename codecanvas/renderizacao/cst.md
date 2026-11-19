# CST (árvore sintática tintada)

## Cores por papel [bundle `DEFAULT_CST`] (batem com o painel [obs `img/17`])
function `#3B82F6` · class/interface/type/enum `#A855F7` · variable/lexical `#22C55E` · conditional `#F97316` · loop `#EC4899` · switch `#EAB308` · errorHandling (try/catch/finally/throw) `#EF4444` · return `#06B6D4` · call `#0EA5E9` · member `#6366F1` · expression `#9CA3AF` · jsx `#22D3EE` · import `#FB923C` · export `#A3E635` · default `#94A3B8`.

**Ocultos por padrão**: if_statement, else_clause, switch_statement (estilo: só borda esquerda, fillAlpha 0, borda .5/α .5). O painel lista cada tipo com `visíveis/total` (ex. import 34/374, export 67/231).

## Estilo por tipo (`img/16`)
Fill Solid | Gradient (cor inicial/final, alpha; horizontal por padrão), cor, fill opacity; Border cor, Width (0.5, dividido pelo zoom), opacity, **Sides** (4 lados); preview "4% at zoom 0.30"; Reset to Default. Alpha do traço = min(2·alpha, 1).

## Desenho por nível de zoom
L1 sem tinta (só hover/seleção); L2+ retângulos tintados (alpha .04/.10). Em modo texto um range multilinha é um polígono "calendário" (1ª coluna da primeira linha, última da última); nos outros, o retângulo envolvente. Nó some se `depth >= {2,2,2,3,3}[nível]` e `linhas < {1,1,1,100,100}[nível]`. Hover: opacidade por aninhamento [1, .5, .25, .12]; hover na lista destaca todas as instâncias do tipo; hover em região mostra contorno + label do símbolo (`miniKindOf`) [obs]; seleção: traço 1.5 px α .9.

## Opções (`img/17`)
Limit CST depth (on, max 6; redução automática com recuperação após 30 s) · Show Hidden Types · Semantic Nodes Only · Single Hover Symbol · Selected Edge CST Nodes (CST de import/export nas pontas das edges dos arquivos selecionados, só com ≤3 selecionados) · Symbol Mapping (ver [lsp.md](lsp.md)).
Cache: payloads em atom SQLite; orçamento de memória por frações de heap (64 MiB base) [bundle, incerto se específico do CST].
