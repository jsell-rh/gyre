# Review — task-239 (Repair verified failure on main b1cdb0ddd80e)

Spec: `GOAL.md` — real implementations and meaningful verification.
Commits under review (scoping per `commits:` frontmatter): `483bf8ad` (explorer WS session-limit scoping), plus the round-1 repair it carries forward (`specs/tasks/task-170.md` attribution append, landed via checkpoint `7cd9dce6`) and the round-3 process commits restoring the task contract and recording `483bf8ad`.
Candidate: `3a878cf3b793738360445f3c0b00240163c3c34b`, base `8c391bc4`.
Verdict: **complete** (approved).

## Diff surface (base `8c391bc4` → candidate `3a878cf3`)

`crates/gyre-server/src/explorer_ws.rs` (+107/−35), `src/lib.rs` (+6), `src/mem.rs` (+1), `src/middleware.rs` (+1), `tests/explorer_ws_integration.rs` (+60), `specs/tasks/task-170.md` (one SHA appended), `specs/tasks/task-239.md` (task file). `git diff base..candidate -- scripts/ web/` is **empty** — no gate, check, exemption file, or frontend source touched.

## Round-1 repair (baseline gate failure): verified

Baseline failure: `scripts/check-task-commit-attribution.sh` exited 1 flagging `b1cdb0dd task-170` (squash-drift class). Repair appends the full SHA to `specs/tasks/task-170.md`'s `commits:` frontmatter — the check's own documented remedy — joining the 11 existing entries (kept, not collapsed). No exemption entry added.

- Gate at candidate: **exit 0** (`attr-gate-candidate.txt`).
- Mutation: removing the appended SHA via sed → **exit 1**, identical violation (`attr-gate-mutation-task170.txt`); restored → exit 0. The pass is attributable to the recorded SHA, not gate drift.

## Round-2 product fix (`483bf8ad`): real production code, verified

Root cause as described is genuine: the per-user concurrent explorer-WS session limit (default 3, oldest-eviction) lived in process-global statics (`ACTIVE_SESSIONS` + `SESSION_ID_COUNTER`); all dev-token connections share the `("default","system")` principal, so parallel `build_state()` test servers in one process evicted each other's mid-flight sessions — the verified `explorer_ws_save_and_load_view` panic.

The fix is a real structural change, not a test workaround:

- Statics removed; `ExplorerSessionRegistry` (same `std::sync::Mutex` + `Notify` mechanics, same eviction algorithm, byte-equivalent logic relocated) is owned by `AppState` (`pub(crate) explorer_sessions: Arc<ExplorerSessionRegistry>`), wired in `build_state`, the in-crate test constructor (`mem.rs`), and the middleware test-state clone. `SessionGuard` now holds an `Arc` to the registry and unregisters on drop — early-return/panic cleanup preserved.
- The AGENTS.md in-memory-state-store invariant is respected: the field is not a bare `Arc<Mutex<HashMap>>` alias, and the state is connection-lifetime-ephemeral (WS sessions cannot outlive their server instance), matching the existing `compose_sessions` precedent. `check-in-memory-state-stores.sh` → OK.
- The regression test `explorer_ws_session_limit_is_per_server_instance` asserts both directions: server A's oldest session survives a same-user connection on server B, **and** the cap is still enforced within one server (4th connection evicts oldest with a "replaced" error) — it pins the production behavior, not the test setup.

Probes (this checkout at `3a878cf3`, working tree clean):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --test explorer_ws_integration` → **8/8 passed, exit 0**, including the originally failing `explorer_ws_save_and_load_view` and the regression test (`explorer-ws-integration-candidate.txt`).
- **Test-the-repair mutation**: temp worktree at the candidate with only the four production files reverted to `483bf8ad^` (test kept) → the regression test **FAILED, exit 101**, panicking with the exact production symptom `Session replaced by a newer connection.` (`explorer-ws-mutation-unfixed-code.txt`). The test kills the real bug; it is not self-confirming. Worktree removed; checkout left clean.

## Round-3 contract restoration: verified

- The task-239 contract (frontmatter minus `commits:`, `## Required behavior`, `## Baseline failure`) is **byte-identical** between the round-1 checkpoint `7cd9dce6` and the candidate (diff of the extracted sections is empty). The round-2 normative additions ("Round 2 baseline failure" section, second Shipped) are gone.
- `483bf8ad` is kept in history and recorded in task-239's `commits:` frontmatter rather than silently dropped from review scope. Mutation: clearing that frontmatter to `commits: []` → attribution gate **exit 1** flagging `483bf8ad` (`attr-gate-mutation-task239.txt`); restored → exit 0. Recording the SHA is load-bearing.

## Notes (non-findings)

- `max_sessions_per_user()` returning `0` (env `GYRE_EXPLORER_MAX_SESSIONS=0`) would panic in `register`'s eviction loop — but the eviction loop is byte-equivalent to the pre-existing inline code; this diff neither introduces nor worsens it. Pre-existing, out of scope.
- The baseline log's `task-214.md:114` blank-line-at-EOF warning fired in the pipeline's baseline checkout state (pipeline-external `tools/checks.sh`); it is a warning, not the gate FAIL, and the file does not exist at this HEAD.

Findings: none. No check, skip, exemption, or gate weakened; no fakery; tests are anchored to the production behavior and verified to fail when it is disabled.

— Reviewer, independent review round (evidence under `/tmp/stage/review-evidence/`)
