#!/usr/bin/env bash
# LSP tab and canvas hover graphs (TASK-12, TASK-44.8, TASK-51).
# Needs rust-analyzer (`rustup component add rust-analyzer`), or another
# server command in SPATIALCODING_LSP_RUST.
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

if [[ -z ${SPATIALCODING_LSP_RUST:-} ]] && ! rust-analyzer --version >/dev/null 2>&1; then
  echo "AVISO: rust-analyzer não funciona aqui; rode 'rustup component add rust-analyzer'." >&2
  echo "       Sem ele, o passo 2 em diante mostra só a mensagem de erro (que também vale testar)." >&2
fi

dir=$(fixture rust)
checklist "12 · Aba LSP: símbolos e referências" \
  "Abrir a sidebar direita (◀) e clicar na aba LSP sem nada selecionado|mensagem \"Select a file node to see its symbols.\"" \
  "Clicar no nó src/util.rs|\"Starting the Rust language server…\" e/ou \"Loading symbols…\" por alguns segundos; depois a lista: mod strings, fn parse_port  L6, fn join  L10" \
  "Passar o mouse sobre fn join|as linhas 10–12 ficam destacadas no nó util.rs do canvas" \
  "Clicar em fn parse_port|o símbolo é fixado como no canvas: \"Pinned Symbols (1)\", Files (src/config.rs, src/util.rs) e o grupo References com linhas como \"ref  config.rs:10\" (se vier vazio, o rust-analyzer ainda indexa: clicar de novo em alguns segundos)" \
  "Clicar em \"ref  config.rs:10\"|config.rs, que não estava no canvas, é aberto, fica selecionado, o canvas centraliza na linha 10 e ela fica destacada" \
  "Dar zoom out até barras (ou mais longe) e clicar de novo na mesma referência|o zoom volta para 1x e a linha fica no centro" \
  "Clicar em \"def sel  util.rs:6\"|volta para o nó util.rs que já existia (não abre outro) e destaca a linha 6; os pins continuam listados" \
  "Selecionar config.rs, abrir impl Config  L8 pelo ▸ e clicar em fn load  L9|o pin troca para load, com resultados em src/config.rs (9) e src/main.rs (8)" \
  "Com Auto Open Matches ligado (padrão), clicar num símbolo usado em arquivos fechados|os arquivos dos resultados abrem sozinhos no canvas (até Max files), também quando o clique foi no código do canvas" \
  "Clicar em dois símbolos seguidos; usar ‹ e ›|volta e avança entre os símbolos fixados pela lista; Clear N Symbols limpa os pins e símbolos, mas mantém o arquivo selecionado" \
  "Engrenagem do Symbol Graph → LSP Advanced Settings; ligar Query On Hover e parar o mouse num símbolo|um preview do grafo aparece no canvas (sem aparecer na lista Pinned Symbols) e some quando o mouse sai da linha; um clique o mantém como pin" \
  "Com Query On Hover ligado, aguardar a indexação e parar 0,4 s sobre o nome Config no código de config.rs (não na lista)|o grafo completo aparece como um pin transitório (cores e curvas dos pins); seleção, câmera e arquivos abertos não mudam" \
  "Passar sobre um uso de Config no código ou sobre uma chamada de parse_port|resolve a mesma definição pelos ranges LSP, sem depender de clique ou de a aba LSP estar aberta" \
  "Mover o mouse para espaços, arrastar o canvas ou desligar Query On Hover|o preview some imediatamente; respostas atrasadas não o fazem reaparecer" \
  "Editar ou fechar um arquivo enquanto o grafo temporário está visível|o preview é refeito na próxima parada; um preview nunca abre arquivos automaticamente" \
  "Dar zoom com um grafo local ativo|marcas seguem os nomes e blocos do código; arquivos recolhidos ou em zoom de título não recebem setas flutuantes" \
  "Zoom 1x; clicar no nome parse_port em util.rs|o símbolo é fixado (pin): referências em âmbar, definição em rosa, chamadores em roxo, chamadas em laranja; um toast diz \"Shift+click to pin multiple symbols. Shift+C to clear all.\"; o token clicado fica com fundo branco claro" \
  "Clicar no corpo de uma função (num espaço vazio dentro das chaves)|o nome da função é fixado (Symbol Mapping); em arrow functions o alvo é o =>; em um inicializador, o nome da variável" \
  "Shift+clicar em outro símbolo|os grafos se somam e o painel mostra Pinned Symbols (2), Files, Results/Edges e os grupos; o botão vira \"Clear 2 Symbols\"" \
  "No painel, ✕ ao lado de um pin; depois Shift+C|o ✕ remove só aquele pin; Shift+C limpa todos; as arestas de dependência voltam ao normal após ~0,5 s" \
  "Zoom abaixo de 0,30x com pins ativos|cada token fixado ganha uma pílula com o nome do símbolo" \
  "Ligar Out of View e afastar a vista de um arquivo que tem referências|aparece uma etiqueta com o nome do arquivo na borda; clicar nela leva a vista até lá" \
  "Selecionar um arquivo e clicar em Highlight top-level symbols (painel File nodes)|todas as declarações de topo são fixadas em sequência; o tooltip diz \"Highlight all N top-level symbols in the selected file (imports skipped)\"; clicar de novo limpa" \
  "Keep Zoom ligado e Smooth Transition ligado/desligado; clicar numa referência com zoom baixo|o zoom não muda; com Smooth Transition a vista desliza, sem ela pula" \
  "Dim Dependency Edges e Dim %|com um símbolo ativo, as arestas que não são dele ficam mais fracas conforme o Dim %" \
  "Out of View ligado; afastar a vista dos arquivos de um pin|etiquetas com o nome do arquivo aparecem na borda do canvas; clicar leva até ele" \
  "Primeira consulta a um servidor frio (ex.: clicar num símbolo TypeScript logo após abrir a pasta)|a linha de atividade no rodapé do canvas diz \"TypeScript language server loading…\" (ou \"indexing…\") até ele ficar pronto; Settings → Activity Indicator desliga a linha" \
  "File nodes → Open files that import the selection com util.rs selecionado|abre os arquivos que referenciam os símbolos de topo (pedindo ao servidor) além dos que importam por link local; o botão fica desabilitado, com dica, quando nada abriria (ex.: um .md sem links)" \
  "File nodes → Open imported files|desabilitado quando o arquivo não importa nada fora do canvas" \
  "⋮ → Language servers|diálogo com contagem por linguagem (Repository/Canvas), comando e status de cada servidor; Load project environment desabilitado com explicação" \
  "Clicar em Cargo.toml na árvore de arquivos e selecionar o nó|\"No language server for TOML.\"" \
  "Fechar o app e rodar SPATIALCODING_LSP_RUST=nao-existe scripts/manual/12-lsp.sh; selecionar util.rs na aba LSP|em vermelho: \"Could not start \`nao-existe\`: … Set SPATIALCODING_LSP_RUST to the server command.\""
launch "$dir" --layout --open src/util.rs --open src/main.rs
