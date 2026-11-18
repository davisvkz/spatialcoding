# Funcionalidades

## Conectar e abrir arquivos
- GitHub: `owner/repo` (público sem login). Local: aba Local (precisa do servidor, Pro). Uma pasta pode ter vários repos Git; cada um é detectado (ícone de branch na árvore).
- Abrir no canvas: arrastar arquivo/pasta da árvore, *Open on canvas*, *Open All*, Search, Changes, extensão VSCode (*Open in Canvas*).
- Qualquer arquivo abre como texto (sem extensão também). Formatos desconhecidos mostram até 256 KiB; binários mostram texto decodificável; prévias truncadas são read-only.
- **Open with collapsed folders**: abre grupos grandes já colapsados (profundidade Auto ou manual). **Namespace/package view**: Folders, Namespaces, Both, None.
- **Long files**: primeiras 1000 linhas + linha "Long file"; símbolos recortados; imports/exports/edges da cauda ligam ao marcador. Configurável (Max lines, Expand all).
- Fechar: Backspace/Delete/Ctrl+X, *Remove from canvas*, Clear Canvas (Shift+X). Abrir no editor: *Open in…* na linha clicada (repos locais).
- **Ignored paths**: ignorar arquivo de configuração só o esconde; package.json, tsconfig.json, Cargo.toml, go.mod, pyproject.toml, .csproj, CMakeLists.txt, Package.swift continuam informando a análise. Ignorar uma pasta a tira da análise (exceções: compile_commands.json, pastas só de config, .props/.targets/global.json de C#).

## Navegação
Pan: Space+arrastar ou botão direito; zoom Ctrl+scroll / Ctrl+± / pinch; F zoom no mouse (60%); Shift+F ajusta seleção ou tudo; zoom máx 100%. Minimap (M). Pastas: E/R (de cima/de baixo para a árvore), Ctrl+E/Ctrl+R (nível absoluto da pasta sob o mouse), Shift+E/Shift+R (todas). Folder scope: `\` seleciona arquivos abertos da mesma pasta (repetir sobe um nível), Shift+`\` abre os ainda fechados.

## Dependency Edges
Aresta = do **export** ao **import** que o resolve. Arquivos esq→dir por dependência. Filtros (quando All está off): Selected, Changes, Backwards, Internal, External (com faixa de profundidade). Em bases enormes, auto-trim avisa. **Barrels**: modo *Re-export edges* (atalho direto do importador à origem) e *Barrel edges* (dois saltos via linha de re-export); Rust `pub use` (nomeado, renomeado, wildcard). **Resolução**: tsconfig paths, aliases Vite, `index.*`, `@/`→`src/`, npm = external; cada projeto usa seu tsconfig/Cargo.toml/go.mod/pyproject mais próximo.

## Layout
Algoritmo hierárquico: começa nas pastas-folha, ordena arquivos por dependência, depois trata pastas filhas ordenadas como nós únicos no nível acima, até a raiz. **Backwards edges** (da esquerda para a direita, em vermelho) sinalizam ciclos ou violação de camadas (podem ser entre módulos/pastas). **Break** quebra ranks para mudar a proporção sem perder relações (nós são altos e estreitos). Reexecuta ao mudar fonte/largura.

## LSP
Hover/click num símbolo destaca: referências (âmbar), definições (rosa), type definitions (rosa com contorno), implementações (verde), incoming calls (roxo claro), outgoing calls (laranja) em todos os arquivos abertos. Click fixa o grafo; Shift+Click fixa vários; Shift+C limpa. Ambiente do projeto (login shell + direnv); um servidor por repositório em pastas multi-repo; `.codecanvas/settings.json` pode sugerir `languageServers.<lang>.command` (pede confirmação, por comando e pasta). Falhas: linha do servidor mostra comando, executável, PATH, stderr e **Copy details**. Ações por seleção: Open Importers (via referências do LS), Open Imports (via grafo), Highlight Top-Level Symbols.

## CST
Árvore sintática concreta tintada por papel (função, classe, condicional, loop) para ler estrutura mesmo com zoom afastado ("minimap embutido"). Detalhe carrega com o zoom e é cacheado conforme memória. **Symbol Mapping**: para regiões grandes (corpo de função), escolhe um token significativo interno para que hover/click em qualquer ponto ative as conexões.

## Diff
Fontes: **Local changes** (tempo real; todos os repos da pasta), **Pull requests** (GitHub), **Commits** (+ **Commit range**: net changes entre dois commits, histórico first-parent; *Back to working tree*). Modos: Changes (arquivo atual com marcas, linhas removidas sinalizadas), Unified, Split (D / Shift+D). LSP funciona nos lados antigo e novo. Ações: Shift+O abrir alterados, Alt+O fechar sem mudanças, fechar pastas sem mudanças, abrir todas as pastas com mudanças (Ctrl+Alt+Shift+O). Settings: Context, Character diff (Word/Char/Off), Show deleted chars, Similarity threshold, C colapsa linhas. Diffs grandes: card "Large Diffs".

## Search
Code (conteúdo) e Files & folders, filtros regex/case/word/ignored/canvas, resultados On/Off Canvas, setas navegam, Enter abre; Keep Zoom e Smooth Transition.

## Agent Notes
Cartões markdown criados por agentes via MCP (`agentNotes:push`), com links `cc://file/<path>`, `cc://folder/<dir>`, `cc://symbol/<path>:LINE:CHAR`, `cc://range/<path>:SL:SC-EL:EC`. Click navega e destaca; hover só pré-visualiza.

## Settings / Feedback
Ver [ui-e-paineis.md](ui-e-paineis.md). Configurações de tamanho/limites ficam salvas no dispositivo.
