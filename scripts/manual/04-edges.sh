#!/usr/bin/env bash
# Dependency edges from the language servers (TASK-4, TASK-30, TASK-31).
# Usage: 04-edges.sh [rust|ts|python|go|mixed]   (default: rust)
# Edges only appear for languages whose server is installed (see 12-lsp.sh):
# rust-analyzer, typescript-language-server, pyright/basedpyright/pylsp, gopls.
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

lang=${1:-rust}
case $lang in
  rust) expected="client.rs: 'server: Server,' e 'Server::bind(…)' → 'pub struct Server' (linhas 3-5) e 'pub fn bind' de server.rs; 'self.server.addr()' → 'pub fn addr'; main.rs: 'util::shout' → helpers/strings.rs (util.rs só reexporta)" ;;
  ts|python|go) expected="cada uso de um símbolo de outro arquivo aberto liga o statement do uso à definição (precisa do servidor da linguagem)" ;;
  mixed) expected="a união das linguagens com servidor instalado; nenhuma aresta entre linguagens" ;;
  *) echo "usage: $0 [rust|ts|python|go|mixed]" >&2; exit 2 ;;
esac

dir=$(fixture "$lang")
checklist "04 · Arestas de dependência via LSP ($lang)" \
  "Abrir o painel direito → Edges logo ao iniciar|'Analysis (language servers)': 'Rust: starting…' ou 'indexing… N/M files' e depois 'M files analyzed'; linguagem sem servidor mostra o erro em vermelho" \
  "Esperar a indexação terminar (rust-analyzer: alguns segundos)|as arestas aparecem aos poucos, sem travar a interface; o layout de --layout é refeito quando a análise termina" \
  "Observar o canvas sem nada selecionado|arestas quase invisíveis: 'Only Selected' vem ligado" \
  "Clicar num arquivo|as arestas dele (entrando e saindo) aparecem fortes, com ponta de seta no destino" \
  "Aproximar (Ctrl+roda) um arquivo selecionado|cada aresta sai da lateral do nó na linha do statement que USA o símbolo e chega na linha da DEFINIÇÃO no outro arquivo (função, struct, método); vários usos geram várias arestas" \
  "Com o arquivo selecionado, olhar o código dos dois lados|os statements de uso e as definições inteiras ficam destacados na cor das arestas" \
  "Conferir as arestas|esperado: $expected" \
  "Um import cujo símbolo é usado no arquivo|não gera aresta própria; um import sem uso liga a linha do import à definição" \
  "Painel direito → Edges → desmarcar Only Selected|todas as arestas visíveis; as do nó selecionado mais fortes (Opacity: Depth)" \
  "Trocar o estilo: Bezier, Step, Line, PCB|o traçado das arestas muda de forma" \
  "Desmarcar Arrow Heads|as setas somem" \
  "Arrastar um nó selecionado|as arestas acompanham o nó"
launch "$dir" --open-all --layout
