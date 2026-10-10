# Review — task-235 (Repair verified failure on main 27bd585ca7eb)

Task: `specs/tasks/task-235.md` — repair the verified `scripts/check-task-commit-attribution.sh` failure on main `27bd585ca7eb`.
Commit under review: `74bf94b152a070f9f6f4726fdb4498efa9012e68` (single commit on base `27bd585c`; diff touches only `specs/tasks/task-155.md` +1/-1 line and adds `specs/tasks/task-235.md`). Tree `78b97f80` matches the commit message claim.
Verdict: **approved**.

## Round 1

Independent probes (evidence under `/tmp/stage/review-evidence/task-235-review-*.txt`):

- **Reproduction at base** (temp worktree at `27bd585c`, no mutation of the review checkout): `bash scripts/check-task-commit-attribution.sh` → exit 1, violation `27bd585c task-155 feat(task-155): Implement gyre search CLI command` — identical to the baseline failure JSON. (`task-235-review-base-repro.txt`, `.exit`)
- **Candidate passes**: same check at `74bf94b1` → exit 0, `OK: every task-labeled product-surface commit is recorded...`. (`task-235-review-candidate.txt`, `.exit`)
- **Mutation check (attribution of the pass)**: removed `27bd585ca7eb...` from task-155's `commits:` frontmatter → gate re-fails exit 1 with the identical violation; `git checkout --` restored → exit 0 again. The pass is attributable to the recorded SHA, not gate drift or history effects. (`task-235-review-mutation.txt`, `task-235-review-restore.txt`)
- **Drift story verified**: none of the 7 previously recorded task-155 SHAs is an ancestor of HEAD (`git merge-base --is-ancestor` each, all NOT ancestors) — squash-drift class; `git diff 27bd585c 4c0df440 -- crates/gyre-cli/src/client.rs crates/gyre-cli/src/main.rs docs/cli.md` is 0 lines, so the landing commit `27bd585c` (product surface: +123/+670/+44 on the three CLI files) is exactly the recorded branch tip's content and recording it is correct, not an exemption-shaped dodge.
- **No weakening**: `git diff base..candidate -- scripts/ web/ crates/ docs/` is empty; `scripts/task-commit-attribution-exemptions.txt` untouched at its frozen 3-entry baseline; `FROZEN_EXEMPTION_COUNT=3` unchanged. The repair is the check's own documented remedy (frontmatter recording), same shape as task-227 (`05709c24` → task-196), task-224 (`a11ba8d3` → task-068), task-219 (`e96d25ab` → task-200) precedents.
- **Frontmatter parsers unaffected**: `cd scripts && python3 -m unittest test_pipeline_catalog test_dev_attribution` → 12 tests OK (the `commits:` inline-array form with the appended full SHA parses; `frontmatter_commits` extracts 8-char prefixes either way). `python3 scripts/dev-attribution.py task-235` derives an empty list — the candidate commit is specs-only, so `commits: []` on task-235 is attribution-canonical.
- **Commit message lint**: `bash scripts/check-commit-msg.sh` on the candidate's message → passed. The candidate commit itself is task-labeled but specs-only (no `crates/`/`web/src`/`web/tests` files), so it is correctly out of the attribution gate's product-surface scope.

## Findings

None. The repair is minimal (1 frontmatter line), addresses the exact verified failure via the check's documented remedy, adds no exemptions, weakens no gate, and the pass demonstrably depends on the added SHA (mutation check). Specs-only change; no Rust/JS surface touched, so Rust/web test suites are out of scope for this round.
