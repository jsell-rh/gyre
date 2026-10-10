---
title: "Repair verified failure on main b1cdb0ddd80e"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3`
Environment fingerprint: `host-d19f791ecfe47060da707de47cb06557c8a0adaaa45a3b1b9045292d3a231e92`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 257 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/tools/checks.sh
rustfmt: changed lines clean (13 Rust files checked)
crates/gyre-adapters/src/sqlite/spec_approval.rs:326: clippy::manual_repeat_n: this `repeat().take()` can be written more concisely
    Checking gyre-common v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-common)
   Compiling gyre-server v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-server)
warning: gyre-server@0.1.0: SKIP_WEB_BUILD=1 set, skipping web build
    Checking gyre-domain v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-domain)
    Checking gyre-cli v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-cli)
    Checking gyre-ports v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-ports)
    Checking gyre-adapters v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/crates/gyre-adapters)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 34s

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

  b1cdb0dd  task-170  feat(task-170): View Specification Grammar — TypeScript types and server-side validation

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/c1f5256adfeb4b4d8637e8843d07ac18/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  b1cdb0dd  task-170  feat(task-170): View Specification Grammar \u2014 TypeScript types and server-side validation\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-sl9snd7n/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Status: repaired.** The verified failure at base `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` was live at assignment HEAD (tree = base + untracked task file) and is now fixed with the check's documented remedy.

- **Reproduction at assignment HEAD**: `bash scripts/check-task-commit-attribution.sh` exited 1 listing `b1cdb0dd  task-170  feat(task-170): View Specification Grammar — TypeScript types and server-side validation` — a product-surface commit (touches `crates/gyre-common/src/view_spec.rs`, `crates/gyre-server/src/api/explorer_views.rs`, `.gitattributes`, plus task/web/spec records) missing from `specs/tasks/task-170.md`'s `commits:` frontmatter. Root cause is the squash-drift class recorded by tasks 213/219/222/224/229/231/232/236: the squashed landing commit cannot contain its own SHA, so the task's recorded list stayed one entry short and the landing surface was invisible to review scoping (task-095 R3-F4 flaw class). Evidence: `/tmp/stage/review-evidence/task-242-reproduction-before-repair.txt`.
- **Repair**: appended the full SHA `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` to `specs/tasks/task-170.md`'s `commits:` frontmatter, joining the 11 SHAs already recorded from the task's branch (kept, not collapsed, matching the task-229 repair shape that landed on main with branch SHAs + landed SHA). This is the check's own documented remedy. No exemptions added; `scripts/task-commit-attribution-exemptions.txt` untouched at its frozen 3-entry baseline; no check, skip, or gate weakened; no Rust/JS source changed (`git diff b1cdb0dd --stat -- crates/ web/ scripts/` is empty; the only source delta is `specs/tasks/task-170.md`, 1 line).
- **Probe after repair**: exit 0 — `OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift).` Evidence: `/tmp/stage/review-evidence/task-242-attribution-after.txt`.
- **Mutation check (test-the-repair)**: with the repair reverted via `git stash`, the gate re-fails with the identical violation (exit 1, `b1cdb0dd task-170`); restoring it re-passes (exit 0) — the pass is attributable to the recorded SHA, not gate drift. Evidence: `/tmp/stage/review-evidence/task-242-mutation-check.txt`.
- **Attribution for this task**: `python3 /tmp/stage/dev-attribution.py task-242` produces no change, so `commits: []` is correct: this repair round adds only task records (`specs/tasks/task-170.md`, `specs/tasks/task-242.md`), no product surface.
- **Baseline clippy line disposition**: the baseline log's `crates/gyre-adapters/src/sqlite/spec_approval.rs:326: clippy::manual_repeat_n` line is not reproducible at this base — the file is 235 lines at `b1cdb0dd` with no `repeat(` anywhere in its history, and no `repeat(...).take(...)` pattern exists anywhere in `crates/` at HEAD. The baseline run's authoritative `GYRE_BASELINE_FAILURE_JSON` names exactly one probe (`check-task-commit-attribution.sh`), and the baseline checks run continued past the clippy warning to completion (all subsequent gates printed OK). Noise from a different environment fingerprint (`61f092e0`), not a finding at this base; not acted on.
- **Transport restriction**: this sandbox cannot accept TCP (`accept(): [Errno 95] Operation not supported`, recorded in `/tmp/stage/capabilities.json`). No runtime surface was touched, so no HTTP probe is applicable; exact-head GitHub checks belong to host verification and remain mandatory.

Independent review, full deterministic gates, and GitHub checks on the exact PR head remain required before merge. All evidence above is under `/tmp/stage/review-evidence/`.
