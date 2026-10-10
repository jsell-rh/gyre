---
title: "Repair verified failure on main a11ba8d32859"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0`
Environment fingerprint: `host-457b2c19eb82c4ef82c00c5fa8a782f7fdf9249a8ef6430730a15fd5b21b8c4d`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 514 files, 3.0GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/tools/checks.sh
rustfmt: changed lines clean (0 Rust files checked)
clippy: changed lines clean (0 Rust files, 343 existing warnings outside changes)
Architecture lint passed: gyre-domain has no forbidden dependencies or I/O.
Hierarchy lint passed: all hierarchy fields are non-optional.
OK: all registered /api/v1/ routes resolve in the ABAC registry (or are exempted legacy entries).
check-abac-exempt-handlers: OK (89 handler(s) checked)
check-mcp-write-tools: OK (8 write-capable tool(s) checked, all gated)
OK: no duplicate Diesel migration versions.
OK: no dialect-only SQL in shared migrations.
OK: every MessageKind variant has an emitter (or documented exemption).
check-byte-slice-truncation: OK
check-relative-path-defaults: OK
OK: no fail-open .unwrap_or_default()/.unwrap_or("") on resolve_ref() results.
FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:

  a11ba8d3  task-068  feat(task-068): Graph Summary & Dry-Run MCP Tools

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  a11ba8d3  task-068  feat(task-068): Graph Summary & Dry-Run MCP Tools\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-5ktqmexj/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

Reproduced and repaired the verified upstream failure: the task-068 landing
squash commit `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0` ("feat(task-068):
Graph Summary & Dry-Run MCP Tools") was absent from
`specs/tasks/task-068.md`'s `commits:` frontmatter, so
`scripts/check-task-commit-attribution.sh` failed. This is the same
structural drift class task-222 repaired for `6bf777a6`/task-200: the
squash-landing commit cannot contain its own SHA in the task file baked
inside it, so the task's recorded list stayed one entry short and the landed
surface was invisible to review scoping.

Repair (one line, `specs/tasks/task-068.md` only):
`a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0` appended to task-068's
`commits:` frontmatter as the last array element, matching the full-SHA
convention of the nine existing entries and the task-222 precedent. The
verification surface that landed in `a11ba8d3` was already independently
reviewed and approved (specs/reviews/task-068.md, round-2 verdict; the
landing commit message records the candidate `bf4b2e1a` and review link) —
this repair only restores review-scoping visibility, it changes no code.

Test evidence (2026-10-10, sandbox HEAD `a11ba8d3` + this one-line change;
evidence under /tmp/stage/review-evidence/task-226-*.txt):

- Reproduced at the exact base commit: detached worktree at
  `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0`, `bash
  scripts/check-task-commit-attribution.sh` → exit 1 with the identical
  violation `a11ba8d3 task-068` (task-226-reproduce-base-a11ba8d3.txt).
- After repair: same probe → exit 0, "OK: every task-labeled
  product-surface commit is recorded ..." (task-226-after-repair.txt).
- Mutation check (test-the-repair): removing the SHA from task-068's
  frontmatter re-fails the gate with the identical violation (exit 1,
  task-226-mutation-check.txt); restoring it re-passes (exit 0,
  task-226-mutation-restore-check.txt). The current pass is attributable
  to the recorded SHA, not gate drift.
- No gate weakened: `git diff a11ba8d3 -- scripts/` is empty (0 lines);
  `scripts/task-commit-attribution-exemptions.txt` unchanged at its frozen
  3-entry baseline (task-226-exemption-count.txt). No Rust/JS product
  surface touched: the full diff vs base is the single line in
  `specs/tasks/task-068.md` plus this task file.
- No independent gate re-run needed: `scripts/` and `crates/`/`web/` are
  byte-identical to the reviewed `a11ba8d3` tree, so all gates that passed
  at that commit (rustfmt/clippy changed-lines clean, arch, hierarchy,
  ABAC registries, migration checks, and the rest of the baseline log)
  remain green; the only failing gate is the one repaired here.

The change is documentation-of-record only; the executor checkpoints and
publishes the source when this assignment ends, and full workspace suites,
GitHub checks, and independent review are owned by verification and
publication as in the task-222 precedent.
