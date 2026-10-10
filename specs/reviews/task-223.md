# Review — task-223 (Repair verified failure on main a11ba8d32859)

Spec: `specs/GOAL.md` — real implementations and meaningful verification. Task: repair the verified upstream attribution-gate failure at base `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0` without weakening checks, skips, or exemptions.

Commit under review: `00ef433833a0f2148de639d90d141bf38b58de77` (assignment base `18c44f1a0f278ca78ce0c8c3ec5e5761d40cd36b`). Round 2: the assigned round-1 repair (task-068 `a11ba8d3` drift) landed on main independently as task-224 (`770785f7`), the round-2 re-verification baseline (`05709c24` task-196 drift) was repaired by prerequisite task-227 (`18c44f1a`), and this candidate merges the round-1 checkpoint (`9739dbe3`) with that landing.

Verdict: **approved**.

## Round 1

All probes run in this review sandbox at the exact candidate SHAs (detached worktrees; main checkout left pristine). Evidence files under `/tmp/stage/review-evidence/` carry the full command output.

Reproduction chain (independently re-run, not trusted from the record):

- Original failing base `a11ba8d32859...` (detached worktree): `bash scripts/check-task-commit-attribution.sh` → exit 1, violation `a11ba8d3 task-068 feat(task-068): Graph Summary & Dry-Run MCP Tools`. Evidence: `task-223-repro-at-original-failing-base-a11ba8d3.txt`.
- Round-2 repair base `05709c242509...` (detached worktree): exit 1, violation `05709c24 task-196 feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation` — matches the durable finding exactly. Evidence: `task-223-repro-at-repair-base-05709c24.txt`.
- Assignment base `18c44f1a` (prerequisite landed): exit 0. Evidence: `task-223-gate-at-assignment-base-18c44f1a.txt`.
- Round-1 checkpoint `9739dbe3`: exit 0; its diff vs parent `a11ba8d3` is exactly the one-line append of `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0` to `specs/tasks/task-068.md`'s `commits:` frontmatter plus the task-223 record. Evidence: `task-223-gate-at-round1-checkpoint-9739dbe3.txt`.
- Candidate HEAD `00ef4338`: exit 0. Evidence: `task-223-attribution-at-candidate-head.txt`.

Merge soundness: merge commit `99faec5c` has parents `9739dbe3` + `18c44f1a`; both parents carried byte-identical `specs/tasks/task-068.md` (the task-224 landing and the round-1 checkpoint applied the identical append), so the merge records each repair SHA exactly once (verified: `grep -c` → 1 occurrence each in task-068/task-196; task-068 has 10 SHAs, task-196 has 4, matching the record).

Load-bearing checks (test-the-repair, run in a throwaway worktree at the candidate HEAD, restored after each):

- Removing `05709c242509...` from task-196's frontmatter → gate exits 1 with the identical violation; restoring → exit 0. Evidence: `task-223-mutation-remove-task196-sha.txt`.
- Removing `a11ba8d32859...` from task-068's frontmatter → gate exits 1 with the identical violation; restoring → exit 0. Evidence: `task-223-mutation-remove-task068-sha.txt`.

Both passes are attributable to the recorded SHAs, not gate drift.

No weakening:

- `git diff 18c44f1a..00ef4338 -- scripts/ crates/ web/` → empty (0 lines). Only `specs/tasks/task-223.md` differs from the assignment base (174 added lines, the record itself).
- `scripts/task-commit-attribution-exemptions.txt` unchanged at its frozen 3-entry baseline (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`); `FROZEN_EXEMPTION_COUNT=3` in the gate script — no new exemptions, no frozen-count raise.
- The repaired commits (`a11ba8d3`, `05709c24`) genuinely touch product surface (`crates/gyre-domain/src/view_query_resolver.rs` + `crates/gyre-server/src/{explorer_ws.rs,mcp.rs}` + `crates/gyre-server/tests/graph_integration.rs`; `crates/gyre-server/src/api/graph.rs` + `web/src/{components/Briefing.svelte,lib/InlineChat.svelte,__tests__/Briefing.test.js}`), so recording them in frontmatter is the check's documented remedy, not a bypass of it.

Attribution correctness for this task: `python3 /tmp/stage/dev-attribution.py task-223` produces no change and exits 0 — correct, because both branch-only commits (`9739dbe3`, `00ef4338`) touch only `specs/tasks/`, no product surface, so `commits: []` is right.

Full deterministic gate set at candidate HEAD (all exit 0): check-arch, check-hierarchy (GYRE_CHECK_HIERARCHY=1 enabled mode), check-abac-route-registry, check-abac-exempt-handlers, check-mcp-write-tools, check-migration-versions, check-migration-sql-portability, check-dead-message-kinds, check-byte-slice-truncation, check-relative-path-defaults, check-fail-open-ref-resolution, check-mem-port-contracts, check-fabricated-scope-defaults, check-lossy-secret-conversion, check-scope-literal-defaults, check-inert-enforcement, check-in-memory-state-stores, check-unbounded-external-http, check-forwarded-header-trust, check-forged-scope-fields, check-task-commit-attribution. Evidence: `task-223-all-shell-gates-at-candidate-head.txt`. rustfmt/clippy changed-lines: 0 Rust files changed vs base, so trivially clean; `git diff --check` clean.

Record accuracy spot-checks:

- The cited durable recurrence cause is real: `scripts/pipeline/stages.py` `publish()` learns the squash `mergeCommit.oid` (line 284) and returns it only in delivery metadata; nothing appends it to the task's `commits:` frontmatter on main. Tasks 215 and 222 both already flagged this as needing its own task — the record's "out of scope for this repair round" stance is consistent with prior rounds' precedent and does not weaken any gate.
- The baseline-failure text embedded in the record matches the host `/tmp/stage/findings.log` and the durable finding verbatim (violation line and GYRE_BASELINE_FAILURE_JSON probe).
- The `commits: []` and 10/4-SHA claims in the record were each re-derived independently (grep counts above).

Findings: none blocking.

- (minor, non-blocking) The task file embeds host-side absolute paths (`/home/jsell/code/gyre/...`, `/tmp/stage/dev-attribution.py`) in its evidence narrative — provenance convention used by prior repair rounds (task-224/227/228 records do the same), not a defect.
- (note) No HTTP/browser probe is applicable: the candidate touches no runtime surface (0 Rust/JS lines), and this sandbox cannot accept TCP anyway (`capabilities.json`: `accept(): [Errno 95] Operation not supported`). Exact-head GitHub checks remain mandatory at host verification, per the standing review protocol.

Merge readiness: the candidate's net diff vs assignment base is `specs/tasks/task-223.md` only; current `origin/main` (`06d70009`, task-228's record-only landing on top of `18c44f1a`) is a strict descendant of the assignment base with no intersecting file, so the candidate merges cleanly with no regression risk to any gate.
