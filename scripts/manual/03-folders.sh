#!/usr/bin/env bash
# Folder container nodes (TASK-3, collapse TASK-44.5, display options TASK-44.6).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture nested)
checklist "03 · Pastas no canvas" \
  "Observar o canvas ao abrir|sem molduras nem nomes de pasta (Hide Folders ligado por padrão, como no v2)" \
  "Painel Folders → desligar Hide Folders|cada pasta com arquivos abertos ganha um quadro com o nome acima, envolvendo seus nós" \
  "Abrir um arquivo em a/b/c (pastas só com uma subpasta)|com Merge Empty Folders o nome aparece uma vez como a/b/c; desligado, cada nível tem seu quadro" \
  "Diminuir o zoom até os nomes se sobreporem; alternar Merge Overlapping Labels e Min Size|nomes sobrepostos viram 'pai / filho'; Min Size alto esconde nomes pequenos" \
  "Comparar frontend/src e backend/src|são dois quadros distintos, cada um só com os arquivos da própria pasta" \
  "Selecionar um nó e arrastá-lo para longe|o quadro da pasta cresce para continuar envolvendo o nó" \
  "Clicar no nome, margem vazia ou cartão de uma pasta; Shift+clique; Esc|a pasta fica com contorno destacado; Shift alterna a seleção e Esc limpa" \
  "Sem selecionar antes, clicar e segurar uma pasta e arrastar (expandida e recolhida, em diferentes zooms)|a pasta e todo seu conteúdo se movem juntos; arquivos de outras pastas ficam parados; ao expandir, a posição relativa do conteúdo é preservada" \
  "Arrastar um arquivo dentro de uma pasta, depois arrastar uma área vazia fora dos quadros|só o arquivo sob o cursor se move; arrastar o fundo não move a seleção antiga" \
  "Arrastar sobre uma pasta com botão direito ou Space+botão esquerdo|a câmera se move sem deslocar a pasta nem seu conteúdo" \
  "Layout (painel direito) → mudar Folder Padding|a margem entre os nós e o quadro da pasta muda" \
  "Clicar num nó e usar Clear Selected na toolbar|o nó some e o contador da pasta diminui; pasta que fica vazia desaparece" \
  "Dar duplo clique na faixa do nome de uma pasta|a pasta vira um cartão compacto 'nome · N files · collapsed'; as arestas dos arquivos escondidos chegam na lateral do cartão; duplo clique no cartão expande" \
  "Painel Folders → Collapse deepest level várias vezes, depois Expand shallowest level|recolhe do nível mais fundo para cima; expande o nível mais raso recolhido" \
  "Collapse leaf folders, Expand topmost collapsed folders, Collapse all folders, Expand all folders|cada ação recolhe/expande as pastas descritas; botões sem efeito ficam cinza com tooltip" \
  "Selecionar um arquivo; Select folder scope e Open folder scope|seleciona os arquivos abertos da pasta dele; abre todos os arquivos da pasta (o botão mostra N arquivos em M pastas)" \
  "Opening com Auto: fechar tudo e usar Open All no Workspace|pastas a partir do nível indicado ('Folders from level N open collapsed') já abrem recolhidas" \
  "Reveal Through Collapse → Selected; selecionar um arquivo e recolher a pasta dele|o arquivo continua visível embaixo do cartão da pasta, com suas arestas" \
  "Advanced → Collapse: Edges Only; recolher uma pasta|os arquivos continuam visíveis, mas as arestas deles saem da moldura da pasta" \
  "Advanced → Auto-Collapse on Zoom; afastar bastante o zoom|pastas pequenas na tela viram blocos sólidos com o nome; aproximar volta ao normal sem mudar posições" \
  "Advanced → Zoom Fog; afastar o zoom com Hide Folders desligado|pastas mais profundas esmaecem" \
  "Numa pasta com git e um arquivo editado: Diff Folder Highlight|quadros/cartões das pastas com mudanças ficam amarelos" \
  "Numa pasta com git e um arquivo editado: Collapse all folders without changes / Expand all folders with changes / Close folders without changes / Open all folders with changes|agem só nas pastas com/sem mudanças"
launch "$dir" --open-all --layout
