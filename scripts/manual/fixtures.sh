#!/usr/bin/env bash
# Fixture project generators for the manual test scripts.
# Each `fixture_<name> DIR` writes a small project into DIR (already empty).
# Sourced by lib.sh.

# put DIR REL — writes stdin to DIR/REL, creating parent folders.
put() {
  mkdir -p -- "$(dirname -- "$1/$2")"
  cat >"$1/$2"
}

# Rust crate: mod declarations, #[path], nested modules, use crate/super, pub use.
# Edges come from the language server (checked by tests/e2e/manual_fixtures.rs):
# client.rs lines 5 and 10 (1-based) → server.rs `pub struct Server` (lines
# 3-5), line 10 → `pub fn bind`, line 14 → `pub fn addr`; main.rs
# `util::shout` → helpers/strings.rs (util.rs only re-exports it).
# Symbol-picking check (TASK-38): select `bind` in server.rs or its use in
# client.rs. Only bind's links brighten; Server and addr stay dim. Keep the
# three definitions in one file so accidental whole-file focus is visible.
fixture_rust() {
  local d=$1
  put "$d" Cargo.toml <<'EOF'
[package]
name = "demo-rust"
version = "0.1.0"
edition = "2021"
EOF
  put "$d" src/main.rs <<'EOF'
mod config;
mod net;
mod util;

use crate::net::client::Client;

fn main() {
    let cfg = config::Config::load();
    let client = Client::new(&cfg);
    println!("{}", util::shout(&client.greet()));
}
EOF
  put "$d" src/config.rs <<'EOF'
use crate::util::parse_port;

pub struct Config {
    pub host: String,
    pub port: u16,
}

impl Config {
    pub fn load() -> Self {
        Self { host: "localhost".into(), port: parse_port("8080") }
    }
}
EOF
  put "$d" src/net/mod.rs <<'EOF'
pub mod client;
pub mod server;
EOF
  put "$d" src/net/client.rs <<'EOF'
use super::server::Server;
use crate::config::Config;

pub struct Client {
    server: Server,
}

impl Client {
    pub fn new(cfg: &Config) -> Self {
        Self { server: Server::bind(&cfg.host, cfg.port) }
    }

    pub fn greet(&self) -> String {
        format!("hello from {}", self.server.addr())
    }
}
EOF
  put "$d" src/net/server.rs <<'EOF'
use crate::util;

pub struct Server {
    addr: String,
}

impl Server {
    pub fn bind(host: &str, port: u16) -> Self {
        Self { addr: util::join(host, port) }
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }
}
EOF
  put "$d" src/util.rs <<'EOF'
#[path = "helpers/strings.rs"]
mod strings;

pub use strings::shout;

pub fn parse_port(s: &str) -> u16 {
    s.parse().unwrap_or(80)
}

pub fn join(host: &str, port: u16) -> String {
    format!("{host}:{port}")
}
EOF
  put "$d" src/helpers/strings.rs <<'EOF'
pub fn shout(s: &str) -> String {
    s.to_uppercase()
}
EOF
  # TASK-41: a real module longer than 50 lines to verify complete file nodes.
  local i
  for ((i = 0; i < 80; i++)); do
    printf '// File continues beyond line 50.\n' >>"$d/src/helpers/strings.rs"
  done
  printf 'pub fn after_fifty_lines() {}\n' >>"$d/src/helpers/strings.rs"
}

