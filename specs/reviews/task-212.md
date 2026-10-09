# Review — task-212 (Repair verified failure on main 8c2d17750585)

Task: `specs/tasks/task-212.md` — repair the verified upstream failure of `scripts/check-task-commit-attribution.sh` at base `8c2d177505852b3e39cd77f4f782fb355de245aa`.
Commit under review: `218ca7771ff6376333f86691d5f91910df7b4579` (`fix(task-212): record task-210 squash a781ede2 in commits frontmatter`), direct child of the assigned base.
Verdict: **approved**.

## Round 1

All probes run independently in this sandbox; implementer claims were not taken as evidence. Evidence under `/tmp/stage/review-evidence/`.

### Diff scope (base..candidate)

Exactly two files, both specs:

- `specs/tasks/task-210.md` — `"a781ede2ea21a5153dbcb49c65771f990344a093"` prepended to `commits:` frontmatter (first position, matching the task-200 round-4 precedent `f0068a0a`, verified in history).
- `specs/tasks/task-212.md` — new task record (baseline failure log, shipped narrative, verification).

`git diff 8c2d1775..218ca777 -- crates/ web/` is empty; `git diff --check` clean. No production surface touched.

### Probes

1. **Baseline reproduction** (isolated worktree at base, `/tmp/gyre-base-review`): `bash scripts/check-task-commit-attribution.sh` → **exit 1**, `FAIL: ... a781ede2  task-210  feat(task-210): Repair verified failure on main cd1c5f044e49` — exact match to the assigned baseline log. Evidence: `review-base-gate-fail.log`.
2. **Candidate gate**: at `218ca777` → **exit 0**, `OK: every task-labeled product-surface commit is recorded...`. Evidence: `review-candidate-gate.log`.
3. **Isolation probe**: applied only the candidate's `specs/tasks/task-210.md` onto base history (worktree) → gate **exit 0**. The pass is attributable to the single frontmatter change alone, nothing else in the candidate. Evidence: `review-candidate-file-at-base.log`. Worktree restored and removed.
4. **Mutation check (test-the-test)**: on the repaired tree, removed the recorded SHA from the frontmatter → gate **exit 1** with the identical violation. The gate is sensitive; the pass is not gate drift or a self-confirming check. Evidence: `review-mutation-check.log`.
5. **No weakening**: `git diff base..candidate -- scripts/` empty; `scripts/task-commit-attribution-exemptions.txt` byte-identical base↔candidate (md5 `9fe9c0ad...` both), still 3 entries = `FROZEN_EXEMPTION_COUNT`. No exemption, skip, or gate change.
6. **SHA validity**: `a781ede2ea21a5153dbcb49c65771f990344a093` resolves, is an ancestor of the candidate and of `origin/main`, and is genuinely a task-210-labeled product-surface commit (touches `crates/gyre-server/src/api/admin.rs`, `web/src`, `web/tests`, 31 files). The 8 pre-existing attempt-chain SHAs also resolve (in pipeline refs) — leaving them intact preserves task-210's reviewed lineage; pruning them was correctly out of scope.
7. **Commit message**: `bash scripts/check-commit-msg.sh` on the candidate's message → passed.
8. **Attribution tooling**: `python3 scripts/dev-attribution.py task-212` → exit 0, no change. The fix commit itself mentions `task-210` in its subject but touches only `specs/`, so the gate's product-surface filter correctly ignores it; `commits: []` in task-212's frontmatter is consistent with the gate's design and the adopted task-211 precedent on main.
9. **Sibling-race context (not a defect)**: `origin/main` has since adopted `f38abb7e` (task-211), the same repair with the SHA appended last instead of first. Both forms pass the gate (set-membership match); this is pipeline adoption racing, not a candidate flaw against its assigned base.

### Findings

None. The repair is the gate's own documented remedy (its error message prescribes exactly this fix), implemented without weakening any check, with the failure reproduced at base and the fix isolated and mutation-tested. All other checks in the baseline log were green at base and read no file this candidate touches.

Required host/CI verification: `bash scripts/check-task-commit-attribution.sh` on the exact PR head with full git history (fetch-depth: 0), plus standard GitHub checks. No Rust/JS runtime surface changed, so no server or browser probe is warranted (TCP listener unsupported in this sandbox per `/tmp/stage/capabilities.json`; irrelevant to a specs-only diff).
