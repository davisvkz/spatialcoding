#!/usr/bin/env bash
# Git (branch, Changes), diff e file watcher (TASK-14, TASK-18, TASK-13).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture showcase)
checklist "15 · Git, diff e file watcher" \
  "Barra superior|um pill com o ícone de branch e o nome (master ou main); clicar abre o painel Changes" \
  "Painel Changes|'On branch …' e grupos Modified (3: Product.ts, price.rs, pricing.py), Added/Untracked (Cache.ts) e Deleted (report.ts), cada um com letra colorida" \
  "Clicar em Product.ts no Changes|abre a janela 'Diff — web/src/models/Product.ts' com '+1' em verde, o hunk '@@' e a linha nova com fundo verde" \
  "Na janela de diff, Split|duas colunas: antes à esquerda, depois à direita, com a linha nova só à direita; Unified volta à lista única" \
  "Clicar em report.ts (Deleted)|'Deleted file' com todas as linhas em vermelho; Cache.ts mostra 'New file' todo em verde" \
  "Redimensionar e fechar a janela de diff|a janela é flutuante e o canvas continua usável; o × fecha" \
  "Ctrl+Shift+O, depois editar web/src/models/Product.ts em outro editor e salvar|o nó atualiza sozinho em até um segundo, sem perder a posição; o painel Changes e o diff aberto seguem a edição" \
  "Salvar por 'escrever arquivo temporário e renomear' (a maioria dos editores)|o nó também recarrega" \
  "Criar server/src/novo.rs no disco|o arquivo aparece na árvore Workspace sem reabrir a pasta, e na lista Untracked; as pastas abertas continuam abertas" \
  "Apagar web/src/utils/emoji.ts (com o nó aberto) e depois a pasta docs/|o nó some do canvas e da árvore; arquivos de docs/ abertos também fecham" \
  "Criar ou alterar arquivos em node_modules/ ou server/target/|nada muda no app (pastas ignoradas)" \
  "Rodar o app com uma pasta que não é repositório Git|sem pill de branch; o painel Changes diz 'Git is unavailable here'"
launch "$dir"
