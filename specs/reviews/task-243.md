# Review — task-243 (Repair verified failure on main b1cdb0ddd80e)

Spec ref: `GOAL.md` — real implementations and meaningful verification.
Task: repair the verified upstream failure of `scripts/check-task-commit-attribution.sh` at base `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3`.
Candidate under review: `44501e0be5b02548950f531c6249578bbec57575` (parent = the base itself).
Verdict: **approved**.

## What the failure was

`b1cdb0dd` (`feat(task-170): View Specification Grammar — TypeScript types and server-side validation`) is a task-labeled product-surface commit — it touches `crates/gyre-common/src/view_spec.rs`, `crates/gyre-server/src/api/explorer_views.rs`, `web/src/lib/types/view-spec.ts`, `web/src/lib/**`, and tests — but was absent from `specs/tasks/task-170.md`'s `commits:` frontmatter. Squash-drift class (cf. tasks 213/219/222/224/229): the squashed landing commit cannot contain its own SHA, so the recorded list stayed one entry short and the landing surface was invisible to review scoping (task-095 R3-F4 flaw class).

## Independent verification

All probes run at candidate `44501e0b` in the assigned checkout (working tree untouched; the mutation check ran in a `git clone --shared` at `/tmp/mut-check`). Evidence: `/tmp/stage/review-evidence/`.

- **Scope of the diff.** `git diff b1cdb0dd..44501e0b --stat` = exactly two files: `specs/tasks/task-170.md` (1 line: the SHA appended to `commits:`) and `specs/tasks/task-243.md` (new task record, 73 lines). `git diff b1cdb0dd..44501e0b -- scripts/ crates/ web/src/ web/tests/` is empty — no production code, no check, no verifier changed.
- **No weakening.** `scripts/task-c....txt` remains at its frozen 3-entry baseline (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`); `FROZEN_EXEMPTION_COUNT` in the check script is still 3; the script itself is byte-identical to base. No skip, exemption, or gate touched.
- **Probe at candidate.** `bash scripts/check-task-commit-attribution.sh` → `OK: every task-labeled product-surface commit is recorded...`, exit 0 (`probe-at-candidate.txt`).
- **Mutation check (repair is load-bearing).** In the shared clone with `specs/tasks/task-170.md` reverted to the base version, the gate re-fails with the identical violation — exit 1, `b1cdb0dd  task-170  feat(task-170): View Specification Grammar...` (`mutation-reverted.txt`). The pass at the candidate is attributable to the recorded SHA, not gate drift.
- **Recorded SHA is correct.** The appended full SHA `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` equals the commit's actual SHA (verified via `git log`); the gate's `frontmatter_commits` extracts `[0-9a-f]{7,40}` and cuts to 8 chars, so `b1cdb0dd` matches the `--short=8` form. The 11 pre-existing branch-round SHAs were kept, matching the landed repair shapes of task-229→task-196 and task-224→task-068.
- **task-243's own `commits: []` is correct.** The candidate commit `44501e0b` touches only `specs/tasks/` (no `crates/|web/src|web/tests` surface), so the gate does not require it in the frontmatter; the attribution claim matches the diff.

## Assessment

The repair uses exactly the check's own documented remedy ("Fix by adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter"), is minimal (one spec-record line plus the task file), is independently reproduced both failing (base) and passing (candidate), and weakens nothing. The baseline failure is live at the base and gone at the candidate for the right reason.

Notes for verification (not findings): this sandbox cannot accept TCP (`capabilities.json`), so no HTTP probe applies — none is needed, as no runtime surface was touched. Full deterministic gates and GitHub checks on the exact PR head remain mandatory controller stages.
