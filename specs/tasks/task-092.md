---
title: "HSI Conflict Prevention — Concurrent Spec Editing Warning"
spec_ref: "human-system-interface.md §7 Conflict Prevention"
depends_on: []
progress: needs-revision
review: specs/reviews/task-092.md
coverage_sections:
  - "human-system-interface.md §7 Conflict Prevention"
commits: ["a979787d4bc9954c358468e8aa0640cf8e2a046d", "e76e1e75df74042195bb69c2acec1f520efec4c9", "355219c787a6660fada308140cbfd8fa0527e2fa", "d6acdf325904db4a067a52c892d6ef3dfdf4c245", "0b0c7a2bf54f2c9ec3214d340af13249eb6aeefa", "eee00dbaaac1978e4e2c6e513e7e7c17c92770d5", "1ac154183d15bf3b14b8f16046683d9181940b72"]
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

## Revision Round (R4 findings F3/F4)

Both R4 findings are resolved on this branch:

- **F3 (client presence heartbeat):** `createPresenceHeartbeat`
  (`web/src/lib/presence.js`) implements all four spec §1 legs — send-on-connect
  (WS `connected` status), 30-second timer (never starved by the debounce),
  view-change re-send (5s debounce, wired to `presenceViewLabel` in
  `App.svelte`), and `view: "disconnected"` on `beforeunload`. The beat carries
  the CURRENT `editing_entity` (fed from DetailPanel via `oneditingentity`) so
  it never clobbers a live concurrent-editing announcement. Tests:
  `presence.test.js` covers each leg plus reconnect and pause-when-no-workspace.
- **F4 (server rebroadcasts departures):** every presence-removal path now
  notifies other workspace subscribers via `broadcast_presence_departure`
  (`crates/gyre-server/src/ws.rs`): graceful `view: "disconnected"`,
  socket-close cleanup, 5-session cap eviction, and the 60s idle sweeper
  (`evict_stale_presence`, `lib.rs`, spawned from `main.rs`). Socket-level
  tests cover each path end-to-end; socket-free tests cover the shared
  primitives for environments without loopback accept().
- **Extra leg (spec §1, found in this round):** on a targeted
  `PresenceEvicted` naming this tab's session, the client now stops
  heartbeating for that tab — previously the evicted tab re-inserted its
  presence entry every 30s, fighting the server's 5-session cap. Commit
  `6af57ea3` with regression tests proving the eviction check is load-bearing.

The `330f8d61` SHA recorded by an earlier round was orphaned by a rebase; the
equivalent product content is preserved in the recorded wip commits
(`10a8c80`, `eca3312`, `55cf293`, `7718b71`) and re-verified here against the
spec, with the `1ac15418`/`eee00dba` SHAs restored to the frontmatter so the
attribution check sees every reachable task-labeled product commit.

## Agent Instructions

Read `specs/system/human-system-interface.md` §7 "Conflict Prevention" for the full spec. The UserPresence WsMessage is defined in `gyre-common/src/protocol.rs`. The presence map is managed in `gyre-server/src/ws.rs`. The spec save flow is in `gyre-server/src/api/specs.rs`. For the frontend, look at how specs are edited in the Svelte components. The existing presence infrastructure (session_id, workspace_id, view tracking) provides the foundation — you're extending it with entity-level granularity.

## Review

### Review changed source code

- package-lock.json

Preserved these edits for implementation. Review cannot approve its own source or verifier edits. Repair them within task scope and request a fresh independent review.
