# Review — task-125 (Implement teams and notifications API endpoints)

Spec: `user-management.md` §API > Teams, §API > Notifications (coverage rows 28–29).
Candidate: `be7681ef93576d005fc5d6475370d0468dd977cb` (base `603cc2f7`); product commit `bc0d0eac` (feat(task-125)) merged with the assigned base; `5114498f`/`b2b2028c`/`2a6e00d6`/`be7681ef` are process-only (task-file notes). The working-tree `M specs/tasks/task-125.md` is the controller's contract restore, not candidate content — the committed candidate records `commits: ["bc0d0eac…"]` correctly.
Verdict: **approved**.

## Probes run (evidence in /tmp/stage/review-evidence)

- `cargo check -p gyre-server` → OK (7m16s; compiles gyre-adapters incl. the postgres module — new `count_unread`/`mark_all_read`/repo-filter Diesel queries type-check on both backends).
- `cargo check -p gyre-server --tests` → OK (type-checks the five new server tests incl. the SSE test).
- `cargo test -p gyre-common notification` → **9 passed** (incl. `rank_band_is_inverse_of_from_rank_and_level_str_parses`: bands partition 1–10 contiguously, inverse of `from_rank`, case-insensitive parse, `critical` → None).
- `cargo test -p gyre-adapters --lib sqlite::notification` → **7 passed** against real SQLite storage (incl. `count_unread_and_mark_all_read_scope_semantics`: unread/read/dismissed split, workspace and user scoping, badge reaches zero; `list_for_user_filters_by_repo`: pushdown hit/miss/no-filter).
- Mechanical gates: `check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh` (79 handlers), `check-mem-port-contracts.sh`, `check-in-memory-state-stores.sh`, `check-forged-scope-fields.sh`, `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`, `check-unbounded-external-http.sh`, `check-relative-path-defaults.sh`, `check-conditional-test-guards.sh`, `check-inert-enforcement.sh` → all OK.

## Verified working (no findings)

- **§API > Teams (row 28): PASS as audit.** All five specced team endpoints pre-exist from task-122 (`api/mod.rs:976-989`): `GET/POST /workspaces/:id/teams`, `PUT/DELETE …/teams/:team_id`, `POST …/teams/:team_id/members`, `DELETE …/teams/:team_id/members/:user_id`. Every handler runs `load_workspace_scoped` + `can_manage_membership` (users.rs:1142-1354) — tenant-scoped and role-gated; covered by task-122's suite. Correctly classified implemented-by-audit, not re-implemented.
- **§API > Notifications (row 29): PASS.** Every row of the spec table exists at the spec path, registered in `api/mod.rs` with `RouteResourceMapping::exempt` resolver entries (per-handler auth, same model as the pre-existing notification routes; no exemption-file growth — route-registry check OK):
  - `GET /api/v1/notifications` — shares `list_my_notifications_impl` with the legacy `/users/me/notifications` (legacy `notification_type` key kept as serde alias; `?type=` is the spec key). Filters `type`/`priority` (four-level name case-insensitively widened to its HSI §8 rank band; unknown level → 400, not silent ignore)/`workspace_id`/`repo_id`; limit/offset pagination. The repo filter is a **new `NotificationRepository::list_for_user` parameter** enforced by SQLite, PostgreSQL, and mem — real storage pushdown, proven by the adapter test, not handler post-filtering.
  - `GET /api/v1/notifications/unread-count` — new `count_unread` port method in all three adapters: active = unresolved ∧ undismissed ∧ unread; optional `?workspace_id=`. Scope semantics proven against real SQLite (other users' and other workspaces' rows excluded).
  - `PUT /api/v1/notifications/{id}/read` — spec method registered on the existing per-handler-auth mark-read handler (user + tenant verified in-handler; POST kept for pre-existing callers — additive, spec-conformant).
  - `POST /api/v1/notifications/mark-all-read` — new `mark_all_read` port method in all three adapters; stamps `read_at`, user- and workspace-scoped, returns `{marked}`; drives unread-count to zero (adapter test asserts the full arc).
  - `GET/PUT /api/v1/notifications/preferences` — spec path on the existing HSI §12 handlers; caller-scoped via `resolve_user_id`; roundtrip test present.
  - `GET /api/v1/notifications/stream` — **real end-to-end SSE**, not a stub: new `AppState::create_notification` is the single fan-out point (persist + broadcast, fire-and-forget so lagged/no subscribers never fail the persist), and every production emitter was converted (verified by grep: only `#[cfg(test)]` code calls `state.notifications.create` directly anymore; ~30 call sites across git_http, signal_chain, constraint_check, spec_patrol, reconciliation, impersonation, specs_assist, secret_rotation, mcp, abandoned_branch, workspace_ownership, prompt_assembly, trust_suggestion, spec_link_staleness, notifications.rs, lib.rs). The handler subscribes before returning the response, filters each event against caller user id AND tenant (plus optional `workspace_id`) before the wire, with 30s heartbeat + 15s keep-alive — the established `/api/v1/audit/stream` pattern. Test asserts delivery of the caller's notification and wire-level absence of cross-user and cross-tenant events.
