---
title: "Repair verified failure on main a11ba8d32859"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: [task-227]
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

**Status: repaired and re-verified on the merged round-2 tree.** Both
the task's assigned failure (round 1) and this round's re-verification
baseline failure are repaired in the candidate tree; the attribution
gate and the full deterministic gate set pass at the merged HEAD.

### Round 1 — assigned failure repaired (base `a11ba8d32859...`)

task-068's product-surface commit `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0`
(`feat(task-068): Graph Summary & Dry-Run MCP Tools`, touching
`crates/gyre-domain/src/view_query_resolver.rs`,
`crates/gyre-server/src/explorer_ws.rs`,
`crates/gyre-server/src/mcp.rs`,
`crates/gyre-server/tests/graph_integration.rs`) landed on main but was
absent from `specs/tasks/task-068.md`'s `commits:` frontmatter — the
task-095 R3-F4 squash-drift class: a squash landing commit cannot
contain its own SHA in the task file it ships, so the recorded list
stays one entry short and the landed surface is invisible to review
scoping. Repair: one line — append
`"a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0"` to task-068's `commits:`
list, the check's own documented remedy. Checkpointed as
`9739dbe3793ec55173716e640aa8209cfb27979c`.

The identical one-line repair landed on main independently as task-224
(`770785f7`, same SHA appended to the same list), so both branches
carried identical content on `specs/tasks/task-068.md` and the merge
below deduplicated it — the SHA is recorded exactly once at the merged
HEAD.

### Round 2 — prerequisite satisfied, merge resolved, verified (assignment base `18c44f1a0f278ca78ce0c8c3ec5e5761d40cd36b`, repair base `05709c242509b89214876339b3c463ede3a31b60`)

This round's baseline failure (`05709c24 task-196` squash-drift,
`feat(task-196): Ground Briefing Q&A in real briefing data with sources
and history validation`) was repaired by prerequisite task-227, landed
as `18c44f1a` (`05709c242509...` appended to task-196's `commits:`
frontmatter). The round-1 checkpoint branch was merged with that
landing as `99faec5c` (ort strategy, no conflicts). The merged HEAD
carries both repairs, each recorded exactly once:

- `specs/tasks/task-068.md` — `commits:` ends
  `"a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0"` (10 SHAs total)
- `specs/tasks/task-196.md` — `commits:` ends
  `"05709c242509b89214876339b3c463ede3a31b60"` (4 SHAs total)

Diff of the merged candidate vs assignment base `18c44f1a`:
`specs/tasks/task-223.md` only (this record). `scripts/`, `crates/`,
and `web/` diffs are empty — no gate weakened, no skip or exemption
added (exemption file unchanged at its frozen 3-entry baseline,
`FROZEN_EXEMPTION_COUNT=3`), no product surface touched, so
`commits: []` is correct (`python3 /tmp/stage/dev-attribution.py
task-223` produces no change).

### Evidence (under `/tmp/stage/review-evidence/`)

- **Reproduction at the round-2 repair base.** Detached worktree at
  `05709c242509b89214876339b3c463ede3a31b60`: `bash
  scripts/check-task-commit-attribution.sh` exits 1 with the identical
  violation (`05709c24 task-196 ...`). Evidence:
  `task-223-repro-at-repair-base.txt`.
- **Assignment base passes** (prerequisite repair landed): detached
  worktree at `18c44f1a0f278ca78ce0c8c3ec5e5761d40cd36b` exits 0.
  Evidence: `task-223-repro-at-assignment-base.txt`.
- **Round-1 checkpoint passes**: detached worktree at
  `9739dbe3793ec55173716e640aa8209cfb27979c` exits 0. Evidence:
  `task-223-checkpoint-pre-merge.txt`.
- **All 21 deterministic gates pass at the merged HEAD `99faec5c`** —
  the baseline `checks.sh` set plus the remaining repo gates: arch,
  hierarchy, abac-route-registry, abac-exempt-handlers,
  mcp-write-tools, migration-versions, migration-sql-portability,
  dead-message-kinds, byte-slice-truncation, relative-path-defaults,
  fail-open-ref-resolution, mem-port-contracts,
  fabricated-scope-defaults, lossy-secret-conversion,
  scope-literal-defaults, inert-enforcement, in-memory-state-stores,
  unbounded-external-http, forwarded-header-trust,
  forged-scope-fields, task-commit-attribution — all exit 0. Evidence:
  `task-223-all-checks-at-merged-head.txt`.
- **rustfmt / clippy changed-lines clean** vs assignment base (0 Rust
  files changed; `git diff --check` clean). Evidence:
  `task-223-rustfmt-clippy-at-merged-head.txt` — includes a toolchain
  note: this sandbox's rustc 1.99.0 reports 1141 pre-existing warnings
  on unchanged debt vs 343 in the host baseline log; the gate only
  fails on warnings on changed lines, and there are none.
- **Mutation checks (test-the-repair).** (A) Removing
  `05709c242509...` from task-196's frontmatter re-fails the gate with
  the identical violation (exit 1); restoring it re-passes (exit 0).
  (B) Removing `a11ba8d32859...` from task-068's frontmatter re-fails
  identically; restoring re-passes. Both recorded SHAs are
  load-bearing at the merged tree — the pass is attributable to the
  recorded SHAs, not gate drift. Evidence:
  `task-223-mutation-check-task196.txt`,
  `task-223-mutation-check-task068.txt` (B re-establishes round 1's
  repair evidence at the merged HEAD).
- **No gate weakened.** `scripts/` diff vs assignment base: 0 lines;
  `crates/`+`web/` diff: 0 lines; exemption file at frozen 3-entry
  baseline. Evidence: `task-223-no-gate-weakening.txt`.
- **Transport restriction**: this sandbox cannot accept TCP
  (`accept(): [Errno 95] Operation not supported`, recorded in
  `/tmp/stage/capabilities.json`). No runtime surface was touched, so
  no HTTP probe is applicable; exact-head GitHub checks belong to host
  verification and remain mandatory.
- **Durable recurrence cause** (re-confirmed against the current tree,
  still unfixed on main, already flagged by tasks 215/222 as needing
  its own task): the merge-confirmation path of `publish()` in
  `scripts/pipeline/stages.py` learns the squash `mergeCommit.oid`
  (line 284) but only stores it in delivery metadata — it never
  appends the landed SHA to the task's `commits:` frontmatter on main,
  so every future product-surface ship re-creates this drift and the
  pipeline keeps minting repair tasks for it. A pipeline-side fix is
  out of scope for this repair round.

Independent review, full deterministic gates, and GitHub checks on the
exact PR head remain required before merge.
