# Review — task-240 (Repair verified failure on main b1cdb0ddd80e)

Task: `specs/tasks/task-240.md` — reproduce and repair the verified
`scripts/check-task-commit-attribution.sh` failure at base
`b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` (task-labeled product-surface
commit `b1cdb0dd` missing from `specs/tasks/task-170.md`'s `commits:`
frontmatter).

Candidate under review: `946a584a399c27d27baca01b63f1d5a3776cbf6c`
(branch `pipeline/task-240/2b523018cb1b4c4481dae7dcf4ab6f85-14`), input base
`1a00087c5bd7dcd8703c7506086a1c5f089f3b13`, assigned failure base
`b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3`.
Verdict: **approved** (no findings).

## What the candidate changes

`git diff 1a00087c 946a584a` is exactly one file: `specs/tasks/task-240.md`
(new task record, +73 lines). No Rust, web, script, spec, or exemption-file
changes. The actual repair — appending the full SHA
`b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` as the 12th entry of
`specs/tasks/task-170.md`'s `commits:` frontmatter — is carried in the
candidate's tree via branch checkpoint `54462db3` ("chore(task-240): record
repair round; attribute base commit b1cdb0dd to task-170") merged with the
input base at `0e653509`. `54462db3` touches only
`specs/tasks/task-170.md` (one line) and `specs/tasks/task-240.md`.

## Independent evidence (all at candidate HEAD, full 1298-commit history)

- **Gate passes at head**: `bash scripts/check-task-commit-attribution.sh`
  → exit 0, `OK: every task-labeled product-surface commit is recorded in
  its task's commits: frontmatter (or exempted legacy drift)`.
  Evidence: `/tmp/stage/review-evidence/task-240-review-gate.txt`.
- **Mutation check (pass is attributable to the repair, not gate drift)**:
  removing exactly the recorded SHA from `specs/tasks/task-170.md`
  (`sed -i 's/, "b1cdb0dd…"//'`) → gate exit 1 with the *identical* baseline
  violation (`b1cdb0dd  task-170  feat(task-170): View Specification
  Grammar — TypeScript types and server-side validation`). Restoring the file
  (`git checkout HEAD -- specs/tasks/task-170.md`, tree verified clean
  afterwards) → gate exit 0 again. This reproduces the baseline failure
  byte-for-byte and proves the repair, not drift, closes it.
  Evidence: `/tmp/stage/review-evidence/task-240-review-mutation.txt`,
  `/tmp/stage/review-evidence/task-240-review-restored.txt`.
- **No weakening**: `git diff 1a00087c 946a584a -- scripts/` is empty, and
  `git diff b1cdb0dd 946a584a -- scripts/check-task-commit-attribution.sh
  scripts/task-commit-attribution-exemptions.txt` is empty — the gate script
  and exemption file are byte-identical to the failure base. The exemption
  file holds exactly its frozen 3 entries (`01493c88 task-097`,
  `17c81d5a task-072`, `a8d036f4 task-091`); no skips or exemptions added.
- **Repair is the check's own documented remedy**: the gate's failure text
  mandates recording the SHA in the task's `commits:` frontmatter, never the
  exemption file. The 11 pre-existing branch SHAs were kept, not collapsed —
  same shape as the task-069→task-196 drift repair (`2f092bcd`).
- **`commits: []` is correct for this task**: the only task-240-labeled
  commits in history are `54462db3` (chore) and `946a584a` (docs), both
  touching only `specs/tasks/` — not product surface
  (`crates/`, `web/src`, `web/tests`), so nothing requires attribution.
- **Contract shape**: task frontmatter (title, spec_ref, depends_on) and the
  normative prose match the assignment body; the only deltas are `progress:`
  and the added Baseline failure / Shipped sections.

## Observations (non-blocking)

- The input base `1a00087c` already carried the same one-line repair via
  task-239's checkpoint `7cd9dce6` ("implement(task-239): pipeline
  checkpoint", task-239 being a parallel repair of the same baseline
  failure, `progress: complete`). The resumed task-240 round independently
  carries the identical change in `54462db3`, so the merge is conflict-free
  and idempotent: the candidate satisfies the contract on its own whether or
  not the parallel repair is present. End state verified directly at the
  candidate head.
- No runtime surface was touched (one frontmatter line + task records), so
  no HTTP/server probe is applicable to this repair; the
  commit-attribution gate is the probe, and it was exercised in both
  directions above. Exact-head full deterministic gates and GitHub checks
  belong to host verification and remain mandatory.

## Conclusion

The verified baseline failure is genuinely repaired at the candidate head by
the check's own documented remedy, with no weakened checks, no new
exemptions, no production-code changes, and the pass proven attributable to
the recorded SHA via mutation. Approved.
