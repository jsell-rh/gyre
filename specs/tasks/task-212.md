---
title: "Repair verified failure on main 8c2d17750585"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `8c2d177505852b3e39cd77f4f782fb355de245aa`
Environment fingerprint: `host-069b583bce46cd99f63342efb1a53f02b74eb8f61e943f214ae36f009df00abf`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/8188650c99fc4b0f8414ac9a9b11f780/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/8188650c99fc4b0f8414ac9a9b11f780/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/8188650c99fc4b0f8414ac9a9b11f780/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/8188650c99fc4b0f8414ac9a9b11f780/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/8188650c99fc4b0f8414ac9a9b11f780/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/8188650c99fc4b0f8414ac9a9b11f780/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/8188650c99fc4b0f8414ac9a9b11f780/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/8188650c99fc4b0f8414ac9a9b11f780/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/8188650c99fc4b0f8414ac9a9b11f780/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/8188650c99fc4b0f8414ac9a9b11f780/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/8188650c99fc4b0f8414ac9a9b11f780/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/8188650c99fc4b0f8414ac9a9b11f780/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 261 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/8188650c99fc4b0f8414ac9a9b11f780/1/tools/checks.sh
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

  a781ede2  task-210  feat(task-210): Repair verified failure on main cd1c5f044e49

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/8188650c99fc4b0f8414ac9a9b11f780/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "8c2d177505852b3e39cd77f4f782fb355de245aa", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  a781ede2  task-210  feat(task-210): Repair verified failure on main cd1c5f044e49\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-j54jnukl/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Failure reproduced on the pristine base.** `bash scripts/check-task-commit-attribution.sh` at `8c2d177505852b3e39cd77f4f782fb355de245aa` exits 1: task-210's product-surface squash commit on main, `a781ede2` (`feat(task-210)`, touches `crates/gyre-server/src/api/admin.rs` + `web/src`), is absent from `specs/tasks/task-210.md`'s `commits:` frontmatter.

**Root cause.** The squash landed on main's first-parent line with its frontmatter repointed at the pre-squash attempt chain: the 8 recorded SHAs (`96b50773`…`cefb7c6e`) resolve only in `origin/pipeline/task-210/*` refs, unreachable from main. The reviewer's scoping list therefore omits the one commit that actually shipped the task's surface — exactly the task-095 R3-F4 failure class the gate exists to catch. Ten prior repair rounds recorded `a781ede2` on branches that were never adopted, so the drift kept re-surfacing with every new baseline.

**Repair (per the gate's own prescription and the reviewed task-200 round-4 precedent `f0068a0a`).** Added `"a781ede2ea21a5153dbcb49c65771f990344a093"` to task-210's `commits:` frontmatter, first position. The 8 attempt-chain SHAs are left intact — they are that task's reviewed lineage, resolvable in pipeline refs, and pruning another task's lineage is not this task's call. `scripts/task-commit-attribution-exemptions.txt` untouched (still frozen at 3 entries; no exemption added). No product surface changed: `git diff 8c2d1775..HEAD -- crates/ web/` is empty.

**Verification.**

- `bash scripts/check-task-commit-attribution.sh` at base → FAIL (exit 1, naming `a781ede2 task-210`); after repair → **OK, exit 0**. Evidence: `/tmp/stage/review-evidence/task-212-before-repair.log` and `task-212-after-repair.log`.
- Exemption file entry count before and after: 3 (frozen baseline).
- `git diff --check 8c2d1775..HEAD` clean; `scripts/check-commit-msg.sh` passes on this commit's message.
- Mutation check of the gate itself (test-the-test): on the repaired tree, removing the recorded SHA from the frontmatter makes the check fail again with the same violation — the pass is attributable to the repair, not to gate drift. Evidence: `/tmp/stage/review-evidence/task-212-mutation-check.log`.
- This sandbox cannot accept TCP (`accept(): [Errno 95] Operation not supported`, recorded in `/tmp/stage/capabilities.json`); live-HTTP and exact-head GitHub checks belong to host verification. No Rust/JS source changed, so no runtime surface was affected.

Independent review and full verification (GitHub CI on the exact PR head) decide approval.
