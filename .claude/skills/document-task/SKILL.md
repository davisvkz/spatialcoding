---
name: document-task
description: Close out a Backlog task by documenting it in docs/ with the backlog and groma CLIs (architecture map, decisions, docs, task summary) and committing the docs branch. Use at the end of every task in this repo, when the user says "documente", "document this", "close the task", or before reporting work as done.
---

# Document a task

This repo keeps all documentation on the `docs` branch, checked out at `docs/`
(see Backlog doc-4, "Agent documentation workflow"). This skill runs that
workflow's closing checklist. Arguments, if given, name the task (`TASK-N`).
Otherwise use the task you worked on in this conversation, or find it with
`backlog task list --status "In Progress" --plain`.

## Steps

1. **Read the rules.** Run `backlog doc view doc-4` and
   `backlog instructions task-finalization`, and read
   `backlog task view TASK-N --plain`.

2. **Find the gaps.** Run `scripts/docs-check.sh`. It runs `groma scan` and
   prints a `TODO` for every gap. Keep its output: you will clear each item.

3. **Verify the code.** Run `scripts/test.sh`, then
   `cargo clippy --all-targets 2>&1 | grep -A4 '^warning'`. Don't continue on
   failures. Fix them, or report them to the user.

4. **Architecture (groma).** Read `groma agent-instructions` and the
   `structure`, `describe` and `relationships` guides before changing the map.
   For every source file the task added or changed, run `groma view <file>`:
   - Curate new file-shaped components: combine them into their owner, or give
     them an ID, a group, a title, a description and an overview.
   - Add `groma add relation` rows for new collaborations, and fix outdated
     descriptions with `groma edit relation`.
   - Update flows if a user-visible scenario changed.
   - Run `groma scan` twice; the second run must create nothing.
   - Record every path printed by structural commands in the task's
     `--modified-file` list. `--modified-file` replaces the list, so repeat
     earlier paths.

5. **Decisions.** For each non-obvious choice the task made, run
   `backlog decision create "<title>" -s accepted --plain`. Then add its
   `## decision-N: <title>` section (Status, Context, Decision, Consequences)
   to doc-1:

   ```sh
   backlog doc view doc-1 > /tmp/log.md   # strip the frontmatter, append the section
   backlog doc update doc-1 --content "$(cat /tmp/log.md)"
   ```

   Keep every earlier section.

6. **Docs.** Update the docs whose subject changed: doc-2 for testing, doc-3
   for the CLI and shortcuts, others as listed by `backlog doc list --plain`.
   Create a guide for a new area. If visible behavior changed, update the
   matching `scripts/manual/NN-*.sh` checklist.

7. **Finalize the task.** Check only the acceptance criteria you verified.
   Write `--final-summary` with what changed, why, how it was verified, the
   commits and any follow-ups. Set the status to `Done`.

8. **Commit.**
   - Commit code on `master` only if the user asked for commits, with a
     conventional message ending in `Refs: TASK-N`.
   - Always commit the documentation:
     ```sh
     git -C docs add -A
     git -C docs commit -m "docs: TASK-N <summary>" -m "<what was recorded>. Code: <master hashes>."
     ```

9. **Confirm.** Run `scripts/docs-check.sh --no-scan`. Every item should be
   `ok`, except tasks that are still legitimately in progress. Report to the
   user what was documented: the task, decisions, docs, map changes and docs
   commit hash.

Never edit files under `docs/backlog` or `docs/groma` by hand; the only
exception is `docs/groma/config.json`.
