# Renderização no canvas do Code Canvas v2

Como o canvas desenha arquivos, edges, LSP e CST. Análise de 2026-11-18/19 (app v2.6.1, `code-canvas-server@2.6.1`), repo de exemplo `reduxjs/redux`.

**Legenda das fontes**: **[obs]** visto ao vivo no app · **[bundle]** valor lido no JS público (identificadores não são ofuscados) · **[inf]** inferido.

| Arquivo | Conteúdo |
|---|---|
| [zoom-lod.md](zoom-lod.md) | níveis L1–L5, limiares de zoom, o que muda em cada um |
| [file-nodes.md](file-nodes.md) | como um nó de arquivo é desenhado (texto, minimap, label, semantic fill) |
| [dependency-edges.md](dependency-edges.md) | ancoragem, estilos, gradiente, cores, camadas, visibilidade |
| [lsp.md](lsp.md) | LSP ao vivo: servidor, hover/click, pin, cores, token edges, painel, configurações |
| [cst.md](cst.md) | tinta por tipo de nó sintático, editor, LOD, opções |
| [limites-e-metodo.md](limites-e-metodo.md) | método usado e o que não foi verificado |

Screenshots em `../img/` (10–19 zoom/edges/CST, 20–32 LSP e seleção).
