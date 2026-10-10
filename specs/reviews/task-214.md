# Review — task-214 (Repair verified failure on main f4acb4ebcaf9)

Spec: GOAL.md — real implementations and meaningful verification. Base
`f4acb4ebcaf930ada2f1318b8aa2adbf244e720f`, candidate
`1e16f13b6802cae0e51348298454c2055785df16` (diff: `specs/tasks/task-189.md`
+1 SHA in `commits:` frontmatter, new `specs/tasks/task-214.md`).

Verdict: **complete**.

## Probes (evidence: /tmp/stage/review-evidence/)

- **Reproduction at base, independently.** Fresh worktree at
  `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f`: `bash
  scripts/check-task-commit-attribution.sh` → exit 1, flagging
  `f4acb4eb  task-189  feat(task-189): Fix persona scope resolution to walk
  the real parent chain` (`attribution-before.txt`). The commit touches
  `crates/gyre-server/src/api/personas.rs` (product surface) and its GitHub
  squash-merge SHA was absent from task-189's `commits:` frontmatter — the
  exact baseline failure.
- **Repair is the check's documented remedy, and nothing else changed.**
  `git diff base..candidate -- scripts/` is empty; the exemption file is
  byte-identical, frozen at its 3-entry baseline; `FROZEN_EXEMPTION_COUNT=3`
  untouched. No skip, exemption, or gate weakened.
- **Candidate passes.** `bash scripts/check-task-commit-attribution.sh` at
  `1e16f13b` → `OK: ...` exit 0 (`attribution-after.txt`).
- **Negative control (causality).** With only the repaired frontmatter line
  reverted in the working tree, the check fails again with the identical
  message (exit 1, `negative-control.txt`); restored afterward, tree clean.
  The pass is caused by the repair, not incidental state.
- **Attribution is true.** `f4acb4eb` is task-189's real ship commit
  (squash-merge of the reviewed pipeline branch;
  `specs/reviews/task-189.md` covers the branch work; `git show --stat`
  confirms the `personas.rs` +272/−10 surface). The append records a fact,
  not a fabrication, and preserves the five pre-existing SHAs.
- **Branch hygiene.** The candidate's two commits (`2595d627`,
  `1e16f13b6802cae0e51348298454c2055785df16`) are `process:`-typed and touch
  only `specs/tasks/*.md` — no product surface, so `commits: []` in
  task-214 is correct (verified with `python3 /tmp/stage/dev-attribution.py
  task-214`, no change produced). The check intentionally skips
  `process:`/`review:` bookkeeping rounds, matching the precedent task-211
  repair (`f38abb7e`, identical shape).
- **Baseline-log secondary failures are not present in this history.**
  `specs_assist.rs` is whole-file rustfmt-clean at the candidate
  (`rustfmt --edition 2021 --check` exit 0), and
  `python3 scripts/check-rustfmt-diff.py f4acb4eb` reports `0 Rust files
  checked` — the flagged formatting/clippy lines came from the superseded
  attempt checkout's own diff, not from base.

## Findings

None. Smallest meaningful probes only; full workspace suites and all-target
Clippy belong to verification. GitHub checks must pass before merge.
