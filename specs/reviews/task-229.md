# Review — task-229 (Repair verified failure on main 05709c242509)

Assigned candidate: `5a991c4f25159f287c49f2d39cf0ea74cb8cec22` (assignment base `db37fd02fcaf0b4ad1b00ca4abde6bf12e7dbae8`).
Verdict: **complete** — the baseline failure is repaired on the candidate, the repair is the gate's documented remedy, nothing was weakened, and the pass is attributable to the recorded SHAs (mutation-verified), not gate drift.

## What the failure was and what the repair is

`scripts/check-task-commit-attribution.sh` failed at the round-1 base `05709c24` because commit `05709c242509b89214876339b3c463ede3a31b60` (`feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation`, product surface: `crates/gyre-server/src/api/graph.rs`, `web/src/components/Briefing.svelte`, `web/src/__tests__/Briefing.test.js`) was absent from `specs/tasks/task-196.md`'s `commits:` frontmatter — the squash-drift class (the squashed landing commit cannot contain its own SHA; tasks 213/219/222/224 repaired the same class before). The round-2 base `db37fd02` carried the prerequisite task-232 repair (task-155 drift, `27bd585c`) plus the same task-196 repair landed independently by task-227/228.

The candidate's delta vs its assigned base is exactly one file, `specs/tasks/task-229.md` (+118 lines: the task record itself — frontmatter `depends_on: [task-232]` per this assignment, the Shipped narrative, and the merge-round section). `git diff db37fd02..5a991c4f -- scripts/ crates/ web/` is empty; `scripts/task-commit-attribution-exemptions.txt` is unchanged at its frozen 3-entry baseline (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`); no gate, skip, exemption, or check was modified anywhere in the delta. The candidate is a clean merge: merge commit `ec20df43` has parents `d4384cde` (this branch's round-1 repair) and `db37fd02` (the assignment base); the task-196 repair is byte-identical on both parents (`git diff d4384cde db37fd02 -- specs/tasks/task-196.md` is empty), so the merge dropped nothing.

Both recorded SHAs verified present in the task files: `specs/tasks/task-196.md` line 8 lists `05709c242509b89214876339b3c463ede3a31b60` (joining the branch SHAs `3b90956c`, `abcfff04`, `88b57180`, matching the task-224→task-068 repair shape that landed on main); `specs/tasks/task-155.md` lists `27bd585ca7eb429905ccbded1f48b4d0167c0c20` (landed by prerequisite task-231/232).

## Independent probes at the exact candidate `5a991c4f` (checkout is clean at HEAD; tree verified `git status` empty before and after each probe)

- **Gate at candidate**: `bash scripts/check-task-commit-attribution.sh` → exit 0, `OK: every task-labeled product-surface commit is recorded…`. Evidence: `task-229-r3-gate-at-candidate.txt`.
- **Mutation check (this task's own repair)**: `05709c24…` removed from `specs/tasks/task-196.md` frontmatter via sed → gate FAIL exit 1 naming exactly `05709c24 task-196 feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation`; restored via `git checkout --` → gate exit 0 again. The pass is attributable to the recorded SHA, not gate drift. Evidence: `task-229-r3-mutation-task196.txt`.
- **Mutation check (prerequisite repair)**: `27bd585c…` removed from `specs/tasks/task-155.md` → gate FAIL exit 1 naming exactly `27bd585c task-155 feat(task-155): Implement gyre search CLI command`; restored → exit 0. Evidence: `task-229-r3-mutation-task155.txt`.
- **Scope delta**: `git diff db37fd02..HEAD --name-status` = `A specs/tasks/task-229.md` only; exemptions file diff empty; `git show 5a991c4f` touches only `specs/tasks/task-229.md`. Evidence: `task-229-r3-scope-delta.txt`.
- **Merge integrity**: `ec20df43` parents are `d4384cde` + `db37fd02`; task-196 repair byte-identical across both parents. Evidence: `task-229-r3-merge-integrity.txt`.
- **Attribution correctness of `commits: []`**: `python3 /tmp/stage/dev-attribution.py task-229` produces no change. Verified independently against the script's logic (`/tmp/stage/dev-attribution.py`): the only non-merge commits in `origin/main..HEAD` are `d4384cde` and `5a991c4f`, both touching only `specs/tasks/` — no product surface, so the canonical list is empty. (Note: `origin/main` has since advanced to `e77537fa`, task-236, specs-only relative to `db37fd02` — a fast-forward for this branch, not a conflict.)
- **Whitespace gate that bit task-200**: `git diff --check db37fd02..HEAD` → exit 0; no trailing whitespace in `specs/tasks/task-229.md`.
- **All other deterministic gates** (arch, ABAC registry + exempt handlers, MCP write tools, migration versions + portability, dead message kinds, byte-slice truncation, relative path defaults, fail-open ref resolution, mem port contracts, fabricated scope defaults, lossy secret conversion, scope literal defaults, inert enforcement, forwarded header trust, in-memory state stores, unbounded external HTTP): all exit 0 at the candidate. Evidence: `task-229-r3-deterministic-gates.txt`.

## Notes

- The executor's round-2 evidence files referenced in the task record (`task-229-round2-*.txt`) were probed at merge head `ec20df43` and are no longer present in this round's `/tmp/stage/review-evidence/` (fresh stage); every claim they represented was re-derived independently at the exact candidate `5a991c4f` above, which is what review approval is scoped to.
- Full `checks.sh`, rustfmt/clippy on the full tree, complete Rust/frontend test suites, and GitHub checks on the exact PR head remain mandatory host-side verification; this review ran only the focused gates relevant to a specs-only delta.
- No runtime surface was touched by this task, so no HTTP/browser probe applies. The sandbox's TCP listener restriction (errno 95, `/tmp/stage/capabilities.json`) is an infrastructure limitation, not a code defect.

Findings:

- (none)

— Independent reviewer, 2026-10-10
