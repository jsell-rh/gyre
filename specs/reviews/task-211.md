# Task-211 Review — Repair verified failure on main 8c2d1775

- **Candidate:** `9dc6f9639a7f6f53b8986dc1a57de36d3f4f4676`
- **Base:** `8c2d177505852b3e39cd77f4f782fb355de245aa`
- **Reviewer:** independent (task-211 review round)
- **Date:** 2026-10-09
- **Verdict:** APPROVED

## Scope of the diff

Candidate vs base touches exactly two files:

- `specs/tasks/task-210.md` — appends full SHA `a781ede2ea21a5153dbcb49c65771f990344a093` to the `commits:` frontmatter list (commit `ce96eb64`).
- `specs/tasks/task-211.md` — new task file carrying the assignment contract, baseline failure log, and shipped notes (commits `38032f4e`, `0599ac9c`, `b70aa057` empty, `9dc6f963`).

Zero changes to `crates/`, `web/`, `docs/`, or `scripts/` (`git diff --name-only base..candidate -- scripts/ crates/ web/ docs/` → 0 files). No exemptions added; `scripts/task-commit-attribution-exemptions.txt` diff is empty and still at its frozen 3-entry baseline.

## Contract reproduction

The verified baseline failure was reproduced at the base commit in a temporary worktree (`/tmp/gyre-review-base`, removed after review):

```
$ bash scripts/check-task-commit-attribution.sh
FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:
  a781ede2  task-210  feat(task-210): Repair verified failure on main cd1c5f044e49
exit=1
```

Evidence: `/tmp/stage/review-evidence/attribution-at-base.txt`.

The offending commit `a781ede2` is a genuine product-surface commit (touches `crates/gyre-server/src/api/admin.rs`, `web/src/**`, `docs/ui.md`, `scripts/relative-path-defaults-exemptions.txt`) labeled `task-210` in its subject, an ancestor of base, whose 8-char short SHA was absent from task-210's frontmatter at base — the exact task-095 R3-F4 flaw class the check exists to catch.

## Repair validity

The fix is the check's own documented remedy ("Fix by adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter"), not an exemption or weakening:

- `frontmatter_commits()` extracts `[0-9a-f]{7,40}` and truncates to 8 chars, so the recorded full SHA matches the scanned `a781ede2` short SHA. Short SHA is unique in history (exactly one `a781ede2`).
- FROZEN_EXEMPTION_COUNT unchanged at 3; exemption file byte-identical to base.
- Post-fix probe at candidate HEAD: `bash scripts/check-task-commit-attribution.sh` → `OK: every task-labeled product-surface commit is recorded...` exit 0. Evidence: `/tmp/stage/review-evidence/attribution-at-candidate.txt`.

## Other baseline-log failures

The baseline log was captured in a superseded attempt checkout (`4f40bb82...`). Each remaining failure was re-run against the **base tree itself** and passes there, confirming they were checkout artifacts, not current-tree defects, and therefore out of this task's repair scope:

- `check-relative-path-defaults.sh` → OK at base (`main.rs:1863` dynamic-default flag does not exist in current tree; exit 0). Evidence: `relpath-at-base.txt`.
- `check-byte-slice-truncation.sh`, `check-fail-open-ref-resolution.sh`, `check-abac-route-registry.sh`, `check-migration-versions.sh`, `check-dead-message-kinds.sh` → all exit 0 at base. Evidence: `base-*.txt`.
- `check-rustfmt-diff.py 8c2d1775` → "changed lines clean (0 Rust files checked)" at base; candidate identical (0 Rust files differ from base), so no formatting debt exists in this history. Evidence: `base-rustfmt.txt`, `candidate-rustfmt.txt`.

## Task-211 self-attribution

`commits: []` in task-211.md is correct: every candidate commit (`38032f4e`, `0599ac9c`, `b70aa057`, `ce96eb64`, `9dc6f963`) touches only `specs/` — no product surface (crates/, web/src, web/tests) — and `review:`/`process:` subjects are out of the check's scope by design. The attribution check passing at the candidate confirms this mechanically.

## Constraints honored

- No production code, scripts, or verifier changes; no skips, exemptions, or weakened checks; the blocked-feature path was not implemented (none was required — the failure was attribution drift, not a missing feature).
- Temporary review worktree removed; main checkout clean at candidate HEAD throughout (`git status` empty).

## Verifier notes (for full verification)

The focused probes above cover the task contract. Full verification should still run the complete `tools/checks.sh` gate suite and GitHub CI (attribution check requires full git history: `fetch-depth: 0`), per the task's requirement to pass full verification and GitHub checks before merge.
