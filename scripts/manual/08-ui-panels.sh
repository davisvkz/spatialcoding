#!/usr/bin/env bash
# Toolbar, sidebars, settings and shortcuts dialogs (TASK-8).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture rust)
checklist "08 · Toolbar, painéis e diálogos" \
  "Toolbar: botão com o nome da pasta, Select/Move, Clear All, Clear Selected, Settings|Select em negrito; Clear Selected desabilitado sem seleção" \
  "Clicar em Move e tentar arrastar um nó selecionado|o nó não se move; voltar para Select reativa o arraste" \
  "Clicar em Settings|abre o diálogo com Viewport, UI, Canvas File Nodes e Repository; Close ou Esc fecha" \
  "Settings → Font Size / Node Width|os nós mudam de tamanho na hora" \
  "Ctrl+/|abre a janela de atalhos; Esc ou Close fecha" \
  "Sidebar esquerda: abas Workspace, Changes, Search, Server|cada aba troca o conteúdo (Changes/Search/Server ainda são stubs)" \
  "Sidebar direita: ◀ abre; clicar na aba ativa recolhe|abas Layout e Edges funcionais, as demais mostram 'Coming soon'" \
  "Redimensionar as sidebars pela borda|largura muda dentro dos limites" \
  "Esc sem diálogos abertos|limpa a seleção"
launch "$dir" --open-all --layout
