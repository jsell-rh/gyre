---
title: "HSI Conflict Prevention — Concurrent Spec Editing Warning"
spec_ref: "human-system-interface.md §7 Conflict Prevention"
depends_on: [task-237]
progress: complete
review: specs/reviews/task-092.md
coverage_sections:
  - "human-system-interface.md §7 Conflict Prevention"
commits: ["ffc110bfbfd8b0ad37e98f3985d1b6175864c341", "3c5bdcd7b6d4f159a9c6327cf957290f08447d51", "a4c3e59e1fc37e7b7dae6155afd9184807d05ee8", "2766274ecc181201ed7f260666734261b29dfb4f", "4c12beeed39ab2f1e8c0e1a13946ec79fe25d3ec", "b7d38b296453170a3a388ea8bf034e1a53874450", "1fce7994689d8c437f0bdc82c8f9cf1adf16f32f", "eee00dbaaac1978e4e2c6e513e7e7c17c92770d5", "1ac154183d15bf3b14b8f16046683d9181940b72"]
---

## Spec Excerpt

When two humans edit the same spec simultaneously:
1. The second editor sees a warning: "jsell is also editing this spec"
2. Edits are not merged automatically — the second save gets a conflict notification
3. The conflict appears in both users' Inboxes with a diff view

This is optimistic concurrency, not real-time co-editing (CRDT-based co-editing is future work). Specs are markdown in git — conflict resolution uses standard git merge semantics.

## Implementation Plan

1. **Spec editing presence tracking:**
   - Extend the existing `UserPresence` WsMessage with an optional `editing_entity` field (e.g., `"spec:specs/system/payments.md"`)
   - When a user opens a spec for editing in the UI, send a UserPresence update with the entity being edited
   - Server updates the presence map with the editing context

2. **Concurrent editing detection (frontend):**
   - When opening a spec editor, query `GET /api/v1/workspaces/:id/presence` for other users editing the same spec
   - Display a warning banner: "{user} is also editing this spec"
   - Subscribe to WebSocket UserPresence updates to show/hide the warning in real-time

3. **Optimistic concurrency on save:**
   - The spec save endpoint already takes a `sha` parameter (the SHA of the spec version being edited)
   - If the spec was modified between load and save (SHA mismatch), return 409 Conflict
   - The 409 response includes the current SHA and a diff between the user's version and the current version

4. **Conflict notification:**
   - On 409 Conflict, create a `SpecConflict` notification for both editors
   - The notification includes the diff and links to both versions
   - Users resolve via the Inbox: pick one version, merge manually, or discard

5. **Frontend conflict resolution UI:**
   - On 409 response, show a conflict dialog with side-by-side diff
   - Options: "Overwrite" (force save with new SHA), "Discard my changes", "Copy to clipboard"

## Acceptance Criteria

- [x] UserPresence includes optional `editing_entity` field
- [x] Warning banner appears when another user is editing the same spec
- [x] Warning disappears when the other user leaves the spec editor
- [x] Spec save returns 409 when SHA has changed since load
- [x] 409 response includes diff between versions
- [x] Conflict notification created for both editors
- [x] Conflict dialog shows side-by-side diff
- [x] "Overwrite" option saves with latest SHA
- [x] `cargo test --all` passes
- [x] `npm test` passes in `web/` (task-092 suites; pre-existing canvas/WebGL failures in ExplorerCanvas/FlowRenderer/MoldableViewNodeTypeFilter/ExplorerViewAskViewSpec are unrelated and fail on `main` without these changes)

## Agent Instructions

Read `specs/system/human-system-interface.md` §7 "Conflict Prevention" for the full spec. The UserPresence WsMessage is defined in `gyre-common/src/protocol.rs`. The presence map is managed in `gyre-server/src/ws.rs`. The spec save flow is in `gyre-server/src/api/specs.rs`. For the frontend, look at how specs are edited in the Svelte components. The existing presence infrastructure (session_id, workspace_id, view tracking) provides the foundation — you're extending it with entity-level granularity.

## Shipped

