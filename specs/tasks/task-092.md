---
title: "HSI Conflict Prevention — Concurrent Spec Editing Warning"
spec_ref: "human-system-interface.md §7 Conflict Prevention"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-092.md
coverage_sections:
  - "human-system-interface.md §7 Conflict Prevention"
commits: ["8aa51cba24ae0fe17567af338bbe91db6d152e32", "92f9430bcfed9dc97f63fd46d178fd062d3a563c", "b4ad4c745935c77517ee24e66081fcb96787076d", "26f3d54fe2466490e4085faa08b5519ed302de56", "0e92422757b648a687cd91fb18cf7d62daf0053c", "eee00dbaaac1978e4e2c6e513e7e7c17c92770d5", "1ac154183d15bf3b14b8f16046683d9181940b72"]
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
