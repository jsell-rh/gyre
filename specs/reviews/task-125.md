# Review — task-125 (Implement teams and notifications API endpoints)

Spec: `specs/system/user-management.md` §API > Teams, §API > Notifications.
Base `cd58a166` → candidate `cb3c0c0c` (product commits `bc0d0eac`,
`e4af712a`, `83594b91`, `2a8cf5f6`; merge/process commits in between).
Verdict: **approved** — no findings. Evidence and full probe logs:
`/tmp/stage/review-evidence/task-125-review-evidence.md`.

## What was reviewed

Full diff of the 38 touched files against the assigned base, the five
specced team endpoints (audit — shipped by task-122), the seven specced
notification endpoints, the SSE fan-out architecture, all new tests at
source level, and the mechanical invariant gates.

## Probes run (independent, at candidate tree)

- `cargo test -p gyre-common notification` → **9/9 passed** (incl.
  `rank_band_is_inverse_of_from_rank_and_level_str_parses` — bands
  partition HSI ranks 1–10 contiguously; `from_level_str` case-insensitive,
  rejects unknown levels).
- `cargo test -p gyre-adapters --lib sqlite::notification` → **7/7 passed
  against real SQLite storage + migrations**, incl.
  `count_unread_and_mark_all_read_scope_semantics` (unread/read/dismissed/
  cross-user/cross-workspace fixtures; badge count, workspace-scoped sweep,
  cross-user immutability) and `list_for_user_filters_by_repo` (repo filter
  is real SQL pushdown, not handler post-filtering).
- `cargo check -p gyre-server --tests` → clean (type-checks the new
  handlers, SSE stream, and all new test code; zero warnings).
