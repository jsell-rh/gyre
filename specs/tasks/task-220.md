---
title: "Repair verified failure on main 6bf777a6a44f"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: [task-226]
progress: ready-for-review
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `6bf777a6a44f28052ed5af28bf6fb013fde6df48`
Environment fingerprint: `host-50e1b4653106b6205d5cde2f9cfe273018060c5013040b6fbf4d4cc6d0cf9cc6`

## Baseline failure

```text
$ bash scripts/check-task-commit-attribution.sh    (at base a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0)
FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:

  a11ba8d3  task-068  feat(task-068): Graph Summary & Dry-Run MCP Tools

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to scripts/task-c....txt.
GYRE_BASELINE_FAILURE_JSON {"base": "a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  a11ba8d3  task-068  feat(task-068): Graph Summary & Dry-Run MCP Tools\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-5ktqmexj/scripts/task-c....txt.\n"}
```

## Shipped

- **Baseline failure (reproduced at the exact failing base):** detached worktree at `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0`, `bash scripts/check-task-commit-attribution.sh` → exit 1 with the identical baseline violation `a11ba8d3 task-068` (`task-220-r4-repro-base-a11ba8d3.log`). The canonical `GYRE_BASELINE_FAILURE_JSON` names this one probe; commit `a11ba8d3` ("Graph Summary & Dry-Run MCP Tools", product surface in `crates/`) is labeled task-068 but absent from `specs/tasks/task-068.md`'s `commits:` frontmatter — invisible to review scoping (task-095 R3-F4 flaw class).
- **Production fix present in this candidate (via prerequisite task-226, this round's base):** full SHA `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0` is recorded in `specs/tasks/task-068.md`'s `commits:` frontmatter — landed on main as `6a9739e5` (task-226's checkpoint), merged into this branch as `6561e329` and preserved unchanged. Gate at candidate HEAD: exit 0, "OK: every task-labeled product-surface commit is recorded …" (`task-220-r4-gate-head.log`).
- **Mutation check (pass attributable to the repair, not gate drift):** removing `a11ba8d3…` from `specs/tasks/task-068.md` re-fails the gate with exactly the baseline violation line (exit 1, `task-220-r4-mutation-remove.log`); restoring it re-passes (exit 0, `task-220-r4-mutation-restore.log`); the file is restored byte-identical (`git diff --quiet` clean).
- **Attribution:** `python3 /tmp/stage/dev-attribution.py task-220` → exit 0, no diff: this round's delta vs base `6a9739e5` is specs-only (`specs/tasks/task-220.md`), so `commits: []` remains attribution-canonical. No exemption added — `scripts/task-c....txt` does not exist in the tree (frozen baseline of 3 trivially preserved, `EXEMPT_TOTAL=0`); no check, skip, or gate was weakened; no Rust/JS source changed, so no runtime surface is affected.
- **Contract:** `requirement_parts` of this file equals the assigned body's — the only deltas are the whitelisted operational sections (`## Baseline failure`, `## Shipped`) and the `depends_on: [task-226]` frontmatter alignment seeded by the controller; the `## Required behavior` section and base/fingerprint lines are byte-identical.
- **Sandbox restriction recorded, not inferred as a code defect:** this runtime cannot accept TCP (`accept(): [Errno 95] Operation not supported`, `/tmp/stage/capabilities.json`); live-HTTP and exact-head GitHub checks belong to host verification and required GitHub CI.
- Independent review and deterministic gates decide approval; exact-head GitHub checks remain mandatory before merge.
