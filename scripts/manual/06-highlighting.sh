#!/usr/bin/env bash
# Syntax highlighting with syntect (TASK-6).
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

dir=$(fixture mixed)
checklist "06 · Syntax highlighting" \
  "Dar zoom (Ctrl+roda) até ler o código de cada nó|cada linguagem com cores próprias (tema base16-ocean.dark)" \
  "Rust, TypeScript (.ts/.tsx), JavaScript, Python, Go|palavras-chave, strings e comentários com cores diferentes" \
  "JSON, YAML, TOML, Markdown, shell|também coloridos" \
  "Comparar o texto com o arquivo original|nenhum caractere perdido; tabs e indentação preservados"
launch "$dir" --layout \
  --open rust/src/main.rs --open web/src/components/Button.tsx --open web/src/legacy.js \
  --open python/src/shop/cart.py --open go/main.go --open config/settings.json \
  --open config/deploy.yaml --open rust/Cargo.toml --open README.md --open scripts/release.sh
