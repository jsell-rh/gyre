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

**Rebase round (base moved to `f4acb4eb`).** The candidate's original repair put `a781ede2` first in task-210's list, citing task-200's round-4 commit `f0068a0a` — verified this round to be a dead-branch precedent: `git merge-base --is-ancestor f0068a0a 8c2d1775` fails; it never landed on main. Meanwhile main adopted the reviewed task-211 repair (ship commit `f38abb7e`), which appends the SHA last. The rebase conflicted on `specs/tasks/task-210.md` (both sides added the same SHA, different positions). Resolved by taking main's canonical form — the file is now byte-identical to `f4acb4eb`'s copy (`git diff f4acb4eb -- specs/tasks/task-210.md` empty): the gate checks set membership per SHA (`frontmatter_commits | grep -q "^$short$"`), so both positions satisfy it identically; main's reviewed form wins over the unreviewed first-position variant. The 8 attempt-chain SHAs are left intact — they are that task's reviewed lineage, resolvable in pipeline refs.

**Second instance of the same failure class, repaired.** The rebase exposed drift the old base could not see: at the merged head `df2331f5` the gate failed again — `f4acb4eb task-189 feat(task-189): Fix persona scope resolution to walk the real parent chain` — because main's newest commit is task-189's upstream squash (touches `crates/gyre-server/src/api/personas.rs`) while `specs/tasks/task-189.md`'s `commits:` frontmatter listed only the five pre-squash attempt-chain SHAs (`b36fad00`…`2d1e74d9`, all unreachable from main). Same squash-drift class as `a781ede2`/task-210. Repair: appended the full SHA `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f` to task-189's `commits:` frontmatter (commit `17203efc`), leaving the five lineage SHAs intact. Frontmatter verified programmatically: the 5 base SHAs are byte-identical and exactly one SHA is appended (`base_shas == cur[:5]` and `cur[5:] == [f4acb4eb…]`, python check) — a hand-retype of the 40-hex line was caught corrupting `a977a917` and replaced by a mechanical append to the base blob.

**No gate weakened.** `scripts/task-commit-attribution-exemptions.txt` untouched, still frozen at 3 entries (verified before/after). No check, skip, or exemption modified; `scripts/check-task-commit-attribution.sh` is byte-identical to base (`git diff f4acb4eb..HEAD -- scripts/` empty). No product surface changed: `git diff f4acb4eb..HEAD -- crates/ web/` is empty; the branch delta is `specs/tasks/task-212.md` (this record) plus the one-line task-189 frontmatter append.

**Verification (evidence under `/tmp/stage/review-evidence/`).**

- `bash scripts/check-task-commit-attribution.sh` at pristine base `8c2d1775` → FAIL (exit 1, naming `a781ede2 task-210`); at merged head `df2331f5` → FAIL (exit 1, naming `f4acb4eb task-189`); at every post-repair head → **OK, exit 0**. Logs: `task-212-before-repair.log` (base), `task-212-round2-before-repair.log` (= `task-212-rebase-after-merge.log`, merged head), `task-212-round2-after-repair.log` (`17203efc`), `task-212-final-head-gate.log` (final head, recorded in `task-212-final-head.txt`).
- Mutation checks (test-the-test): with the repair present, removing the recorded SHA re-fails the gate with the identical violation. Round 1: at a detached worktree of base `8c2d1775` with the task-210 repair applied (gate OK exit 0), removing `a781ede2` re-fails (exit 1, identical violation) — `task-212-mutation-check.log`. Round 2: at the merged head `df2331f5` with the task-189 repair present in the working tree (gate OK exit 0; subsequently committed as `17203efc`), removing `f4acb4eb` re-fails (exit 1, identical violation) — `task-212-round2-mutation-check.log`. The passes are attributable to the repairs, not gate drift.
- Exemption file entry count before and after: 3 (frozen baseline).
- `git diff --check f4acb4eb..HEAD` clean; `scripts/check-commit-msg.sh` passes on both repair commits' messages.
- This sandbox cannot accept TCP (`accept(): [Errno 95] Operation not supported`, recorded in `/tmp/stage/capabilities.json`); live-HTTP and exact-head GitHub checks belong to host verification. No Rust/JS source changed, so no runtime surface was affected.

Independent review and full verification (GitHub CI on the exact PR head) decide approval.
