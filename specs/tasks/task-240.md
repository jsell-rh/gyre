---
title: "Repair verified failure on main b1cdb0ddd80e"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3`
Environment fingerprint: `host-93e627f1f22866ce849448177b990905a17863afd4ce6c4cb5b0d34ecd6f36df`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/9c166ffbddb348c599ab0f50dfa77f54/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/9c166ffbddb348c599ab0f50dfa77f54/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/9c166ffbddb348c599ab0f50dfa77f54/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/9c166ffbddb348c599ab0f50dfa77f54/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/9c166ffbddb348c599ab0f50dfa77f54/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/9c166ffbddb348c599ab0f50dfa77f54/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/9c166ffbddb348c599ab0f50dfa77f54/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/9c166ffbddb348c599ab0f50dfa77f54/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/9c166ffbddb348c599ab0f50dfa77f54/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/9c166ffbddb348c599ab0f50dfa77f54/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/9c166ffbddb348c599ab0f50dfa77f54/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/9c166ffbddb348c599ab0f50dfa77f54/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 257 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/9c166ffbddb348c599ab0f50dfa77f54/1/tools/checks.sh
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

  b1cdb0dd  task-170  feat(task-170): View Specification Grammar — TypeScript types and server-side validation

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/9c166ffbddb348c599ab0f50dfa77f54/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  b1cdb0dd  task-170  feat(task-170): View Specification Grammar \u2014 TypeScript types and server-side validation\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-dyjubs8f/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Status: repaired.** The verified failure at base `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` was live and is fixed with the check's own documented remedy. This round resumed from checkpoint `54462db3` (the prior assignment completed the repair and its final gate but was cut off by an inference-budget error before close-out); the repair is carried in this branch's head `0e653509` (checkpoint `54462db3` + input base `1a00087c`) and was fully re-verified there.

- **Root cause** (squash-drift class recorded by tasks 213/219/222/224/229): the task-170 implementation landed as a single squash commit `b1cdb0dd`, which cannot contain its own SHA, so `specs/tasks/task-170.md`'s `commits:` frontmatter stayed one entry short (11 branch SHAs) and the landing surface — `feat(task-170): View Specification Grammar — TypeScript types and server-side validation`, touching `crates/gyre-common/src/view_spec.rs`, `crates/gyre-server/src/api/explorer_views.rs`, `web/src/lib/types/view-spec.ts`, and 12 more files — was invisible to review scoping (task-095 R3-F4 flaw class).
- **Repair**: appended the full SHA `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` to `specs/tasks/task-170.md`'s `commits:` frontmatter, joining the 11 SHAs already recorded from the task's branch (`394ac741`, `6ab72e55`, `9447554c`, `5cf54bce`, `c76fa1fd`, `b69e0100`, `08831630`, `95801f2b`, `49614e41`, `9aa19ef9`, `3c41b419` — kept, not collapsed, matching the task-069→task-196 repair shape that landed on main with branch SHAs + landed SHA, commit `2f092bcd`). This is the check's own documented remedy. No exemptions added; `scripts/task-c....txt` verified untouched at its frozen 3-entry baseline; no check, skip, or gate weakened; no Rust/JS source changed (the repair's delta vs base `b1cdb0dd` is exactly `specs/tasks/task-170.md` one line + this task file).
- **Gate at head**: `bash scripts/check-task-commit-attribution.sh` at merged head `0e653509` exits 0 — `OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift)`. Evidence: `/tmp/stage/review-evidence/task-240-resume-gate.txt`.
- **Mutation check (test-the-repair)**: at the same head, reverting exactly the one-line repair (`git checkout b1cdb0dd -- specs/tasks/task-170.md`) makes the gate re-fail with the identical baseline violation (exit 1, `b1cdb0dd task-170`); restoring it (`git checkout HEAD -- specs/tasks/task-170.md`) re-passes (exit 0) — the pass is attributable to the recorded SHA, not gate drift. Evidence: `/tmp/stage/review-evidence/task-240-resume-mutation.txt` and `/tmp/stage/review-evidence/task-240-resume-restored.txt`.
- **Attribution for this task**: `python3 /tmp/stage/dev-attribution.py task-240` exits 0 with no diff, so `commits: []` is correct: this repair round adds only task records (`specs/tasks/task-170.md` one line, `specs/tasks/task-240.md` this file), no product surface.
- **Transport restriction**: this sandbox cannot accept TCP (`accept(): [Errno 95] Operation not supported`, recorded in `/tmp/stage/capabilities.json`). No runtime surface was touched, so no HTTP probe is applicable; exact-head GitHub checks belong to host verification and remain mandatory.

Independent review, full deterministic gates, and GitHub checks on the exact PR head remain required before merge. All evidence above is under `/tmp/stage/review-evidence/`.
