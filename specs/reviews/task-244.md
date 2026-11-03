# Review — task-244 (Repair verified failure on main b1cdb0dd: task-commit attribution drift)

Spec: `GOAL.md` — real implementations and meaningful verification. Assigned base
`b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3`, candidate
`b99aa1b4087adece09bff52421eefffaf7dfa2e9` (one commit on top of base).
Verdict: **approved**.

## What the baseline failure was

`scripts/check-task-commit-attribution.sh` failed at base because commit
`b1cdb0dd` — subject `feat(task-170): View Specification Grammar — TypeScript
types and server-side validation` — is a task-170-labeled product-surface
commit (touches `crates/gyre-common/src/view_spec.rs`,
`crates/gyre-server/src/api/explorer_views.rs`, `web/src/`, `web/tests/`,
`web/dist/`) absent from `specs/tasks/task-170.md`'s `commits:` frontmatter.
This is the squash-drift class (tasks 213/219/222/224/229): the squashed
landing commit cannot contain its own SHA, so the recorded list stays one entry
short and the landed surface is invisible to review scoping (task-095 R3-F4
flaw class).

## Independent verification

All probes run in this sandbox; raw command + output saved under
`/tmp/stage/review-evidence/`.

- **Reproduction at base** (isolated worktree at `b1cdb0dd`):
  `bash scripts/check-task-commit-attribution.sh` → **exit 1** with exactly the
  baseline violation (`b1cdb0dd  task-170  feat(task-170): View Specification
  Grammar — ...`). Evidence: `check-at-base.txt`. Matches the baseline failure
  log byte-for-byte in substance.
- **Check at candidate** (`b99aa1b4`, the assigned checkout):
  `bash scripts/check-task-commit-attribution.sh` → **exit 0** — "OK: every
  task-labeled product-surface commit is recorded in its task's commits:
  frontmatter (or exempted legacy drift)." Evidence: `check-at-candidate.txt`.
- **Mutation check (test-the-repair)**: in an isolated worktree at the
  candidate with the one-line repair reverted
  (`git checkout b1cdb0dd -- specs/tasks/task-170.md`), the check **re-fails
  with the identical violation, exit 1**; the assigned checkout tree was never
  modified. Evidence: `check-mutation-reverted.txt`. The pass is attributable
  to the recorded SHA, not gate drift.
- **SHA correctness**: `git rev-parse b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3`
  resolves to the base commit itself; the appended 40-char SHA is the real
  landed commit, and the check's frontmatter parser (`[0-9a-f]{7,40}`, cut to
  8) matches its short form `b1cdb0dd` — proven by the exit-0 run, not by
  inspection alone.

## Diff audit (no weakening, no scope creep)

`git diff b1cdb0dd..b99aa1b4` touches exactly two files:

- `specs/tasks/task-170.md` — one line: appends
  `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` to the existing `commits:`
  array (11 branch SHAs kept, not collapsed — same repair shape as task-229).
  This is the check's own documented remedy, quoted verbatim in its failure
  message.
- `specs/tasks/task-244.md` — new task record (frontmatter + required
  behavior + baseline failure log + shipped notes). Process-only.

`git diff b1cdb0dd..b99aa1b4 -- scripts/ crates/ web/` is **empty** — no
production code, no check, no gate, no skip touched.
`scripts/task-c....txt` is byte-identical to baseline, still at its frozen
3-entry count (`FROZEN_EXEMPTION_COUNT=3` in the script unchanged); no
exemption was added or grown. No other gate reads `specs/tasks/` `commits:`
frontmatter for enforcement (grep of `scripts/*.sh`), so no sibling check can
regress from this diff. The candidate commit itself touches only
`specs/tasks/`, so it is outside the check's product-surface filter;
`task-244.md`'s `commits: []` is correct for a process-only round.

## Notes for verification

- The verifier should re-run `bash scripts/check-task-commit-attribution.sh`
  on the exact candidate head with full history (fetch-depth 0) — it is the
  baseline probe and is history-sensitive.
- GitHub checks on the exact head remain mandatory and belong to host
  verification.
- No runtime surface was touched; no HTTP/server probe applies. TCP `accept()`
  remains unsupported in this sandbox (capabilities.json), which is
  infrastructure, not a code matter.

No findings. The repair is minimal, matches the gate's prescribed remedy, is
proven by an independent mutation check, and weakens nothing.
