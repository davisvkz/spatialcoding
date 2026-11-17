#!/usr/bin/env bash
# Workspace de exemplo com tudo junto (TASK-46): a loja "shop" em TypeScript/TSX,
# Rust, Python e Go, mais Markdown e JSON, como repositório Git com mudanças.
# Os arquivos-fonte estão em examples/showcase (a fixture só adiciona o que não
# deve ser versionado: node_modules, target, um arquivo latin-1, um arquivo
# longo gerado e o histórico/mudanças do Git).
# Relações sem servidor de linguagem: Imports e links de documentos. Inherits,
# Renders e References entre símbolos precisam dos servidores (veja 12-lsp.sh).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture showcase)
checklist "13 · Workspace de exemplo (showcase)" \
  "Painel Workspace → Open All, ou Ctrl+Shift+O|abre ~60 arquivos; node_modules/ e server/target/ não aparecem; .github só com Settings → dot folders" \
  "Apertar 1 (Apply Layout) e Shift+F|tudo abre expandido (Folders → Open with collapsed folders by default vem desligado); web/, server/, tools/, service/, config/, docs/ viram caixas aninhadas; em server/src o main.rs fica à esquerda e domain/, net/, config.rs e util.rs ficam empilhados (cerca de 1,2:1, não uma fila de 4 colunas); importadores à esquerda de quem usam nos grupos só de arquivos"
  "Folders → Collapse all folders, depois Fit to nodes|os cartões mostram o nome da pasta e 'N files · collapsed' legíveis, sem ícone quebrado; ligar Open with collapsed folders by default e reabrir colapsa a partir do nível Auto"
  "Dependency Edges → Relationship types (Imports, Inherits, References, Renders)|Imports: web/src/index.ts→App.tsx, models/index.ts→Entity.ts, mod do Rust, from . do Python, imports Go; References: docs e config; alternar cada tipo oculta só aquele tipo" \
  "Com typescript-language-server: Product.ts (extends Entity implements Describable), Order.ts, Repository.ts (implements)|arestas Inherits para Entity.ts, Describable e Repository" \
  "Com typescript-language-server: ProductList.tsx, App.tsx|arestas Renders para <Card>, <Button> e <UI.ProductList> (JSX); o valor de props continua References" \
  "Com rust-analyzer: product.rs e order.rs (impl Entity for / impl Priced for), entity.rs (trait Priced: Entity)|arestas Inherits; usos de Order, Product e cents() em client.rs e main.rs ligam à definição (References)" \
  "Com pyright: models.py (Item(Base), Bundle(Item)), plugins/discount.py (Discount(Rule))|Inherits; tools/shop/__init__.py com __all__ é re-export (Barrel and Re-export)" \
  "Com gopls: service/main.go, internal/store/store.go e internal/pricing/pricing.go|Imports locais (module example.com/shop, go.work); usos de store.Item e store.New viram References" \
  "Barrel and Re-export: web/src/models/index.ts e web/src/ui/index.ts (4+ re-exports)|Show barrel edges (padrão ligado) mostra; Show re-export edges (padrão desligado) mostra os export … from e pub use; Highlight pinta" \
  "Clicar no nome Money em Order.ts (símbolo), Shift+clique em Product|só as arestas dessa definição acendem; Esc limpa; o resto fica apagado" \
  "web/src/utils/emoji.ts, referências depois dos 🎉|o destaque do nome continua em cima do nome (colunas UTF-16 corretas)" \
  "README.md e docs/architecture.md: links para código e entre os dois; config/schema.json: \$ref e extends|arestas References de documentos; o link dentro do bloco de código e o \$ref https:// não geram aresta" \
  "web/src/data/catalog.ts (1200 linhas), File nodes → Collapse long files (Max lines 1000)|o nó vira só a moldura com '1205 lines · collapsed'; desligar mostra o código inteiro" \
  "File nodes → Open all changed files / Close files without changes|abre Product.ts, price.rs, pricing.py e Cache.ts (git status); report.ts foi apagado e não abre" \
  "Dependency Edges → Diff: baseline HEAD → Refresh comparison|contagens de imports/links adicionados e removidos: Cache.ts (novo) e index.ts→lazy/report.ts (removido, só no baseline); Changes/Diff Colors/Deleted realçam" \
  "Folders → Collapse all folders / Expand all folders; Collapse all folders without changes|cada pasta vira um cartão compacto que representa seus arquivos e as arestas vão até ela; sem mudanças colapsa tudo menos web/, server/ e tools/ (arquivos alterados)" \
  "Painel CST com web/src/ui/App.tsx e server/src/domain/product.rs|árvore tree-sitter (TSX com jsx_element, Rust com impl_item); passar o mouse destaca as linhas no nó" \
  "Painel LSP Symbols com entity.rs ou Entity.ts selecionado|lista de símbolos (interface, class, trait); clicar numa referência salta para a linha em outro arquivo" \
  "Layout: Rank Breaking, H/V Align, Sort, File/Folder Spacing; Reset Config|o layout muda ao aplicar e volta ao padrão no reset" \
  "Abrir docs/latin1.txt|aparece 'Café au lait…' sem quebrar (arquivo em latin-1)" \
  "Ctrl+K, Ctrl+/ e o minimap no canto|paleta de comandos (veja 14), atalhos e o retângulo do viewport acompanha o pan e o zoom"
launch "$dir" --open-all --layout
