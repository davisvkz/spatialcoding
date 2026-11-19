# Dependency edges

## Ancoragem [bundle + obs]
- Cada edge liga **a linha** do import (origem, **borda direita** do nó importador) à linha do export (destino, **borda esquerda** do nó exportado); y = centro da linha. O layout é esq→dir, então as arestas correm para a direita e passam **atrás** dos nós [obs].
- Import sem linha específica usa o topo do nó. Com o arquivo de origem selecionado, um endpoint por linha de referência (até 4096).
- **Connection Markers**: círculo r = max(0.5, (lineHeight-2)/2) em `#9ABAF2`/default alpha .5, mais retângulo de endpoint com gradiente alpha .05→0 em 75% da linha e traço .25→0 (esquerda no export, direita no import). Pastas: raio 8, gap 4.
- **Seta**: 10 px × 5 de meia-largura, na ponta do importador; não aparece em backwards nem quando o gradient gap começa em 0. Escala `(1/zoom)^(1+ln(p)/ln(1000))`, p = Min Size (0.3–1, default .5). Colisão em grade de 16 px; prioridade dependency < backwards < selected < selectedBackwards < symbol.

## Estilos (`img/13`, `14`, `15`)
- **Bezier** (default): alças horizontais `h = dx*k` (k 0–2; dx<0 → `min(|dx|*k, 3000)`); cp1=(sx+h,sy), cp2=(tx-h,ty). Achatado em tela, erro 0.35 px, máx 4096 segmentos.
- **Step** (`stepped`): `(sx,sy)→(mid,sy)→(mid,ty)→(tx,ty)`, mid = (sx+tx)/2.
- **Line** (`straight`).
- **PCB**: horizontal → diagonal 45° → horizontal; `Nc=(dx-|dy|)/2`; |dy|<1 reta; Nc<0 cai para step. Sem desvio de nós.

## Gradiente (gap no meio)
Só se a aresta tem comprimento ≥ Min Dist (default 40000 px de mundo, quase nenhuma). Modo **%**: apaga `0.5 ± p/200` (80% → 10% visível em cada ponta). Modo **Fixed**: `px` visíveis em cada ponta (default 1000). Fade 1→0 em sub-traços com jitter (passo ~6 px, ≤20 passos, <800 edges). Desligado para diff, re-export e barrel.

## Opacidade, espessura, cores
- Opacity **Depth** (default): 0→1, ≤2→.5, ≤4→.25, senão .125 por distância de pastas; **Full** ignora.
- Espessura 1.5 (sqrt do nº de edges fundidas); selecionada 2.75; backwards 4; selecionada-backwards 4.4.
- Cores: default `#7684A0` (× Lightness/100), semantic fade `#3E3D48`, selected `#AED6FF`, backwards `#EF4444` (`img/15`), selected-backwards `#E85580`, added `#4CD964`, removed `#FF5F56`. Relações (80%): uses `#6AA7A0`, extends `#B69B65` (a aresta amarelada), inherits `#9B8AB5`, conforms `#85A87C`. Re-export highlight `#38BDF8` (≥3), barrel highlight `#F97316` (≥2.5).
- Backwards = `source.x + source.width > target.x`.

## Camadas e z-order
Onze containers fixos nos índices 0–10 do viewport, **abaixo** do `node-collection`. Selecionadas (e backwards destacadas) vão em 3 gráficos adicionais, acima [inf]. Modos: **pixel** (default, `pixelLine` 1 px), **mesh** (largura real; só se zoom > `meshPixelFlipZoom` .02), **sampled pixel** (12 seg.), **mesh geometry** (quad strips), **batched strokes** (sem gradiente). Abaixo do zoom L3 as edges finas trocam para a cor semantic fade.

## Visibilidade e orçamento
Com All desligado a aresta fica se: toca seleção, é do diff, é backwards, é interna à pasta, cruza pastas dentro da faixa de nesting (max 20 = sem limite) ou tem profundidade de pasta ≤ `localFolderMaxDepth` (1). Não há condição de zoom, apesar do nome interno "zoomedOut".
Orçamento 384 MiB (ou 25% do heap, 64 MiB–1 GiB): ao estourar, `automaticallyLimitEdgeVisibility` desliga All, profundidade 1 e mostra "Edge drawing limited" (não se All foi ligado manualmente).

## Defaults [bundle `DEFAULT_EDGE_SETTINGS`]
style bezier · baseLightness 100 · thickness sqrt · opacity depth · gradient on, gap % 80, fixed 1000, minDist 40000 · arrowheads on/only selected/scale on/min .5 · endpoint rects e dots on · barrel on, re-export off · dim LSP on, 80% · mesh off, pixel on, sampled off.
