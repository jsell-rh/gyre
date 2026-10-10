# Review — task-224 (Repair verified failure on main a11ba8d32859)

Task: `specs/tasks/task-224.md` — repair the verified upstream failure of `scripts/check-task-commit-attribution.sh` at base `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0`.
Candidate: `6e5d7ced3d28e597e315c99509753e834ebb9de1` (single commit on top of base).
Verdict: **approved**.

## Scope of the change

`git diff a11ba8d3..6e5d7ced` touches exactly two files, both task records:

- `specs/tasks/task-068.md` (+1/-1): appends the full SHA `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0` to the `commits:` frontmatter array (9 → 10 entries).
- `specs/tasks/task-224.md` (+73, new file): the assigned task contract plus the shipped record.

No production code, scripts, verifiers, exemptions, or specs outside `specs/tasks/` changed (`git diff base..candidate -- scripts/` is empty). This is precisely the remedy the failing check's own message prescribes ("Fix by adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter"), the same repair shape as task-219's `e96d25ab` for the `6bf777a6`/task-200 drift.

## Evidence (all under /tmp/stage/review-evidence/)

1. **Reproduction at base** (`task-224-review-repro-at-base.txt`): in a detached `git worktree` at `a11ba8d3`, `bash scripts/check-task-commit-attribution.sh` exits 1 with the exact recorded violation — `a11ba8d3 task-068 feat(task-068): Graph Summary & Dry-Run MCP Tools`. The failure is live at the assigned base, not fabricated.
2. **Failure characterization** (`task-224-review-failure-analysis.txt`): commit `a11ba8d3` exists, is subject-labeled `task-068`, touches product surface (`crates/gyre-domain/src/view_query_resolver.rs`, `crates/gyre-server/src/explorer_ws.rs`, `crates/gyre-server/src/mcp.rs`, `crates/gyre-server/tests/graph_integration.rs`), and its SHA is absent from `specs/tasks/task-068.md` at base — the squash-drift class recorded by tasks 213/219/222 (a squashed landing commit cannot contain its own SHA, so the recorded list stayed one entry short and the +1096-line landing surface was invisible to review scoping).
3. **Gate passes at candidate** (`task-224-review-attribution-after.txt`, `task-224-review-final.txt`): exit 0 — `OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift).` Full history present (1233 commits, 217 task-labeled), so this is a real scan, not a shallow-checkout SKIP.
4. **Mutation check — pass is attributable to the repair, not gate drift** (`task-224-review-mutation.txt`): with the candidate checked out, `git checkout a11ba8d3 -- specs/tasks/task-068.md` (revert just the repair) re-fails the gate with the identical violation (exit 1); `git checkout 6e5d7ced -- specs/tasks/task-068.md` (re-apply) re-passes (exit 0). Tree restored clean afterward.
5. **No weakening of any check or freeze** (`scripts/` diff empty; `scripts/task-commit-attribution-exemptions.txt` unchanged at its frozen 3 entries; `git log base..candidate` is a single `implement(task-224)` commit; no exemptions, skips, or `FROZEN_EXEMPTION_COUNT` changes).
6. **task-224's own `commits: []` is correct** (`task-224-review-attribution-nochange.txt`): `python3 scripts/dev-attribution.py task-224` exits 0 and produces zero working-tree change — the repair round adds only task records, no product surface. No self-labeling drift introduced.
7. **Frontmatter integrity**: the candidate's `commits:` line parses as a valid JSON array of 10 unique 40-hex SHAs, the last being `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0`; no duplicate entries.

## Notes

- The shipped record's stash-based mutation description is imprecise about mechanics (stashing an already-committed change is a no-op; the implementer's own evidence files were produced against a then-uncommitted tree). My independent mutation probe (evidence item 4) establishes the same causal fact by a different mechanism, so this does not affect the verdict.
- No runtime surface was touched (markdown-only diff), so no server/browser probe is applicable; the sandbox's TCP-listener restriction (`/tmp/stage/capabilities.json`) is likewise irrelevant here. Full deterministic gates and GitHub checks on the exact PR head remain the verifier's responsibility.
