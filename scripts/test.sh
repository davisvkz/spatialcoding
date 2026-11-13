#!/usr/bin/env bash
# Runs the automated tests.
#   scripts/test.sh          unit + e2e
#   scripts/test.sh unit     unit tests only (cargo test --lib)
#   scripts/test.sh e2e      end-to-end tests only (cargo test --test e2e)
# Extra arguments go to the test binary, e.g. `scripts/test.sh e2e canvas::`.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

suite=${1:-all}
[[ $# -gt 0 ]] && shift

case $suite in
  unit) cargo test --lib -- "$@" ;;
  e2e) cargo test --test e2e -- "$@" ;;
  all) cargo test -- "$@" ;;
  *) echo "usage: $0 [unit|e2e|all] [test filter...]" >&2; exit 2 ;;
esac
