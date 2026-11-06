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

- [x] **F1: `selfUserId` never wired from `App.svelte` — same-user multi-tab produces a false self-warning.** `ConcurrentEditBanner`'s contract (`ConcurrentEditBanner.svelte:19`) is `selfUserId — current user id, to exclude own sessions`, and `isSelf` (lines 37-41) excludes presence entries by `user_id` when `selfUserId` is known. But `App.svelte` renders both `DetailPanel` instances (`App.svelte:1610-1617` full-page, `1645-1652` slide-in) passing only `{wsStore}` and `workspaceId` — never `selfUserId` — so it defaults to `null` and flows as `null` into `DetailPanel` → `EditorSplit`/`ConcurrentEditBanner`. Self-exclusion then relies solely on `session_id`. The runtime user id IS available: `App.svelte:1015` calls `api.me()` (server `get_me` returns `id`, `users.rs:102`) but only extracts `global_role` (line 1016), discarding `id`. Consequence: with the same spec open in two tabs (a scenario the presence model explicitly supports — per-tab `session_id`, 5-session cap, §1), tab B receives tab A's presence entry (different `session_id`, same `user_id`) and renders the warning naming the user themselves. This contradicts §7's stated behavior — the warning identifies *another* human ("jsell is also editing this spec"), not yourself. Fix: capture `me.id` into reactive state in `App.svelte` and pass `selfUserId` to both `DetailPanel` instances.

## Round 2

Re-review of the same tree (worktree HEAD `b1ed5079`; implementation commit `25854f64` unchanged — no fix commit followed R1). Verified F1 independently and found one additional spec gap.

- [x] **F1 (still open): `selfUserId` never wired from `App.svelte`.** Confirmed unresolved — code is byte-identical to R1. `web/src/App.svelte:1610` (full-page) and `:1645` (slide-in) still render `<DetailPanel>` with only `{wsStore}` and `workspaceId={currentWorkspace?.id ?? null}`; neither passes `selfUserId`. `api.me()` at `App.svelte:1015` uses the result solely for `userIsAdmin` and discards `me.id`, so no user-id source exists to thread down. `ConcurrentEditBanner.svelte:39` (`if (selfUserId && entry.user_id === selfUserId) return true;`) therefore never fires; self-exclusion relies only on `session_id`, so a second tab of the *same* user editing the same spec triggers a false "X is also editing this spec" warning. All four `ConcurrentEditBanner.test.js` cases pass `selfUserId: null`, so the user-id self-exclusion path has zero coverage. Fix: store `me.id` in reactive state after `api.me()` and pass it as `selfUserId` to both `<DetailPanel>` sites; add a banner test with two sessions sharing one `user_id` and `selfUserId` set, asserting the banner stays hidden.
- [x] **F2: neither editor's Inbox provides a diff view — spec §7 item 3 unmet.** Spec §7: "The conflict appears in both users' Inboxes with a diff view." `spec_conflict_response` (`specs_assist.rs:216-222`) persists the notification `body` with only `{spec_path, base_sha, current_sha, diff_summary}` — `diff_summary` is a bare count string (`"+{added} / -{removed} lines"`, line 189). The actual diff data (`current_content`, `submitted_content`, `diff`) is emitted only in the transient 409 HTTP response (lines 249-251) and never stored. The Inbox card for a `SpecConflict` notification (`Inbox.svelte:455-456`) renders only `body.diff_summary` text and a related-spec link (`:467-473`) that opens the current spec in the detail panel — not a diff between the two versions. Consequently the *first* editor (who never receives the 409) has no diff view at all, and the *second* editor loses the diff once the save-time dialog is dismissed. Fix: persist the diff (or enough to reconstruct it — e.g. the `diff` array or both SHAs plus a diff-fetch path) in the notification body and render a diff view when a `SpecConflict` Inbox card is opened.

## Round 3 — Fixes applied

Both open findings resolved. `cargo test -p gyre-server --lib specs_assist` green (21/21); affected web suites green (`ConcurrentEditBanner`, `Inbox`, `SpecConflictDialog`, `ws`, `EditorSplit` — 86 tests). Pre-existing, unrelated failures remain in `FlowRenderer`, `MoldableViewNodeTypeFilter`, `ExplorerViewAskViewSpec` (canvas/WebGL/SSE in jsdom — identical count with these changes stashed).

