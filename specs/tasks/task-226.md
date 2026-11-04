---
title: "Repair verified failure on main a11ba8d32859"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: [task-229, task-240]
progress: complete
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

This task repairs the verified upstream failure class "task-labeled
product-surface commit absent from its task's `commits:` frontmatter"
(squash-landing drift, task-095 R3-F4): a squash-landing commit cannot
contain its own SHA in the task file baked inside it, so the landed surface
is invisible to review scoping and
`scripts/check-task-commit-attribution.sh` fails.

### Round 1 (base a11ba8d3): task-068 drift repaired

Reproduced at the exact base commit `a11ba8d32859…` (detached worktree,
gate exit 1, violation `a11ba8d3 task-068`), then appended the full SHA to
`specs/tasks/task-068.md`'s `commits:` frontmatter — one line, no code.
The identical repair landed on main as task-224 (`770785f7`), so the
branch-side and main-side task-068 repairs are byte-identical and merged
cleanly.

### This round (assignment base 58deeb75, prerequisites task-229 + task-240)

The new baseline failure for this assignment was the same drift class one
link further down the chain: `b1cdb0dd task-170` ("feat(task-170): View
Specification Grammar — TypeScript types and server-side validation")
missing from `specs/tasks/task-170.md`. It was repaired on main by
prerequisite task-240 (landing `58deeb75`: full SHA
`b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` appended to task-170's
`commits:` frontmatter alongside its 11 branch SHAs). The executor merged
base `58deeb75` into this branch (merge `b2571778`, no conflicts; no
active merge or rebase remained).

This round changed no product source: the merged tree vs base `58deeb75`
is exactly `specs/tasks/task-226.md` (this file: `depends_on` updated to
`[task-229, task-240]` per the assignment contract, plus this section).
`git diff 58deeb75 HEAD -- scripts/ crates/ web/` is empty;
`scripts/task-c....txt` is unchanged at its frozen 3-entry baseline
(`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`); no
gate, skip, exemption, or check was weakened, and no Rust/JS surface was
touched. Since `scripts/`, `crates/`, and `web/` are byte-identical to
the reviewed `58deeb75` tree, every gate that passed there remains green;
the only gate this task's chain repairs is the attribution gate itself.

### Fresh verification on the merged tree (HEAD b2571778)

All evidence under `/tmp/stage/review-evidence/task-226-r3-*.txt`:

- Reproduction at the exact failing base: detached worktree at
  `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3`, `bash
  scripts/check-task-commit-attribution.sh` → exit 1 with the identical
  baseline violation `b1cdb0dd task-170`.
- `bash scripts/check-task-commit-attribution.sh` at merged HEAD → exit
  0, "OK: every task-labeled product-surface commit is recorded …".
- Mutation checks (test-the-repair, one per recorded landing SHA on this
  chain): removing `b1cdb0dd…` from `specs/tasks/task-170.md` (task-240's
  repair), `a11ba8d3…` from `specs/tasks/task-068.md` (this task's
  round-1 repair), or `05709c24…` from `specs/tasks/task-196.md`
  (task-229's repair) each re-fails the gate with exactly that commit's
  own violation line (exit 1); restoring each re-passes (exit 0). The
  pass at HEAD is attributable to all three recorded SHAs, not gate
  drift.
- `python3 /tmp/stage/dev-attribution.py task-226` → exit 0, no diff:
  this round's delta vs base `58deeb75` is specs-only
  (`specs/tasks/task-226.md`), so `commits: []` remains
  attribution-canonical.

The TCP `accept()` listener probe remains unsupported in this sandbox
(`/tmp/stage/capabilities.json`); no runtime surface was touched, so no
HTTP probe applies. Independent review, full deterministic gates, and
GitHub checks on the exact PR head remain required before merge; the
executor checkpoints and publishes the source when this assignment ends.
