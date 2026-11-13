# Review — task-125 (Implement teams and notifications API endpoints)

Spec: `specs/system/user-management.md` §API > Teams, §API > Notifications.
Commits under review: `bc0d0eac` (feat), `e4af712a` (verification-finding repair), plus process/merge commits on `pipeline/task-125/...-1` (base `276918d2`, candidate `b308318f`).
Verdict: **needs-revision** — one material finding (F1, incomplete SSE fan-out backed by a false coverage claim); one minor (F2).

## Probes run (checkout at candidate `b308318f`)

- `bash scripts/check-arch.sh` → OK.
- `bash scripts/check-abac-route-registry.sh` → OK (all new spec-path routes resolve).
- `bash scripts/check-abac-exempt-handlers.sh` → OK (79 handlers; the five new exempt routes all carry per-handler caller-identity scoping like the pre-existing notification routes).
- `bash scripts/check-mem-port-contracts.sh`, `check-forged-scope-fields.sh`, `check-in-memory-state-stores.sh` → OK.
- `cargo test -p gyre-common --lib notification` → 9/9 passed, incl. the new `rank_band_is_inverse_of_from_rank_and_level_str_parses`.
- `cargo test -p gyre-adapters --lib sqlite::notification` → 7/7 passed against real SQLite, incl. the new `count_unread_and_mark_all_read_scope_semantics` and `list_for_user_filters_by_repo` (user/workspace/repo scoping, read_at stamping, badge reaches zero, cross-user rows untouched).
- `cargo check -p gyre-server --lib` → clean (4m44s).
- `cargo test -p gyre-server --lib api::users::tests::` → **blocked by the executor's 4 GiB wrapper** (exit 64). The verifier must run it; the five new server tests to confirm are `spec_path_notifications_list_with_filters`, `spec_path_unread_count_and_mark_all_read`, `put_read_is_the_spec_method`, `spec_path_preferences_roundtrip`, `notification_stream_delivers_own_notifications_only`.
- Full diff inspection base..candidate; all non-notification hunks are mechanical (`list_for_user` signature call-sites, emitter reroutes to `AppState::create_notification`).

## Verified working (no findings)

- **Route surface is complete and spec-shaped.** All seven §API > Notifications rows exist at the spec paths (`GET /api/v1/notifications`, `unread-count`, `PUT :id/read` with POST kept for legacy callers, `mark-all-read`, `GET/PUT preferences`, `GET stream`), registered in `api/mod.rs` with `RouteResourceMapping::exempt` entries; no exemption-file growth. Team rows (§API > Teams) audited as already shipped by task-122: `add_team_member`/`remove_team_member` handlers and all five routes present.
- **Filtering is real storage push-down, not handler post-filtering.** `repo_id` is a new `NotificationRepository::list_for_user` parameter enforced by SQLite, PostgreSQL, and mem; `?priority=` parses the four-level name case-insensitively (`NotificationPriority::from_level_str`) and widens to its HSI §8 rank band via the new `rank_band()` (bands verified to partition ranks 1–10 with no gaps/overlap); unknown level → 400.
- **unread-count / mark-all-read are real port methods in all three adapters** with consistent semantics (active = unresolved ∧ undismissed ∧ unread for counting; the sweep flips every unread row so the badge deterministically reaches zero; user- and workspace-scoped; `read_at` stamped). SQLite adapter tests exercise real storage.
- **SSE stream is real end-to-end for the routed emitters**: `AppState::create_notification` persists then broadcasts (ordering correct — a client can never receive an event that failed to persist); the handler filters every event against caller user id AND tenant (optional `workspace_id`) before the wire, with heartbeat keep-alive. The server test seeds own / other-user / cross-tenant notifications and asserts only the caller's event reaches the wire. Broadcast is fire-and-forget so a lagged subscriber cannot fail a persist.
- **Emitter reroute is broad and genuine**: ~17 production modules (git_http, signal_chain, constraint_check, spec_patrol, reconciliation, impersonation, specs_assist, secret_rotation, mcp, abandoned_branch, workspace_ownership, prompt_assembly, trust_suggestion, spec_link_staleness, notifications.rs helpers, lib.rs) now persist via the fan-out point; every remaining direct `state.notifications.create(` call is in a `mod tests`.
- **Preferences** spec path binds the existing HSI §12 per-user handlers; roundtrip test goes through the real router.
- **No test inflation found.** The new tests drive the real router with bearer tokens and real (SQLite / mem-with-code-guards) storage, assert negative cases (cross-user non-mutation, 400 on bad priority, cross-tenant stream filtering), and would fail if the behavior regressed.

## Findings

- [-] **F1 (major): two production notification emitters bypass the SSE fan-out, and the coverage claim "every production emitter (~30 call sites ...) routes through it" is false.** `crates/gyre-server/src/graph_extraction.rs:691` (`ConflictingInterpretations`) and `:1414` (`SpecAssertionFailure`) persist via `ports.notification_repo.create(&notif)` directly, and both production feeders pass the raw repository: `git_http.rs:1295` (`notification_repo: state_clone.notifications.as_ref()`, the git-push path) and `mirror_sync.rs:82`. These notifications are stored (they show in `GET /api/v1/notifications`) but never broadcast — a connected `/notifications/stream` client does not receive them in real time. Note the implementer's grep (`notifications\.create(`) misses this second persistence path because the field is named `notification_repo`; `grep -rn "notification_repo\.create" crates/gyre-server/src` finds it. The `specs/coverage/system/user-management.md` row 29 "implemented" justification and the shipped notes both assert complete fan-out, which is exactly the hollow-verification shape the goal prohibits. **Repair:** route the two graph_extraction persistence sites through `AppState::create_notification` (e.g. a persist closure or broadcasting wrapper in `DivergencePorts` instead of the bare repo), then correct the "every production emitter" wording in the coverage row.
- [-] **F2 (minor): SSE `RecvError::Lagged` treated as channel-closed.** `users.rs` `notification_stream`: `Ok(Err(_)) => return None` conflates tokio broadcast's `Closed` and `Lagged`; a subscriber that falls behind the 1024-slot buffer gets its stream silently terminated instead of resuming (missed messages skipped). EventSource auto-reconnect bounds the impact, but the handler should match the variant and `continue` on `Lagged`.

## Verifier must run (blocked here by the 4 GiB build wrapper)

- `cargo test --all` (task AC).
- `cargo test -p gyre-server --lib api::users::tests::` — the five new tests named above.
