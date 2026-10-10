# Review — task-219 (Repair verified failure on main 6bf777a6a44f)

Spec: `GOAL.md` — real implementations and meaningful verification. Task contract: reproduce and repair the verified `check-task-commit-attribution.sh` failure on base `6bf777a6a44f28052ed5af28bf6fb013fde6df48` without weakening checks or adding exemptions.

Candidate under review: `309a96cff8a861e7d2d3c0250b9a2d9c582d5f54` (exact assigned SHA, confirmed HEAD). Diff from base is exactly two commits, both `specs/`-only:

- `5df2f9ab` `process(task-200)` — appends `"6bf777a6a44f28052ed5af28bf6fb013fde6df48"` to `specs/tasks/task-200.md` `commits:` frontmatter (13 → 14 SHAs, no duplicates, all 40-hex, all resolve to real commits).
- `309a96cf` `docs(task-219)` — adds `specs/tasks/task-219.md` (this task's record).

No changes to `scripts/`, `crates/`, `web/`, or any gate. `scripts/task-commit-attribution-exemptions.txt` is untouched at its frozen 3-entry baseline (count re-verified). `FROZEN_EXEMPTION_COUNT` in the script is untouched. `git diff --check` clean.

Verdict: **complete** — approved, no findings.

## Independent probes (evidence under /tmp/stage/review-evidence/)

- **Reproduction at exact base** (detached worktree at `6bf777a6`, no tree side effects): `bash scripts/check-task-commit-attribution.sh` → exit 1 with the identical violation recorded in the baseline log: `6bf777a6  task-200  feat(task-200): Message bus — per-kind payload schema validation (reject invalid payloads with 400)` (`attribution-before.txt`). The commit is product-surface (touches `crates/gyre-common/src/message.rs`, `crates/gyre-server/src/api/messages.rs`, `crates/gyre-server/src/mcp.rs`), so the gate's product-surface filter is genuinely triggered — this is the real failure, not a fixture.
- **Repair at candidate HEAD**: `bash scripts/check-task-commit-attribution.sh` → `OK`, exit 0 (`attribution-after.txt`). The remedy is exactly the one the gate's failure text documents (record the SHA in the task file's `commits:` frontmatter), not an exemption or skip.
- **Mutation (attribution of the pass)**: in a detached worktree at the candidate, removing the recorded SHA from task-200's frontmatter re-fails the gate with the identical violation; restoring the file re-passes (`mutation-check.txt`, `mutation-restore-check.txt`). The pass is attributable to this repair, not gate drift.
- **No check weakened**: exemption entry count 3 (frozen baseline), script bytes unchanged across the diff, no new exemption lines, no `SKIP` path taken (git available).
- **Task-219's own `commits: []`**: correct. Both branch commits touch `specs/` only — no product surface, so no attribution obligation; `commits: []` matches the established convention of sibling repair tasks 215/216/218 (frontmatter compared). `python3 scripts/dev-attribution.py task-219` produces no tree change (`attribution-task219.txt`, exit 0).
- **Adjacent gates at candidate**: `check-arch.sh` OK; full dev-pipeline unittest suite `python3 -m unittest discover -s scripts -p 'test_dev*.py'` → 40 tests, OK (`test-dev-all.txt`), using temp fixtures — the suite does not read the live `specs/tasks/` files. No cargo/vitest path reads `specs/tasks/`, so the Test/web-build/E2E CI jobs are unaffected by this diff; the Format/Clippy diff jobs have no Rust lines to judge (diff touches no Rust files).
- **Commit subjects**: `docs(task-219)` passes `scripts/check-commit-msg.sh`; `process(task-200)` is not in that lint's type list, but the lint is wired only as a local pre-commit `commit-msg`-stage hook (`.pre-commit-config.yaml:578-583`) and is not run in `.github/workflows/`, and 18 prior `process:`/`review:` subjects already exist on main (the attribution script itself deliberately classifies them as bookkeeping rounds). No GitHub-check risk.

## Notes (pre-existing, out of scope for this candidate)

- `dev-attribution.py task-200` (if a future pipeline ever ran it for task-200) would rewrite the frontmatter list to only the reachable scoped SHA, dropping the 13 unreachable branch-checkpoint SHAs recorded before this task. That behavior is pre-existing at base (the 13 stale entries were already in the file at `6bf777a6`), the attribution gate passes either way (it never resolves listed SHAs; it only requires history-commits to be listed), and the pipeline runs the tool for its own task only. Not a defect of this candidate; recorded so a future round doesn't mistake it for one.
- The baseline failure log's environment fingerprint differs from the assignment header's (`61f092e0...` vs `host-df92513f...`); the assignment header is authoritative and matches the task file. Cosmetic.

## Verdict checks for the verifier (host/CI)

1. `git rev-parse HEAD` equals `309a96cff8a861e7d2d3c0250b9a2d9c582d5f54`.
2. `bash scripts/check-task-commit-attribution.sh` (full history) → OK, exit 0.
3. `git diff 6bf777a6..309a96cf --name-only` → exactly `specs/tasks/task-200.md`, `specs/tasks/task-219.md`.
4. Exemption file entry count == 3.
5. GitHub CI on the PR head (Format, Clippy diff, arch-lint, Test, web-build) — no Rust/JS lines changed, expected green.
