# Review — task-222 (Repair verified failure on main 6bf777a6a44f)

Candidate: `894a0f57fccc178e2f95f1cfd90433721c68dba9` (branch `pipeline/task-222/cc51ed0a11fe4ba99e3e3c7a329dd52b-1`), assigned base `e96d25abcdbb51f8890ea36d11541bfa9f80a2b8`, upstream failing commit `6bf777a6a44f28052ed5af28bf6fb013fde6df48`.
Task contract: reproduce and repair the verified upstream failure; no weakened checks, no skips/exemptions, no fabricated work; independent review plus full verification and GitHub checks on the exact head.
Verdict: **approved** — the verified failure is genuinely repaired at the candidate, the repair is the check's own documented remedy, and the candidate adds no product-surface or gate changes.

## What the candidate is

`git diff e96d25ab..894a0f57` touches exactly one file: `specs/tasks/task-222.md` (+89 lines, the task record). `git diff 6bf777a6..894a0f57 --stat -- scripts/ crates/ web/` is empty. The actual repair of the verified failure landed in the assignment base itself as task-219 (ship commit `e96d25ab`, the assignment base, merged to main): appending `6bf777a6a44f28052ed5af28bf6fb013fde6df48` to `specs/tasks/task-200.md`'s `commits:` frontmatter. Sibling repairs of the same verified failure exist in history (`4635b533` task-220, `5bacbc27` task-221) — the pipeline re-dispatched after the repair merged, the same duplicate-dispatch pattern tasks 215/216/218 documented for the `f4acb4eb` drift. A fresh code change on this branch would have been fabricated work; the record instead documents reproduction and verification with evidence. That is the correct resolution for a re-dispatched already-repaired failure.

## Independent probes (all run in this checkout at candidate unless noted; evidence under `/tmp/stage/review-evidence/`)

- **Failure reproduces at the failing commit.** Detached worktree at `6bf777a6` (`git worktree add --detach`): `bash scripts/check-task-commit-attribution.sh` → exit 1 with the identical violation `6bf777a6 task-200 feat(task-200): Message bus — per-kind payload schema validation (reject invalid payloads with 400)` (`repro-at-6bf777a6.txt`). The commit touches `crates/gyre-common/src/message.rs`, `crates/gyre-server/src/api/messages.rs`, `crates/gyre-server/src/mcp.rs` — product surface, so the gate's requirement applies.
- **Gate passes at candidate and at base.** `bash scripts/check-task-commit-attribution.sh` at `894a0f57` → OK exit 0 (`gate-at-candidate-894a0f57.txt`); at assignment base `e96d25ab` in a detached worktree → OK exit 0 (`gate-at-base-e96d25ab.txt`). Base is contained in `origin/main` (`git merge-base --is-ancestor` verified; main has since advanced to `73a31e0b` past the branch point, as expected).
- **Mutation check — the pass is attributable to the recorded SHA, not gate drift.** With the recorded SHA removed from `specs/tasks/task-200.md`'s frontmatter at candidate, the gate re-fails with the identical violation (exit 1, `mutation-remove-sha.txt`); restoring it (`git checkout --`) re-passes (exit 0, `mutation-restore-sha.txt`). Tree left clean after both directions.
- **No gate weakened, no exemption added.** `scripts/task-commit-attribution-exemptions.txt` has exactly 3 active entries (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`), matching `FROZEN_EXEMPTION_COUNT=3` in `scripts/check-task-commit-attribution.sh:31`; no exemption exists for `6bf777a6`. The repair is precisely the remedy the gate's failure message prescribes ("add the short SHA to specs/tasks/task-NNN.md's commits: frontmatter. Do NOT add entries to ...exemptions.txt").
- **Recording the SHA is honest, not review-scope fakery.** `specs/reviews/task-200.md` shows the `6bf777a6` content (message.rs schema validation, REST/MCP wiring) was genuinely examined across review rounds with mutation probes of the enforcement calls; the recorded SHA makes the squash landing commit visible to scoping instead of hiding it.
- **Baseline-log clippy/rustfmt lines correctly dispositioned as foreign-checkout artifacts.** The log's unprefixed lines reference `attempts/e50a8815.../1/checkout` paths. Against this history: `crates/gyre-adapters/src/call_graph.rs` is 245 lines at both `6bf777a6` and candidate (flagged line 269 does not exist); `crates/gyre-domain/src/call_graph_resolve.rs` contains zero `map_or` occurrences; the referenced line 306 sits inside a `#[tokio::test]` body. CI's clippy gate is diff-based (`check-clippy-diff.py` changed-lines semantics) and this branch changes no Rust files: `python3 scripts/check-rustfmt-diff.py 6bf777a6` → `changed lines clean (0 Rust files checked)` exit 0 (`rustfmt-diff-at-candidate.txt`). `git diff --check` over both ranges is clean. No repair action owed for those lines.
- **Task bookkeeping correct.** `commits: []` in task-222's frontmatter is right: the branch adds only the task record, no product surface; the repair belongs to task-219's record. `git diff --name-only 6bf777a6..e96d25ab` shows only `specs/tasks/task-200.md` and `specs/tasks/task-219.md`.
- **Durable recurrence-cause claim verified against source.** `publish()` in `scripts/pipeline/stages.py` (lines ~276–292) learns the squash `mergeCommit.oid` and stores it only in delivery metadata; it never appends the landed SHA to the task's frontmatter on main, so future product-surface ships will re-create this drift — matching task-215's durable finding. A pipeline-side fix is out of scope for this repair round (it would be new product surface in `scripts/pipeline/` requiring its own task and test coverage; the task instruction also forbids implementing beyond the repair).
- **Transport restriction.** `/tmp/stage/capabilities.json`: `tcp_listener_probe.supported=false` (`accept(): [Errno 95] Operation not supported`). No runtime surface was touched, so no HTTP probe is applicable. Exact-head GitHub checks remain mandatory for host verification; this approval does not waive them.

## Findings

- (none)

## Notes for the verifier

1. Run `bash scripts/check-task-commit-attribution.sh` at the exact PR head — must exit 0.
2. Confirm `git diff <base>..<head> --stat -- scripts/ crates/ web/` is empty and the diff contains only `specs/tasks/task-222.md`.
3. Confirm `scripts/task-commit-attribution-exemptions.txt` still has exactly 3 active entries.
4. GitHub checks (including the diff-based clippy gate) must pass on the exact head before merge.

— Independent reviewer, 2026-10-10. Evidence: `/tmp/stage/review-evidence/` (repro-at-6bf777a6.txt, gate-at-base-e96d25ab.txt, gate-at-candidate-894a0f57.txt, mutation-remove-sha.txt, mutation-restore-sha.txt, rustfmt-diff-at-candidate.txt, evidence-bundle.txt).
