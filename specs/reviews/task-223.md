# Review — task-223 (Repair verified failure on main a11ba8d3)

Candidate: `9739dbe3793ec55173716e640aa8209cfb27979c` (branch `pipeline/task-223/814d29e8274e4a87acc796a088777c9b-1`), assigned base `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0` — the base IS the failing commit (candidate is its direct child).
Task contract: reproduce and repair the verified upstream failure; no weakened checks, no skips/exemptions, no fabricated work; independent review plus full verification and GitHub checks on the exact head.
Verdict: **approved** — the verified failure is genuinely repaired at the candidate by the gate's own documented remedy, and the candidate adds no gate weakening, no exemptions, and no product-surface changes.

## What the candidate is

`git diff a11ba8d3..9739dbe3 --stat` touches exactly two files:

- `specs/tasks/task-223.md` (+131): the task record, byte-shape-identical to the task-222 repair-round template.
- `specs/tasks/task-068.md` (1 line): appends `"a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0"` to the `commits:` frontmatter — precisely the remedy the gate's failure message prescribes ("Fix by adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter. Do NOT add entries to ...exemptions.txt").

`git diff a11ba8d3..9739dbe3 -- scripts/ crates/ web/` is empty. Root cause analysis in the task record is accurate: `a11ba8d3` is the task-068 squash landing on main (product surface: `view_query_resolver.rs`, `explorer_ws.rs`, `mcp.rs`, `graph_integration.rs`); a squash commit cannot contain its own SHA in the task file it ships, so the recorded list was one entry short until this repair.

## Independent probes (all run in this checkout at candidate HEAD unless noted; evidence under `/tmp/stage/review-evidence/`)

- **Failure reproduces at the exact base.** Detached worktree at `a11ba8d3` (`git worktree add --detach`, removed after): `bash scripts/check-task-commit-attribution.sh` → exit 1 with the identical violation `a11ba8d3  task-068  feat(task-068): Graph Summary & Dry-Run MCP Tools` (`repro-at-base-a11ba8d3-task223.txt`). The failure is the only failing gate in the baseline log; every other gate line in the baseline is OK.
- **Gate passes at candidate HEAD.** `bash scripts/check-task-commit-attribution.sh` → `OK: every task-labeled product-surface commit is recorded...`, exit 0 (`attribution-after-task223.txt`).
- **Mutation check — the pass is attributable to the recorded SHA, not gate drift.** With the appended SHA removed from `specs/tasks/task-068.md`'s frontmatter, the gate re-fails with the identical violation (exit 1, `mutation-check-task223.txt`); restoring the file (verified `git status --short` clean, `git diff` empty) re-passes (exit 0, `mutation-restore-check-task223.txt`).
- **No gate weakened, no exemption added.** `git diff a11ba8d3..9739dbe3 -- scripts/` empty; `scripts/task-commit-attribution-exemptions.txt` has exactly 3 active entries (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`) matching `FROZEN_EXEMPTION_COUNT=3`; no exemption exists for `a11ba8d3`.
- **Recording the SHA is honest, not review-scope fakery.** `a11ba8d3` is real product surface (+1096/−37 across 4 crate files). Its four product-surface blobs are identical (`git rev-parse` per-path comparison) to `209465f0` (`verify(task-068): reconcile candidate with main`), the landing that the task-068 review record (`specs/reviews/task-068.md`) examined across two rounds with mutation probes of the enforcement calls; `04f3a3d7` recorded independent approval. The recorded SHA makes the squash landing visible to scoping instead of hiding it. Gate-history analysis: the `verify:`/`process:`/`review:`/`implement:` side-branch commits touching crates are NOT ancestors of HEAD (only the squashes landed on main), so `a11ba8d3` is the sole task-068 labeled commit the gate scans — correctly now recorded.
- **Frontmatter integrity.** Both task files' `commits:` arrays parse as JSON; task-068 has 10 unique 40-hex entries (the new one last); the gate's `frontmatter_commits` parser (grep `[0-9a-f]{7,40}` → cut to 8) matches `a11ba8d3` for the full SHA. task-223's `commits: []` is correct: the branch touches only task bookkeeping, no product surface — `dev-attribution.py`'s scoping rule (product-surface paths only) yields the empty list.
- **Static gate battery re-run at candidate HEAD** (baseline-log gates): `check-arch`, `check-hierarchy`, `check-abac-route-registry`, `check-abac-exempt-handlers`, `check-mcp-write-tools`, `check-migration-versions`, `check-dead-message-kinds`, `check-byte-slice-truncation`, `check-relative-path-defaults`, `check-fail-open-ref-resolution`, `check-in-memory-state-stores`, `check-unbounded-external-http`, `check-lossy-secret-conversion`, `check-scope-literal-defaults`, `check-fabricated-scope-defaults`, `check-inert-enforcement`, `check-migration-sql-portability`, `check-forged-scope-fields`, `check-forwarded-header-trust`, `check-mem-port-contracts`, `check-identical-baselines` — all PASS. `check-crypto-verify` FAILS with an awk syntax error **identically at the base commit** — pre-existing environment incompatibility (same class as task-200's recorded "36 scripts fail identically on the task base"), not a candidate defect. `python3 scripts/check-rustfmt-diff.py a11ba8d3` → clean, 0 Rust files (`rustfmt-diff-at-candidate.txt`); `git diff --check` clean.
- **Commit-message convention.** The branch's `implement(task-223): pipeline checkpoint` subject is the pipeline's own checkpoint convention (176 such commits in history); `check-commit-msg.sh` types apply to the published squash (`feat(task-223): ...` per the task-222 precedent `653a696f`). Not a candidate defect.
- **Durable recurrence-cause claim verified against source.** `publish()` in `scripts/pipeline/stages.py` (lines 281–292) learns the squash `mergeCommit.oid` and stores it only in delivery metadata; it never appends the landed SHA to the task's frontmatter on main, so every future product-surface ship re-creates this drift. Confirmed by reading the code this round. A pipeline-side fix is out of scope for this repair round (new product surface in `scripts/pipeline/`, its own task; the task instruction also forbids implementing beyond the repair).
- **Transport restriction.** `/tmp/stage/capabilities.json`: `tcp_listener_probe.supported=false` (`accept(): [Errno 95] Operation not supported`). No runtime surface was touched, so no HTTP probe is applicable. Exact-head GitHub checks remain mandatory for host verification; this approval does not waive them.

## Findings

- (none)

## Notes for the verifier

1. Run `bash scripts/check-task-commit-attribution.sh` at the exact PR head — must exit 0.
2. Confirm `git diff a11ba8d3..<head> --stat` contains only `specs/tasks/task-068.md` (1 line) and `specs/tasks/task-223.md` (new), and `git diff a11ba8d3..<head> -- scripts/ crates/ web/` is empty.
3. Confirm `scripts/task-commit-attribution-exemptions.txt` still has exactly 3 active entries.
4. GitHub checks (including the cold full-clippy gate — this branch changes 0 Rust files, base passed CI on main) must pass on the exact head before merge.

— Independent reviewer, 2026-10-10. Evidence: `/tmp/stage/review-evidence/` (repro-at-base-a11ba8d3-task223.txt, attribution-after-task223.txt, mutation-check-task223.txt, mutation-restore-check-task223.txt, rustfmt-diff-at-candidate.txt, gate-*.txt battery).
