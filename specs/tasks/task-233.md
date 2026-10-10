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
Environment fingerprint: `host-88e5982a8b65dfab050a3fd4ab0c79a6c71c3e6a87bc3e5a5b877d2e2419b262`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 257 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/tools/checks.sh
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
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "27bd585ca7eb429905ccbded1f48b4d0167c0c20", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  27bd585c  task-155  feat(task-155): Implement gyre search CLI command\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-xplcd8ns/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Reproduction:** at assignment HEAD (tree = base `27bd585c` + untracked task
file), `bash scripts/check-task-commit-attribution.sh` exited 1 listing
`27bd585c task-155 feat(task-155): Implement gyre search CLI command` — the
task-155 landing commit on main touches product surface (`crates/gyre-cli/src/
client.rs`, `crates/gyre-cli/src/main.rs`, `docs/cli.md`) but was absent from
`specs/tasks/task-155.md`'s `commits:` frontmatter. Root cause is the
squash-landing drift class recorded by tasks 213/219/222/224/228: the
published squashed commit cannot contain its own SHA, so the task's recorded
list stayed one entry short and the landing surface was invisible to review
scoping (task-095 R3-F4 flaw class). Evidence:
`/tmp/stage/review-evidence/task-233-attribution-before.txt` (exit 1).

**Repair (1 line, `specs/tasks/task-155.md:8`):** appended the full SHA
`27bd585ca7eb429905ccbded1f48b4d0167c0c20` to the existing 7-entry `commits:`
list. This is the check's own documented remedy — same repair shape as
task-224's `a11ba8d3` (task-068 drift) and task-228's `05709c24` (task-196
drift). No exemptions added: `scripts/task-commit-attribution-exemptions.txt`
untouched at its frozen 3-entry baseline; no check, skip, or gate weakened; no
Rust/JS source changed — `git diff 27bd585ca7eb429905ccbded1f48b4d0167c0c20
HEAD -- scripts/ crates/ web/src/` is empty; the working-tree delta vs base is
exactly this task file (untracked) + the one-line task-155 frontmatter fix.

**Probe after repair:** exit 0 — `OK: every task-labeled product-surface
commit is recorded in its task's commits: frontmatter (or exempted legacy
drift).` Evidence:
`/tmp/stage/review-evidence/task-233-attribution-after.txt`.

**Mutation check (test-the-repair):** repair present → exit 0; SHA removed
(sed mutation) → identical FAIL exit 1 (`27bd585c task-155 ...`); repair
restored → exit 0 again. The pass is attributable to the recorded SHA, not
gate drift. Evidence: `task-233-mutation-check.txt` (mutated, exit 1),
`task-233-mutation-restore-check.txt` (restored, exit 0).

**Attribution for this task:** `python3 /tmp/stage/dev-attribution.py
task-233` derives an empty list — this round changes only `specs/tasks/`
records, no product surface — so `commits: []` is attribution-canonical.

**Sandbox limitation (recorded, not a code defect):** this sandbox cannot run
TCP `accept()` (errno 95, Operation not supported), so no live server probe
was attempted or relevant — this repair touches no server surface. Full
workspace suites, architecture checks, and GitHub checks remain owned by the
verification and publication stages.