- F1 resolved: `web/src/App.svelte` now captures `selfUserId = me?.id` into reactive state (`:730`, set at the `api.me()` site) and passes `{selfUserId}` to both `<DetailPanel>` instances (full-page + slide-in). The prop threads unchanged through `DetailPanel` → `EditorSplit`/`ConcurrentEditBanner` (already wired). New banner tests: same-user second tab (different `session_id`, same `user_id`, `selfUserId` set) stays hidden; a genuinely different user still warns.
- F2 resolved: `spec_conflict_response` (`crates/gyre-server/src/api/specs_assist.rs`) now persists the full line `diff` array in the notification `body` (alongside `diff_summary`), so BOTH editors get a diff view in their Inbox — not only the second editor's transient 409 dialog. `Inbox.svelte` renders the shared `SpecDiffView` when a `SpecConflict` card carries `body.diff`. Extracted `SpecDiffView.svelte` (reused by `SpecConflictDialog` and the Inbox card — no logic duplication). Backend test `save_spec_conflict_notifies_caller_and_concurrent_editor` now asserts the persisted `diff` array; new Inbox test asserts the diff rows render on expand.


## Round 4

Re-verified the R3 fixes (commits `1ac15418`, `eee00dba`) against the spec, then swept for fix-class-exhaustion misses and new flaw classes. Backend `cargo test -p gyre-server --lib specs_assist` green (21/21); task web suites green (`ConcurrentEditBanner`, `Inbox`, `SpecConflictDialog`, `EditorSplit`, `ws`, `presence` — 91 tests).

- [-] [process-revision-complete] **F1 (resolved R3, re-verified R4): `selfUserId` wiring.** `App.svelte` captures `selfUserId = me?.id` at the `api.me()` site and passes it to both `<DetailPanel>` instances; banner tests cover same-user-other-tab exclusion and different-user warning. Holds.
- [-] [process-revision-complete] **F2 (resolved R3, re-verified R4): Inbox diff view.** `spec_conflict_response` persists the full `diff` array in the notification body; `Inbox.svelte` renders `SpecDiffView` from `body.diff` on expand; shared renderer with `SpecConflictDialog`. Holds.

New findings (same root: presence liveness is only half-implemented, so the feature degrades after 60 seconds of editing — the normal case):

- [-] [process-revision-complete] **F3: no client presence heartbeat — active editors are evicted after 60s, breaking the warning and the both-Inboxes requirement.** Spec §1 (line 84): presence updates are sent immediately after WS connect, then on both a 30-second timer AND view changes, with `view: "disconnected"` sent on `beforeunload`. The only `UserPresence` sender in `web/src` is `sendEditingPresence` (`web/src/lib/presence.js:18`), called solely from DetailPanel's editor-tab effect (`DetailPanel.svelte:993-996`) — one announce on open, one clear on in-app navigation away. There is no 30s interval, no send-on-connect, no send-on-view-change, and no `beforeunload` handler sending a disconnect (grep across `web/src`: `beforeunload` appears only in ExplorerView/MetaSpecs dirty-check… Process surface patched: implementation.md item 161 (client liveness contracts are all-or-nothing — every spec'd leg enumerated and tested) + verifier.md flaw-class bullet. Product fix owned by task-092's revision round.
- [-] [process-revision-complete] **F4: server never rebroadcasts departures — the warning does not disappear when the other user closes their tab.** Spec §7 Presence Awareness (line 1180): the server rebroadcasts `UserPresence` to other workspace subscribers; §1 (line 84): graceful disconnect via `view: "disconnected"`. `ConcurrentEditBanner` clears a live warning only when it receives a `UserPresence` for that session with `view: "disconnected"` or cleared `editing_entity` (`ConcurrentEditBanner.svelte:78-86`). Three server paths remove a presence entry without telling any other subscriber: (1) `view: "disconnected"` handling removes the map entry but the rebroadcast block sits inside the `else` branch (`ws.rs:217-296`), so a disconnect is never rebroadcast; (2) socket-close cleanup… Process surface patched: implementation.md item 161 (every removal path must notify other subscribers — enumerate all removal sites) + verifier.md flaw-class bullet. Product fix owned by task-092's revision round.

## Round 5 — repair round verified; environmental socket failures ruled out

Bounded handoff: the prior round's repair note deferred socket-level tests to
the controller. This sandbox has loopback, and the new socket tests fail — the
question was whether that is a sandbox artifact or a product bug. Decisive
control experiment (evidence: `/tmp/stage/review-evidence/`):

- The 4 pre-existing socket tests (`ws_valid_auth_succeeds`, `ws_ping_pong`,
  `ws_invalid_auth_fails`, `ws_activity_event_emits_to_telemetry` — untouched
  by this task) fail at HEAD `de063c0` with `ConnectionReset` (Os code 104) at
  the first `connect_async`.
