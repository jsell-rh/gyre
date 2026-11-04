# Review — task-244 (Repair verified failure on main b1cdb0dd)

Candidate: `d15aaa3ca4df39a67df2326ecca09eb304d8c6f5` (merge of base `63aa1486` into
`pipeline/task-244/...`, carrying checkpoint `b99aa1b4` branched from the failure point
`b1cdb0dd`). Comparison base for review: `63aa1486210dc579f2fcd4e8b5c6b91e6f66fd77`.

## Verdict: APPROVED

## Assigned contract and how it is met

The verified upstream failure is `bash scripts/check-task-commit-attribution.sh`
exiting 1 at `b1cdb0dd`: the task-170-labeled product-surface commit `b1cdb0dd`
(feat(task-170): View Specification Grammar — touches `crates/gyre-common/src/view_spec.rs`,
`crates/gyre-server/src/api/explorer_views.rs`, `web/src/`, `web/tests/`) was absent from
`specs/tasks/task-170.md`'s `commits:` frontmatter.

1. **Repair is present and real.** `specs/tasks/task-170.md:15` at the candidate records
   `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` as the 12th entry, joining the 11 branch
   SHAs (kept, matching the task-229 repair shape). The recorded SHA points at the real
   landed commit (verified: `git show --name-only b1cdb0dd` lists the product surface).
2. **Gate passes at the candidate.** `bash scripts/check-task-commit-attribution.sh` →
   exit 0 ("OK: every task-labeled product-surface commit is recorded ...").
   Evidence: `review-evidence/gate-attribution-candidate.txt`.
3. **Pass is attributable to the repair (mutation check).** Removing the recorded SHA
   from task-170.md re-fails the gate with the exact baseline violation (exit 1,
   `b1cdb0dd  task-170 ...`); restoring the file re-passes (exit 0). Working tree
   restored to the candidate byte-for-byte (`git status` clean at `d15aaa3c`).
   Evidence: `review-evidence/gate-mutation-no-sha.txt`.
4. **No weakening.** `git diff 63aa1486..d15aaa3c -- scripts/` is empty;
   `scripts/task-commit-attribution-exemptions.txt` is untouched since pre-base commit
   `2448e4b1` and still holds its frozen 3 entries (01493c88, 17c81d5a, a8d036f4);
   `FROZEN_EXEMPTION_COUNT=3` unchanged. No skips or exemptions added anywhere.
5. **No code surface touched.** The candidate's entire delta from its assigned base is
   `specs/tasks/task-244.md` (+105 lines, the task record itself). `commits: []` in the
   frontmatter is correct: the only task-244-labeled non-merge commit (`b99aa1b4`)
   touches only `specs/tasks/`, so there is no product surface to attribute.
6. **Merge integrity.** `d15aaa3c` preserves both parents: task-244.md from the branch;
   task-170.md's fix is identical in both parents (base received the same one-line
   remedy via task-239's checkpoint `7cd9dce6`), and the gate's `--no-merges` scan
   correctly keeps the merge commit itself out of attribution scope.

The shipped notes' claims (reproduction at `b1cdb0dd`, remedy shape, exemption-file
baseline, no source change) all check out against the repository.

## Durable finding: `explorer_ws_rate_limiting` under `cargo test --all` — not a candidate defect

A prior verification of this content failed at
`crates/gyre-server/tests/explorer_ws_integration.rs:390` ("Second rapid message
should be rate limited"). The candidate is byte-identical to base for all of
`crates/` and `web/` (`git diff 63aa1486..d15aaa3c -- crates/ web/` is empty), so this
failure is a pre-existing condition at base, outside task-244's assigned failure.

Static root cause (full analysis in `review-evidence/rate-limit-race-analysis.txt`):
the test's second rapid `Message` is consumed by the during-agent-run listener inside
`tokio::select!` (explorer_ws.rs ~755-820), which explicitly ignores `Message` types —
so the main loop's rate limiter (explorer_ws.rs:614) never sees it and no "wait" error
is emitted. Whether the listener or the (instantly-completing, no-LLM, in-memory-store)
agent future wins the select! poll race is timing-dependent — a flake introduced with
the test itself in `d7940e85` (PR #627), not by this candidate.

This sandbox cannot compile the probe (crates.io unreachable, no registry cache — the
same restriction task-192 recorded as infrastructure, not a code defect). Verifier must
run, on candidate and on base `63aa1486` alike:
`cargo test -p gyre-server --test explorer_ws_integration explorer_ws_rate_limiting`
repeated (>=10x; it is a race) plus the standard full-suite verification. If it
reproduces on base, it needs its own repair task (as task-239 did for the clippy
failure in this same test file), not rejection of a candidate whose diff touches no
Rust/JS code.

## Evidence index (`/tmp/stage/review-evidence/`)

- `candidate-diff.txt` — base→candidate diff (task-244.md only), empty scripts/ diff,
  recorded SHA present at task-170.md:15.
- `gate-attribution-candidate.txt` — gate exit 0 at candidate.
- `gate-mutation-no-sha.txt` — gate exit 1 with the baseline violation when the SHA is
  removed (mutation check).
- `rate-limit-test-run1.txt`, `rate-limit-test-run2.txt` — blocked probe attempts
  (crates.io unreachable).
- `rate-limit-race-analysis.txt` — static race analysis + verifier instructions.
