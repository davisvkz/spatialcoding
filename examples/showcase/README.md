# Showcase workspace

One small "shop" implemented in four languages, written to exercise every
feature of SpatialCoding. See [the architecture notes](docs/architecture.md),
the web models in [Product](web/src/models/Product.ts), the Rust crate in
[main.rs](server/src/main.rs), the Python helpers in
[pricing.py](tools/shop/pricing.py) and the Go service in
[main.go](service/main.go).

| Folder | Language | What it shows |
|---|---|---|
| `web/` | TypeScript, TSX, JS | imports, barrels, re-exports, inheritance, JSX rendering, dynamic import, `require` |
| `server/` | Rust | `mod`, `use`, supertraits, `impl Trait for`, nested modules, `include_str!` |
| `tools/` | Python | relative imports, `__all__` re-exports, subclassing |
| `service/` | Go | module imports, interfaces, structs |
| `config/` | JSON | `$ref` and `extends` links between documents |
| `docs/` | Markdown | links to code and between documents |

Open everything with `Ctrl+Shift+O`, then press `1` to lay it out.
