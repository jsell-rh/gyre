---
title: "Repair verified failure on main a11ba8d32859"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: [task-229]
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

### Merge round (assignment base 34248324, prerequisite task-229)

**Context:** while this task's round-1 repair sat at candidate `607b8b2f`
(repaired `a11ba8d3` task-068 drift on top of `a11ba8d3`), main advanced:
task-224 had landed the identical task-068 repair as `770785f7`, then
task-227/228/229 landed the task-196 repair (`05709c24…` recorded in
`specs/tasks/task-196.md`), task-155's squashed landing `27bd585c` was
repaired by task-231/232, and the chain culminated in `34248324`
(task-229). The executor merged base `34248324` into this branch (merge
`0b312332`, no conflicts — the task-068 repair is byte-identical on both
sides: `git diff 607b8b2f 34248324 -- specs/tasks/task-068.md` is
empty). No active merge or rebase remained to resolve.

**This round changed no product source:** the merged tree vs base
`34248324` is exactly `specs/tasks/task-226.md` (this task file:
`depends_on: []` → `[task-229]` per this assignment's contract, plus
this section). `git diff 34248324 HEAD -- scripts/ crates/ web/` is
empty; `scripts/task-commit-attribution-exemptions.txt` unchanged at its
frozen 3-entry baseline (`01493c88 task-097`, `17c81d5a task-072`,
`a8d036f4 task-091`); no gate, skip, or check weakened.

**Fresh verification on the merged tree (HEAD `0b312332`):**

- Reproduction at the exact base commit: detached worktree at
  `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0`, `bash
  scripts/check-task-commit-attribution.sh` → exit 1 with the identical
  violation `a11ba8d3 task-068` (task-226-round2-reproduce-base-a11ba8d3.txt).
- `bash scripts/check-task-commit-attribution.sh` at merged HEAD — exit
  0 (task-226-round2-gate-at-merged-head.txt).
- Mutation check (this task's own round-1 repair, re-verified after the
  merge): `a11ba8d3…` removed from `specs/tasks/task-068.md` → gate
  FAIL exit 1 with exactly `a11ba8d3 task-068 feat(task-068): Graph
  Summary & Dry-Run MCP Tools`; restored → exit 0
  (task-226-round2-mutation-check-task068.txt,
  task-226-round2-mutation-restore-task068.txt).
- Mutation check (prerequisite drift repaired on main by task-229, the
  assignment's prerequisite): `05709c24…` removed from
  `specs/tasks/task-196.md` → gate FAIL exit 1 with exactly `05709c24
  task-196 feat(task-196): Ground Briefing Q&A in real briefing data
  with sources and history validation`; restored → exit 0
  (task-226-round2-mutation-check-task196.txt). The pass on the merged
  tree is attributable to both recorded SHAs, not gate drift.
- `python3 /tmp/stage/dev-attribution.py task-226` — no change: the
  branch delta vs base `34248324` is only `specs/tasks/task-226.md`
  (specs-only), so `commits: []` remains attribution-canonical for this
  round.

The TCP `accept()` listener probe remains unsupported in this sandbox
(errno 95, `/tmp/stage/capabilities.json`); no runtime surface was
touched, so no HTTP probe applies. Exact-head GitHub checks belong to
host verification and remain mandatory.

Independent review, full deterministic gates, and GitHub checks on the
exact PR head remain required before merge. All evidence above is under
`/tmp/stage/review-evidence/`.
