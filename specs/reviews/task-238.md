# Review — task-238 (Repair verified failure on main 27bd585ca7eb)

Spec: GOAL.md — real implementations and meaningful verification.
Candidate under review: `fb044c3767fe8ad183be8a519c259b0c7ca0f03f` (base `27bd585ca7eb429905ccbded1f48b4d0167c0c20`).
Verdict: **approved**.

## Scope of the change

`git diff base..candidate` is exactly two files:

- `specs/tasks/task-155.md` — one line: the landing commit SHA `27bd585ca7eb429905ccbded1f48b4d0167c0c20` appended to the existing `commits:` frontmatter array (the 7 prior branch SHAs preserved byte-identical).
- `specs/tasks/task-238.md` — the task file itself (contract, baseline log, Shipped narrative).

`git diff base..candidate -- scripts/ crates/ web/` is empty — no product source, no gate scripts, no checks, no exemptions touched.

## Independent verification

All probes were run by this reviewer from clean worktrees; evidence under `/tmp/stage/review-evidence/`.

1. **Base reproduction** (clean worktree at `27bd585c`): `bash scripts/check-task-commit-attribution.sh` → exit 1 with exactly the recorded violation: `27bd585c task-155 feat(task-155): Implement gyre search CLI command`. This matches the sole failing probe in the baseline `GYRE_BASELINE_FAILURE_JSON` embedded in the task file. (task-238-repro-base.txt)
2. **Repair effect** (candidate tree): same check → exit 0, `OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift).` (task-238-attribution-after-repair.txt)
3. **Mutation (test-the-repair)**: landing SHA replaced via sed in a scratch worktree → identical FAIL exit 1; restored → exit 0. The pass is attributable to the recorded SHA, not gate drift. (task-238-mutated.txt)
4. **No weakening**: exemptions file unchanged at its frozen 3-entry baseline (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`); no new exemption entries; the check script is untouched.
5. **Full static gate suite** on the candidate: all 21 gates exit 0 (arch, hierarchy, abac-route-registry, abac-exempt-handlers, mcp-write-tools, migration-versions, migration-sql-portability, dead-message-kinds, byte-slice-truncation, relative-path-defaults, fail-open-ref-resolution, task-commit-attribution, mem-port-contracts, fabricated-scope-defaults, lossy-secret-conversion, scope-literal-defaults, inert-enforcement, forged-scope-fields, forwarded-header-trust, in-memory-state-stores, unbounded-external-http). (task-238-check-*.txt)
6. **Diff-scoped lint gates vs base**: `check-rustfmt-diff.py` → exit 0 (0 Rust files changed); `check-clippy-diff.py` → exit 0 (changed lines clean; 1141 existing warnings outside changes). (task-238-rustfmt-diff-gate.txt, task-238-clippy-diff-gate.txt)
7. **Frontmatter integrity**: the check's own awk parser extracts all 8 short SHAs from the single 361-char `commits:` line; all 8 exist as commits; the 7 branch SHAs are not ancestors of main (squash-merge), so removing them (as `dev-attribution.py` would, since it rewrites rather than appends — independently confirmed by running it in a scratch worktree, restored afterward) would erase recorded review-scoping history. Appending is the correct repair and matches the approved task-228 precedent (task-196 drift, identical flaw class and remedy).

## Baseline-log lint lines (not main-state failures — corroborated)

The baseline log's `explorer_ws_integration.rs` rustfmt (lines 464,465,466,469,471) and clippy (`:412 useless_conversion`) entries reference file states that do not exist at base: the file is 420 lines at `27bd585c`; only unmerged pipeline branches for task-069 (498 lines) and task-077 (422 lines) carry those states, and the quoted rustfmt-diff shape reproduces from the 498-line variant. The diff-scoped gates at the candidate compare base..HEAD and are empty. The machine-readable baseline failure JSON names exactly one failing probe — the attribution check repaired here. The current-stable-clippy `never_loop` at `rust_extractor.rs:1071` is pre-existing main debt (introduced in `d7940e85`, zero diff vs base, warning-level under the CI invocation `-W clippy::all`).

## Transport note

No server/browser probe is applicable: the diff is specs-only, and the sandbox does not support TCP listener accept (capabilities.json). Static-gate probes above are the complete verification surface for this change.

## Conclusion

The repair is the check's own documented remedy, minimal (one frontmatter line), precedent-consistent, with no weakened gates, no exemptions added, and no product-source changes. Reproduction, repair, and mutation checks all behave as claimed. Approved.
