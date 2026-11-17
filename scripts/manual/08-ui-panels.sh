#!/usr/bin/env bash
# Toolbar, sidebars, settings and shortcuts dialogs (TASK-8, TASK-44).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture rust)
checklist "08 · Toolbar, painéis e diálogos" \
  "Em build de desenvolvimento, passar o mouse sobre os controles|não aparece o inspetor de debug do egui com IDs, retângulos e pilha de widgets; tooltips normais continuam funcionando" \
  "Barra superior (só ícones): pílula com o nome da pasta, ponteiro (Select tool), mão (Move tool), × (Clear all nodes) e lixeira (Clear selected nodes)|a ferramenta ativa fica preenchida; a lixeira fica desabilitada sem seleção" \
  "Clicar na mão (Move tool) e tentar arrastar um nó selecionado|o nó não se move; voltar ao ponteiro reativa o arraste" \
  "Clicar na engrenagem Settings do rodapé da rail esquerda|abre o diálogo com Viewport, UI, Canvas File Nodes e Repository; Close ou Esc fecha" \
  "Settings → Font Size / Node Width|os nós mudam de tamanho na hora" \
  "Ctrl+/|abre a janela de atalhos; Esc ou Close fecha" \
  "Sidebar esquerda: abas Workspace, Changes, Search, Server|cada aba troca o conteúdo (Changes/Search/Server ainda são stubs)" \
  "Sidebar direita: clicar num ícone do trilho abre o painel; clicar de novo fecha|ordem Layout, LSP Symbols, Dependency Edges, Folders, File nodes, CST, Agent notes, Performance; título igual ao nome do painel" \
  "Em cada painel, clicar no ⋮ do cabeçalho|abre o menu do painel (ex.: Reset Layout settings); Performance não tem ⋮; clicar num item fecha o menu" \
  "Mudar File Spacing no Layout, depois ⋮ → Reset Layout settings|o valor volta ao padrão" \
  "Painel Performance: passar o mouse em Auto, Eager/On Open/Lazy, Use SQLite database e no ⓘ de Analysis|controles cinza com o motivo (análise ao abrir, em memória); On Open marcado; CST Depth igual ao do painel CST" \
  "Painel Agent notes: passar o mouse em Connect an MCP agent; abrir o ⋮|link e itens do menu desabilitados com a explicação de que não há conexão MCP" \
  "Mudar um ajuste de painel, fechar o app e abrir de novo|o ajuste continua (arquivo ~/.config/spatialcoding/settings.json; apague-o para voltar aos padrões)" \
  "Reduzir a altura da janela; abrir Layout e rolar para baixo|controles inferiores (Sort/Alt Rev) ficam acessíveis; cabeçalho e botão de fechar permanecem fixos; canvas não se move" \
  "Abrir Dependency Edges e rolar até Symbol Colors; alternar Layout/Edges|cada painel mantém sua própria posição de rolagem" \
  "No painel LSP Symbols com muitos símbolos, rolar e passar o mouse/clicar numa linha|hover e clique continuam correspondendo ao símbolo visível; controles CST também permanecem acessíveis por rolagem" \
  "Redimensionar as sidebars pela borda|largura muda dentro dos limites" \
  "Esc sem diálogos abertos|limpa a seleção"
launch "$dir" --open-all --layout
