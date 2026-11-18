# Atalhos e comandos

## Atalhos (painel Ctrl+/)
**Viewport**: Ctrl+Scroll/pinch zoom · Ctrl++/Ctrl+- zoom · Space+drag ou botão direito pan · F focar no mouse · Shift+F fit · trackpad: dois dedos pan.
**Canvas**: Shift+C limpar símbolos · Shift+X limpar canvas · 1 apply layout · Ctrl+Shift+O open all · R colapsar folhas · E expandir mais altas colapsadas · Shift+E expandir pasta/todas · Shift+R colapsar pasta/todas · Ctrl+R colapsar nível · Ctrl+E expandir nível.
**Seleção**: arrastar da árvore abre · Ctrl+A tudo · Backspace/Delete/Ctrl+X remove · Esc desseleciona · `\` select folder scope · Shift+`\` open folder scope.
**Diff**: Ctrl+Alt+Shift+O abrir pastas com mudanças · D / Shift+D modo · C linhas colapsadas · Shift+O abrir alterados · Alt+O fechar sem mudanças.
**Geral**: Ctrl+. sidebars · Ctrl+S esconder/restaurar painéis · Ctrl+/ atalhos · Ctrl+K paleta · Ctrl+Shift+E Workspace · Ctrl+Shift+F Search · Ctrl+Shift+S Changes · Ctrl+, Settings · Ctrl+Shift+/ Docs · Shift+V Zoom Levels · M minimap.

## IDs de comandos internos (extraídos do bundle; acessíveis por `use_command`)
- **canvas**: addNode(s), removeNode(s), openFile(s), openAll, openChanged, openChangedFolders, closeUnchanged, closeUnchangedFolders, openSelectedImporters, openSelectedImports, highlightSelectedTopLevelSymbols, selectAll/selectNode/setSelection/toggleNodeSelection, clearSelection, clearAll, deleteSelection, moveNodes, updateNodePosition, setNodePositions, undo, redo, invalidate
- **viewport**: fitNodes, moveTo, pan, zoom, zoomBy, zoomToMouse, reset
- **layout**: apply, toggleAutoLayout, updateConfig, resetConfig, setWalkthroughStep/KeepCentered/UpdateEdgesDuringMotion
- **folder**: collapse/expand, collapseAll/expandAll, collapseLevel/expandLevel, collapseRelativeLevel/expandRelativeLevel, collapseUnchanged, expandChanged, toggle, moveContents, selectNextScope, openNextScope, setOpenCollapsedByDefault, setOpenCollapseAuto, setOpenCollapseDepth, toggleDiffHighlight
- **files**: setLineLimit, expandLongFile, expandAllLongFiles
- **edges**: show/hide/toggle, setStyle, setEdgeOpacityMode, setThicknessMode, gradient (toggleGradientEnabled, setGradientGapMode, setGradientMiddleGapPercent, setGradientFixedVisiblePx, setGradientMinDistance), arrowheads (toggleArrowheads, OnlySelected, scale, MinScreenPercent), bezier controls, endpoint dots/rectangles/handles, toggleBackwardsEdges, toggleShowBarrelEdges/ReExportEdges + highlight, toggleDiffHighlight, toggleDeletedDiffEdges, toggleRelationshipKind, zoomed-out visibility (setZoomedOutVisibilityAll, ExternalNestingLevelRange, LocalFolderDepth), dim on LSP selection, render layers (mesh, pixel, sampled), resetSettings
- **cst**: toggleShowHighlights, setTypeVisibility, toggleTypeVisibility, showAllTypes, hideAllTypes, setHoveredType
- **lsp**: init, detect, status, query, configure, dispose, disposeAll, syncSnapshot, syncBase, clearSnapshot
- **diff**: openLocalSession, refreshLocalSession, refreshCanvasLocalDiffs, toggleLocalDiffs, fetchPRs(NextPage), openPR, selectFile, expandGroup, setViewMode, cycleViewMode, setContextLines, toggleCollapsed, close
- **search**: setQuery, setMode, setFlags, setActiveMatch, togglePanel, clear
- **agentNotes**: push, updateNote, open, openFiles, clear
- **ui**: toggleCommandPalette, toggleDocsWindow, openShortcutsPanel, openZoomLevelsPanel, openRailPanel, openRepoDropdown, openModal, navigateSection, toggleSidebars(WithMemory), toggleMinimap, togglePerfDetails, toggleAnalysisMemory, notify, update, updateCodeNodeText
