# Review — task-214 (Repair verified failure on main f4acb4ebcaf9)

Candidate: `c527e47a8256fde298680ea476abbf02c2e30852` (merge of assigned base `1a00087c` into
`pipeline/task-214/34994f34c4eb4d35aae1a2abdebfe953-5`). Net diff over base: 5 files
(`explorer_ws.rs`, `gate_executor.rs` (doc comment only), `lib.rs`, `middleware.rs`,
`specs/tasks/task-214.md`) — no scripts, gates, or exemption files touched.

Verdict: **approved**.

## Probes (all at candidate `c527e47a`, evidence in `/tmp/stage/review-evidence/`)

- `bash scripts/check-task-commit-attribution.sh` → **OK, exit 0** (`attribution-at-candidate.txt`).
  The verified baseline failure (task-189 ship commit `f4acb4eb` absent from frontmatter) is
  repaired — the full SHA is recorded in `specs/tasks/task-189.md`'s `commits:` list (present at
  the assigned base via main; preserved by the candidate). The task-155 drift repair
  (`27bd585c` in `specs/tasks/task-155.md`) is likewise present.
- Frozen gates: `check-byte-slice-truncation.sh` OK, `check-in-memory-state-stores.sh` OK,
  `check-relative-path-defaults.sh` OK (`frozen-gates-at-candidate.txt`).
- `python3 scripts/check-rustfmt-diff.py 1a00087c` → changed lines clean, 4 Rust files
  (`rustfmt-at-candidate.txt`).
- `cargo test -p gyre-server --lib explorer_ws::` → **42 passed, 0 failed**
  (`cargo-test-explorer-ws.txt`), including all 7 new tests: 4 `ExplorerSessionRegistry` tests
  (per-instance isolation, oldest-first eviction with exact-signal assertions, idempotent
  release, zero-cap admission) and 3 `truncate_invalid_query_preview` tests (multibyte backs
  off to byte 498 — the real char boundary for 3-byte chars under a 500 cap; ASCII keeps
  exactly 500; short input passthrough).
- `cargo test -p gyre-server --test explorer_ws_integration` → **8 passed, 0 failed**
  (`cargo-test-explorer-ws-integration.txt`). Note: this sandbox's listener probe is supported
  (`/tmp/stage/capabilities.json`), so unlike prior rounds the integration binary ran locally.
  All 8 pass, including `explorer_ws_message_too_long` — the durable verification finding's
  recorded failure — and `explorer_ws_session_limit_is_per_server_instance`.
- **Kill-test (bug-injection):** in a throwaway worktree at the candidate,
  `handle_explorer_session` was rewired to a process-global `OnceLock<Arc<ExplorerSessionRegistry>>`
  (the original bug) with everything else untouched. `explorer_ws_session_limit_is_per_server_instance`
  then **FAILED** with exactly the durable finding's symptom — "Session replaced by a newer
  connection." eviction delivered to a live session on another server
  (`kill-test-process-global.txt`, exit 101). The test genuinely detects the bug class; the
  candidate's per-AppState registry is what fixes it. Worktree removed; checkout untouched.

## Assessment

- The baseline failure class (task-189/task-155 attribution drift) is repaired by frontmatter
  appends — the check's documented remedy — with no exemption entries added
  (`task-c....txt` frozen at its 3-entry baseline; the candidate's diff touches no `scripts/` file).
- The explorer WS session-cap fix is real and meaningful: the registry is scoped to `AppState`,
  `SessionGuard` releases panic-safely on drop, eviction signals exactly the oldest session, and
  the fix is kill-verified above. Net-new over the assigned base (which already carries a
  task-217 variant of the registry): a genuine defect fix — `register(user, 0)` panicked on
  `Vec::remove(0)` of an empty slot list (reachable via `GYRE_EXPLORER_MAX_SESSIONS=0`); the
  candidate treats a 0 cap as 1, admitting the registering session, with a unit test pinning it —
  plus 7 unit tests the base lacked, and the `truncate_invalid_query_preview` helper extracted
  from base's inline logic with identical semantics.
- No check weakened, no skips or exemptions added, no blocked feature implemented.
- Task record (`specs/tasks/task-214.md`) lists all three product commits
  (`52a7d4af`, `8e585249`, `96d2a28c`) in `commits:` frontmatter; the attribution check confirms.

## Notes (not findings)

- The shipped notes state the WS integration binary cannot run in this sandbox (ENOTSUP) and
  expect 7/7. That restriction is stale for this environment: the current sandbox supports
  listeners and the suite is now 8 tests (the 8th, `explorer_ws_session_limit_is_per_server_instance`,
  arrived with the base merge). Observed: 8/8.
- The remaining diff over base (visibility widening `pub(crate)` → `pub`, renames
  `unregister` → `release`, `next_session_id` → `next_id`) is cosmetic merge resolution; fields
  stay private and behavior is unchanged.