# TypeScript/JS: relative imports, index resolution, tsconfig paths,
# .js→.ts mapping, require and dynamic import.
# With typescript-language-server: index→components/index, index→lib/math (@lib alias),
# components/index→Button, Button→lib/math, Button→lib/format (.js→.ts),
# legacy.js→lib/math (require), lazy→lib/format (dynamic import).
fixture_ts() {
  local d=$1
  put "$d" package.json <<'EOF'
{ "name": "demo-ts", "private": true }
EOF
  put "$d" tsconfig.json <<'EOF'
{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@lib/*": ["src/lib/*"] }
  }
}
EOF
  put "$d" src/index.ts <<'EOF'
import { Button } from './components';
import { add } from '@lib/math';

export function main(): string {
  return Button({ label: String(add(1, 2)) });
}
EOF
  put "$d" src/components/index.ts <<'EOF'
export * from './Button';
EOF
  put "$d" src/components/Button.tsx <<'EOF'
import { add } from '../lib/math';
import { pad } from '../lib/format.js';

export interface ButtonProps {
  label: string;
}

export function Button(props: ButtonProps): string {
  return `<button>${pad(props.label, add(2, 3))}</button>`;
}
EOF
  put "$d" src/lib/math.ts <<'EOF'
export const add = (a: number, b: number): number => a + b;
EOF
  put "$d" src/lib/format.ts <<'EOF'
export function pad(s: string, n: number): string {
  return s.padStart(n);
}
EOF
  put "$d" src/legacy.js <<'EOF'
const { add } = require('./lib/math');
module.exports = () => add(40, 2);
EOF
  put "$d" src/lazy.ts <<'EOF'
export async function later(): Promise<string> {
  const { pad } = await import('./lib/format');
  return pad('x', 3);
}
EOF
}

# Python (src layout): absolute, relative and package imports.
# With a Python server: cart→models, cart→pricing, pricing→models, cli→cart,
# __init__→cart.
fixture_python() {
  local d=$1
  put "$d" pyproject.toml <<'EOF'
[project]
name = "shop"
version = "0.1.0"
EOF
  put "$d" src/shop/__init__.py <<'EOF'
from .cart import Cart

__all__ = ["Cart"]
EOF
  put "$d" src/shop/models.py <<'EOF'
from dataclasses import dataclass


@dataclass
class Item:
    name: str
    price: float
EOF
  put "$d" src/shop/pricing.py <<'EOF'
from shop.models import Item


def with_tax(item: Item, rate: float = 0.1) -> float:
    return item.price * (1 + rate)
EOF
  put "$d" src/shop/cart.py <<'EOF'
from .models import Item
from . import pricing


class Cart:
    def __init__(self) -> None:
        self.items: list[Item] = []

    def total(self) -> float:
        return sum(pricing.with_tax(i) for i in self.items)
EOF
  put "$d" src/shop/cli.py <<'EOF'
import shop.cart


def run() -> None:
    print(shop.cart.Cart().total())
EOF
}

# Go module: module-local package imports (stdlib imports are ignored).
# With gopls: main→pkg/api, main→internal/store, api→internal/store.
fixture_go() {
  local d=$1
  put "$d" go.mod <<'EOF'
module example.com/demo

go 1.22
EOF
  put "$d" main.go <<'EOF'
package main

import (
	"fmt"

	"example.com/demo/internal/store"
	"example.com/demo/pkg/api"
)

func main() {
	s := store.New()
	fmt.Println(api.Handle(s, "ping"))
}
EOF
  put "$d" pkg/api/api.go <<'EOF'
package api

import "example.com/demo/internal/store"

func Handle(s *store.Store, msg string) string {
	s.Put(msg)
	return "pong"
}
EOF
  put "$d" internal/store/store.go <<'EOF'
package store

type Store struct{ items []string }

func New() *Store { return &Store{} }

func (s *Store) Put(v string) { s.items = append(s.items, v) }
EOF
}

# All four languages plus things the loader must skip or survive:
# dot folders, node_modules, target, a binary and a non-UTF-8 file.
fixture_mixed() {
  local d=$1
  fixture_rust "$d/rust"
  fixture_ts "$d/web"
  fixture_python "$d/python"
  fixture_go "$d/go"
  put "$d" README.md <<'EOF'
# Mixed fixture

Four small projects (rust/, web/, python/, go/) side by side.
EOF
  put "$d" config/settings.json <<'EOF'
{ "theme": "dark", "zoom": 1.0 }
EOF
  put "$d" config/deploy.yaml <<'EOF'
replicas: 2
image: demo:latest
EOF
  put "$d" scripts/release.sh <<'EOF'
#!/bin/sh
echo "releasing"
EOF
  put "$d" .github/workflows/ci.yml <<'EOF'
on: push
jobs: { test: { runs-on: ubuntu-latest } }
EOF
  put "$d" node_modules/leftpad/index.js <<'EOF'
module.exports = (s) => s;
EOF
  put "$d" target/debug/build.log <<'EOF'
should never show up
EOF
  mkdir -p "$d/assets" "$d/docs"
  printf '\x89PNG\r\n\x1a\n\0\0\0\rIHDR' >"$d/assets/logo.png"
  printf 'caf\xe9 em latin-1\n' >"$d/docs/latin1.txt"
}

# ~150 long Rust files: m_i declares/uses m_(i/2), giving a deep tree of
# edges. For LOD, minimap and performance.
fixture_large() {
  local d=$1 i parent n=150
  put "$d" Cargo.toml <<'EOF'
[package]
name = "demo-large"
version = "0.1.0"
edition = "2021"
EOF
  {
    for ((i = 1; i < n; i++)); do printf 'mod m%03d;\n' "$i"; done
    printf '\nfn main() {\n    m001::run();\n}\n'
  } | put "$d" src/main.rs
  for ((i = 1; i < n; i++)); do
    parent=$((i / 2))
    {
      if ((parent > 0)); then printf 'use crate::m%03d;\n\n' "$parent"; fi
      printf '/// Module %d of the large fixture.\npub fn run() {\n' "$i"
      local lines=$((40 + (i * 37) % 120)) l
      for ((l = 0; l < lines; l++)); do
        printf '    let v%d = %d * %d + "%s".len() as i32; // line %d\n' "$l" "$i" "$l" "padding text" "$l"
      done
      if ((parent > 0)); then printf '    m%03d::run();\n' "$parent"; fi
      printf '}\n'
    } | put "$d" "src/m$(printf '%03d' "$i").rs"
  done
}

# Deep folders and two different folders both called src/.
fixture_nested() {
  local d=$1
  put "$d" frontend/src/app.ts <<'EOF'
import { api } from './api/client';
export const app = () => api('/health');
EOF
  put "$d" frontend/src/api/client.ts <<'EOF'
export const api = (path: string) => fetch(path);
EOF
  put "$d" backend/src/server.py <<'EOF'
from .handlers.health import health
EOF
  put "$d" backend/src/__init__.py </dev/null
  put "$d" backend/src/handlers/__init__.py </dev/null
  put "$d" backend/src/handlers/health.py <<'EOF'
def health() -> str:
    return "ok"
EOF
  put "$d" a/b/c/d/e/f/deep.md <<'EOF'
# Six levels down
EOF
}

# Names accepted by `fixture`.
FIXTURE_NAMES=(rust ts python go mixed large nested)
