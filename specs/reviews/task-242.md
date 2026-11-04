# Review — task-242 (Repair verified failure on main b1cdb0ddd80e)

Spec: task-242 (`specs/tasks/task-242.md`) — reproduce and repair the verified upstream failure of `scripts/check-task-commit-attribution.sh` at base `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` without weakening any check, skip, exemption, or gate.
Candidate under review: `352e120f57a84933563d718f7114d29bb2f4da37` (base `b1cdb0dd`).
Verdict: **approved**.

## Change surface

`git diff b1cdb0dd 352e120f` touches exactly two files:

- `specs/tasks/task-170.md` (1 line): appends the full SHA `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` to the existing 11-SHA `commits:` frontmatter array. The 11 pre-existing branch SHAs are kept unchanged (diff is a pure append). This is the check's own documented remedy ("Fix by adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter").
- `specs/tasks/task-242.md` (new, 83 lines): the task record itself (frontmatter + baseline log + shipped narrative). `commits: []` is correct — the candidate commit touches no product surface, only `specs/tasks/`.

No production code, scripts, verifiers, or exemption files changed: `git diff b1cdb0dd 352e120f -- scripts/ crates/ web/` is empty.

## Independent evidence (all under `/tmp/stage/review-evidence/`)

- **Baseline reproduction at base** (`task-242-review-gate-at-base.txt`): in a detached worktree at `b1cdb0dd`, `bash scripts/check-task-commit-attribution.sh` exits 1 with exactly the reported violation — `b1cdb0dd task-170 feat(task-170): View Specification Grammar — TypeScript types and server-side validation`. The failure is real at the assigned base, not a stale report.
- **Verified the violation is genuine product surface**: `git show --name-only b1cdb0dd` touches `crates/gyre-common/src/view_spec.rs`, `crates/gyre-server/src/api/explorer_views.rs`, `.gitattributes`, plus web/dist and task/spec records — the squash-drift class (the squashed landing commit cannot contain its own SHA), same shape as the task-213/219/229 repairs.
- **Gate passes at candidate** (`task-242-review-gate-at-candidate.txt`): at `352e120f` in the review checkout, the gate exits 0 — `OK: every task-labeled product-surface commit is recorded ...`. The gate scans full history including the candidate commit itself; `implement(task-242)` touches only `specs/tasks/`, so it correctly does not trip attribution.
- **Mutation check** (`task-242-review-mutation.txt`): in an isolated worktree at `352e120f` with only `specs/tasks/task-170.md` reverted to the base version, the gate re-fails with the identical violation (exit 1); restoring the file re-passes. The pass is attributable to the recorded SHA, not to gate drift or the added task file.
- **No weakening**: exemption file (`scripts/task-commit-attribution-exemptions.txt`, tracked, unchanged, still 3 entries matching `FROZEN_EXEMPTION_COUNT=3`), the check script, and every other gate script are byte-identical between base and candidate.
- **SHA integrity**: the appended full SHA resolves to the base commit itself (`git rev-parse b1cdb0ddd80e...` = the base), and the gate's 8-char short form `b1cdb0dd` matches the violation's short SHA.

## Baseline clippy line disposition (corroborated, not a finding)

The baseline log's `crates/gyre-adapters/src/sqlite/spec_approval.rs:326: clippy::manual_repeat_n` line is not reproducible at this tree: the file is 235 lines at both base and candidate with no `repeat(` anywhere, and no `repeat(...).take(...)` pattern exists in `crates/`. It was a warning (the baseline checks run continued past it to completion), and the authoritative `GYRE_BASELINE_FAILURE_JSON` names exactly one failing probe — `check-task-commit-attribution.sh`. Consistent with the executor's disposition: noise from the baseline capture environment (fingerprint `61f092e0...`), not a defect at this base.

## Scope notes

- The candidate touches no Rust/JS/scripts surface, so compile, lint, and architecture gates cannot be affected by this diff; full deterministic gates and GitHub checks on the exact PR head remain mandatory host-verification stages (as the shipped record itself states).
- This sandbox cannot accept TCP (`accept(): [Errno 95] Operation not supported`, `/tmp/stage/capabilities.json`); no runtime surface was touched, so no HTTP probe is applicable — a transport restriction, not a code defect.

The repair is real, minimal, causal (proven by A/B and mutation), and uses exactly the remedy the check documents, with no exemptions, skips, or weakened gates. Approved.
