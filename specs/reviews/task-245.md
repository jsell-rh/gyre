# Review — task-245 (Repair verified failure on main b1cdb0ddd80e)

Task: reproduce and repair the verified upstream failure at baseline
`b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` —
`bash scripts/check-task-commit-attribution.sh` FAIL because the
task-labeled product-surface commit `b1cdb0dd`
(`feat(task-170): View Specification Grammar`) was absent from
`specs/tasks/task-170.md`'s `commits:` frontmatter.

Candidate under review: `8613491823c2d840294301ef243e442d35f17bad`
(assignment base `63aa1486210dc579f2fcd4e8b5c6b91e6f66fd77`).
Verdict: **approved**.

## Independent verification (evidence in /tmp/stage/review-evidence/)

- `probe-at-candidate-86134918.txt` — `bash
  scripts/check-task-commit-attribution.sh` at the candidate head:
  `OK: every task-labeled product-surface commit is recorded ...`,
  exit 0.
- `review-probe-at-baseline-b1cdb0dd.txt` — same probe in an isolated
  worktree at the baseline `b1cdb0dd`: FAIL with the exact recorded
  message (`b1cdb0dd task-170 feat(task-170): View Specification
  Grammar ...`), exit 1. The reported failure is real.
- `ancestry-and-exemption-checks.txt` —
  - `b1cdb0dd` IS an ancestor of the assignment base `63aa1486` (the
    failing state is genuine upstream history).
  - The repairing commit `7cd9dce6` IS an ancestor of the base: its
    diff to `specs/tasks/task-170.md` appends exactly
    `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` to the `commits:`
    frontmatter — the correct fix path prescribed by the check's own
    failure message (frontmatter recording, not exemption).
  - The exemption file `scripts/task-c....txt` exists at neither the
    baseline nor the candidate: nothing was grandfathered.

## Candidate diff assessment

`git diff 63aa1486..86134918` is exclusively `specs/tasks/task-245.md`
(+98 lines): the task record documenting reproduction, the already-landed
repair, and verification evidence. No production code, no scripts, no
specs, no exemption files were touched; no check was weakened or
skipped; no blocked feature was implemented.

The "no new product code" outcome is legitimate and honest: the defect
class (missing frontmatter attribution for `b1cdb0dd`) was already
repaired on main by `7cd9dce6` before the assignment base, so the only
correct contribution for this assignment was to reproduce the failure,
verify the repair, and record it — which the candidate does accurately
(every claim in the Shipped section was independently re-confirmed
above). Re-implementing the fix would have been duplicate/conflicting
work.

The candidate's own task-245-labeled commits touch only
`specs/tasks/task-245.md`, which is not product surface per the check
(`crates/`, `web/src`, `web/tests`), so task-245's empty `commits:`
frontmatter is consistent — mechanically confirmed by the probe passing
at the head.

## Findings

None. The task contract (repair the verified failure without weakening
checks) is satisfied: the probe that failed at the baseline passes at
the candidate via the real frontmatter fix, with no exemptions added.
