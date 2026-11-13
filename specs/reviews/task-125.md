# Review — task-125 (Implement teams and notifications API endpoints)

Spec: `specs/system/user-management.md` §API > Teams, §API > Notifications.
Candidate: `04cc5245` (base `d9c419a0`, branch
`pipeline/task-125/da072aa8ac9f42259d7974dc2d4a31bb-1`). Product commits under
review: `bc0d0eac` (implementation), `e4af712a`, `83594b91` (SSE fan-out repair),
`2a8cf5f6` (rustfmt), `02b4afa6` (Clippy E0061 repair after the task-191 merge).

Verdict: **approved** — every acceptance criterion is backed by production code
and real storage; no fakery, no test inflation, no scope defects found.

## Probes run (evidence: /tmp/stage/review-evidence/, README.md has full detail)

- Mechanical gates, all exit 0: `check-arch.sh`, `check-abac-route-registry.sh`
  (new routes are resolver `RouteResourceMapping::exempt` entries — exemption
  file not grown), `check-abac-exempt-handlers.sh`, `check-mem-port-contracts.sh`,
  `cargo fmt --check`, and `check-task-commit-attribution.sh` on the committed
  tree (the working-tree failure is the controller's contract restore of
  task-125.md to `commits: []`; the committed candidate carries all five SHAs —
  verified via temporary stash, working tree restored byte-identical).
- `cargo test -p gyre-common notification` → 9/9 (incl. new
  `rank_band_is_inverse_of_from_rank_and_level_str_parses`).
- `cargo test -p gyre-adapters sqlite::notification` → 7/7 against real SQLite
  storage (incl. new `count_unread_and_mark_all_read_scope_semantics` and
  `list_for_user_filters_by_repo` — user/workspace scoping, read/dismissed
  exclusion, sweep drives the badge to zero, other users untouched, repo filter
  pushed down to the stored `repo_id` column).
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → clean.
  The `--tests` pass covers the E0061 class that failed Clippy CI run 43458944185
  (budget.rs:2332 arity, repaired in `02b4afa6`).
- Clippy diff-scoped approximation of CI's `check-clippy-diff.py`: JSON clippy on
  gyre-common/ports/adapters (lib+tests) and gyre-server (lib), primary spans
  intersected with `git diff d9c419a0..HEAD` changed lines → **0 hits**.

## What was verified

**Endpoints.** All seven §API > Notifications rows exist at the spec path in
`api/mod.rs`: `GET /api/v1/notifications` (limit/offset pagination; `?type=`,
`?priority=` widened case-insensitively to its HSI rank band via new
`NotificationPriority::from_level_str`/`rank_band`, unknown level → 400;
`?workspace_id=`, `?repo_id=`), `GET .../unread-count`, `PUT .../{id}/read`
(POST kept for pre-existing callers of the same handler), `POST
.../mark-all-read` (`{marked}` response, optional workspace sweep),
`GET/PUT .../preferences`, `GET .../stream` (SSE). All five §API > Teams rows
are registered and were re-audited in the handlers: workspace-scoped load,
Owner/Admin/TenantAdmin gate, cross-workspace team id → 404, members restricted
to workspace members, duplicate membership → 409 (task-122-shipped; task-125 is
the audit the task assigns).

**Real storage, not stand-ins.** `count_unread`, `mark_all_read`, and the
`list_for_user` repo filter are new `NotificationRepository` port methods
implemented in SQLite (diesel queries against the real table, tested), PostgreSQL,
and mem with matching semantics; `check-mem-port-contracts.sh` passes.

**Per-handler auth (ABAC-exempt model, same as the pre-existing notification
routes).** Listing/count/sweep/preferences/stream are keyed to the
`AuthenticatedAgent` caller's user id; `:id` routes load the notification
(get is user-scoped, so cross-user UUID probing is a 404) and 403 on tenant
mismatch; the SSE handler re-checks user id AND tenant on every event (and the
optional workspace filter) before anything reaches the wire — the new server
test seeds cross-user and cross-tenant notifications and asserts they never
appear in the stream bytes.

**SSE fan-out is single-point and complete.** `AppState::create_notification`
delegates to `BroadcastingNotificationRepo` (persist, then fire-and-forget
broadcast). Every production notification-create path routes through it:
`notify`/`notify_rich`, the ~30 direct `create_notification` call sites
(specs_assist, git_http persona/cross-workspace, reconciliation, signal chain,
etc.), and the two push-scoped graph_extraction persistence sites, whose four
`DivergencePorts` feeders (git push, mirror sync, repo import ×2) now pass
`state.broadcasting_notifications()` instead of the bare repo (review repair
`83594b91`, with a regression test that fails at the broadcast assertion when a
bare repo is used). Remaining `state.notifications.create(` sites were each
confirmed to be inside `#[cfg(test)]` modules. Lagged receivers keep streaming;
only channel `Closed` ends a stream; heartbeat keep-alive present.

**Test quality.** The new tests drive the real router (`Router.oneshot` with
bearer token) or real SQLite storage, assert scoping negatives (other users'
rows untouched, cross-tenant events filtered), and treat invalid input as 400
rather than silently ignoring the filter. No self-confirming or mirrored-logic
tests found.

## Non-blocking observations (recorded, no action required)

- Pagination is limit/offset, not cursor-based; the implementation plan
  suggested a cursor approach but the acceptance criterion requires only
  "pagination", and limit/offset matches every other list endpoint in the API.
- `mark_all_read` sweeps dismissed-but-unread rows too (documented in the port
  doc comment) so the badge count deterministically reaches zero — consistent
  across all three adapters and covered by tests.
- Generated `web/dist/` is untouched; this task has no web-source changes, so
  there is nothing to regenerate (observation only, per review policy).

## Deferred to the verifier (build-wrapper restriction, not a code defect)

The sandbox wrapper rejects gyre-server test-target compilation (4 GiB budget).
The verifier must run: `cargo test -p gyre-server --lib api::users` (the five
new server tests + team suite), `cargo test -p gyre-server --lib
push_check_notifications_broadcast_to_stream_subscribers`, `cargo test --all`,
and `python3 scripts/check-clippy-diff.py <merge base>` (gyre-server test
targets are the one class this review could not clippy locally; `cargo check -p
gyre-server --tests` is clean and lib clippy has zero changed-line hits).
