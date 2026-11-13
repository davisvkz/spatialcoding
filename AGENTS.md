
<!-- BACKLOG.MD GUIDELINES START -->
<!-- backlog.md-instructions-version: 1.55.3 -->
<CRITICAL_INSTRUCTION>

## Backlog.md Workflow

This project uses Backlog.md for task and project management.

**At the beginning of each conversation in this project, run `backlog instructions overview` before answering or taking action. Re-read it only if you have not read it yet in the current conversation.**

Use the overview to decide whether to search, read, create, or update Backlog tasks.

Before task lifecycle actions, read the matching detailed guide:
- `backlog instructions task-creation` before creating or splitting tasks
- `backlog instructions task-execution` before planning, changing status or assignee, adding a plan or implementation notes, or implementing task work
- `backlog instructions task-finalization` before checking acceptance criteria, writing final summaries, or moving tasks to terminal statuses

Use `backlog <command> --help` before running unfamiliar commands. Help shows options, fields, and examples.

Do not edit Backlog task, draft, document, decision, or milestone markdown files directly. Use the `backlog` CLI so metadata, relationships, and history stay consistent.

</CRITICAL_INSTRUCTION>
<!-- BACKLOG.MD GUIDELINES END -->

<!-- groma:start -->
## Groma

This project uses Groma. Before you scan, inspect, or curate architecture, or change files for a Backlog task, run `groma agent-instructions` and read the guide it names for that job. When it reports a first scan, ask the user whether they want you to curate the architecture. Do not edit Groma-owned architecture files directly.
<!-- groma:end -->

## Documentation workflow (required for every task)

All documentation lives in `docs/`, a git worktree of the `docs` branch. The
`backlog` and `.groma` symlinks at the repo root point into it, and `master`
never shows its changes, so always check `git -C docs status` too. The full
workflow is Backlog **doc-4 "Agent documentation workflow"**
(`backlog doc view doc-4`). Read it before your first task in a conversation.

A task is done only when:

1. The Backlog task records the plan, notes, every modified file
   (`--modified-file` after each change), its checked acceptance criteria and a
   final summary.
2. The groma map reflects the change: run `groma scan`; curate, describe and
   relate any new or changed component; a second scan must create nothing.
3. Every non-obvious choice is a decision (`backlog decision create`) with a
   section in doc-1 "Decision log". The CLI cannot write decision bodies.
4. Affected docs are updated: doc-2 Testing guide, doc-3 CLI and shortcuts, or
   a new guide.
5. `scripts/test.sh` passes, and `scripts/docs-check.sh` reports no gaps for the
   task.
6. The `docs` branch is committed:
   `git -C docs add -A && git -C docs commit -m "docs: TASK-N ..."`.
   Code commits on `master` happen only when the user asks for them.

Claude Code agents: run the `/document-task` skill
(`.claude/skills/document-task/`) to go through this checklist. Other agents:
follow doc-4 section 3.
