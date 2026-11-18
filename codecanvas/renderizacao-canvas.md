# Renderização no canvas: Dependency Edges, LSP e CST

Legenda: **[obs]** observado ao vivo no app (redux/src, v2.6.1); **[bundle]** extraído do código JS público (identificadores
não são ofuscados, valores abaixo são os do código); **[inf]** inferido. LSP **não foi observado ao vivo**: o app só consulta LSP com o
servidor local, e o browser de teste não conseguiu abri-lo (ver "Limites").

## 1. Níveis de zoom (LOD)
Painel Shift+V (barra L5…L1, mostra zoom/pos/screen/visible). Zoom vai de 0.001 a 1 (máx 100%).

| Nível | Zoom | Texto do arquivo | CST | Label do arquivo |
|---|---|---|---|---|
| L1 | ≥ 0.30 | código colorido legível (char hit) | sem tinta base; só hover/seleção | visível (vira "pill" fixo no topo quando o topo do nó sai da tela [obs]) |
| L2 | ≥ 0.10 | "minimap": um retângulo por trecho não branco, na cor do token, alpha .4 | tinta base alpha .04 | visível |
| L3 | ≥ 0.04 | nenhum | tinta .10 | visível |
| L4 | ≥ 0.01 | nenhum | .10 | fade (0.04→0.01) |
| L5 | < 0.01 | nenhum | .10 | oculto; **semantic fill** (nó preenchido por seleção/diff/busca) |

[bundle] `ZOOM_LEVEL_THRESHOLD_*`, `renderModeByLevel = text, minimap, none, none, none`, `cstBaseModeByLevel = text, minimap, minimap, minimap, none`.
Texto: fonte "Monaco, Consolas, monospace" 11 px, linha 18 px, largura de nó 650 (700 nas configurações), texturas em chunks de 2048 px.
Capturas: `img/19-zoom-0.6-text.png`, `img/10-zoom-0.45.png`, `img/11-zoom-0.26-cst.png`, `img/12-zoom-0.13.png`.

## 2. Dependency Edges

### Ancoragem [bundle + obs]
- Cada edge liga **a linha** do import (origem, na **borda direita** do nó importador) à linha do export (destino, na **borda esquerda** do nó exportado); y = centro da linha.
  Como o layout é esq→dir, o importador fica à esquerda e as arestas correm para a direita. Import sem linha específica (arquivo inteiro) usa o topo do nó.
- Pontos de conexão (Connection Markers): círculo r = max(0.5, (lineHeight-2)/2), cor `#9ABAF2`/edge default alpha .5, mais um retângulo de endpoint com gradiente alpha .05→0 cobrindo 75% da linha (esquerda no export, direita no import) e traço .25→0.
- Com o arquivo de origem selecionado, desenha-se um endpoint por linha de referência (até 4096).
- Seta (Arrow Heads): 10 px de comprimento, 5 de meia-largura, **na ponta do importador**; não aparece em backwards nem se o gradient gap começa em 0. Escala `(1/zoom)^(1+ln(p)/ln(1000))`, p = Min Size (0.3–1, default .5 → expoente ≈ 0.90). Colisão em grade de 16 px, prioridade: dependency < backwards < selected < selectedBackwards < symbol.

### Estilos [bundle; obs em `img/13`, `14`, `15`]
- **Bezier** (default): alças horizontais `h = dx*k` (k 0–2; se dx<0, `min(|dx|*k, 3000)`); cp1=(sx+h,sy), cp2=(tx-h,ty). Achatado em espaço de tela, erro 0.35 px, máx 4096 segmentos.
- **Step** (`stepped`): `(sx,sy)→(mid,sy)→(mid,ty)→(tx,ty)`, mid = (sx+tx)/2.
- **Line** (`straight`): reta.
- **PCB**: horizontal → diagonal 45° → horizontal; `Nc=(dx-|dy|)/2`; se |dy|<1 reta; se Nc<0 cai para step. Não há roteamento que desvie de nós.

