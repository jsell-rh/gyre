---
title: "Repair verified failure on main 27bd585ca7eb"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `27bd585ca7eb429905ccbded1f48b4d0167c0c20`
Environment fingerprint: `host-a1f156acf46bf420f2804fa7b59a00f631ea70e1b6d044d563e0b29cdf5dde8c`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3eaf9a9827014612af0d338c6ff2f84b/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3eaf9a9827014612af0d338c6ff2f84b/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3eaf9a9827014612af0d338c6ff2f84b/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3eaf9a9827014612af0d338c6ff2f84b/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3eaf9a9827014612af0d338c6ff2f84b/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3eaf9a9827014612af0d338c6ff2f84b/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3eaf9a9827014612af0d338c6ff2f84b/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3eaf9a9827014612af0d338c6ff2f84b/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3eaf9a9827014612af0d338c6ff2f84b/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3eaf9a9827014612af0d338c6ff2f84b/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3eaf9a9827014612af0d338c6ff2f84b/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3eaf9a9827014612af0d338c6ff2f84b/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 263 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/3eaf9a9827014612af0d338c6ff2f84b/1/tools/checks.sh
rustfmt: changed lines clean (16 Rust files checked)
clippy: changed lines clean (16 Rust files, 343 existing warnings outside changes)
new verification exemptions forbidden: scripts/scope-literal-defaults-exemptions.txt: ['crates/gyre-server/src/lib.rs:494', 'crates/gyre-server/src/lib.rs:538']
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
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/3eaf9a9827014612af0d338c6ff2f84b/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "27bd585ca7eb429905ccbded1f48b4d0167c0c20", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  27bd585c  task-155  feat(task-155): Implement gyre search CLI command\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-o85l4z43/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Reproduction:** `bash scripts/check-task-commit-attribution.sh` at assignment
HEAD exits 1 with the exact recorded violation — `27bd585c task-155
feat(task-155): Implement gyre search CLI command`. Evidence:
`/tmp/stage/review-evidence/task-236-attribution-repro.txt` (exit=1).

**Root cause:** squash-drift class (same flaw class as the task-228/224/219
repairs). Task-155's landing commit `27bd585c` touches product surface
(`crates/gyre-cli/src/client.rs`, `crates/gyre-cli/src/main.rs`) and is labeled
task-155, but none of the 7 SHAs recorded in `specs/tasks/task-155.md`'s
`commits:` frontmatter is an ancestor of HEAD — they are pipeline-branch
commits (subjects `feat(cli)`, `wip(task-155)`, `fix(task-155)`,
`checkpoint`) squashed into the landing commit, which cannot contain its own
SHA at squash time. The landing surface was therefore invisible to review
scoping (task-095 R3-F4 flaw class).

**Repair (1 line, `specs/tasks/task-155.md:8`):** appended the full SHA
`27bd585ca7eb429905ccbded1f48b4d0167c0c20` to the existing `commits:` list,
preserving the 7 branch SHAs. The check's own documented remedy — same repair
shape as task-228's `05709c24` (task-196 drift), task-224's `a11ba8d3`
(task-068 drift), and task-219's `e96d25ab` (task-200 drift). No exemptions
added — `scripts/task-commit-attribution-exemptions.txt` frozen at its
3-entry baseline; no gate, skip, or check weakened; no Rust/JS source
changed (branch delta vs base is `specs/tasks/` only).

**Probe after repair:** exit 0 — `OK: every task-labeled product-surface
commit is recorded in its task's commits: frontmatter (or exempted legacy
drift).` Evidence: `/tmp/stage/review-evidence/task-236-attribution-after-repair.txt`.

**Mutation check (test-the-repair):** repair present → exit 0; SHA removed
(sed mutation) → identical FAIL exit 1; repair restored → exit 0 again. The
pass is attributable to the recorded SHA, not gate drift. Evidence:
`/tmp/stage/review-evidence/task-236-mutation-check.txt` (mutated state).

**Not a second live failure (investigated, no repair needed):** the baseline
log line `new verification exemptions forbidden:
scripts/scope-literal-defaults-exemptions.txt: ['crates/gyre-server/src/lib.rs:494',
'crates/gyre-server/src/lib.rs:538']` does not reproduce on main's history:
`git diff 06d70009 27bd585c -- scripts/scope-literal-defaults-exemptions.txt`
is empty (the file is identical at base and its parent), and the entries
exist only on parallel pipeline branches (task-146/128/160 checkpoint
states, verified `NOT ancestor of HEAD` for each). The gate itself passes on
HEAD: `bash scripts/check-scope-literal-defaults.sh` → exit 0. The
GYRE_BASELINE_FAILURE_JSON records only the attribution probe; this
exemption-growth line fired from the baseline attempt checkout's own HEAD
vs HEAD^1 (a task-146 re-pin state), not from main.

**Prior parallel attempt (context, superseded by this task):** commit
`74bf94b1` (task-235, branch
`pipeline/task-235/872fddc12e8042bc9f32f280625ae887-1`) performed the
identical repair but is `NOT ancestor of HEAD` — review pending, never
landed on main. This task re-issues the repair for task-236.

**Attribution for this task:** `python3 /tmp/stage/dev-attribution.py
task-236` derives an empty list — this round's only delta is
`specs/tasks/task-155.md` (1 line) + this task file, no product surface — so
`commits: []` is attribution-canonical for this specs-only repair round.
