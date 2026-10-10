---
title: "Repair verified failure on main 8c2d17750585"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: [task-215, task-219]
progress: complete
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `8c2d177505852b3e39cd77f4f782fb355de245aa`
Environment fingerprint: `host-5f73a16d4d80d5d4e910d257d6cbd16b98e75a915f9ad11e516a4d9e6c3957fc`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d9546b22f48c436e8fd19da2537110fb/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d9546b22f48c436e8fd19da2537110fb/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d9546b22f48c436e8fd19da2537110fb/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d9546b22f48c436e8fd19da2537110fb/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d9546b22f48c436e8fd19da2537110fb/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d9546b22f48c436e8fd19da2537110fb/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d9546b22f48c436e8fd19da2537110fb/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d9546b22f48c436e8fd19da2537110fb/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d9546b22f48c436e8fd19da2537110fb/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d9546b22f48c436e8fd19da2537110fb/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d9546b22f48c436e8fd19da2537110fb/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d9546b22f48c436e8fd19da2537110fb/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 4598 files, 16.2GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/d9546b22f48c436e8fd19da2537110fb/1/tools/checks.sh
rustfmt: crates/gyre-server/src/api/graph.rs: changed lines need formatting: 1204, 1301, 1302, 1303, 1308, 1309, 2160, 2161, 2162, 2308, 2309
clippy: changed lines clean (1 Rust files, 347 existing warnings outside changes)
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
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/d9546b22f48c436e8fd19da2537110fb/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "8c2d177505852b3e39cd77f4f782fb355de245aa", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  a781ede2  task-210  feat(task-210): Repair verified failure on main cd1c5f044e49\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-jyqk4di4/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

Reproduced the failure at base `8c2d1775` on this branch: `bash scripts/check-task-commit-attribution.sh` exited 1 naming `a781ede2  task-210  feat(task-210): Repair verified failure on main cd1c5f044e49`.

**Root cause.** Commit `a781ede2` is a product-surface commit (touches `crates/gyre-server/src/api/admin.rs`, `web/src/**`) labeled `task-210`, and the commit that created `specs/tasks/task-210.md` listed its 8 branch SHAs in `commits:` but never recorded itself. The 14 prior repairs recording it (8d28d65e, 6ae8207f, 63d66b46, a3530260, 4ef94106, e1e16020, b97aa290, 69a63669, 4dd13430, 04ce9194, 7523084c, dd9a7159, ce96eb64, 60b8b2fb) all landed on checkpoint branches that were never merged into main's lineage — each fixed a copy of the file, never the lineage the gate scans. Verified: `git merge-base --is-ancestor` shows none of the 14 are ancestors of HEAD.

