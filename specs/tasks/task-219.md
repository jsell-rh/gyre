---
title: "Repair verified failure on main 6bf777a6a44f"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `6bf777a6a44f28052ed5af28bf6fb013fde6df48`
Environment fingerprint: `host-df92513fbc21db67aab359bc1ef32da5bf901e5827f5aa1bdca043692e68a3dd`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5f34f02fdb1d470c8e88f7dcd9003a60/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5f34f02fdb1d470c8e88f7dcd9003a60/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5f34f02fdb1d470c8e88f7dcd9003a60/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5f34f02fdb1d470c8e88f7dcd9003a60/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5f34f02fdb1d470c8e88f7dcd9003a60/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5f34f02fdb1d470c8e88f7dcd9003a60/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5f34f02fdb1d470c8e88f7dcd9003a60/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5f34f02fdb1d470c8e88f7dcd9003a60/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5f34f02fdb1d470c8e88f7dcd9003a60/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5f34f02fdb1d470c8e88f7dcd9003a60/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5f34f02fdb1d470c8e88f7dcd9003a60/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5f34f02fdb1d470c8e88f7dcd9003a60/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 257 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/5f34f02fdb1d470c8e88f7dcd9003a60/1/tools/checks.sh
rustfmt: changed lines clean (0 Rust files checked)
clippy: changed lines clean (0 Rust files, 347 existing warnings outside changes)
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

  6bf777a6  task-200  feat(task-200): Message bus — per-kind payload schema validation (reject invalid payloads with 400)

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/5f34f02fdb1d470c8e88f7dcd9003a60/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "6bf777a6a44f28052ed5af28bf6fb013fde6df48", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  6bf777a6  task-200  feat(task-200): Message bus \u2014 per-kind payload schema validation (reject invalid payloads with 400)\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-fif33aqm/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

- **Reproduced** the verified baseline failure at assignment HEAD (`6bf777a6`, tree equal to base plus only the untracked task file): `bash scripts/check-task-commit-attribution.sh` exited 1 listing `6bf777a6 task-200 feat(task-200): Message bus — per-kind payload schema validation (reject invalid payloads with 400)` — a product-surface commit (touches `crates/gyre-common/src/message.rs`, `crates/gyre-server/src/api/messages.rs`, `crates/gyre-server/src/mcp.rs`) missing from task-200's `commits:` frontmatter. Root cause: the squashed landing commit cannot contain its own SHA, so the task's recorded list stayed one entry short and the landing surface was invisible to review scoping (task-095 R3-F4 flaw class). Evidence: `/tmp/stage/review-evidence/attribution-before.txt`.
- **Repair** (the check's documented remedy, commit `5df2f9ab`): appended the full SHA `6bf777a6a44f28052ed5af28bf6fb013fde6df48` to `specs/tasks/task-200.md`'s `commits:` frontmatter, joining the 13 SHAs already recorded from the task's branch. No exemptions added — `scripts/task-commit-attribution-exemptions.txt` untouched at its frozen 3-entry baseline; no check, skip, or gate weakened; no Rust/JS source changed.
- **Probe after fix**: `bash scripts/check-task-commit-attribution.sh` → OK, exit 0 (evidence: `attribution-after.txt`; HEAD `5df2f9ab` recorded in `head-commit.txt`).
- **Mutation check (test-the-repair)**: with the repair present, removing the recorded SHA from the frontmatter re-fails the gate with the identical violation (`6bf777a6 task-200`), and restoring it re-passes (`mutation-check.txt`, `mutation-restore-check.txt`) — the pass is attributable to this repair, not gate drift.
- **Other gates on HEAD `5df2f9ab`**: `check-arch.sh` OK; dead-message-kinds, mcp-write-tools, migration-versions, byte-slice-truncation, relative-path-defaults, fail-open-ref-resolution all OK; `git diff --check 6bf777a6..HEAD` clean; `check-rustfmt-diff.py 6bf777a6` reports `changed lines clean (0 Rust files checked)` — the diff touches no Rust files, so clippy's all-target run (owned by verification/publication) has no changed lines to judge. Task-219 attribution verified via `python3 /tmp/stage/dev-attribution.py task-219` — no change produced; `commits: []` is correct because the only branch commit (`5df2f9ab`) touches `specs/` only, no product surface.
- This sandbox cannot accept TCP (`accept(): [Errno 95] Operation not supported`, recorded in `/tmp/stage/capabilities.json`); live-HTTP and exact-head GitHub checks belong to host verification. No runtime surface was touched, so no server/browser probe is applicable. Independent review and full verification (GitHub CI on the exact PR head) decide approval.

Candidate base: 6bf777a6a44f28052ed5af28bf6fb013fde6df48
Repair commit: 5df2f9ab (process(task-200) frontmatter record); this task record is the branch tip that follows it.