- **Coverage matrix update is honest:** rows 28–29 → implemented with accurate notes; counts updated 21→19 task-assigned, 4→6 implemented. No `verified` claims without production code.

## Non-findings (checked, judged acceptable)

- SSE handler treats `broadcast::RecvError` as channel-closed and ends the stream — including `Lagged`. This is byte-for-byte the pre-existing `api/audit.rs::audit_stream` idiom (the pattern the task instructions pointed at); with capacity 1024 and per-event consumption, lag is improbable, and the deviation is inherited, not introduced. Worth a follow-up pattern-wide fix (`Lagged(_) => continue`, as `mcp.rs:3891` already does), not a revision blocker for this task.
- Task plan suggested "cursor-based" pagination; implementation uses limit/offset. The spec table says only "paginated, filterable" — contract satisfied; the plan is not the contract.
- `mark_all_read` also flips dismissed-but-unread rows (documented in the port doc-comment and asserted by tests: count 1 → marked 2). Harmless — makes the badge deterministically zero.
- Listing/badge/mark-all handlers filter by the caller's user id but not tenant — identical to the pre-existing `/users/me/notifications` model; user ids are globally unique, so the user filter is the scope. The per-item routes (read/unread/dismiss/resolve) and the SSE stream do check tenant explicitly.

## Environment notes for the verifier (not code defects)

- `scripts/check-assertionless-tests.sh` exits 2 **in this sandbox** — system awk is mawk 1.3.4, which lacks gawk's 3-arg `match()`; reproduced identically on the **base** commit `603cc2f7` (clean worktree). CI must run it under gawk.
- `scripts/check-task-commit-attribution.sh` fails **only against the working tree**, because the controller's restore blanked `commits:` in `specs/tasks/task-125.md`; the committed candidate `be7681ef` records `commits: ["bc0d0eac099f133695f07aeb09b3ccb22d72afac"]`.

## Tests the independent verifier must execute (server test compilation deferred per the 4 GiB budget rule)

- `cargo test -p gyre-server --lib api::users::tests::spec_path_notifications_list_with_filters`
- `cargo test -p gyre-server --lib api::users::tests::spec_path_unread_count_and_mark_all_read`
- `cargo test -p gyre-server --lib api::users::tests::put_read_is_the_spec_method`
- `cargo test -p gyre-server --lib api::users::tests::spec_path_preferences_roundtrip`
- `cargo test -p gyre-server --lib api::users::tests::notification_stream_delivers_own_notifications_only`
- `cargo test -p gyre-adapters --lib` (full; the notification subset passed here)
- `cargo test --all` (task acceptance criterion)

— Reviewer, 2026-11-13
