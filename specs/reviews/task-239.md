# Review — task-239 (Repair verified failure on main b1cdb0ddd80e)

Candidate: `c0cef9de3de2f2348f0eef201a3c60bdddcbdc18` (branch
`pipeline/task-239/3e009d368c864d69bb3e924fa36047c8-1`), base
`fc63ab6002b698ed079517a2f38c96f455b0973e`. Product commits in range:
`483bf8ad` (explorer WS session registry), `3e62cdbb` (UTF-8-safe preview
truncation), plus process/checkpoint commits and `specs/tasks/task-170.md`
frontmatter repair.
Verdict: **approved**.

## Baseline reproduction

- `bash scripts/check-task-commit-attribution.sh` at base `fc63ab60` →
  **exit 1** (`b1cdb0dd task-170` missing from task-170's `commits:` list) —
  the verified upstream failure reproduces exactly. At candidate → **exit 0**.
  The repair appends the full SHA to `specs/tasks/task-170.md` frontmatter —
  the check's own documented remedy, not an exemption or weakening.
- The byte-slice truncation failure does not reproduce at base itself (the
  line-pinned `explorer_ws.rs:2882` exemption still matches there); it
  reproduces at the intermediate merge state `19d859bc` (base + the branch's
  own `483bf8ad`, which shifted the site to line 2921): gate → **exit 1**,
  flagging `&raw_preview[..500]` at `explorer_ws.rs:2921`. At candidate →
  **exit 0**.

## Fix verification (evidence in `/tmp/stage/review-evidence/`)

- **Real fix, not re-exemption**: `explorer_ws.rs` now truncates the invalid
  view-query preview through `crate::gate_executor::truncate_bytes`
  (char-boundary-safe, made `pub(crate)`). The `explorer_ws.rs:2882` exemption
  entry is **deleted** — the list shrinks, no new entries, no gate weakened.
- **Mutation check 1 (gate)**: reverting the fix at the candidate back to
  `&raw_preview[..500]` → gate fails again (exit 1, flags the site); restored →
  exit 0. The fix is load-bearing.
- **Mutation check 2 (test)**: rewiring both `AppState` constructors to share
  one process-global registry (simulating the old buggy behavior) makes the
  new integration test `explorer_ws_session_limit_is_per_server_instance`
  **fail** with the exact eviction error ("Session replaced by a newer
  connection"). Restored → passes. The test is load-bearing, not
  self-confirming.
- **Focused tests**: `truncate_bytes` unit tests 3/3 (including the
  multibyte-boundary panic case); explorer WS integration suite **8/8**
  including the new per-server-instance test.
- **Registry design**: `ExplorerSessionRegistry` is owned by `AppState`
  (`Arc`, shared by middleware-test clones), keyed `(tenant_id, agent_id)`,
  eviction and `SessionGuard` drop semantics identical to the prior code —
  only the storage scope changed from process-global to per-instance. The
  field is intentionally not port-backed (WebSocket sessions cannot outlive
  the server instance); `check-in-memory-state-stores.sh` passes and the
  doc comment justifies the ephemerality.
- **Related gates at candidate**: `check-byte-slice-truncation`,
  `check-task-commit-attribution`, `check-in-memory-state-stores`,
  `check-arch`, `check-migration-versions`,
  `check-relative-path-defaults` — all exit 0.
- **Formatting**: `rustfmt --check` flags pre-existing drift only on lines
  unchanged vs base (sandbox rustfmt version drift, matching the shipped
  note); no fmt diff on any line this candidate changed.

## Notes

- `max_sessions_per_user()` returning 0 (env var `GYRE_EXPLORER_MAX_SESSIONS=0`)
  would make the eviction loop `remove(0)` on an empty Vec — but this behavior
  is byte-identical at base (pre-existing, unchanged by this diff) and outside
  this repair task's scope.
- Exemption file went from 19 to 18 lines (deletion only), per the file's own
  "when you fix one, DELETE its exemption line" policy.