### Gradiente (gap no meio da aresta) [bundle]
Switch Gradient + Gap Mode **%** | **Fixed**. O gap só existe se a aresta tiver comprimento ≥ `Min Dist` (default 40000 px de mundo, ou seja quase nenhuma). Em % o trecho apagado é `0.5 ± p/200` (80% → só 10% de cada ponta visível); em Fixed ficam `px` (default 1000) visíveis em cada ponta. Cada ponta faz fade 1→0 em sub-traços com jitter (passo ~6 px, ≤20 passos; menos de 800 edges). Desligado para edges de diff, re-export e barrel.

### Opacidade, espessura, cores [bundle]
- Opacity **Depth** (default): multiplica por distância de pastas: 0→1, ≤2→.5, ≤4→.25, senão .125. **Full** ignora.
- Espessura base 1.5 (sqrt do nº de edges fundidas); selecionada 2.75; backwards 4; selecionada-backwards 4.4.
- Cores: default `#7684A0` (× Lightness/100), semantic fade `#3E3D48`, selected `#AED6FF`, backwards `#EF4444` (vermelho, `img/15`), selected-backwards `#E85580`, added `#4CD964`, removed `#FF5F56`. Relações (a 80% de brilho): uses `#6AA7A0`, extends `#B69B65` (a aresta amarelada vista no canvas), inherits `#9B8AB5`, conforms `#85A87C`. Re-export highlight `#38BDF8` (largura ≥3), barrel highlight `#F97316` (≥2.5).
- Backwards = `source.x + source.width > target.x`.

### Camadas e z-order [bundle]
Onze containers fixos nos índices 0–10 do viewport, **abaixo** do `node-collection`: edges base ficam **atrás** dos arquivos [obs]. Selecionadas (e backwards destacadas) vão em 3 gráficos adicionais acima (inferido).
Modos: **pixel** (default; `pixelLine`, 1 px de dispositivo), **mesh** (largura real; só se zoom > `meshPixelFlipZoom` .02), **sampled pixel** (polilinha 12 seg.), **mesh geometry** (quad strips), **batched strokes** (agrupa por largura/cor/alpha; sem gradiente). Abaixo do zoom L3 (0.04) as edges finas trocam para a cor semantic fade.

### Visibilidade e orçamento [bundle]
Filtro ativo quando All está off: mantém a aresta se toca seleção (`selected`), faz parte do diff (`changes`), é backwards, é interna à mesma pasta-pai (`internal`), cruza pastas dentro da faixa de nesting (`external`, max 20 = sem limite) ou tem profundidade de pasta ≤ `localFolderMaxDepth` (default 1). Não achei condição de zoom, apesar do nome interno "zoomedOut".
Orçamento de memória 384 MiB (ou 25% do heap, 64 MiB–1 GiB): ao estourar, `automaticallyLimitEdgeVisibility` desliga All, usa profundidade 1 e mostra toast "Edge drawing limited" (não se o usuário ligou All manualmente).
Defaults completos: baseLightness 100, thickness sqrt, gradient on, gap 80%, markers on, arrowheads on/only selected/scale on, barrel on, re-export off.

## 3. LSP
Fluxo [obs]: hover/click num token → consulta ao servidor; sem servidor aparece o toast "Local server required: the local server needs to be connected before LSP symbol graph results can run" com botão Open Server Panel (`img/18`). O click mostra o retângulo do token (contorno) e deixa as edges do arquivo clara/destacadas.

Overlays [bundle]:

| Tipo | Cor | Alpha fill | Traço |
|---|---|---|---|
| token de origem | `#FFFFFF` | .35 | |
| reference | `#FFC465` âmbar | .28 | |
| type definition | `#FF5CBF` rosa | .26 | 1 px α .6 |
| definition (selection / context / origin) | `#FF5CBF` | .34 / .10 / .24 | α .6 / .7 em context/origin |
| implementation (selection / context) | `#00FF65` verde | .30 / .05 | α .55 |
| incoming: callsite / caller / caller-selection | `#C4A3FF` roxo | .26 / .16 / .30 | α .55 |
| outgoing: callsite / callee / callee-selection | `#FF6B24` laranja | .26 / .16 / .30 | α .55 |

