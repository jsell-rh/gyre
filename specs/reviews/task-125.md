# Review — task-125 (Implement teams and notifications API endpoints)

Spec: `user-management.md` §API > Teams, §API > Notifications.
Candidate `34eb5078` (base `b9487b1b`); task-labeled product commits
`bc0d0eac` (spec-path notifications API + SSE stream + filters + port
methods), `e4af712a` (verification-finding repair), `83594b91` (review-repair:
SSE fan-out closure). Verdict: **approved**.

This is the third round: round 1 (checkpoint `7a4f6ca1`) found two defects —
push-scoped `DivergencePorts` persistence bypassing the SSE fan-out, and
broadcast `RecvError::Lagged` treated as channel-closed — both repaired by
`83594b91` and re-verified independently this round.

## What was verified (evidence: /tmp/stage/review-evidence/, this tree)

- **All specced endpoints exist at the spec paths** (`api/mod.rs:929-955`):
  `GET /api/v1/notifications` (limit/offset pagination; filters `?type=`
  (serde alias for legacy `notification_type`), `?priority=` (Low/Medium/High/
  Urgent, case-insensitive, widened to its HSI §8 rank band via new
  `NotificationPriority::from_level_str`/`rank_band`; unknown level → 400),
  `?workspace_id=`, `?repo_id=`, plus pre-existing rank filters),
  `GET /api/v1/notifications/unread-count`, `PUT /api/v1/notifications/:id/read`
  (POST kept), `POST /api/v1/notifications/mark-all-read`,
  `GET/PUT /api/v1/notifications/preferences`, `GET /api/v1/notifications/stream`
  (SSE). All registered with `RouteResourceMapping::exempt` entries and
  per-handler caller-identity scoping — same model as the pre-existing
  notification routes; no exemption-file growth, no duplicate resolver entries.
- **Real storage, not handler post-filtering:** the repo/priority/type filters
  and the new `count_unread`/`mark_all_read` port methods are pushed down into
  SQLite, PostgreSQL, and mem. Ran against real SQLite:
  `cargo test -p gyre-adapters --lib notification` → 9 passed (incl.
  `count_unread_and_mark_all_read_scope_semantics`,
  `list_for_user_filters_by_repo`). `cargo test -p gyre-common --lib` → 125
  passed (incl. the band/level-parse round-trip test).
- **SSE fan-out closure (round-1 finding 1): repaired and swept.** Every
  production persistence site now routes through
  `AppState::create_notification`/`BroadcastingNotificationRepo` (30
  `create_notification` call sites); the only two bare-repo paths
  (`graph_extraction.rs:697,1420` via `ports.notification_repo`) are fed by four
  production `DivergencePorts` constructors (git_http.rs:1299, mirror_sync.rs:86,
  api/repos.rs:663,762), all now passing `broadcasting_notifications()`. All
  remaining bare `.create` uses verified inside `#[cfg(test)]` modules. The
  regression test `push_check_notifications_broadcast_to_stream_subscribers`
  drives a real failing-assertion push check through the wrapper and asserts
  persist + broadcast.
- **Lagged handling (round-1 finding 2): repaired.** `notification_stream`
  continues on `RecvError::Lagged`, only `Closed` ends the stream; per-event
  filtering by caller user + tenant (+ optional workspace) before the wire.
- **Teams section: audit-only claim is accurate.** All five §API > Teams
  endpoints shipped with task-122 and exist in `api/mod.rs:976-988` with the
  task-122 test suite (team CRUD/slug/lead/membership flow, workspace scoping,
  role gating) — re-verified present.
- **Mechanical gates:** check-arch, check-abac-route-registry,
  check-abac-exempt-handlers (79 handlers), check-mem-port-contracts,
  check-in-memory-state-stores, check-conditional-test-guards,
  check-inert-enforcement — all pass on this tree. Two non-candidate notes:
  check-task-commit-attribution fails only because the review controller reset
  the working-tree task file to the pristine contract (the candidate HEAD
  itself lists all three commits); check-assertionless-tests cannot run in this
  sandbox (mawk lacks gawk's 3-arg `match()`) — verifier must re-run it with
  gawk.
- **Compile:** `cargo check -p gyre-server` and `cargo check -p gyre-server
  --tests` clean.

## Test quality

The five new server tests and the graph_extraction regression test assert
real behavior (filter narrowing, cross-user/cross-tenant negatives, 400 on
unknown priority, unread-count arithmetic incl. dismissed/read exclusion,
PUT method, `{marked}` count, persist+broadcast dual assertion). No
self-confirming, stub-backed, or assertionless tests found in the diff.

## Deferred to the independent verifier (4 GiB build wrapper)

Server test execution belongs to the verifier; required commands (also in
/tmp/stage/review-evidence/README.md):

    cargo test -p gyre-server --lib notification_stream_delivers_own_notifications_only
    cargo test -p gyre-server --lib spec_path_notifications_list_with_filters
    cargo test -p gyre-server --lib spec_path_unread_count_and_mark_all_read
    cargo test -p gyre-server --lib put_read_is_the_spec_method
    cargo test -p gyre-server --lib spec_path_preferences_roundtrip
    cargo test -p gyre-server --lib push_check_notifications_broadcast_to_stream_subscribers
    cargo test -p gyre-server --lib push_check_creates_priority9_notifications_for_failed_assertions
    cargo test -p gyre-server --lib notifications
    cargo test --all
    bash scripts/check-assertionless-tests.sh   (with gawk)

Route-conflict risk assessed nil: static/param siblings already exist in the
router (`/api/v1/repos/mirror` vs `/api/v1/repos/:id`), so the new static
`/notifications/{stream,preferences,unread-count,mark-all-read}` routes
coexist with `/notifications/:id/*` by matchit's static priority.

No findings. Coverage rows 28–29 (`implemented`) match the shipped code.

— Reviewer, 2026-11-13
