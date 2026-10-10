---
title: "task-231 — Repair verified failure on main 27bd585ca7eb"
spec_ref: "GOAL.md — real implementations and meaningful verification"
task: task-231
candidate: a7905b4000b356c3fc6a087a755026b3e99bb6e1
base: 27bd585ca7eb429905ccbded1f48b4d0167c0c20
verdict: approved
---

# Review: task-231 (candidate a7905b40)

## Assignment

Repair the verified upstream failure: `bash scripts/check-task-commit-attribution.sh`
exit 1 on base `27bd585c` — squashed landing commit `27bd585c` (task-155 label,
product surface in `crates/gyre-cli/`) was absent from task-155's `commits:`
frontmatter, leaving the landing surface invisible to review scoping
(task-095 R3-F4 flaw class).

## Diff scope (verified from git, not labels)

`git diff 27bd585c a7905b40` is exactly two files:

- `specs/tasks/task-155.md` — one line: `commits:` frontmatter gains
  `"27bd585ca7eb429905ccbded1f48b4d0167c0c20"` appended after the 7 pre-existing
  candidate-lineage SHAs (append-only; all 8 SHAs verified reachable via
  `git cat-file -t` → commit). Line grows 317 → 361 bytes; single-line inline
  array form matches the pre-existing shape and the `frontmatter_commits()`
  parser (inline-array branch, `[0-9a-f]{7,40}` extraction, first-8 cut).
- `specs/tasks/task-231.md` — new process-only task file (assignment text,
  baseline failure log, Shipped notes).

`git diff 27bd585c a7905b40 -- scripts/ crates/ web/` is empty. Exemptions file
frozen at its 3-entry baseline (`01493c88 task-097`, `17c81d5a task-072`,
`a8d036f4 task-091`) — no new entries, no count change, no check/gate/skip
weakened. This is the check's own documented remedy and matches precedent
`2f092bcd` (one-line append of base SHA `05709c24` to task-196, task-069 round).

## Probes (independent, in isolated git worktrees; main checkout untouched)

| Probe | Command | Result |
|---|---|---|
| Reproduce baseline | `bash scripts/check-task-commit-attribution.sh` at base worktree | FAIL exit 1, violation `27bd585c task-155 feat(task-155): Implement gyre search CLI command` |
| Candidate check | same, at candidate worktree | OK exit 0 |
| Mutation (kills the pass) | strip the recorded SHA via sed, rerun | FAIL exit 1, identical violation; restored → OK exit 0 |
| Lint family | 17 `check-*.sh` scripts incl. `check-arch.sh`, ABAC, migrations, in-memory-state, forged-scope, etc. at candidate | all exit 0 |
| Task-file structure | `migrate-task-frontmatter.sh` | exit 0, task-231 SKips as already migrated |
| Attribution canonicality | `python3 scripts/dev-attribution.py task-231` at candidate | no-op — derived list equals committed `commits: []` |
| Contract-hash invariance | `dev-contract.py requirement_parts()` on committed file vs progress-flipped/Shipped-stripped variant | front/prose/generation identical — Shipped append cannot masquerade as contract amendment |

The pass is attributable to the recorded SHA, not gate drift: the mutation round
reproduces the exact baseline failure and restore returns to exit 0. Evidence
under `/tmp/stage/review-evidence/task-231-*`.

## Notes

- `27bd585c` is genuine product surface (+975 lines: `gyre-cli/src/main.rs`,
  `gyre-cli/src/client.rs`, `docs/cli.md`), so recording it is correct, not
  gaming — the SHA now scopes that surface for review.
- `commits: []` on task-231 is attribution-canonical (specs-only round;
  `dev-attribution.py` derives the empty list and leaves the file byte-identical).
- Pre-existing repo-wide `| not-started |` coverage counts are unchanged by this
  diff and out of this task's scope.
- Candidate commit subject `implement(task-231): pipeline checkpoint` is the
  pipeline's checkpoint convention on pipeline branches (567 such commits);
  landing on main is a squash with a conventional `feat(task-231):` subject, so
  `check-commit-msg.sh` is not violated by the landed change.

## Verdict

Approved. The repair is minimal, append-only, uses the check's documented
remedy, weakens nothing, and the failure is reproduced before and killed by the
exact one-line change (mutation round proves attribution). No findings.
