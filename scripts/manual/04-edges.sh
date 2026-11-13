#!/usr/bin/env bash
# Dependency edges and per-language analysis (TASK-4, TASK-10).
# Usage: 04-edges.sh [rust|ts|python|go|mixed]   (default: mixed)
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

lang=${1:-mixed}
case $lang in
  rust) expected="main→config, main→net/mod, main→net/client, main→util, config→util, net/mod→client, net/mod→server, client→server, client→config, server→util, util→helpers/strings" ;;
  ts) expected="index→components/index, index→lib/math (@lib), components/index→Button, Button→lib/math, Button→lib/format (.js→.ts), legacy.js→lib/math (require), lazy→lib/format (import())" ;;
  python) expected="__init__→cart, cart→models, cart→pricing, pricing→models, cli→cart" ;;
  go) expected="main→pkg/api, main→internal/store, api→internal/store (fmt é ignorado)" ;;
  mixed) expected="a união de rust, ts, python e go (ver '04-edges.sh <linguagem>'); nenhuma aresta entre linguagens" ;;
  *) echo "usage: $0 [rust|ts|python|go|mixed]" >&2; exit 2 ;;
esac

dir=$(fixture "$lang")
checklist "04 · Arestas de dependência ($lang)" \
  "Observar o canvas sem nada selecionado|arestas quase invisíveis: 'Only Selected' vem ligado" \
  "Clicar num arquivo|as arestas dele (entrando e saindo) aparecem fortes, com ponta de seta no destino" \
  "Conferir as arestas, selecionando cada arquivo|esperado: $expected" \
  "Painel direito → Edges → desmarcar Only Selected|todas as arestas visíveis; as do nó selecionado mais fortes (Opacity: Depth)" \
  "Trocar o estilo: Bezier, Step, Line, PCB|o traçado das arestas muda de forma" \
  "Opacity Mode: Full|todas as arestas com a mesma opacidade" \
  "Desmarcar Arrow Heads|as setas somem" \
  "Arrastar um nó selecionado|as arestas acompanham o nó"
launch "$dir" --open-all --layout