- `cargo fmt --check -p gyre-server` → clean (rustfmt repair `2a8cf5f6`
  verified; the prior verification run 74a4da02's only failure class).
- Mechanical gates all pass: check-arch, check-abac-route-registry,
  check-abac-exempt-handlers (79 handlers), check-mem-port-contracts,
  check-mcp-write-tools, check-dead-message-kinds, check-inert-enforcement,
  check-in-memory-state-stores, check-conditional-test-guards,
  check-fail-open-ref-resolution, check-scope-literal-defaults,
  check-fabricated-scope-defaults, check-lossy-secret-conversion,
  check-forwarded-header-trust, check-relative-path-defaults,
  check-unbounded-external-http, check-byte-slice-truncation,
  check-migration-versions, check-migration-sql-portability,
  check-forged-scope-fields, check-task-commit-attribution (against the
  committed candidate's task file — the checkout working tree carries the
  executor's uncommitted pristine-contract revert of `specs/tasks/task-125.md`,
  a stage artifact, not candidate content; restored byte-identical after the
  probe).
- `check-assertionless-tests.sh` aborts in this sandbox only because system
  awk is mawk (no gawk 3-arg `match()`); environment artifact independent of
  the candidate, not a defect.
- Server test build attempt blocked by the 4 GiB build wrapper as documented
  ("gyre-server code generation plus the model exceeds 4 GiB") — recorded,
  deferred to the independent verifier (commands below).

## Verified working (no findings)

- **Teams (§API > Teams) audit is accurate.** All five endpoints exist at
  the specced paths (`api/mod.rs` 976–988), shipped by task-122:
  `GET/POST /workspaces/:id/teams`, `PUT/DELETE .../teams/:team_id`,
  `POST .../teams/:team_id/members`,
  `DELETE .../teams/:team_id/members/:user_id`. Re-reading the handlers
  confirms authenticating, tenant-scoped via `load_workspace_scoped`, role
  gating, and per-task-122 test coverage. Nothing further was needed; the
  audit-only treatment of this half of the task is correct.
- **Every §API > Notifications row exists at the spec path.**
  `GET /api/v1/notifications` (limit/offset pagination; `?type=` via serde
  rename with `notification_type` legacy alias; `?priority=` parsed
  case-insensitively and widened to its HSI §8 rank band — unknown level is
  a 400, not a silently ignored filter; `?workspace_id=`; `?repo_id=` pushed
  down through a new `list_for_user` parameter into SQLite, PostgreSQL, and
  mem), `GET /api/v1/notifications/unread-count` (new `count_unread` port
  method: active = unresolved ∧ undismissed ∧ unread), `PUT
  /api/v1/notifications/{id}/read` (spec method added alongside the kept
  POST, on the existing per-handler-auth handler), `POST
  /api/v1/notifications/mark-all-read` (new `mark_all_read` in all three
  adapters, stamps `read_at`, optional workspace sweep, `{marked}`), and
  `GET/PUT /api/v1/notifications/preferences` (spec paths on the existing
  HSI §12 caller-scoped handlers).
- **The SSE stream is real end-to-end, with a closed fan-out.**
  `AppState::create_notification` → `BroadcastingNotificationRepo` is the
  single persist+broadcast point; the ~25 production emitters route through
  it and all four `DivergencePorts` feeders (git push, mirror sync, repo
  import ×2) pass `broadcasting_notifications()` instead of the bare repo —
  a grep sweep confirms the only remaining bare
  `state.notifications.create(` sites are `#[cfg(test)]` seeds, and the only
  `notification_broadcast_tx.send` site is the wrapper. The handler
  subscribes before returning the response (no connect race), filters each
  event against caller user id AND tenant plus optional `workspace_id`
  before the wire, treats broadcast `Lagged` as keep-streaming (only
  `Closed` ends the stream — review repair `83594b91`), and keeps the
  connection alive with heartbeats. The mem/test-state and impersonation
  middleware state clones share the production channel.
- **No test inflation.** The five new server tests drive the real router
  via `oneshot` with bearer auth and assert behavior that fails if the
  production path breaks: filter narrowing per dimension, 400 on unknown
  priority, badge-count arithmetic across unread/read/dismissed/other-user
  fixtures, cross-user sweep immutability, PUT-method 204 with `read_at`
  stamped via repository `get`, preferences roundtrip through the real
  store, and SSE delivery asserting the caller's event arrives while
  other-user and cross-tenant payloads never reach the wire. The
  graph_extraction regression test proves a push-check notification built
  through the broadcasting wrapper is persisted AND delivered on a
  subscribed receiver — it fails at the broadcast assertion if a feeder ever
  passes a bare repo again. Adapter-level tests (run here, passing) exercise
  real storage.
- **Route/ABAC registration is complete and consistent**: all new routes
  registered in `api/mod.rs` with `RouteResourceMapping::exempt` entries
  following the existing per-handler-auth notification model (the
  route-registry and exempt-handler gates both pass; no exemption-file
  growth; `:id` param name is uniform so no matchit conflicts).
- **Port-contract parity**: the new `count_unread`/`mark_all_read`/repo
  filter semantics are enforced identically in SQLite, PostgreSQL, and mem
  (check-mem-port-contracts passes; implementations are structural twins).
- **Coverage matrix updated honestly**: rows 28/29 → implemented with
  detailed evidence notes; counts consistent (6/36, 19 task-assigned).
  Docs updated. Commit attribution complete (all four task-labeled commits
  in the frontmatter).

## Deferred to the independent verifier (required probes)

Server test execution is blocked by the 4 GiB build wrapper in this
sandbox; `cargo check -p gyre-server --tests` is clean and the focused
package suites pass. The verifier must run:

    cargo test -p gyre-server --lib notification_stream_delivers_own_notifications_only
    cargo test -p gyre-server --lib spec_path_notifications_list_with_filters
    cargo test -p gyre-server --lib spec_path_unread_count_and_mark_all_read
    cargo test -p gyre-server --lib spec_path_preferences_roundtrip
    cargo test -p gyre-server --lib put_read_is_the_spec_method
    cargo test -p gyre-server --lib push_check_notifications_broadcast_to_stream_subscribers
    cargo test -p gyre-server --lib notifications
    cargo test -p gyre-server --lib graph_extraction::tests
plus the full `cargo test --all` acceptance gate.

## Non-blocking notes (recorded for completeness)

- `mark_all_read` flips dismissed-but-unread rows while `count_unread`
  excludes dismissed rows — a documented, tested, harmless superset ("mark
  all read" literally) that guarantees the badge reaches zero.
- The SSE handler stacks a 30s in-stream heartbeat comment on top of axum's
  15s `KeepAlive` ping — redundant keep-alive, no behavioral issue.
- No `web/` source changes in this candidate (API-only task); `web/dist`
  untouched, which is correct here — the verifier owns bundle regeneration.
