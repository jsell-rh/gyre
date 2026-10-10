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
Environment fingerprint: `host-d787ee79d7a6292dcfc75d2e16291079241026c8cd9d980ff97543e10f5cb1c3`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c329b385df5a4ef0a922b29c742443ac/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c329b385df5a4ef0a922b29c742443ac/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c329b385df5a4ef0a922b29c742443ac/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c329b385df5a4ef0a922b29c742443ac/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c329b385df5a4ef0a922b29c742443ac/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c329b385df5a4ef0a922b29c742443ac/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c329b385df5a4ef0a922b29c742443ac/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c329b385df5a4ef0a922b29c742443ac/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c329b385df5a4ef0a922b29c742443ac/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c329b385df5a4ef0a922b29c742443ac/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c329b385df5a4ef0a922b29c742443ac/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/c329b385df5a4ef0a922b29c742443ac/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 4598 files, 16.2GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/c329b385df5a4ef0a922b29c742443ac/1/tools/checks.sh
rustfmt: changed lines clean (6 Rust files checked)
clippy: changed lines clean (6 Rust files, 343 existing warnings outside changes)
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
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/c329b385df5a4ef0a922b29c742443ac/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "27bd585ca7eb429905ccbded1f48b4d0167c0c20", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  27bd585c  task-155  feat(task-155): Implement gyre search CLI command\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-y0zhss3v/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Repair (1 line, `specs/tasks/task-155.md:8`):** appended the full SHA
`27bd585ca7eb429905ccbded1f48b4d0167c0c20` (short `27bd585c`) to task-155's
existing `commits:` frontmatter, preserving the 7 recorded candidate-lineage
SHAs (`4c0df440`..`6bc9a54d`). This is the check's own documented remedy, same
repair shape as task-228's `05709c24` (task-196 drift) and task-224's
`a11ba8d3` (task-068 drift): the squashed landing commit `27bd585c` (feat(task-155):
Implement gyre search CLI command, +937 lines across `crates/gyre-cli/src/main.rs`,
`crates/gyre-cli/src/client.rs`, `docs/cli.md`, task file) cannot contain its own
SHA, so the recorded list stayed one entry short and the landing surface was
invisible to review scoping (task-095 R3-F4 flaw class). Diff vs base is exactly
that one line; `git diff 27bd585ca7eb429905ccbded1f48b4d0167c0c20 -- scripts/
crates/ web/` is empty.

**Reproduction (before repair):** `bash scripts/check-task-commit-attribution.sh`
exit 1 with the baseline violation `27bd585c task-155 feat(task-155): Implement
gyre search CLI command`. Evidence:
`/tmp/stage/review-evidence/task-231-attribution-repro.txt` (exit=1).

**Probe after repair:** exit 0 — `OK: every task-labeled product-surface commit
is recorded in its task's commits: frontmatter (or exempted legacy drift).`
Evidence: `task-231-attribution-after-repair.txt` (exit=0) and, after the
mutation round, `task-231-attribution-restored.txt` (exit=0).

**Mutation check (test-the-repair):** repair present → exit 0; SHA stripped via
sed mutation → identical FAIL exit 1 (`27bd585c task-155 ...`); repair restored
→ exit 0. The pass is attributable to the recorded SHA, not gate drift.
Evidence: `task-231-attribution-mutation.txt` (mutated, exit=1) and
`task-231-attribution-restored.txt` (restored, exit=0).

**No weakening:** `scripts/task-commit-attribution-exemptions.txt` remains
frozen at its 3-entry baseline (`01493c88 task-097`, `17c81d5a task-072`,
`a8d036f4 task-091`); `git diff <base> -- scripts/task-commit-attribution-exemptions.txt`
is empty. No check, gate, skip, or exemption changed; no Rust/JS source changed;
the 7 pre-existing candidate-lineage SHAs in task-155's list are preserved
append-only (all 7 verified reachable in this clone via `git cat-file -t`).

**Attribution for this task:** `python3 scripts/dev-attribution.py task-231`
derives an empty list — this round's branch touches only `specs/tasks/`, no
product surface — so `commits: []` is attribution-canonical for this specs-only
repair round. (Note: `dev-attribution.py` rewrites the task file's `commits:`
line in place; it was run read-intent for task-231 only. The task-155 list was
restored to the 7+1 append-only form after an accidental invocation; the final
diff vs base is exactly the one-line append.)

**Contract-hash invariance proof:** `dev-contract.py requirement_parts()` maps
the task file (before vs after the `progress` flip and `## Shipped` append) to
identical `(front, prose)` — both are operational and excluded from the
requirement generation. Evidence:
`/tmp/stage/review-evidence/task-231-contract-hash-proof.txt`.