- Identical command at comparison base `66422bd` (isolated worktree, private
  `CARGO_TARGET_DIR`): same 4 failures, same signature (base `ws.rs:513/539/
  565/587` ≡ HEAD `:583/609/635/965`, same statements — line offset only from
  the task's added lines). Both runs exit 101, 0 passed / 4 failed.
- Additional control: `tty::tests::tty_auth_valid` + `tty_auth_invalid_rejected`
  (also `connect_async`, untouched) fail identically at HEAD.

Conclusion: **the ConnectionReset is environmental to this sandbox (loopback
connections accepted then reset), not a task-092 regression.** Socket-level
gates remain the controller's to run on the host. The socket-free F4 tests
(`broadcast_presence_departure_reaches_only_workspace_subscribers`,
`evict_stale_presence_removes_stale_and_notifies_evictee_and_subscribers`)
cover the same removal-path logic without TCP and pass at HEAD (2/2).

Repair-round claims re-verified at HEAD `de063c0`:

- Root lockfile stub gone (tree + index); anchored `/package-lock.json` ignore
  rule does not affect the tracked `web/`, `scripts/`, `docker/gyre-agent/`
  lockfiles (`git check-ignore` exit 1 for all three).
- `scripts/check-sandbox-sweep-artifacts.sh` is a genuine gate (tracked-file
  check, anchor check, unignored-stub check), wired into `.pre-commit-config.yaml`
  and `.github/workflows/ci.yml` as non-advisory. Runs OK.
- `cargo test -p gyre-server --lib specs_assist` — 21/21 ok.
- Six task web suites (presence, ConcurrentEditBanner, Inbox, SpecConflictDialog, ws, EditorSplit) — 105/105 ok.
- Mutation probe (isolated to the file, restored, `git status` clean after): disabling the
  eviction-check condition (`if (msg?.type === 'PresenceEvicted' && msg.session_id === wsStore.sessionId)`)
  makes `stops heartbeating after PresenceEvicted names its own session` FAIL — the eviction-leg
  regression test is load-bearing, not self-confirming.
- Attribution gate (`scripts/check-task-commit-attribution.sh`) passes with no new exemptions
  (3 frozen, none for task-092); every task-labeled product-surface commit in the range is
  recorded in the frontmatter (unlisted wips touch only `specs/` + the removed root lockfile
  stub, outside the gate's product-surface scope). `6af57ea3` cited in the task body was
 orphaned by the rebase; its content is preserved in recorded `a4c3e59`.
- `spawn_presence_eviction` is spawned from `main.rs:73`; `evict_stale_presence`
  extracted as a testable unit.
- F1 (`selfUserId` wiring: `App.svelte:775/1104`, both DetailPanel instances) and
- F2 (`spec_conflict_response` persists `diff` array; `Inbox.svelte:459-462` renders
  `SpecDiffView` from `body.diff`) intact post-rebase.
- F4 wiring verified in source: all four removal paths (graceful disconnect
  `ws.rs:219-231`, socket-close cleanup `:445-457` broadcast-before-deregister,
  5-session cap eviction `:276-284`, idle sweeper `lib.rs evict_stale_presence`)
  call `broadcast_presence_departure`; synthesized departure carries
  server-verified `user_id` and `editing_entity: None`. Lock ordering
  (`ws_connection_workspaces` → `ws_connections`, both read-only in broadcasts)
  is consistent with the existing update path; no new deadlock surface.
- F3 wiring verified in source: `createPresenceHeartbeat` implements all four
  §1 legs + session-scoped eviction stop; wired in `App.svelte` with
  `getEditingEntity` fed from DetailPanel's `oneditingentity` so beats re-send
  the current entity; view-change leg wired to `presenceViewLabel` with the
  registered-status-first reconnect guard; `ws.js` exposes the required
  `onMessage`/`onStatus`/`sessionId` API.
- ConcurrentEditBanner additionally gained the §7 "reconnect re-seeds presence
  from `GET /workspaces/:id/presence`" leg (spec line 1198) with a
  stale-response sequence guard.

No new findings. F1-F4 all hold; the repair round's only change (lockfile
sweep hardening) is sound and mechanically gated.

**Verdict: `progress: complete`.** The R4/R3 fixes and repair round meet
HSI §7 (warning banner, optimistic concurrency 409 + diff, both-Inboxes diff
view, conflict dialog) and the §1/§7 presence liveness contracts (heartbeat
legs, departure rebroadcast on all removal paths, session-scoped eviction
stop, reconnect re-seed). Socket-level end-to-end tests remain for the
controller's host run — they are environmental here.