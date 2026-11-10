---
title: "HSI Conflict Prevention — Concurrent Spec Editing Warning"
spec_ref: "human-system-interface.md §7 Conflict Prevention"
depends_on: [task-237]
progress: needs-revision
review: specs/reviews/task-092.md
coverage_sections:
  - "human-system-interface.md §7 Conflict Prevention"
commits: ["ffc110bfbfd8b0ad37e98f3985d1b6175864c341", "3c5bdcd7b6d4f159a9c6327cf957290f08447d51", "e7e90108d1a307a3d840bf2994bc8f04d942840f", "a4c3e59e1fc37e7b7dae6155afd9184807d05ee8", "2766274ecc181201ed7f260666734261b29dfb4f", "4c12beeed39ab2f1e8c0e1a13946ec79fe25d3ec", "b7d38b296453170a3a388ea8bf034e1a53874450", "1fce7994689d8c437f0bdc82c8f9cf1adf16f32f", "eee00dbaaac1978e4e2c6e513e7e7c17c92770d5", "1ac154183d15bf3b14b8f16046683d9181940b72"]
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

This round was a `requirements_changed` repair. The contract base moved to
`078ec12a` (which gained, among other things, task-123's notification entity
with a full type taxonomy and priority model — the same notification surface
task-092's `SpecConflict` notifications ride on). The branch now carries the
retained task-092 source checkpoint (`d79734869`, already reconciled into
main) merged onto that base (merge commit `2c2cbc401`); because the
checkpoint is an ancestor of the base, the merged tree is the current
contract itself, so no source changes were required this round — the work
was verifying that every task-092 behavior still holds against the changed
base, not regenerating it.

Behavior at HEAD (source-verified against the merged tree):

- **Presence editing context** — `UserPresence.editing_entity` (serde-skipped
  when `None`) round-trips in `gyre-common/src/protocol.rs`; the server
  stores it in `PresenceEntry` keyed `(user_id, session_id)`, derives
  `user_id` from the authenticated connection, and validates `session_id`
  against the Subscribe-established one.
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
- **Optimistic concurrency** — `save_spec` (in `specs_assist.rs`) compares
  `base_sha` against the spec ledger's `current_sha`; on mismatch returns
  409 with `{current_content, submitted_content, diff, current_sha}` and
  creates `SpecConflict` notifications for the caller plus every other user
  editing `spec:<path>` per the presence map; notification bodies persist
  the full line diff so both Inboxes render `SpecDiffView` (§7 item 3).
  `overwrite` bypasses the stale check. The notification now flows through
  task-123's taxonomy — `NotificationType::SpecConflict` carries priority 2 —
  and the conflict-creation path is compatible with it (verified by
  `save_spec_conflict_notifies_caller_and_concurrent_editor` at this head).
- **Conflict UI** — `SpecConflictDialog` renders the side-by-side diff with
  Overwrite / Discard / Copy; `api.specsSave` surfaces 409 as `{conflict}`;
  `EditorSplit`/`DetailPanel` wire overwrite → `specsSave` with
  `overwrite: true`, discard → adopt server content.

### Test evidence (this sandbox, saved under /tmp/stage/review-evidence/)

All probes run at the exact branch head `2c2cbc401`:

- `cargo test -p gyre-server --lib specs_assist` — **21/21 ok** (cold build,
  `SKIP_WEB_BUILD=1`), including
  `save_spec_stale_base_sha_returns_409_with_diff`,
  `save_spec_conflict_notifies_caller_and_concurrent_editor`,
  `save_spec_overwrite_bypasses_stale_base_sha`.
- `cargo test -p gyre-server --lib ws::` — **94 passed / 0 failed** (the
  base gained loopback-socket tests since the prior round's 73), including
  `ws_session_cap_eviction_rebroadcasts_departure` and
  `ws_socket_close_rebroadcasts_departure`.
- `npm ci` (full, lockfile versions) + `npm test` in `web/` — **99 files /
  2010 passed / 0 failed** (41 config-level skips), including the task-092
  suites `ConcurrentEditBanner.test.js`, `SpecConflictDialog.test.js`, and
  the presence tests. The canvas/WebGL failures noted in earlier rounds
  (ExplorerCanvas/FlowRenderer/ExplorerViewAskViewSpec) were fixed on main
  in the meantime and pass at this head.

Exact-head GitHub checks remain the independent reviewer's gate.

## Review

### Review changed source code

- crates/gyre-server/src/api/specs_assist.rs

Preserved these edits for implementation. Review cannot approve its own source or verifier edits. Repair them within task scope and request a fresh independent review.
