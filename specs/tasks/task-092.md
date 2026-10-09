---
title: "HSI Conflict Prevention — Concurrent Spec Editing Warning"
spec_ref: "human-system-interface.md §7 Conflict Prevention"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-092.md
coverage_sections:
  - "human-system-interface.md §7 Conflict Prevention"
commits: ["a4c3e59e1fc37e7b7dae6155afd9184807d05ee8", "2766274ecc181201ed7f260666734261b29dfb4f", "4c12beeed39ab2f1e8c0e1a13946ec79fe25d3ec", "b7d38b296453170a3a388ea8bf034e1a53874450", "1fce7994689d8c437f0bdc82c8f9cf1adf16f32f", "eee00dbaaac1978e4e2c6e513e7e7c17c92770d5", "1ac154183d15bf3b14b8f16046683d9181940b72"]
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

Assignment was a repair continuation from checkpoint `cabd3577` (prior attempt
aborted on an infra 401 after merge). All implementation work (F1-F4 findings
plus the R3 fixes) was already present in this branch head and verified at the
current HEAD `51f3905a` — no new product code was needed; this round produced
fresh test evidence under this sandbox's restrictions.

Behavior at HEAD (source-verified this round):

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

Test evidence (this sandbox, evidence in `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-server --lib specs_assist` — 21/21 ok (cold build,
  `SKIP_WEB_BUILD=1`), including
  `save_spec_stale_base_sha_returns_409_with_diff`,
  `save_spec_conflict_notifies_caller_and_concurrent_editor`,
  `save_spec_overwrite_bypasses_stale_base_sha`.
- `cargo test -p gyre-server --lib ws::` — 57 passed / 8 failed; all 8 are
  loopback-socket tests failing at first connect with `ConnectionReset`,
  matching the sandbox restriction (`accept()` errno 95, capabilities.json,
  independently re-probed this round; R5 proved identical failures at the
  pre-task base). The socket-free F4 logic tests pass. Socket-level gates
  remain for host CI.
- `npm ci` incomplete (registry timeout; 131/241 packages, full vitest/jsdom/
  testing-library/svelte toolchain present) — vitest invoked directly from
  the locked install with `--pool=forks --maxWorkers=1`: presence +
  ConcurrentEditBanner 26/26, SpecConflictDialog + Inbox + ws + EditorSplit
  79/79, and the full `web/` suite 58 files / 1548 passed / 0 failed
  (41 config-level skips).

Exact-head GitHub checks remain the independent reviewer's gate.
