#!/usr/bin/env bash
# Dependency edges from the language servers (TASK-4, TASK-30, TASK-31, TASK-43).
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
  "Visibility → desmarcar All sem seleção; depois selecionar um arquivo|sem seleção não há arestas; ao selecionar aparecem só as relações habilitadas ligadas à seleção" \
  "Relationship types → alternar Imports e References|imports de módulos locais têm filtro próprio; usos de símbolos e links locais em documentos/configs continuam em References" \
  "Fixture ts → abrir View.tsx/Base.ts/Button.tsx e aguardar o LSP|View extends Base gera Inherits; JSX Button gera Renders; alternar os tipos oculta somente a relação correspondente" \
  "Highlights → Selected e Backwards; Highlight Backwards Edges|Selected controla o realce da seleção; Backwards destaca entradas na seleção; a última opção destaca arestas indo da direita para a esquerda" \
  "Levels → ativar e variar Depth|arestas sem override de seleção usam cores das pastas relativas à raiz, agrupadas pela profundidade escolhida" \
  "Diff → Refresh comparison sem repositório Git/HEAD|erro ou indisponibilidade visível; Changes/Diff Colors/Deleted continuam desabilitados, sem inventar um diff vazio" \
  "Em projeto Git com commit → baseline HEAD (ou outra revisão) → Refresh comparison|carregamento assíncrono, hash imutável e contagens de relações adicionadas/removidas/iguais; somente imports/documentos locais entre arquivos abertos entram na comparação" \
  "Após remover/adicionar imports, reabrir os arquivos e atualizar a comparação; ativar Changes/Diff Colors/Deleted|Changes realça as mudanças; adicionadas verdes/removidas vermelhas; Deleted oculta as removidas, que usam headers sem marcar linhas antigas no código atual" \
  "Editar baseline ou abrir/fechar arquivos durante a comparação|resultado antigo não reaparece; controles aguardam nova comparação; vínculos somente do LSP são explicitamente não comparados" \
  "Clicar num arquivo|borda branca no nó; as arestas dele (entrando e saindo) ficam azul-claras, opacas e por cima das outras, com ponta de seta no destino; as demais continuam visíveis, mais apagadas" \
  "Com zoom médio/próximo, olhar as bordas dos arquivos|cada linha ligada por aresta tem um círculo (port) na borda voltada para o outro arquivo e uma faixa na linha; as arestas terminam no círculo; não existem badges de símbolo com borda azul" \
  "Comparar uma linha de import (ex.: mod/use em main.rs, reexport em util.rs) com um uso ou definição de símbolo|ports de import teal, de símbolo cinza; várias arestas da mesma linha compartilham um único círculo; ports da seleção ficam mais claros" \
  "Edges → Connections → trocar as cores Import/Symbol e depois desmarcar Ports|cores mudam nos círculos e faixas; sem Ports, círculos e faixas somem e as arestas voltam a encostar no nome no código" \
  "Com Ports desmarcado, aproximar (Ctrl+roda) um arquivo selecionado|a aresta sai do identificador USADO e chega ao nome na DEFINIÇÃO, não à borda do arquivo nem à primeira linha de comentários do statement; alinhamento acompanha o zoom" \
  "Recolher um nó ou afastar até só restarem blocos|sem ports; as arestas usam o header" \
  "Com o arquivo selecionado, olhar o código dos dois lados|os statements de uso e as definições inteiras ficam destacados na cor do tipo do símbolo (fn, struct…, editável na aba Edges), com uma barra na borda esquerda" \
  "Na definição, olhar o nome do símbolo (ex.: 'pub struct Server', 'pub fn bind') em zooms diferentes|a marca mais forte cobre exatamente o nome, sem começar no meio dele nem passar para o '{' ou o '('" \
  "Após indexar, clicar exatamente em bind no código de server.rs (não na margem)|somente as conexões de bind e seus statements ficam destacados; Server e addr, no mesmo arquivo, não ganham destaque; a câmera não salta" \
  "Clicar em bind no uso Server::bind de client.rs|seleciona o mesmo símbolo definido em server.rs; client.rs continua sendo o nó arrastável" \
  "Shift+clique no nome addr; depois Shift+clique em bind|primeiro combina os símbolos; depois remove bind do destaque" \
  "Clicar na margem vazia de um arquivo, depois selecionar um símbolo e usar Esc ou clicar no fundo|a margem volta à seleção do arquivo inteiro; Esc/fundo limpam a seleção de símbolos" \
  "Painel LSP → clicar em um símbolo|lista referências como antes e aplica o mesmo foco por símbolo às dependências; clicar numa referência continua navegando até ela" \
  "Conferir as arestas|esperado: $expected" \
  "Um import cujo símbolo é usado no arquivo|mantém a aresta Imports para o módulo local; o LSP evita duplicar a referência de import quando há usos reais do símbolo" \
  "Painel direito → Edges → desmarcar Only Selected|todas as arestas ganham ponta de seta, não só as da seleção" \
  "Abrir helpers/strings.rs e navegar até o final|o bloco inclui after_fifty_lines depois da linha 50, sem truncar o arquivo" \
  "Settings → UI → Dot Grid|a grade de pontos volta a aparecer no fundo" \
  "Trocar o estilo: Bezier, Step, Line, PCB|o traçado das arestas muda de forma" \
  "Desmarcar Arrow Heads|as setas somem" \
  "Arrastar um nó selecionado|as arestas acompanham o nó"
launch "$dir" --open-all --layout
