# Review — task-212 (Repair verified failure on main 8c2d17750585)

Spec ref: `GOAL.md` — real implementations and meaningful verification. Assigned base `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f`, candidate `2dc3e7c8bddb0b9d42d1244cca99de4a19f61518` (verified `git rev-parse HEAD` == candidate). Evidence under `/tmp/stage/review-evidence/` (per-probe logs + `REVIEW-SUMMARY.md`).

Verdict: **complete** (approved).

## Round 1

### Scope of the delta

`git diff f4acb4eb..2dc3e7c8 --name-only` is exactly two files:
`specs/tasks/task-189.md` (one frontmatter line, +1/−1) and
`specs/tasks/task-212.md` (new, 82 lines — the task record itself).
`git diff f4acb4eb..2dc3e7c8 -- scripts/ crates/ web/` is empty. No check,
exemption, verifier, or product code changed. The repair class is exactly what
the task assigned: bookkeeping drift that fails the attribution gate, fixed by
recording the missing SHAs — not by weakening the gate (exemption file still
3 entries, byte-identical to base; `FROZEN_EXEMPTION_COUNT=3` untouched).

### Failure reproduction (independent)

- Pristine base `8c2d1775` (detached worktree): `bash scripts/check-task-commit-attribution.sh` → **exit 1**, FAIL naming `a781ede2 task-210` (`feat(task-210)`, touches `crates/gyre-server/src/api/admin.rs` + `web/src/App.svelte` — product surface, correctly in gate scope). Matches the task's Baseline failure verbatim (`gate-at-pristine-base-8c2d1775.log`).
- Assigned base `f4acb4eb` (detached worktree): **exit 1**, FAIL naming `f4acb4eb task-189` (`crates/gyre-server/src/api/personas.rs`) — the second squash-drift instance, visible only from the moved base (`gate-at-assigned-base-f4acb4eb.log`).
- Merged head `df2331f5` (pre-repair): **exit 1**, same `f4acb4eb task-189` violation (`gate-at-merged-head-df2331f5.log`).
- Candidate head `2dc3e7c8`: **exit 0, OK** — re-run again after all mutation probes on a pristine tree: still exit 0 (`gate-at-candidate-head.log`, `gate-final-pristine-head-rerun.log`).

### Mutation checks (pass attributable to the repair, not gate drift)

- Removing `f4acb4eb…720f` from task-189's frontmatter at the candidate head re-fails the gate with the identical violation; file restored (`mutation-task-189.log`, `restored: True`).
- Removing `, "a781ede2…0a093"` from task-210's frontmatter at the candidate head re-fails with the identical a781ede2 violation; restored (`mutation-task-210-at-candidate.log`).

### Structural verification

- **task-189 append is mechanical, not retyped**: programmatically compared frontmatter at base vs candidate — base 5 SHAs byte-identical prefix, exactly one appended SHA (`f4acb4ebcaf930ada2f1318b8aa2adbf244e720f`). The five pre-squash lineage SHAs remain recorded.
- **task-210 merge resolution is main's canonical form**: candidate's task-210.md is byte-identical to `f4acb4eb`'s copy (append-last, from reviewed ship commit `f38abb7e`, an ancestor of the candidate); no conflict markers survive in df2331f5's copy of either task file. The gate matches by set membership per SHA (`frontmatter_commits | grep -q "^$short$"`), so position is cosmetic; taking main's reviewed form is the conservative resolution.
- **Dead-branch precedent claim verified**: `f0068a0a` (task-200 round-4) is an ancestor of neither `8c2d1775` nor the candidate — the Shipped narrative's rejection of it as precedent is factually correct.
- **No gate weakened**: exemption file at frozen baseline (3 entries: 01493c88/17c81d5a/a8d036f4), unchanged from base; scripts diff empty; no new exemptions, skips, or check modifications.
- **Conventions**: `check-commit-msg.sh` passes on all three repair commits; `git diff --check` clean; `bash scripts/check-arch.sh` → OK.
- **task-212's own `commits: []` is consistent**: its three commits touch only `specs/tasks/`, outside the gate's product-surface filter (`crates/|web/src|web/tests`) — same as the reviewed task-211 precedent (`f38abb7e`, specs-only, `commits: []`).

### Notes for host verification

- This sandbox cannot accept TCP (`accept(): [Errno 95] Operation not supported`, `/tmp/stage/capabilities.json`). No server/browser probe is applicable regardless: the candidate changes no Rust/JS runtime surface (scripts/crates/web diff empty).
- Host CI must run `bash scripts/check-task-commit-attribution.sh` on the exact PR head with full history (fetch-depth: 0); it passes on full local history at the candidate head here.

No findings. The repair is the minimum-blast-radius correct fix for the failure class: both drifted squash SHAs (`a781ede2`, `f4acb4eb`) are now recorded in their task frontmatters, both recordings are load-bearing under mutation, and nothing else moved.
