# Review — task-092 (HSI §7 Conflict Prevention)

Spec: `human-system-interface.md` §7 "Conflict Prevention" (+ §7 Presence Awareness, §1 presence model).
Commit under review: `25854f64`.

## Round 1

Verified working (no findings):

- Backend `UserPresence.editing_entity` optional field present in `protocol.rs`, serde-skipped when `None`, round-trips (protocol tests).
- `ws.rs` stores `editing_entity` in `PresenceEntry` and rebroadcasts `UserPresence` (with verified `user_id`) to workspace subscribers; `GET /workspaces/:id/presence` returns `editing_entity`.
- Optimistic concurrency in `specs_assist::save_spec`: sends `base_sha`, compares against ledger `current_sha`, returns 409 with `{current_content, submitted_content, diff, current_sha}` and creates `SpecConflict` notifications for caller + concurrent editors from the presence map. Backend tests `save_spec_stale_base_sha_returns_409_with_diff` and `save_spec_conflict_notifies_caller_and_concurrent_editor` pass (21/21 specs_assist tests green).
- Frontend `ConcurrentEditBanner` fetches initial presence, subscribes to live `UserPresence`/`PresenceEvicted`, shows/hides banner, clears on `editing_entity` cleared / `disconnected` / eviction. Component tests cover appear + disappear + evict (62/62 web tests green).
- `SpecConflictDialog` renders side-by-side diff with Overwrite / Discard / Copy. `api.specsSave` surfaces 409 as `{conflict}`. `presence.sendEditingPresence` announces/clears `spec:<path>`; DetailPanel effect announces while editing and clears on cleanup. `ws.js` `send`/`subscribe` with auth-gated queue and reconnect re-subscribe.

Findings:

- [ ] **F1: `selfUserId` never wired from `App.svelte` — same-user multi-tab produces a false self-warning.** `ConcurrentEditBanner`'s contract (`ConcurrentEditBanner.svelte:19`) is `selfUserId — current user id, to exclude own sessions`, and `isSelf` (lines 37-41) excludes presence entries by `user_id` when `selfUserId` is known. But `App.svelte` renders both `DetailPanel` instances (`App.svelte:1610-1617` full-page, `1645-1652` slide-in) passing only `{wsStore}` and `workspaceId` — never `selfUserId` — so it defaults to `null` and flows as `null` into `DetailPanel` → `EditorSplit`/`ConcurrentEditBanner`. Self-exclusion then relies solely on `session_id`. The runtime user id IS available: `App.svelte:1015` calls `api.me()` (server `get_me` returns `id`, `users.rs:102`) but only extracts `global_role` (line 1016), discarding `id`. Consequence: with the same spec open in two tabs (a scenario the presence model explicitly supports — per-tab `session_id`, 5-session cap, §1), tab B receives tab A's presence entry (different `session_id`, same `user_id`) and renders the warning naming the user themselves. This contradicts §7's stated behavior — the warning identifies *another* human ("jsell is also editing this spec"), not yourself. Fix: capture `me.id` into reactive state in `App.svelte` and pass `selfUserId` to both `DetailPanel` instances.