**Token edges**: bezier de largura 1.25 entre o token de origem e cada alvo, na cor do overlay do alvo, ancorados na **coluna de caractere** (`x + gutter + origem + char*charWidth`); alpha: reference .5 (×.55 em contexto import/export; cai para .1 se há incoming calls ou > 5 refs), typeDef .55, impl .52/.38, incoming callsite .8. Lado `right`, ou `left` quando alvo e origem estão no mesmo arquivo. Limites: 300 overlays e 400 edges por tipo. Gradient gap só se `applyGradientToTokenEdges` (off).
**Estados**: `hoverTokenAtom`, `tokensAtom` (seleção), `pinnedTokenGraphsAtom` (lista de grafos fixados com Shift+Click; Shift+C limpa).
**Dim**: com seleção/pin que cruza arquivos, as dependency edges recebem alpha × 0.2 (`lspSelectionDimPercent` 80, ligado); some com atraso de 500 ms.
**Out of View** (off por padrão): uma tag com o basename do arquivo conectado, posta onde a linha origem→centro do arquivo cruza a borda da viewport (margem 20 px, tags espalhadas); clique faz pan.
**Symbol Mapping** (qual token consultar quando o hover cai numa região grande do CST): corpo de função → nome; corpo de arrow → `=>`; corpo de classe → nome; inicializador → nome da variável; for-in/of → iterável; argumentos de chamada → expressão chamada.

## 4. CST
Tintas por papel [bundle `DEFAULT_CST`], batendo com as cores do painel [obs `img/17`]:
function `#3B82F6` · class/interface/type/enum `#A855F7` · variable/lexical `#22C55E` · conditional `#F97316` · loop `#EC4899` · switch `#EAB308` · errorHandling (try/catch/finally/throw) `#EF4444` · return `#06B6D4` · call `#0EA5E9` · member `#6366F1` · expression `#9CA3AF` · jsx `#22D3EE` · import `#FB923C` · export `#A3E635` · default `#94A3B8`.
**Ocultos por padrão**: if_statement, else_clause, switch_statement (estilo: só borda esquerda, fillAlpha 0, borda .5/α .5). No painel cada tipo mostra `visíveis/total` (ex. import 34/374).
**Estilo por tipo** (`img/16`): Fill Solid | Gradient (cor inicial/final, alpha, horizontal por padrão), cor, fill opacity; Border cor, Width (0.5, dividido pelo zoom), opacity, **Sides** (4 lados); preview "4% at zoom 0.30"; Reset to Default. Alpha do traço = min(2·alpha, 1).
**Desenho por LOD**: em L1 sem tinta (só hover/seleção); em L2+ retângulos tintados. Em modo texto, um range multilinha é um polígono "calendário" (segue a coluna inicial da 1ª linha e a final da última); nos demais, o retângulo envolvente. Um nó some se `depth >= {2,2,2,3,3}[nível]` e `linhas < {1,1,1,100,100}[nível]`. Hover: opacidade por aninhamento [1, .5, .25, .12]; seleção: traço 1.5 px α .9; hover numa linha da lista destaca todas as instâncias do tipo, e numa região mostra contorno + label do símbolo (`miniKindOf` [obs]).
**Opções** (`img/17`): Limit CST depth (on, max 6; há redução automática com recuperação após 30 s), Show Hidden Types, Semantic Nodes Only, Single Hover Symbol, Selected Edge CST Nodes (destaca import/export CST nas pontas das edges dos arquivos selecionados; só com ≤3 nós selecionados), Symbol Mapping.

## Limites
- LSP ao vivo não verificado. `npx code-canvas-server` rodou na 9315 (`/__code_canvas/health` ok), mas o Chrome de teste bloqueou o acesso do site à rede local (WebSocket e fetch falharam); contornar exigiria desligar proteções do navegador, o que não foi feito. Para ver ao vivo: usar o app com um browser normal (que pergunta a permissão de rede local) ou a extensão VSCode.
- Valores [bundle] não foram conferidos medindo pixels; `endpointDotSizePx` parece sem uso e a ordem exata acima dos nós das edges selecionadas é inferida.
