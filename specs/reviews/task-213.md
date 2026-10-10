# Review: task-213 — Repair verified failure on main 8c2d17750585 (Round 4)

- **Candidate:** `ffd0956dd6a1e830e7dfd7bded4a6b2eba834e26`
- **Assigned base:** `e96d25abcdbb51f8890ea36d11541bfa9f80a2b8` (task-219 repair, merged via `e203e764`)
- **Failure base:** `6bf777a6a44f28052ed5af28bf6fb013fde6df48`
- **Verdict:** **Approved** (independent evidence supports the task contract)

## Scope

Round-4 assignment for the recurring attribution-drift repair task. The baseline failure (durable findings, `5f34f02fdb1d470c8e88f7dcd9003a60`): `scripts/check-task-commit-attribution.sh` exits 1 naming `6bf777a6 task-200` — the squashed task-200 ship commit absent from `specs/tasks/task-200.md`'s `commits:` frontmatter.

## Candidate delta

`git diff e96d25ab..ffd0956d` touches only `specs/tasks/task-213.md` (+94 lines): process/docs commits recording the round-3 verification narrative. No production code, no scripts, no checks, no exemptions. The repair itself (`e96d25ab`, task-219: appending full SHA `6bf777a6a44f28052ed5af28bf6fb013fde6df48` to task-200's frontmatter, 13→14 entries) is in the assigned base, carried via merge `e203e764`; `depends_on: [task-215, task-219]` records both prerequisites.

## Independent probes (evidence: /tmp/stage/review-evidence/task-213-r4-*)

1. **Baseline reproduction** — detached worktree at `6bf777a6`: gate exits 1 with the exact recorded violation (`6bf777a6 task-200 feat(task-200): Message bus — per-kind payload schema validation...`). `task-213-r4-baseline-repro.txt`.
2. **Gate at candidate** — at `ffd0956d`: gate exits 0, "OK: every task-labeled product-surface commit is recorded". `task-213-r4-attribution-at-candidate.txt`.
3. **Mutation check** — detached worktree at `ffd0956d`, removed the repair SHA from task-200's frontmatter: gate re-fails with the identical violation, exit 1. The pass is attributable to the recorded repair, not gate drift. `task-213-r4-mutation-check.txt`.
4. **No weakening** — `git diff` across `6bf777a6..ffd0956d` and `8c2d1775..ffd0956d` shows zero changes under `scripts/`; exemption file at its frozen 3 entries; no product-surface files (`crates/`, `web/`) touched anywhere in `6bf777a6..ffd0956d` (verified via name-only log + grep, exit 1 = none found).
5. **Frontmatter integrity** — task-200 `commits` parses as a 14-entry JSON list with the repair SHA as final entry; task-189 (6 entries, carries `f4acb4eb`) and task-210 (9 entries, carries `a781ede2`) prior repairs intact. `git diff --check 6bf777a6..ffd0956d` clean. `python3 /tmp/stage/dev-attribution.py task-213` produces no change — `commits: []` is correct for this specs-only task.

## Task-contract analysis

The required behavior is "reproduce and repair the verified upstream failure... correct a genuinely broken test setup; do not weaken checks, add skips or exemptions." Main repaired this independently as task-219 (in the assigned base); this branch's job was to carry that repair in its lineage and record the verification — which it does. The candidate delta contains no code, so the only risks were (a) the repair being fake or ineffective, (b) the pass coming from gate drift rather than the repair, and (c) scope creep into unrelated files. All three are disproven by the probes above: the failure is real pre-repair, the repair clears exactly it (mutation check), and the gate machinery is byte-identical from the failure base to the candidate.

## Out of scope for this review

Sandbox TCP listener probe unsupported (`accept` errno 95, `/tmp/stage/capabilities.json`) — irrelevant for a specs-only change with no runtime surface. Full workspace suites, all-target Clippy, and exact-head GitHub checks remain with verification and publication.
