# Review — task-226 (Repair verified failure on main a11ba8d32859)

Candidate: `607b8b2f437a49c55d06a55c3b2377e2eec32a8f` (branch `pipeline/task-226/410ff383286c4cd2aaf5ac63cc29a026-1`), assigned base `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0` — the base IS the failing commit (the task-068 squash landing).
Task contract: reproduce and repair the verified upstream failure; no weakened checks, no skips/exemptions, no fabricated work; independent review plus full verification and GitHub checks on the exact head.
Verdict: **approved** — the failure is genuinely repaired at the candidate, the repair is exactly the gate's own documented remedy, and the candidate adds no product-surface or gate changes.

## What the candidate is

`git diff a11ba8d3..607b8b2f --name-only` shows exactly two files: `specs/tasks/task-068.md` (1 insertion, 1 deletion) and `specs/tasks/task-226.md` (+111, the task record). The single substantive change appends `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0` as the last element of task-068's `commits:` frontmatter array (9→10 entries, no removals, no other edits to the file; verified by full-SHA extraction before/after). `git diff a11ba8d3 607b8b2f -- scripts/ crates/ web/` is empty — zero Rust/JS/gate surface touched.

The failure class is the structural drift task-222 repaired for `6bf777a6`/task-200: the squash-landing commit cannot contain its own SHA in the task file baked inside it, so the recorded list stayed one entry short. The remedy is precisely what the gate's failure message prescribes ("add the short SHA to specs/tasks/task-NNN.md's commits: frontmatter. Do NOT add entries to ...exemptions.txt").

## Independent probes (all run in this checkout at candidate `607b8b2f` unless noted; evidence under `/tmp/stage/review-evidence/`, commands in `task-226-review-commands.txt`)

- **Failure reproduces at the exact base commit.** Detached worktree at `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0`: `bash scripts/check-task-commit-attribution.sh` → exit 1 with the identical violation `a11ba8d3  task-068  feat(task-068): Graph Summary & Dry-Run MCP Tools` (`task-226-reproduce-base-a11ba8d3.txt`). The commit touches `crates/gyre-common`, `crates/gyre-server`, `web/` (MCP graph_summary/dryrun tools) — product surface, so the gate's requirement applies to it.
- **Gate passes at candidate.** At HEAD `607b8b2f`: exit 0, "OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter" (`task-226-candidate-gate.txt`; re-confirmed after the mutation restore in `task-226-after-restore.txt`).
- **Mutation check — the pass is attributable to the recorded SHA, not gate drift.** With the SHA removed from task-068's frontmatter at candidate (`sed` removing the array's last element), the gate re-fails with the identical violation (exit 1, `task-226-mutation-check.txt`); `git checkout --` restore re-passes (exit 0, `task-226-after-restore.txt`). Tree left clean; probe worktree removed.
- **No gate weakened, no exemption added.** `git diff a11ba8d3 607b8b2f -- scripts/` is empty (0 lines). `scripts/task-commit-attribution-exemptions.txt` has exactly 3 active entries (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`), matching `FROZEN_EXEMPTION_COUNT=3` at `scripts/check-task-commit-attribution.sh:31`; no exemption exists for `a11ba8d3` (`task-226-exemption-count.txt`, `task-226-scripts-diff-empty.txt`).
- **Recording the SHA is honest, not review-scope fakery.** The `a11ba8d3` content (graph_summary/dryrun/search MCP tools, `gyre_domain::view_query_resolver`) was genuinely examined and approved in the task-068 review (`specs/reviews/task-068.md`, round-2 **complete** verdict with mutation probes and real-store tests; the landing commit message records candidate `bf4b2e1a` and links the review record). The appended SHA makes the squash landing visible to future review scoping rather than hiding anything.
- **Task bookkeeping correct.** `commits: []` in task-226's frontmatter is right: the branch adds only the task record plus the frontmatter line in task-068.md, no product surface; the candidate's `process(task-226):` subject and `specs/`-only diff place it outside the gate's product-surface scope by the gate's own rules (subject-type skip + path filter). The `process:`/`review:` skip in the gate (line 103-105) predates this change and is not modified by it.
- **Baseline-log clippy/rustfmt lines correctly dispositioned.** The baseline log's warning lines reference foreign-checkout paths (`attempts/42ffd2b11fe34efc96bb6c91a66472b5/1/checkout`) and report "changed lines clean (0 Rust files checked)" — this branch changes no Rust files (`git diff a11ba8d3 607b8b2f --stat -- crates/ web/` empty), so diff-based rustfmt/clippy gates are trivially clean on the exact head. All other gates that passed at base are unaffected by a specs-only diff.
- **Transport restriction.** `/tmp/stage/capabilities.json`: `tcp_listener_probe.supported=false` (`accept(): [Errno 95] Operation not supported`). No runtime surface was touched, so no HTTP/browser probe is applicable. Exact-head GitHub checks remain mandatory for host verification; this approval does not waive them.

## Findings

- (none)

## Notes for the verifier

1. Run `bash scripts/check-task-commit-attribution.sh` at the exact PR head — must exit 0.
2. Confirm `git diff <base>..<head> --name-only` shows only `specs/tasks/task-068.md` and `specs/tasks/task-226.md`, and `git diff <base>..<head> -- scripts/ crates/ web/` is empty.
3. Confirm `scripts/task-commit-attribution-exemptions.txt` still has exactly 3 active entries.
4. GitHub checks (including the diff-based clippy gate) must pass on the exact head before merge.

— Independent reviewer, 2026-10-10. Evidence: `/tmp/stage/review-evidence/` (task-226-reproduce-base-a11ba8d3.txt, task-226-candidate-gate.txt, task-226-mutation-check.txt, task-226-after-restore.txt, task-226-exemption-count.txt, task-226-scripts-diff-empty.txt, task-226-review-commands.txt).