**Repair (per the check's documented remedy).** Appended the full SHA `a781ede2ea21a5153dbcb49c65771f990344a093` to `specs/tasks/task-210.md`'s `commits:` frontmatter. No exemptions added; the frozen exemption file stays at its 3 committed entries.

**Test evidence.**
- Before: probe FAIL exit 1 (violation listing `a781ede2 task-210`) — `/tmp/stage/review-evidence/attribution-before.txt`.
- After: probe OK exit 0 — `/tmp/stage/review-evidence/attribution-after.txt` and `attribution-after-commit.txt` (re-run after committing the repair, so the check scans the new task-labeled commit itself and still passes).
- `git diff --check 8c2d1775 HEAD` clean.

Sandbox transport probe unsupported (`accept` errno 95) per `/tmp/stage/capabilities.json` — no server/browser probes attempted; full verification and GitHub checks remain with the independent reviewer and the deterministic gates. Round-2 scope below extends, not replaces, this constraint.

**Round 2 (base moved to `19d65446`; repair prerequisite task-215).** The verified failure drifted with main: at the round-2 repair base `f4acb4eb` (main's task-189 squash, which touches `crates/gyre-server/src/api/personas.rs`), the gate fails again, now naming `f4acb4eb  task-189  feat(task-189): Fix persona scope resolution to walk the real parent chain` -- the ship commit was absent from `specs/tasks/task-189.md`'s `commits:` frontmatter (same squash-drift class as the round-1 `a781ede2`/task-210 drift). Main repaired this independently as task-215 (commit `19d65446`: appended the full SHA `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f` to task-189's frontmatter, keeping the five pre-squash lineage SHAs as the review-scoping record). This branch carries that repair in its lineage: merge `81f8c910` brings `19d65446` into the round-1 branch, and `depends_on: [task-215]` records the prerequisite. No additional repair was needed on top of it: the branch delta vs `19d65446` is specs-only, so both drift instances (`a781ede2`/task-210, `f4acb4eb`/task-189) are cleared in this lineage, and the frozen exemption file stays at its 3 committed entries.

**Round 2 verification (evidence under `/tmp/stage/review-evidence/`).**
- Before: gate at pristine `f4acb4eb` in a detached worktree → FAIL exit 1, the exact round-2 baseline violation (`f4acb4eb task-189`) -- `task-213-round2-before-repair.log`.
- After: gate at merged HEAD `81f8c910` (before this record commit) → OK exit 0 -- `task-213-round2-attribution-after.txt`; head and base recorded in `task-213-round2-head.txt` / `task-213-round2-base.txt`; post-commit re-run saved as `task-213-round2-after-record-commit.txt`.
- Mutation check (test-the-test): in a detached worktree at HEAD, removing `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f` from task-189's frontmatter re-fails the gate with the identical violation, exit 1 -- `task-213-round2-mutation-check.log`. The pass is attributable to the prerequisite repair, not gate drift.
- Integrity: task-189 frontmatter parses; `commits` is a 6-entry JSON list containing the full repair SHA; task-210's list still carries `a781ede2ea21a5153dbcb49c65771f990344a093` (9 entries). Exemption file at its frozen 3 entries before and after. `git diff --check 19d65446..HEAD` clean; `git diff 19d65446..HEAD` touches only `specs/tasks/task-213.md`; no product-surface commit exists since base (`git log 19d65446..HEAD --no-merges -- crates web` empty). `python3 /tmp/stage/dev-attribution.py task-213` produces no change, so `commits: []` remains correct for this specs-only task.

Sandbox transport probe unsupported (`accept` errno 95) per `/tmp/stage/capabilities.json`; no server/browser probes apply to this specs-only change. Full deterministic gates and exact-head GitHub checks remain with the independent reviewer, verification, and publication.

**Round 3 (base moved to `6bf777a6`; repair prerequisite task-219).** The verified failure drifted with main again: at the round-3 assignment base, the gate fails naming `6bf777a6  task-200  feat(task-200): Message bus — per-kind payload schema validation (reject invalid payloads with 400)` — the squashed task-200 ship commit (touches `crates/gyre-common/src/message.rs`, `crates/gyre-server/src/api/messages.rs`, `crates/gyre-server/src/mcp.rs`) was absent from `specs/tasks/task-200.md`'s `commits:` frontmatter (same squash-drift class as rounds 1 and 2: the squashed landing commit cannot contain its own SHA). Main repaired this independently as task-219 (commit `e96d25ab`: appended the full SHA `6bf777a6a44f28052ed5af28bf6fb013fde6df48` to task-200's frontmatter, joining the 13 SHAs already recorded from the task's branch). This branch carries that repair in its lineage via merge `e203e764`, and `depends_on: [task-215, task-219]` records both prerequisites. No additional repair was needed on top of it: the branch delta vs `6bf777a6` is specs-only, so all three drift instances (`a781ede2`/task-210, `f4acb4eb`/task-189, `6bf777a6`/task-200) are cleared in this lineage, and the frozen exemption file stays at its 3 committed entries.

**Round 3 verification (evidence under `/tmp/stage/review-evidence/`).**
- After: gate at merged HEAD → OK exit 0 — `task-213-round3-attribution-after.txt`; head and base recorded in `task-213-round3-head.txt` / `task-213-round3-base.txt`; post-commit re-run saved as `task-213-round3-after-record-commit.txt`.
- Mutation check (test-the-test): in a detached worktree at HEAD, removing `6bf777a6a44f28052ed5af28bf6fb013fde6df48` from task-200's frontmatter re-fails the gate with the identical violation (`6bf777a6 task-200`), exit 1 -- `task-213-round3-mutation-check.txt`. The pass is attributable to the prerequisite repair, not gate drift. (The first mutation attempt was a no-op — the sed pattern assumed a trailing comma that the final-list-entry position does not have — corrected and re-run; the recorded log is the effective mutation.)
- Integrity: task-200 frontmatter parses; `commits` is a 14-entry JSON list whose final entry is the full repair SHA; task-189 and task-210 lists still carry their repair SHAs. Exemption file at its frozen 3 entries before and after. `git diff --check 6bf777a6..HEAD` clean; `git diff 6bf777a6..HEAD` touches only `specs/tasks/`; no product-surface commit exists since base (`git log 6bf777a6..HEAD --no-merges -- crates web` empty). `python3 /tmp/stage/dev-attribution.py task-213` produces no change, so `commits: []` remains correct for this specs-only task.

Sandbox transport probe unsupported (`accept` errno 95) per `/tmp/stage/capabilities.json`; no server/browser probes apply to this specs-only change. Full deterministic gates, full workspace suites, all-target Clippy, and exact-head GitHub checks remain with the independent reviewer, verification, and publication.
