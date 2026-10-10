# Review — task-227 (Repair verified failure on main 05709c242509)

Spec: GOAL.md — real implementations and meaningful verification; task contract: reproduce and repair the verified upstream `check-task-commit-attribution.sh` failure at base `05709c242509b89214876339b3c463ede3a31b60` without weakening any check, adding skips/exemptions, or touching the blocked feature.
Commit under review: `f0ff18555638e42aedb9bbc3af145de3654fc822` (single commit over base; diff touches only `specs/tasks/task-196.md` and the new `specs/tasks/task-227.md`).
Verdict: **complete**.

## Round 1

All probes run independently in this sandbox (evidence under `/tmp/stage/review-evidence/`); the base reproduction and mutation check ran in a `git worktree` isolated from the main checkout, which was pristine at candidate `f0ff1855` throughout and after review (verified: `git status --porcelain` empty, `git diff f0ff1855` empty).

- **Base failure reproduced**: `bash scripts/check-task-commit-attribution.sh` at `05709c24` (isolated worktree) → **exit 1**, identical violation text to the baseline log: `05709c24  task-196  feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation`. Evidence: `task-227-base-repro.txt`.
- **Candidate passes**: same check at `f0ff1855` → **exit 0** ("OK: every task-labeled product-surface commit is recorded…"). Evidence: `task-227-candidate-pass.txt`.
- **Violation class confirmed live at base**: `git show --name-only 05709c24` lists `crates/gyre-server/src/api/graph.rs`, `web/src/__tests__/Briefing.test.js`, `web/src/components/Briefing.svelte`, `web/src/lib/InlineChat.svelte` — genuine product surface under the check's `crates/|web/src|web/tests` filter; the commit subject labels task-196.
- **Root cause (squash drift) verified**: none of the three SHAs previously recorded in task-196's `commits:` frontmatter (`3b90956c`, `abcfff04`, `88b57180`) is an ancestor of HEAD (`git merge-base --is-ancestor` → false for each). They exist as unreachable objects in the clone (dangling pipeline-branch commits). Content-identity spot-check: `git diff 05709c24 88b57180 -- <four product files>` is formatting-only (rustfmt line-wrapping deltas in `graph.rs` only; the three web files byte-identical) — the repair records the commit that actually landed the surface, not a divergent one.
- **Mutation check (test-the-repair)**: in the isolated worktree at `f0ff1855`, removing the appended SHA from task-196's frontmatter → check **FAILS** (exit 1, identical violation); restoring the file → check **passes** (exit 0); worktree left clean. The pass is attributable to the recorded SHA, not gate drift. Evidence: `task-227-mutation.txt`, `task-227-mutation-restore.txt`.
- **No gate weakened**: `scripts/task-commit-attribution-exemptions.txt` unchanged at its frozen 3-entry baseline (01493c88, 17c81d5a, a8d036f4); `git diff 05709c24 f0ff1855 -- scripts/` is empty; no check script, skip, or exemption touched anywhere in the diff.
- **No production surface touched**: `git diff --stat 05709c24 f0ff1855` = `specs/tasks/task-196.md` (+1/-1, the SHA append) and `specs/tasks/task-227.md` (new, 75 lines). No Rust/JS source changed, so no runtime probe is applicable; `python3 scripts/check-rustfmt-diff.py 05709c24` → "changed lines clean (0 Rust files checked)"; `git diff --check 05709c24` → clean.
- **Repair shape matches check's documented remedy and repo precedent**: the check's own FAIL text prescribes "adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter" — the repair appends the full landing SHA (its first 8 chars are what `frontmatter_commits` extracts). Same shape as task-224's `a11ba8d3` for task-068 and task-222's `6bf777a6` for task-200 (both verified present as the last entries of the respective frontmatter lists).
- **Self-attribution correct**: the candidate commit `f0ff1855` is labeled `task-227` but touches only `specs/tasks/` — no product surface — so it is legitimately outside the check's scope and `commits: []` in task-227.md is accurate (specs-only repair round, matching the task-224 precedent).
- **Task-file content**: task-227.md records the baseline failure verbatim, the reproduction, root cause, repair, mutation check, gate re-runs, and the TCP-transport restriction — accurate against my independent observations.

Findings: none.

Notes for host verification (sandbox restriction, not a code defect): this sandbox cannot accept TCP (`accept(): [Errno 95] Operation not supported`, `/tmp/stage/capabilities.json`). No runtime surface changed, so no HTTP probe applies; the full deterministic gate battery and GitHub checks on the exact PR head remain mandatory at verification.

— Reviewer, 2026-10-10