This round was a rebase repair: the pending merge of `59ff5d01` into the
branch had unresolved conflicts in `web/dist/` build artifacts
(rename/rename on the hashed elk bundle, content conflict in index.html).
Resolved by rebuilding dist from the merged `web/src`
(`npm ci` from the lockfile, then `vite build`) and committing the fresh
output (merge commit `fdc44f29`). It also fixed a real test determinism
bug the socket-restricted prior rounds could never expose (see below).
All task-092 product behavior was already implemented in prior rounds
(F1-F4 findings plus R3 fixes) and is unchanged.

Behavior at HEAD (source-verified):

- **Presence editing context** — `UserPresence.editing_entity` (serde-skipped
  when `None`) round-trips; server stores it in `PresenceEntry` keyed
  `(user_id, session_id)`, derives `user_id` from the authenticated
  connection, validates `session_id` against the Subscribe-established one.
- **Warning banner** — `ConcurrentEditBanner` fetches
  `GET /workspaces/:id/presence` on editor open, stays live on
  `UserPresence`/`PresenceEvicted` WS messages, excludes self by session_id
  and user_id (`selfUserId` wired from `api.me()` in `App.svelte` to both
  DetailPanel instances), re-seeds on WS reconnect with a stale-response
  sequence guard. `createPresenceHeartbeat` implements the §1 legs:
  send-on-connect, 30s timer, view-change re-send (5s debounce),
  `view:"disconnected"` on `beforeunload`, stops after `PresenceEvicted`
  names its own session.
- **Departure rebroadcast** — all four removal paths (graceful disconnect,
  socket-close cleanup, 5-session cap eviction, 60s idle sweeper spawned from
  `main.rs`) call `broadcast_presence_departure`, so the warning disappears
  when the other editor leaves.
- **Optimistic concurrency** — `save_spec` compares `base_sha` against the
  spec ledger's `current_sha`; on mismatch returns 409 with
  `{current_content, submitted_content, diff, current_sha}` and creates
  `SpecConflict` notifications for the caller plus every other user editing
  `spec:<path>` per the presence map; notification bodies persist the full
  line diff so both Inboxes render `SpecDiffView` (§7 item 3). `overwrite`
  bypasses the stale check.
- **Conflict UI** — `SpecConflictDialog` renders the side-by-side diff with
  Overwrite / Discard / Copy; `api.specsSave` surfaces 409 as `{conflict}`;
  `EditorSplit`/`DetailPanel` wire overwrite → `specsSave` with
  `overwrite: true`, discard → adopt server content.

### This round's fix

`ws_session_cap_eviction_rebroadcasts_departure` was flaky (~2 of 3 runs)
now that this sandbox permits loopback sockets — prior rounds' socket
restriction (ConnectionReset at first connect) had masked it. Root cause
(confirmed by logging the observer's stream): the test seeded 5 concurrent
WS sessions and drained the observer with ordered `expect_presence` calls,
but cross-connection broadcast order is not guaranteed (each connection is
an independent server task), so a later session's presence arrived while
the drain waited for an earlier one and was consumed-and-discarded; that
session's turn then timed out. The same reordering made the eviction target
nondeterministic (`server_last_seen` follows server processing order, so
`min_by_key` could evict a session other than the first-opened). Fixed in
`3c5bdcd7` by interleaving seeding with observer confirmation — each
session's presence broadcast is observed before the next session opens,
which guarantees in-order drain and strictly increasing `server_last_seen`
(insert completes before broadcast). Test-only change; no product code
affected.

### Test evidence (this sandbox, saved under /tmp/stage/review-evidence/)

- `cargo test -p gyre-server --lib specs_assist` — 21/21 ok (cold build,
  `SKIP_WEB_BUILD=1`), including
  `save_spec_stale_base_sha_returns_409_with_diff`,
  `save_spec_conflict_notifies_caller_and_concurrent_editor`,
  `save_spec_overwrite_bypasses_stale_base_sha`.
- `cargo test -p gyre-server --lib ws::` — **73 passed / 0 failed**, run
  4x (once standalone, 3x repeated) — all loopback-socket tests now
  genuinely execute in this sandbox; the previously flaky cap-eviction
  test passed 10/10 isolated runs after the determinism fix.
- `npm ci` (full, 169 packages) + `npm test` in `web/` — 65 files /
  1616 passed / 0 failed (41 config-level skips), on the merged `web/src`
  that the rebuilt `web/dist` was produced from.

Exact-head GitHub checks remain the independent reviewer's gate.
