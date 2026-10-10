---
title: "Repair verified failure on main 27bd585ca7eb"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `27bd585ca7eb429905ccbded1f48b4d0167c0c20`
Environment fingerprint: `host-4bf5f1391a9170ff0825a033b8c31d847fa21f2dc735b4d938713eb9435bd4ef`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/ae0f2622b417438c946dd042b491b2e3/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/ae0f2622b417438c946dd042b491b2e3/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/ae0f2622b417438c946dd042b491b2e3/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/ae0f2622b417438c946dd042b491b2e3/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/ae0f2622b417438c946dd042b491b2e3/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/ae0f2622b417438c946dd042b491b2e3/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/ae0f2622b417438c946dd042b491b2e3/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/ae0f2622b417438c946dd042b491b2e3/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/ae0f2622b417438c946dd042b491b2e3/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/ae0f2622b417438c946dd042b491b2e3/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/ae0f2622b417438c946dd042b491b2e3/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/ae0f2622b417438c946dd042b491b2e3/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 257 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/ae0f2622b417438c946dd042b491b2e3/1/tools/checks.sh
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

  27bd585c  task-155  feat(task-155): Implement gyre search CLI command

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/ae0f2622b417438c946dd042b491b2e3/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "27bd585ca7eb429905ccbded1f48b4d0167c0c20", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  27bd585c  task-155  feat(task-155): Implement gyre search CLI command\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-sltkkly1/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Reproduction:** `bash scripts/check-task-commit-attribution.sh` at base
`27bd585ca7eb429905ccbded1f48b4d0167c0c20` exits 1 with the identical
violation: `27bd585c task-155 feat(task-155): Implement gyre search CLI
command`. Evidence: `/tmp/stage/review-evidence/task-234-attribution-repro.txt`.

**Root cause:** the squashed task-155 landing commit `27bd585c` touches
product surface (`crates/gyre-cli/src/client.rs`, `main.rs`) and carries the
`task-155` label, but the task file's `commits:` frontmatter recorded only
the 7 candidate-branch SHAs — the landing SHA cannot appear inside its own
tree, so the recorded list stayed one entry short and the landing surface was
invisible to review scoping. Same drift class as task-224's `a11ba8d3`
(task-068) and task-228's `05709c24` (task-196): the check's own documented
remedy is to record the SHA, not exempt it.

**Repair (1 line, `specs/tasks/task-155.md:8`):** appended the full SHA
`27bd585ca7eb429905ccbded1f48b4d0167c0c20` to the existing `commits:` list,
preserving all 7 candidate SHAs. All 7 remain resolvable on remote
`pipeline/task-155/*` and `devloop/task-155/*` refs — dropping them (as
`dev-attribution.py` regeneration from merged-main history would) would
shrink review scope and lose the candidate lineage. No exemptions added —
`scripts/task-commit-attribution-exemptions.txt` untouched at its frozen
3-entry baseline; no gate, skip, or check weakened; no Rust/JS source changed
(`git diff 27bd585c -- scripts/ crates/ web/src/ web/tests/` is empty).

**Probe after repair:** exit 0 — `OK: every task-labeled product-surface
commit is recorded in its task's commits: frontmatter (or exempted legacy
drift).` Evidence: `/tmp/stage/review-evidence/task-234-attribution-after-repair.txt`.

**Mutation check (test-the-repair):** SHA removed from frontmatter (sed
mutation) → identical FAIL exit 1 (`task-234-attribution-mutation.txt`);
SHA restored → exit 0 again (`task-234-attribution-restored.txt`). The pass
is attributable to the recorded SHA, not gate drift.

**Attribution for this task:** this round changes only
`specs/tasks/task-155.md` and this task file — no product surface — so
`commits: []` is attribution-canonical for task-234 itself.

**Sandbox limitation (recorded, not a code defect):** this sandbox cannot run
TCP `accept()` (errno 95, Operation not supported, per
`/tmp/stage/capabilities.json`) — irrelevant here: the failing probe is a
pure git-history script needing no server, listener, or browser. Full gates
and GitHub CI remain for the verification/publication stage as usual.
