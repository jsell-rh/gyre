---
title: "Repair verified failure on main 05709c242509"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `05709c242509b89214876339b3c463ede3a31b60`
Environment fingerprint: `host-0d7fca759a4efd75148128476abc3bb7fc4835ac727a7ed32ac26169476e57cb`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/fa549d486faa46218a410ae3f0576e86/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/fa549d486faa46218a410ae3f0576e86/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/fa549d486faa46218a410ae3f0576e86/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/fa549d486faa46218a410ae3f0576e86/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/fa549d486faa46218a410ae3f0576e86/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/fa549d486faa46218a410ae3f0576e86/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/fa549d486faa46218a410ae3f0576e86/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/fa549d486faa46218a410ae3f0576e86/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/fa549d486faa46218a410ae3f0576e86/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/fa549d486faa46218a410ae3f0576e86/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/fa549d486faa46218a410ae3f0576e86/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/fa549d486faa46218a410ae3f0576e86/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 257 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/fa549d486faa46218a410ae3f0576e86/1/tools/checks.sh
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

  05709c24  task-196  feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/fa549d486faa46218a410ae3f0576e86/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "05709c242509b89214876339b3c463ede3a31b60", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  05709c24  task-196  feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-h0yvlgj7/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Status: repaired.** The verified failure at base `05709c242509b89214876339b3c463ede3a31b60` was live and is now fixed with the check's documented remedy.

- **Reproduction at assignment HEAD** (tree = base + untracked task file): `bash scripts/check-task-commit-attribution.sh` exited 1 listing `05709c24 task-196 feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation` — a product-surface commit (touches `crates/gyre-server/src/api/graph.rs`, `web/src/components/Briefing.svelte`, `web/src/__tests__/Briefing.test.js`) missing from `specs/tasks/task-196.md`'s `commits:` frontmatter. Root cause is the squash-drift class recorded by tasks 213/219/222/224: the squashed landing commit cannot contain its own SHA, so the task's recorded list stayed one entry short and the landing surface was invisible to review scoping (task-095 R3-F4 flaw class). Evidence: `task-229-reproduction-before-repair.txt`.
- **Repair**: appended the full SHA `05709c242509b89214876339b3c463ede3a31b60` to `specs/tasks/task-196.md`'s `commits:` frontmatter, joining the 3 SHAs already recorded from the task's branch (`3b90956c`, `abcfff04`, `88b57180` — kept, not collapsed, matching the task-224→task-068 repair shape that landed on main with branch SHAs + landed SHA). This is the check's own documented remedy. No exemptions added; `scripts/task-commit-attribution-exemptions.txt` untouched at its frozen 3-entry baseline; no check, skip, or gate weakened; no Rust/JS source changed (`git diff 05709c24 -- crates/ web/ scripts/` is empty; the only source delta is `specs/tasks/`).
- **Probe after repair**: exit 0 — `OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift).` Evidence: `task-229-attribution-after.txt`.
- **Mutation check (test-the-repair)**: with the repair reverted via `git stash`, the gate re-fails with the identical violation (exit 1, `task-229-reproduction-before-repair.txt`); restoring it re-passes (exit 0) — the pass is attributable to the recorded SHA, not gate drift.
- **Attribution for this task**: `python3 /tmp/stage/dev-attribution.py task-229` produces no change, so `commits: []` is correct: this repair round adds only task records (`specs/tasks/task-196.md`, `specs/tasks/task-229.md`), no product surface.
- **Transport restriction**: this sandbox cannot accept TCP (`accept(): [Errno 95] Operation not supported`, recorded in `/tmp/stage/capabilities.json`). No runtime surface was touched, so no HTTP probe is applicable; exact-head GitHub checks belong to host verification and remain mandatory.

Independent review, full deterministic gates, and GitHub checks on the exact PR head remain required before merge. All evidence above is under `/tmp/stage/review-evidence/`.
