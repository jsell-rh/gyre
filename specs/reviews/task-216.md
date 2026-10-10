# Task-216 Review — Repair verified failure on main f4acb4eb

- **Candidate:** `1582211c075d07ec45b0358d91bd111975505ae6`
- **Base:** `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f`
- **Reviewer:** independent (task-216 review round)
- **Date:** 2026-10-10
- **Verdict:** APPROVED

## Scope of the diff

Candidate vs base touches exactly two files:

- `specs/tasks/task-189.md` — appends full SHA `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f` to the `commits:` frontmatter list, preserving all five existing checkpoint SHAs (append-only; review scope grows, never shrinks).
- `specs/tasks/task-216.md` — new task file carrying the assignment contract, baseline failure log, and shipped notes.

Zero changes to `crates/`, `web/`, `docs/`, `scripts/`, or `.github/` (`git diff --name-only base..candidate -- scripts/ crates/ web/ docs/ .github/` → 0 files). `scripts/check-task-commit-attribution.sh` and `scripts/task-commit-attribution-exemptions.txt` are byte-identical to base; `FROZEN_EXEMPTION_COUNT=3` unchanged. No check, skip, or gate weakened.

## Contract reproduction

The verified baseline failure was reproduced at the pristine base commit in a temporary worktree (`/tmp/gyre-review-base`, removed after review):

```
$ bash scripts/check-task-commit-attribution.sh
FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:
  f4acb4eb  task-189  feat(task-189): Fix persona scope resolution to walk the real parent chain
exit=1
```

Evidence: `attribution-at-base.txt`, `attribution-at-base-exit.txt`.

The offending commit `f4acb4eb` is a genuine product-surface commit (touches `crates/gyre-server/src/api/personas.rs`, +280 lines) labeled `task-189` in its subject, an ancestor of base, whose 8-char short SHA was absent from task-189's frontmatter at base — the exact task-095 R3-F4 flaw class the check exists to catch. The short SHA is unique in history (exactly one `f4acb4eb` in `git log --all`), so the appended full SHA matches the scanned entry unambiguously via the check's `frontmatter_commits()` (extracts `[0-9a-f]{7,40}`, truncates to 8).

## Repair validity

The fix is the check's own documented remedy ("Fix by adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter"), not an exemption or weakening:

- Post-fix probe at candidate HEAD: `bash scripts/check-task-commit-attribution.sh` → `OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift).` exit 0. Evidence: `attribution-at-candidate.txt`, `attribution-at-candidate-exit.txt`.
- Exemption file byte-identical to base, still 3 entries (task-097, task-072, task-091). Evidence: `exemptions.txt`, `check-script-unchanged.txt`.
- Same remedy as the merged task-211 repair of the identical drift class (`f38abb7e` for task-210's `a781ede2`) and task-208's `d0d573a8` fix of this same task-189 entry (verified: that commit appends the same full SHA).
- Whitespace clean (`git diff --check` → clean). Evidence: `whitespace-check.txt`.

## Other baseline-log failures

The baseline log was captured in a superseded attempt checkout (`7c247f5f...`). Its rustfmt/clippy lines reference line numbers that do not exist at base (`crates/gyre-domain/src/user.rs` flagged at 396–422 but is 191 lines at base; `sqlite/user.rs` flagged at 454/458 but is 353 lines; evidence: `file-lengths-at-base.txt`), confirming checkout artifacts, not base defects. Re-verified at candidate:

- `python3 scripts/check-rustfmt-diff.py f4acb4eb` → "changed lines clean (0 Rust files checked)", exit 0 (0 Rust files differ base→candidate, so diff-scoped clippy is trivially in scope-clean too). Evidence: `rustfmt-diff-base.txt`.
- `check-arch.sh`, `check-migration-versions.sh`, `check-dead-message-kinds.sh`, `check-byte-slice-truncation.sh`, `check-relative-path-defaults.sh` → all OK at candidate. Evidence: `base-check-*.txt`.

The exit-81 root cause was confirmed against `scripts/dev-static-gate.py`: the wrapper re-runs a failing check at the pristine base; the attribution check failed at base itself, so the wrapper emitted `GYRE_BASELINE_FAILURE_JSON` naming it as the probe — a main-baseline failure, exactly the class this task repairs.

## Task-216 self-attribution

`commits: []` in task-216.md is correct: the branch's only commit (`1582211c`, subject `process(task-216): record commit f4acb4eb in task-189 frontmatter to clear attribution drift`) touches only `specs/` — no product surface (crates/, web/src, web/tests) — so the check's product-surface filter skips it. Verified with `python3 /tmp/stage/dev-attribution.py task-216` (no change produced; worktree clean after). Evidence: `attribution-task216.txt`, `worktree-clean-after-attribution.txt`. This matches the shipped task-211 precedent on main (`progress: complete`, `commits: []`).

## Constraints honored

- No production code, scripts, or verifier changes; no skips, exemptions, or weakened checks; the blocked-feature path was not implemented (none was required — the failure was attribution drift, not a missing feature).
- Temporary review worktree removed; main checkout clean at candidate HEAD throughout (`git status` empty both before and after probes).

## Verifier notes (for full verification)

The focused probes above cover the task contract. Full verification should still run the complete `tools/checks.sh` gate suite and GitHub CI (attribution check requires full git history: `fetch-depth: 0`), per the task's requirement to pass full verification and GitHub checks before merge.
