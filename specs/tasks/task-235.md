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
Environment fingerprint: `host-0365a575de4b432b8bcc453e67a469384c0178a8735293dbd33e4fddb90f72a4`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/926af758f72041a891e6031c64d2c30c/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/926af758f72041a891e6031c64d2c30c/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/926af758f72041a891e6031c64d2c30c/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/926af758f72041a891e6031c64d2c30c/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/926af758f72041a891e6031c64d2c30c/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/926af758f72041a891e6031c64d2c30c/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/926af758f72041a891e6031c64d2c30c/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/926af758f72041a891e6031c64d2c30c/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/926af758f72041a891e6031c64d2c30c/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/926af758f72041a891e6031c64d2c30c/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/926af758f72041a891e6031c64d2c30c/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/926af758f72041a891e6031c64d2c30c/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 257 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/926af758f72041a891e6031c64d2c30c/1/tools/checks.sh
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
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/926af758f72041a891e6031c64d2c30c/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "27bd585ca7eb429905ccbded1f48b4d0167c0c20", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  27bd585c  task-155  feat(task-155): Implement gyre search CLI command\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-s4gq_mp1/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Reproduction at assignment HEAD** (tree = base `27bd585ca7eb` + untracked
task file): `bash scripts/check-task-commit-attribution.sh` exited 1 listing
`27bd585c task-155 feat(task-155): Implement gyre search CLI command` — a
product-surface commit (+837 lines across `crates/gyre-cli/src/client.rs`,
`crates/gyre-cli/src/main.rs`, `docs/cli.md`) missing from
`specs/tasks/task-155.md`'s `commits:` frontmatter. Evidence:
`/tmp/stage/review-evidence/task-235-attribution-before.txt` (exit 1,
identical violation text).

**Root cause — squash-drift class (tasks 213/219/222/224/227/228 precedent):**
task-155's frontmatter recorded seven pipeline-branch SHAs (`4c0df440` gate
repair, `2b6f3372` feature commit, `1292303a`/`951037f8` checkpoint-recover,
`0196a149`/`4e5b20d2`/`6bc9a54d` wip) — none is an ancestor of HEAD (verified
with `git merge-base --is-ancestor` for each); the branch work was squashed
into the landing commit `27bd585c`, which cannot contain its own SHA at squash
time, so the recorded list stayed one entry short and the entire landed surface
was invisible to review scoping (task-095 R3-F4 flaw class). The landed tree is
content-identical to the recorded branch tip `4c0df440` on task-155's three
product files (verified: `git diff 27bd585c 4c0df440 -- crates/gyre-cli/src/
client.rs crates/gyre-cli/src/main.rs docs/cli.md` is empty, 0 lines). Evidence:
`/tmp/stage/review-evidence/task-235-drift-verification.txt`.

**Repair (1 line, `specs/tasks/task-155.md:8`):** appended the full SHA
`27bd585ca7eb429905ccbded1f48b4d0167c0c20` to task-155's `commits:`
frontmatter, joining the 7 SHAs already recorded. This is the check's own
documented remedy — same repair shape as task-227's `05709c24` (task-196
drift), task-224's `a11ba8d3` (task-068 drift), and task-219's `e96d25ab`
(task-200 drift). No exemptions added; `scripts/task-commit-attribution-
exemptions.txt` untouched at its frozen 3-entry baseline (`01493c88 task-097`,
`17c81d5a task-072`, `a8d036f4 task-091`); no check, skip, or gate weakened;
no Rust/JS source changed (`git diff 27bd585c` touches only `specs/tasks/`).

**Probe after repair:** exit 0 — `OK: every task-labeled product-surface commit
is recorded in its task's commits: frontmatter (or exempted legacy drift).`
Evidence: `/tmp/stage/review-evidence/task-235-attribution-after.txt`.

**Mutation check (test-the-repair):** with the repair present, removing the SHA
from the frontmatter re-fails the gate with the identical violation (exit 1),
and restoring it re-passes (exit 0) — the pass is attributable to the recorded
SHA, not gate drift. Evidence:
`/tmp/stage/review-evidence/task-235-mutation-check.txt` (mutated state),
`/tmp/stage/review-evidence/task-235-mutation-restore-check.txt` (restored).

**Attribution for this task:** `python3 scripts/dev-attribution.py task-235`
derives an empty list — the branch's only commit touches only `specs/tasks/`,
no product surface — so `commits: []` is attribution-canonical for this
specs-only repair round.
