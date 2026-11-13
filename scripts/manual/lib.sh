#!/usr/bin/env bash
# Shared helpers for the manual test scripts. Source it, don't run it.
#
# Environment:
#   SPATIALCODING_RELEASE=1   build and run the release binary
#   SPATIALCODING_FIXTURES    where fixtures go (default: target/manual-fixtures)
#   DRY_RUN=1                 generate the fixture and print the command, but
#                             don't build or open the app

set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURES_DIR="${SPATIALCODING_FIXTURES:-$REPO_ROOT/target/manual-fixtures}"

# shellcheck source=fixtures.sh
source "$REPO_ROOT/scripts/manual/fixtures.sh"

if [[ -t 1 ]]; then
  BOLD=$'\e[1m' DIM=$'\e[2m' CYAN=$'\e[36m' RESET=$'\e[0m'
else
  BOLD='' DIM='' CYAN='' RESET=''
fi

# fixture NAME — regenerates fixture NAME from scratch and prints its path.
fixture() {
  local name=$1
  if [[ ! " ${FIXTURE_NAMES[*]} " == *" $name "* ]]; then
    echo "unknown fixture '$name' (known: ${FIXTURE_NAMES[*]})" >&2
    return 1
  fi
  local dir="$FIXTURES_DIR/$name"
  rm -rf -- "$dir"
  mkdir -p -- "$dir"
  "fixture_$name" "$dir"
  printf '%s\n' "$dir"
}

# checklist TITLE "action|expected" ... — prints the manual test script.
checklist() {
  local title=$1 i=0 item
  shift
  printf '\n%s== %s ==%s\n\n' "$BOLD" "$title" "$RESET"
  for item in "$@"; do
    i=$((i + 1))
    printf '%s%2d.%s %s\n' "$CYAN" "$i" "$RESET" "${item%%|*}"
    if [[ $item == *"|"* ]]; then
      printf '    %s→ %s%s\n' "$DIM" "${item#*|}" "$RESET"
    fi
  done
  printf '\n'
}

# launch DIR [ARGS...] — builds the app and opens DIR with ARGS.
# --open paths are checked first, so a typo fails here, not silently.
launch() {
  local dir=$1 prev=''
  shift
  for arg in "$@"; do
    if [[ $prev == --open && ! -f "$dir/$arg" ]]; then
      echo "fixture has no file $arg (in $dir)" >&2
      return 1
    fi
    prev=$arg
  done

  local profile=debug cargo_args=(build --quiet)
  if [[ ${SPATIALCODING_RELEASE:-0} == 1 ]]; then
    profile=release
    cargo_args+=(--release)
  fi
  local bin="$REPO_ROOT/target/$profile/spatialcoding"

  printf '%s$ %s %s%s\n' "$DIM" "${bin#"$REPO_ROOT"/}" "$(printf '%q ' "$dir" "$@")" "$RESET"
  if [[ ${DRY_RUN:-0} == 1 ]]; then
    return 0
  fi
  (cd -- "$REPO_ROOT" && cargo "${cargo_args[@]}")
  exec "$bin" "$dir" "$@"
}
