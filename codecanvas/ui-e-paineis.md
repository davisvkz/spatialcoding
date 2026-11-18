# Interface e painéis

## Tela
- **Topo-esquerda**: seletor de repositório (dropdown; mostra "No folder" ou `owner/repo`), ferramenta **Select** e **Move** (mão), *Clear all nodes*, *Clear selected nodes*.
- **Rail esquerdo** (ícones): Workspace (Ctrl+Shift+E), Changes (Ctrl+Shift+S), Search (Ctrl+Shift+F), Local Server (ponto laranja = servidor não conectado). Embaixo: Send feedback (ícone de bug), Help, Settings (Ctrl+,), Sign in.
- **Rail direito** (ícones): Layout, LSP Symbols, Dependency Edges, Folders, File nodes, CST, Agent notes, Performance.
- Painéis abrem **sobrepostos** ao canvas; Ctrl+. alterna sidebars, Ctrl+S esconde/restaura lembrando quais estavam abertos.
- **Minimap** no canto inferior direito (M): retângulo branco = viewport, redimensionável.
- **Tela vazia**: cartão *Local Server* (`npx code-canvas-server`), cartão *Repository* (Connect repository, "See some example repositories": facebook/create-react-app, myxvisual/react-uwp, reduxjs/redux, honojs/hono), botões Docs / Shortcuts / Command Palette e vídeo de quick start.
- Quase todo painel tem menu ⋮ (Reset settings do painel) e botão de vídeo demo.

## Left rail
**Workspace**: árvore do repo com contagem de arquivos por pasta, **Open All N** (Ctrl+Shift+O), Expand all folders, menu ⋮ (Clear repository, Show ignored files, Ignored file settings, Show folder file counts). Dica "Drag a folder onto the canvas". Menu de contexto de pasta/arquivo: *Open on canvas*, *Add to ignore list…* (e Remove from canvas, Open in… editor).
**Changes**: fonte (Local changes / Pull Requests / commits), modos Unified | Split (+ Changes), engrenagem de diff settings, lista de PRs ("No pull request is selected").
**Search**: modos Code | Files & folders (Tab alterna), filtros `.*` regex, `Aa`, Word, Ignored, Canvas; resultados On Canvas / Off Canvas.
**Local Server**: status ("Local server not running"), comando, seção **MCP** com config JSON copiável e agentes conectados.
**Settings**: Viewport (Smooth Zoom, Viewport Details, Activity Indicator), UI (Controls Bar, Minimap + reset), Canvas File Nodes (Font Size 8–20, default 12; Node Width 300–2100, default 700), Performance (Diff Scan Limits), Repository (Show dot-prefixed folders), VS Code/Cursor, Reset (all / grupos), versão + Check for updates.

## Right rail
**Layout**: Apply Layout (tecla 1), auto layout, alinhamento horizontal (Left/Center/Right) e vertical (Top/Bottom/Fill), sort (Off/Desc/Asc/Alt/Alt Rev), espaçamento de arquivos e pastas, folder padding, slider **Break** (rank breaking), Reset Config.
**LSP Symbols**: Clear Symbols (Shift+C), Open Server Panel (lista de language servers, Command override, Copy details), advanced settings (Query On Hover, Auto Open Matches + Max files, Keep Zoom, Smooth Transition, out-of-view labels, Mirror IDE editor).
**Dependency Edges**: Visibility (All; filtros Selected, Changes, Backwards, Internal, External + slider de nesting), Highlight Backwards Edges, Style (Bezier, Step, Line, PCB), Opacity (Full/Depth), Connection Markers, Arrow Heads (Only selected, Scale, Min Size), Gradient (switch, % ou Fixed, Gap %, Min Dist), Diff Colors (Deleted), Barrel and Re-export (Show/Highlight re-export edges, Show/Highlight barrel edges).
**Folders**: botões Collapse/Expand (deepest, shallowest, leaf, topmost, all), os equivalentes "sem mudanças"/"com mudanças" em diff, Select/Open folder scope, **Open with collapsed folders by default** (Auto ou profundidade manual; "Repository depth: 7 levels"), Hide Folders, Merge Empty Folders, Merge Overlapping Labels, Folder Names (zoom mínimo, 60%), Diff Folder Highlight (Selected, Diff Files, Diff Folders, Backwards Edges), Advanced.
**File nodes**: Open all files, Open all changed files, Close files without changes, **Open files that import the selection**, **Open imported files**, Fit to nodes, Select all, Unselect all, Remove selected, **Highlight top-level symbols** (+ Skip imports), Long files (Collapse long files, Max lines, Expand all N), File Names (zoom mínimo).
**CST**: switch geral, "Selected Edge CST Nodes", Symbol Mapping, uma linha por tipo de nó (arrow function, call signature, class declaration, export/import statement, for/while/do, function declaration/expression/signature, interface, JSX elements, lexical declaration, method, property signature, return/switch/throw/try, type alias...) com contagem visível/total, editor de cor, fill (solid/gradient), opacidade, borda e visibilidade por tipo.
**Agent notes**: lista de notas empurradas por agentes MCP; botão "Connect an MCP agent".
**Performance**: Analysis Auto/Eager/On Open/Lazy, Use SQLite database, Limit CST depth + Max depth, "About analysis storage".

## Outros
- Paleta (Ctrl+K): lista única de ações; hover mostra **Go to UI**, que destaca onde o controle vive sem executá-lo.
- Shortcuts (Ctrl+/) e Zoom Levels (Shift+V; "Reset zoom levels to defaults").
- Feedback: bug icon; log opcional (prévia de um snapshot fixo, redação opcional, credenciais sempre redigidas; sem código-fonte).
