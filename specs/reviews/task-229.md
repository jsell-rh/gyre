# Review: task-229 — Repair verified failure on main 05709c24

**Candidate:** `d4384cdea41ecfc939e6aac9c0db081838536bcb`
**Base:** `05709c242509b89214876339b3c463ede3a31b60`
**Verdict:** APPROVED (evidence under `/tmp/stage/review-evidence/`)

## Scope of change

Two files, both under `specs/tasks/`:

- `specs/tasks/task-196.md` — appended `05709c242509b89214876339b3c463ede3a31b60` to the existing 3-SHA `commits:` frontmatter array.
- `specs/tasks/task-229.md` — new task record (baseline failure log + shipped narrative).

No product surface, no scripts, no exemptions changed: `git diff base..candidate -- crates/ web/ scripts/` is empty (0 lines), including `scripts/task-commit-attribution-exemptions.txt` (still at its frozen 3-entry baseline: `01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`). The candidate commit itself touches no `crates/|web/src|web/tests` path, so `commits: []` in task-229 is consistent — this repair round adds only task records.

## Probes (focused, smallest meaningful)

1. **Reproduction at base** (`task-229-reproduction-at-base.txt`): checked out base `05709c24`, ran `bash scripts/check-task-commit-attribution.sh` → exit 1 with exactly the reported violation (`05709c24 task-196`). Failure is live at base, not fabricated.
2. **Repair at candidate** (`task-229-attribution-candidate.txt`): at candidate HEAD → exit 0, `OK: every task-labeled product-surface commit is recorded...`. Confirmed the check actually walks full history (`git log --no-merges`) — the pass covers the newly added commit too, not just the repaired entry.
3. **Mutation check** (`task-229-mutation-no-sha.txt`): removed the appended SHA from `task-196.md` at candidate HEAD (sed), re-ran the gate → exit 1 with the identical violation; restored the file (exit 0 again on re-run). The pass is attributable to the recorded SHA, not gate drift or history accident. The check's frontmatter parser (`grep -oE '[0-9a-f]{7,40}' | cut -c1-8`) does pick up the appended 40-char SHA — it is not silently ignored.

## Analysis

- The fix is the check's own documented remedy (script header lines 27–30 and the failure message itself: "Fix by adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter"). No check weakened, no skip added, no exemption grown, no blocked feature implemented.
- Root-cause class matches prior squash-drift repairs (tasks 213/219/222/224): the squashed landing commit `05709c24` is itself task-196-labeled product surface (touches `crates/gyre-server/src/api/graph.rs`, `web/src/components/Briefing.svelte`, `web/src/__tests__/Briefing.test.js` via its parent diff) and could not contain its own SHA in the task's branch-time frontmatter.
- Keeping the 3 prior branch SHAs plus the landed SHA matches the task-224→task-068 repair shape already on main.

## Notes for verification

- No runtime surface touched; no HTTP/browser probe applicable (sandbox cannot accept TCP — infra restriction, not a code defect).
- Host verification must still run the full deterministic gate suite and GitHub checks on the exact PR head `d4384cdea41ecfc939e6aac9c0db081838536bcb`, with full git history (fetch-depth: 0) for this check.
