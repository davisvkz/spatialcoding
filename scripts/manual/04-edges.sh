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
  "Observar o canvas sem nada selecionado|fundo quase preto, sem grade; arestas finas azul-acinzentadas e translúcidas formando uma teia (não uma parede)" \
  "Clicar num arquivo|borda branca no nó; as arestas dele (entrando e saindo) ficam azul-claras, opacas e por cima das outras, com ponta de seta no destino; as demais continuam visíveis, mais apagadas" \
  "Com o arquivo selecionado e zoom médio/próximo|não existem badges/etiquetas de símbolo com borda azul; as setas encostam no próprio nome no código" \
  "Aproximar (Ctrl+roda) um arquivo selecionado|a aresta sai do identificador USADO e chega ao nome na DEFINIÇÃO, não à borda do arquivo nem à primeira linha de comentários do statement; alinhamento acompanha o zoom" \
  "Com o arquivo selecionado, olhar o código dos dois lados|os statements de uso e as definições inteiras ficam destacados na cor do tipo do símbolo (fn, struct…, editável na aba Edges), com uma barra na borda esquerda" \
  "Na definição, olhar o nome do símbolo (ex.: 'pub struct Server', 'pub fn bind') em zooms diferentes|a marca mais forte cobre exatamente o nome, sem começar no meio dele nem passar para o '{' ou o '('" \
  "Após indexar, clicar exatamente em bind no código de server.rs (não na margem)|somente as conexões de bind e seus statements ficam destacados; Server e addr, no mesmo arquivo, não ganham destaque; a câmera não salta" \
  "Clicar em bind no uso Server::bind de client.rs|seleciona o mesmo símbolo definido em server.rs; client.rs continua sendo o nó arrastável" \
  "Shift+clique no nome addr; depois Shift+clique em bind|primeiro combina os símbolos; depois remove bind do destaque" \
  "Clicar na margem vazia de um arquivo, depois selecionar um símbolo e usar Esc ou clicar no fundo|a margem volta à seleção do arquivo inteiro; Esc/fundo limpam a seleção de símbolos" \
  "Painel LSP → clicar em um símbolo|lista referências como antes e aplica o mesmo foco por símbolo às dependências; clicar numa referência continua navegando até ela" \
  "Conferir as arestas|esperado: $expected" \
  "Um import cujo símbolo é usado no arquivo|não gera aresta própria; um import sem uso liga a linha do import à definição" \
  "Painel direito → Edges → desmarcar Only Selected|todas as arestas ganham ponta de seta, não só as da seleção" \
  "Abrir helpers/strings.rs e navegar até o final|o bloco inclui after_fifty_lines depois da linha 50, sem truncar o arquivo" \
  "Settings → UI → Dot Grid|a grade de pontos volta a aparecer no fundo" \
  "Trocar o estilo: Bezier, Step, Line, PCB|o traçado das arestas muda de forma" \
  "Desmarcar Arrow Heads|as setas somem" \
  "Arrastar um nó selecionado|as arestas acompanham o nó"
launch "$dir" --open-all --layout
