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
Environment fingerprint: `host-eebfb712a1f0d390c4f1aeac41a0d289a9b7dbab9cf313db39d1c191d1ed1134`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/1aea27202cd04b378d5f6f36f6ed50bf/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/1aea27202cd04b378d5f6f36f6ed50bf/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/1aea27202cd04b378d5f6f36f6ed50bf/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/1aea27202cd04b378d5f6f36f6ed50bf/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/1aea27202cd04b378d5f6f36f6ed50bf/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/1aea27202cd04b378d5f6f36f6ed50bf/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/1aea27202cd04b378d5f6f36f6ed50bf/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/1aea27202cd04b378d5f6f36f6ed50bf/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/1aea27202cd04b378d5f6f36f6ed50bf/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/1aea27202cd04b378d5f6f36f6ed50bf/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/1aea27202cd04b378d5f6f36f6ed50bf/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/1aea27202cd04b378d5f6f36f6ed50bf/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 4598 files, 16.2GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/1aea27202cd04b378d5f6f36f6ed50bf/1/tools/checks.sh
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
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/1aea27202cd04b378d5f6f36f6ed50bf/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  a11ba8d3  task-068  feat(task-068): Graph Summary & Dry-Run MCP Tools\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-21ydk_5j/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**The verified failure is repaired by recording the landed commit in the
drifted task's frontmatter** — the fix the gate's own diagnostic prescribes
and the exact repair pattern of tasks 215/216/218/219/222 for the same
flaw class.

Root cause (task-095 R3-F4 drift class): task-068's product-surface
commit `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0`
(`feat(task-068): Graph Summary & Dry-Run MCP Tools`, touching
`crates/gyre-domain/src/view_query_resolver.rs`,
`crates/gyre-server/src/explorer_ws.rs`,
`crates/gyre-server/src/mcp.rs`,
`crates/gyre-server/tests/graph_integration.rs`) landed on main but was
absent from `specs/tasks/task-068.md`'s `commits:` frontmatter. A squash
landing commit cannot contain its own SHA in the task file it ships
(the file's content is fixed at squash time, one entry short), so the
landing surface stays invisible to review scoping until recorded.

Change: one line — append
`"a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0"` to task-068's `commits:`
frontmatter list. No gate weakened: `scripts/` untouched, exemption file
unchanged at its frozen 3-entry baseline, no new exemptions.

Evidence (under `/tmp/stage/review-evidence/`):

- **Reproduction at the exact base.** Detached worktree at
  `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0`: `bash
  scripts/check-task-commit-attribution.sh` exits 1 with the identical
  violation (`a11ba8d3 task-068 feat(task-068): Graph Summary & Dry-Run
  MCP Tools`). Evidence: `repro-at-base-a11ba8d3-task223.txt`.
- **Reproduction at assignment HEAD (pre-fix).** Same exit-1 failure
  with the identical violation at the branch HEAD before the edit
  (observed in-session as this round's first probe; the worktree was
  clean except the untracked task-223.md, so HEAD carried the base
  tree).
- **Pass after repair.** `bash scripts/check-task-commit-attribution.sh`
  exits 0, `OK: every task-labeled product-surface commit is recorded in
  its task's commits: frontmatter (or exempted legacy drift).` Evidence:
  `attribution-after-task223.txt`.
- **Mutation check (test-the-repair).** Removing the recorded SHA from
  task-068's frontmatter re-fails the gate with the identical violation
  (exit 1); restoring it re-passes (exit 0) — the pass is attributable
  to the recorded SHA, not gate drift. Evidence:
  `mutation-check-task223.txt`, `mutation-restore-check-task223.txt`.
- **No gate weakened.** `git diff` on this branch shows exactly one
  changed line in `specs/tasks/task-068.md`; `scripts/` and
  `crates/`/`web/` diffs are empty; exemption file at frozen 3-entry
  baseline.
- **Attribution for this task**: `python3
  /tmp/stage/dev-attribution.py task-223` produced no change — `commits:
  []` is correct: this branch touches only task bookkeeping, no product
  surface.
- **Transport restriction**: this sandbox cannot accept TCP
  (`accept(): [Errno 95] Operation not supported`, recorded in
  `/tmp/stage/capabilities.json`). No runtime surface was touched, so
  no HTTP probe is applicable; exact-head GitHub checks belong to host
  verification and remain mandatory.
- **Durable recurrence cause** (re-confirmed, still unfixed on main,
  already flagged by task-215/222 and needing its own task): `publish()`
  in `scripts/pipeline/stages.py` (lines 281–292) learns the squash
  `mergeCommit.oid` but only stores it in delivery metadata — it never
  appends the landed SHA to the task's `commits:` frontmatter on main,
  so every future product-surface ship re-creates this drift and the
  pipeline keeps minting repair tasks for it. A pipeline-side fix
  (record the merge SHA in the task file in the merge-confirmation path)
  is out of scope for this repair round.

Independent review, full deterministic gates, and GitHub checks on the
exact PR head remain required before merge.
