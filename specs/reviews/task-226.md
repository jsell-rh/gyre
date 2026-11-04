# Review — task-226 (Repair verified failure on main a11ba8d32859)

Spec ref: `GOAL.md` — real implementations and meaningful verification.
Candidate under review: `834f128dd4c521f23f98e5ea541627f6346f71d1` (assignment base `58deeb75d0eb6f5b08a3baf4e7f1af67360c0f09`).
Verdict: **approved**.

## Scope

The verified upstream failure class is "task-labeled product-surface commit
absent from its task's `commits:` frontmatter" — `scripts/check-task-commit-attribution.sh`
exit 1 at base `a11ba8d3` (violation `a11ba8d3 task-068`), and, one link down
the chain, at `b1cdb0dd` (violation `b1cdb0dd task-170`). The candidate is the
round-3 merge of prerequisite repairs (task-229 `05709c24`→task-196,
task-240 `b1cdb0dd`→task-170) plus this task's round-1 repair
(`a11ba8d3`→task-068, identical to main's task-224).

## Independent probes (evidence under /tmp/stage/review-evidence/)

- **Reproduction at the exact failing bases** (detached worktrees, unmodified
  gate): `bash scripts/check-task-commit-attribution.sh` at `a11ba8d3` → exit 1,
  violation `a11ba8d3 task-068` (`gate-repro-at-a11ba8d3.txt`); at `b1cdb0dd` →
  exit 1, violation `b1cdb0dd task-170` (`gate-repro-at-b1cdb0dd.txt`).
- **Gate at candidate HEAD `834f128d`** → exit 0, "OK: every task-labeled
  product-surface commit is recorded …" (`gate-at-candidate-head-834f128d.txt`).
  Also exit 0 at assigned base `58deeb75` (`gate-at-assigned-base-58deeb75.txt`).
- **Test-the-repair mutations** (scratch worktree at `834f128d`, restored after
  each): removing `b1cdb0dd…` from `specs/tasks/task-170.md`,
  `a11ba8d3…` from `specs/tasks/task-068.md`, or `05709c24…` from
  `specs/tasks/task-196.md` each re-fails the gate (exit 1) with exactly that
  commit's own violation line; restoring each re-passes (exit 0). The pass at
  HEAD is attributable to the recorded SHAs, not gate drift
  (`mutation-remove-*.txt`, `gate-restore-check.txt`).
- **No weakening**: `git diff 58deeb75 HEAD -- scripts/ crates/ web/` is empty;
  the candidate's whole delta vs its assigned base is `specs/tasks/task-226.md`
  (task record only). The gate script and `scripts/task-c....txt` are
  byte-identical to the original failing base `a11ba8d3` (0 diff lines), and no
  `check-*` script or exemption file changed between `b1cdb0dd` and `58deeb75`.
  `scripts/task-c....txt` remains at its frozen 3-entry baseline
  (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`).
- **`commits: []` is attribution-canonical for this task**: every non-merge
  task-226-labeled commit in history (`834f128d`, `fcb5e59d`, `607b8b2f`, plus
  review/process checkpoints) touches only `specs/` files — no product surface,
  so nothing requires recording.

## Findings

None. The failure was reproduced at both failing bases with the unmodified
gate, the repair is real (recorded SHAs in frontmatter, proven by per-SHA
mutation kills), and no check, exemption, skip, or product surface was touched
by this candidate.

Transport note: the TCP `accept()` listener probe is unsupported in this
sandbox (`/tmp/stage/capabilities.json`); no runtime surface changed, so no
HTTP probe applies. Full workspace suites and GitHub checks on the exact PR
head belong to the verifier.
