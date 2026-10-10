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
Environment fingerprint: `host-d90b3c64660b8c76729825ef7a15fba649ca4873882aacf47f4ed5bbd100c963`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/eb918b9eb5e44d1f8d0723b5b869df5d/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/eb918b9eb5e44d1f8d0723b5b869df5d/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/eb918b9eb5e44d1f8d0723b5b869df5d/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/eb918b9eb5e44d1f8d0723b5b869df5d/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/eb918b9eb5e44d1f8d0723b5b869df5d/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/eb918b9eb5e44d1f8d0723b5b869df5d/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/eb918b9eb5e44d1f8d0723b5b869df5d/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/eb918b9eb5e44d1f8d0723b5b869df5d/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/eb918b9eb5e44d1f8d0723b5b869df5d/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/eb918b9eb5e44d1f8d0723b5b869df5d/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/eb918b9eb5e44d1f8d0723b5b869df5d/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/eb918b9eb5e44d1f8d0723b5b869df5d/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 257 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/eb918b9eb5e44d1f8d0723b5b869df5d/1/tools/checks.sh
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
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/eb918b9eb5e44d1f8d0723b5b869df5d/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  b1cdb0dd  task-170  feat(task-170): View Specification Grammar \u2014 TypeScript types and server-side validation\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-znk9eq6g/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Reproduction.** At assignment HEAD (`b1cdb0dd`, tree = base + untracked task
file), `bash scripts/check-task-commit-attribution.sh` exited 1 listing exactly
`b1cdb0dd  task-170  feat(task-170): View Specification Grammar — TypeScript
types and server-side validation` — a product-surface commit (touches
`crates/gyre-common/src/view_spec.rs`, `crates/gyre-server/src/api/explorer_views.rs`,
`web/src/`, `web/tests/`, `web/dist/`) missing from
`specs/tasks/task-170.md`'s `commits:` frontmatter. Root cause is the
squash-drift class recorded by tasks 213/219/222/224/229: the squashed landing
commit cannot contain its own SHA, so the task's recorded list stayed one entry
short and the landed surface (view-spec grammar, 450+ lines of server
validation) was invisible to review scoping (task-095 R3-F4 flaw class).
Evidence: `/tmp/stage/review-evidence/task-244-repro-before-repair.txt`.

**Repair.** Appended the full landed SHA
`b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` to `specs/tasks/task-170.md`'s
`commits:` frontmatter, joining the 11 branch SHAs already recorded
(`394ac741` … `3c41b419`; kept, not collapsed — matching the task-229 repair
shape: branch SHAs + landed SHA). This is the check's own documented remedy.
No exemptions added; `scripts/task-commit-attribution-exemptions.txt`
untouched at its frozen 3-entry baseline; no check, skip, or gate weakened; no
Rust/JS source changed (`git diff b1cdb0dd --stat -- crates/ web/ scripts/` is
empty; the only source delta is `specs/tasks/`, 1 file, 1 line).

**Probe after repair.** Exit 0 — `OK: every task-labeled product-surface
commit is recorded in its task's commits: frontmatter (or exempted legacy
drift).` Evidence: `/tmp/stage/review-evidence/task-244-attribution-after.txt`.

**Mutation check (test-the-repair).** With the repair reverted via
`git stash`, the gate re-fails with the identical violation (exit 1,
`task-244-mutation-reverted-fails.txt`); restoring it re-passes (exit 0,
`task-244-mutation-restored-passes.txt`) — the pass is attributable to the
recorded SHA, not gate drift.

**Attribution for this task.** `python3 /tmp/stage/dev-attribution.py
task-244` produces no change, so `commits: []` is correct: this repair round
adds only task records (`specs/tasks/task-170.md`, `specs/tasks/task-244.md`),
no product surface.

The TCP `accept()` listener probe remains unsupported in this sandbox
(errno 95, `/tmp/stage/capabilities.json`); no runtime surface was touched, so
no HTTP probe applies. Exact-head GitHub checks belong to host verification
and remain mandatory.
