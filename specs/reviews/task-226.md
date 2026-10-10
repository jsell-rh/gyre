# Review — task-226: Repair verified failure on main a11ba8d32859

**Candidate:** `fcb5e59d270233d6b8235c83ad0845c5622b50c8`
**Assigned base:** `34248324e1e5546b84697d277604a8aaa35375af`
**Verdict:** approved (independent evidence below; probes under `/tmp/stage/review-evidence/`)

## Assignment contract

Reproduce and repair the verified upstream failure on main `a11ba8d3` — the
task-068 landing squash commit absent from `specs/tasks/task-068.md`'s
`commits:` frontmatter, failing `scripts/check-task-commit-attribution.sh`.
No weakened checks, no skips/exemptions, no product-source changes without
review. Prerequisite task-229 recorded in `depends_on`.

## What the candidate contains

- `git diff 34248324 fcb5e59d` is exactly one file: `specs/tasks/task-226.md`
  (new, +167) — the task record with the assignment contract, baseline log,
  round-1 repair narrative, and the merge-round narrative.
- `git diff 34248324 fcb5e59d -- scripts/ crates/ web/` is empty; the
  exemption file is at its frozen 3-entry baseline; `FROZEN_EXEMPTION_COUNT=3`
  unchanged in the gate script.
- History: round-1 repair `607b8b2f` (recorded `a11ba8d3` in task-068.md,
  the check's own documented remedy) → merge `0b312332` of assigned base
  `34248324` → checkpoint `fcb5e59d`. `task-068.md` is byte-identical on both
  merge sides (main landed the same repair as `770785f7`/task-224), so the
  merge was a no-conflict formality, not a divergence.
- `commits: []` is attribution-canonical for this round: the branch delta vs
  the assigned base is specs-only, so there is no product-surface commit to
  attribute to task-226. Verified with `dev-attribution.py task-226` (exit 0).

## Independent evidence (all probes run by this reviewer at the exact commits)

1. **Reproduction at the failing base** — detached worktree at
   `a11ba8d3`, `bash scripts/check-task-commit-attribution.sh` → exit 1
   with the identical violation `a11ba8d3 task-068 feat(task-068): Graph
   Summary & Dry-Run MCP Tools` (probe1).
2. **Gate at candidate HEAD** — exit 0 (probe0). Gate at assigned base
   `34248324` — exit 0 (probe2); the task-068 and task-196 (prerequisite
   task-229) repairs were already on main at that point.
3. **Mutation tests** — removing `a11ba8d3…` from task-068.md's frontmatter
   re-fails the gate with the exact baseline violation (probe3, exit 1);
   restoring re-passes (probe4, exit 0). Removing `05709c24…` from
   task-196.md re-fails with that exact violation (probe5, exit 1); restore
   re-passes (probe6, exit 0). The pass is attributable to the recorded
   SHAs, not gate drift. (One probe methodology note: substituting a
   marker string containing the SHA still passes because the frontmatter
   parser extracts `[0-9a-f]{7,40}` substrings; only full removal is a
   valid mutation. The recorded probes use full removal.)
4. **No weakening** — scripts/, crates/, web/ byte-identical to base;
   exemption count 3; no new exemptions.

## Checks for verification

- Full `tools/checks.sh` on the exact PR head (the changed-lines rustfmt/
  clippy surface here is 0 Rust files, but the suite gate must still run).
- Full workspace `cargo test --all` and `web` test suite at verification
  (no code changed vs base; expected to match base results).
- GitHub CI (fetch-depth: 0) on the PR head for the attribution gate.

## Residual notes

- The candidate's task record claims listener-probe restrictions (errno 95)
  which match `/tmp/stage/capabilities.json`; no runtime surface was touched,
  so no HTTP probe applies to this round.
- This round's only defensible output is documentation-of-record: the actual
  gate-passing repair (SHA recording) predates the assigned base. That is the
  correct, precedent-consistent shape (task-222/224), not a defect — but it
  means the round adds no new production behavior, by design of the
  repair-task assignment.
