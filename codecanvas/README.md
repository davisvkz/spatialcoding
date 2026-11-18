# Code Canvas v2 (https://v2.codecanvas.app/): análise e documentação

Análise feita em 2026-11-18 com `agent-browser` (app v2.6.1, servidor local `code-canvas-server@2.6.1`).
Fontes: docs embutidos no app (Ctrl+Shift+/), todos os painéis e menus, painel de atalhos, paleta de comandos,
tráfego de rede, bundle JS público e pacote npm do servidor local.

## O que é
Um **canvas infinito** para explorar e revisar código. Cada arquivo vira um nó (com o código real, colorido) posicionado
dentro da sua pasta real, e o layout também leva em conta quem importa quem. O resultado é um mapa da arquitetura:
arquivos organizados da esquerda para a direita por dependência, agrupados por pasta, ligados por arestas (edges).

Usos declarados: onboarding em codebases novas; revisar mudanças de IA (diff) vendo onde elas ficam na arquitetura.

## Formas de uso
| Forma | Notas |
|---|---|
| **Browser** (v2.codecanvas.app) | Repos públicos do GitHub funcionam sem nada local. Pasta local, language servers, diff local, MCP e file watching exigem `npx code-canvas-server`. |
| **Extensão VSCode** (`alex-c.code-canvas-app`, Open VSX para Cursor/Windsurf) | Servidor embutido no backend da extensão. Espelha o editor ativo (cursor, seleção) no canvas. Alt+Click abre o arquivo ao lado. |
| **Desktop macOS** (Apple Silicon) | Servidor local embutido; pastas locais e file watching exigem Pro. |

Atalho de URL: trocar `github.com` por `v2.codecanvas.app` em qualquer URL de repo (`v2.codecanvas.app/facebook/react`).
Planos: repo público sem login; repo privado e pastas locais exigem login + **Pro**.

## Linguagens
- **Best**: JavaScript, TypeScript, React, Next.js, Vue, Svelte, Python, Rust, Swift (mais edges, análise detalhada, mais arquivos de config).
- **Good** (highlight, syntax tree, edges por padrão): PHP, Ruby, Java, Kotlin, Go, C, C++, Objective-C, C#. LSP só onde há servidor integrado (Kotlin ainda não).

## Índice
- [arquitetura.md](arquitetura.md): stack, pipeline, workers, servidor local
- [ui-e-paineis.md](ui-e-paineis.md): tela e cada painel com seus controles
- [funcionalidades.md](funcionalidades.md): como cada recurso funciona
- [atalhos-e-comandos.md](atalhos-e-comandos.md): atalhos e IDs de comandos internos
- [mcp-e-servidor-local.md](mcp-e-servidor-local.md): MCP, Agent Notes, servidor local
- `img/`: screenshots (home, repo redux aberto, atalhos, paleta, painéis)

## Limites desta análise
Não foram exercitados: login/Pro, pasta local (precisa do servidor), PRs/commits (login para privados), MCP conectado,
LSP ao vivo. O que está descrito desses itens vem dos docs oficiais, do bundle e do pacote npm, não de uso direto.
