#!/usr/bin/env bash
# Reports what is still undocumented before a task is closed.
# Read-only, except that it runs `groma scan`, which refreshes the stored
# architecture with the current code (that is part of documenting).
#
#   scripts/docs-check.sh          run all checks
#   scripts/docs-check.sh --no-scan  skip `groma scan`
#
# Exits 1 if any check needs attention, so it can gate a task.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

scan=1
[[ ${1:-} == --no-scan ]] && scan=0

issues=0
ok() { printf '  ok    %s\n' "$*"; }
todo() { printf '  TODO  %s\n' "$*"; issues=$((issues + 1)); }
section() { printf '\n%s\n' "$*"; }

section "docs/ worktree"
if [[ -e docs/.git ]] && git -C docs rev-parse --abbrev-ref HEAD >/dev/null 2>&1; then
  branch=$(git -C docs rev-parse --abbrev-ref HEAD)
  if [[ $branch == docs ]]; then ok "docs/ is the docs branch"; else todo "docs/ is on branch '$branch', expected 'docs'"; fi
else
  todo "docs/ worktree missing: git worktree add docs docs"
  exit 1
fi
for link in backlog .groma; do
  if [[ -L $link && -d $link ]]; then ok "$link symlink resolves"; else todo "$link symlink is missing or broken"; fi
done

section "Architecture (groma)"
if ((scan)); then
  summary=$(groma scan 2>&1 | grep -E '^created [0-9]+' || true)
  created=$(sed -E 's/^created ([0-9]+).*/\1/' <<<"$summary")
  if [[ ${created:-0} -gt 0 ]]; then
    todo "groma scan created $created component(s) for new files: curate them (groma agent-instructions structure)"
  else
    ok "groma scan: ${summary:-no summary}"
  fi
fi
undescribed=()
while IFS= read -r f; do
  grep -q '^description:' "$f" || undescribed+=("$(basename "$f" .md)")
done < <(find .groma/systems -path '*/components/*.md' -type f | sort)
if ((${#undescribed[@]})); then
  todo "components without a description: ${undescribed[*]}"
else
  ok "every component has a description"
fi

section "Decisions"
log=$(backlog doc list --plain 2>/dev/null | grep -E '^doc-[0-9]+ - Decision log$' | grep -oE '^doc-[0-9]+' || true)
if [[ -z $log ]]; then
  todo "no 'Decision log' doc"
else
  log_text=$(backlog doc view "$log" 2>/dev/null)
  missing=()
  while IFS= read -r id; do
    grep -q "^## $id:" <<<"$log_text" || missing+=("$id")
  done < <(backlog decision list --plain | grep -oE '^decision-[0-9]+')
  if ((${#missing[@]})); then
    todo "decisions without a section in $log: ${missing[*]}"
  else
    ok "every decision has a section in $log"
  fi
fi

section "Tasks"
active=$(backlog task list --status "In Progress" --plain 2>/dev/null | grep -E '^\s+TASK-' || true)
if [[ -n $active ]]; then
  while IFS= read -r line; do todo "still In Progress:${line}"; done <<<"$active"
else
  ok "no task left In Progress"
fi

section "Uncommitted documentation (docs branch)"
pending=$(git -C docs status --porcelain | wc -l)
if ((pending)); then
  todo "$pending uncommitted change(s) in docs/: commit them on the docs branch"
  git -C docs status --short | head -15 | sed 's/^/        /'
else
  ok "docs/ is clean"
fi

printf '\n'
if ((issues)); then
  printf '%d item(s) need attention.\n' "$issues"
  exit 1
fi
printf 'Documentation is complete.\n'
