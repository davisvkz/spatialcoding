#!/usr/bin/env bash
# LSP tab: document symbols, references across files, jump to a reference (TASK-12, v2 controls TASK-44.8).
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
  "Clicar em fn parse_port|\"References to parse_port\": src/config.rs (L1 e L10) e src/util.rs (L6); o contador diz \"3 references in 2 files\" (se vier vazio, o rust-analyzer ainda indexa: clicar de novo em alguns segundos)" \
  "Clicar em \"L10  Self { host: …parse_port(\"8080\") }\"|config.rs, que não estava no canvas, é aberto, fica selecionado, o canvas centraliza na linha 10 e ela fica destacada" \
  "Dar zoom out até barras (ou mais longe) e clicar de novo na mesma referência|o zoom volta para 1x e a linha fica no centro" \
  "Clicar em \"L6  pub fn parse_port…\"|volta para o nó util.rs que já existia (não abre outro) e destaca a linha 6; a lista de referências continua lá" \
  "Selecionar config.rs, abrir impl Config  L8 pelo ▸ e clicar em fn load  L9|referências em src/config.rs (L9) e src/main.rs (L8)" \
  "Com Auto Open Matches ligado (padrão), clicar num símbolo usado em arquivos fechados|os arquivos das referências abrem sozinhos no canvas (até Max files)" \
  "Clicar em dois símbolos seguidos; usar ‹ e ›|volta e avança entre as consultas; Clear Symbols limpa referências e símbolos, mas mantém o arquivo selecionado" \
  "Engrenagem do Symbol Graph → LSP Advanced Settings; ligar Query On Hover e parar o mouse num símbolo|as referências aparecem sem selecionar o símbolo" \
  "Keep Zoom ligado e Smooth Transition ligado/desligado; clicar numa referência com zoom baixo|o zoom não muda; com Smooth Transition a vista desliza, sem ela pula" \
  "Dim Dependency Edges e Dim %|com um símbolo ativo, as arestas que não são dele ficam mais fracas conforme o Dim %" \
  "Out of View ligado; afastar a vista dos arquivos das referências|bolinhas com a contagem aparecem na borda do canvas apontando para eles" \
  "⋮ → Language servers|diálogo com contagem por linguagem (Repository/Canvas), comando e status de cada servidor; Load project environment desabilitado com explicação" \
  "Clicar em Cargo.toml na árvore de arquivos e selecionar o nó|\"No language server for TOML.\"" \
  "Fechar o app e rodar SPATIALCODING_LSP_RUST=nao-existe scripts/manual/12-lsp.sh; selecionar util.rs na aba LSP|em vermelho: \"Could not start \`nao-existe\`: … Set SPATIALCODING_LSP_RUST to the server command.\""
launch "$dir" --layout --open src/util.rs --open src/main.rs
