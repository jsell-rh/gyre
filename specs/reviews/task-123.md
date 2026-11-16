# Review — task-123 (Notification entity: full type taxonomy and priority model)

Spec: `specs/system/user-management.md` §Notification Entity (amended
priority-band membership table, task-112 review).
Candidate under review: `48aa8f58c31fa16f8e1fcb557ac416a35bee5c6f` on assigned
base `047d652495dde3b1ae6ddbea4f8cf1b077a3c18f` (merge `b48693f6`, rustfmt
repair `58f0fae8`, process record `48aa8f58`).
Verdict: **approved** — no findings.

## Scope of this round

The task's original surface (entity fields, 18-variant taxonomy,
`NotificationPriority`, `NotificationSource`, migration 000059, typed
emitters, mark-read endpoints, Inbox UI labels) shipped in earlier
task-labeled commits (`27bfb2a6`, `deec7953`, `f92a4891`, `d6592051`,
`12f5495f`, `6b3d4641`, `90edbb2f`) and is already contained in the assigned
base via mainline merges; this candidate's residual delta against the base
(986 insertions, 17 files — full diff saved as
`/tmp/stage/review-evidence/candidate-diff.txt`) is the priority-band
consistency repair plus a whitespace-only rustfmt repair:

- The entity's four-level `priority` band now derives from the notification
  TYPE via `NotificationPriority::for_notification_type` (gyre-common, the
  canonical amended membership table) — at creation (`Notification::new` →
  `default_priority`) and at read time (SQLite/PostgreSQL rows reconstruct
  the band from the persisted `notification_type`) — never from the HSI §8
  1–10 numeric rank, which remains the Inbox-ordering scale
  (`priority_rank`).
- The domain channel-routing
  `gyre-domain::NotificationPriority::for_notification_type` now delegates
  to the same canonical table, so the entity band, the response's
  `priority_level`, webhook/email payloads, and channel `min_priority`
  gating cannot disagree.
- `?priority=` on `GET /api/v1/notifications` /
  `GET /users/me/notifications` filters by the type-derived band: the level
  expands via `NotificationPriority::notification_type_strings()` (canonical
  strings plus the legacy DB names of renamed variants, so pre-unification
  rows keep filtering by their band) into the port's new exact-match
  type-set filter, intersected with `?type=`. Unknown level → 400. An empty
  set matches nothing in every adapter (SQL early-return, mem `contains`,
  broadcasting wrapper delegates).
- `list_for_user`'s `notification_type: Option<&str>` became
  `notification_types: Option<&[&str]>` across the port and all four
  implementations; all production call sites updated.
- spec_patrol error findings keep the rank-3 Inbox position but drop the
  `from_rank(3)` band override (band stays the type's Medium, per the
  amended table and the spec's own band comments: Medium = "spec drift").

## Verification performed

Independent probes (evidence under `/tmp/stage/review-evidence/`):

- Full base→candidate diff read line-by-line.
- Membership-table cross-check: the code's `for_notification_type` bands all
  41 variants identically to the base's domain routing table (the delegation
  is behavior-preserving) and to the amended spec table in
  `user-management.md` (Urgent/High/Medium/Low rows verified variant by
  variant; post-amendment merged types banded by the same documented
  classes). The spec's own level examples (Low=merged/passed,
  Medium=stale/drift, High=budget-warning/gate-failure,
  Urgent=security/escalation/reverted) all hold.
- `cargo test -p gyre-common` → 128 passed, incl. 12 notification tests:
  the amended membership table pinned per variant independently of
  `for_notification_type`, `all()` enumerates the taxonomy exactly once
  (array-size `[Self; 41]` + dedup + parse round-trip makes the enumeration
  compile-complete), and the band type-string sets partition the taxonomy
  with legacy names riding their variant's band.
- `cargo test -p gyre-adapters --lib notification` → 13 passed against the
  real SQLite migration, including the two killing tests:
  `read_path_reconstructs_band_from_type_not_rank` (persists rank-2/4/7/9
  rows and asserts the loaded band is the type's band — `from_rank` would
  return Urgent/High/Medium/Low respectively and fail) and
  `list_for_user_type_set_filter_matches_persisted_column` (inserts a raw
  legacy `SpecPendingApproval` row and asserts the High set matches the
  persisted column, empty set matches nothing, no filter returns all).
- `cargo test -p gyre-domain` → 563 passed (delegation compiles and behaves).
- `cargo check -p gyre-server` → clean (5m04s). Server test codegen is
  blocked by the sandbox build wrapper (4 GiB); required server probes
  recorded in `deferred-probes.txt`, notably
  `api::users::tests::spec_path_notifications_list_with_filters`, which
  seeds a rank-2 `SpecApprovalRequested` and asserts `?priority=high`
  returns it labeled `priority_level: "High"` while `?priority=urgent`
  returns none — the exact assertions the old rank-band widening fails.
- `bash scripts/check-arch.sh` → pass; `bash scripts/check-mem-port-contracts.sh`
  → pass.
- rustfmt: the assigned base SHA is no longer fetchable from origin in this
  sandbox (partial clone, "not our ref"), so the changed-lines gate could
  not be re-run against it; substitute probe — all 14 changed `.rs` files
  are fully clean under the gate's exact invocation
  (`rustfmt --edition 2021 --config skip_children=true --check`), strictly
  stronger than the changed-lines gate. `58f0fae8` verified whitespace-only
  (two re-indented lines in a test call site).
- Grep sweep: no production band derivation from a numeric rank remains —
  `from_rank`/`rank_band` have test-only callers and are documented as the
  numeric scale's own band description.
- Consistency sweep of every band consumer: `NotificationResponse`
  (REST + MCP inbox serialize through it), SSE broadcast (entity at
  creation), dispatcher payloads, CLI (passes the level name through;
  client-side normalization only), web Inbox (uses the numeric rank and
  P-badge only — no `priority_level` dependency, so no frontend change was
  needed this round and none was made).
- Handler edge cases verified in source: `from_level_str` is
  case-insensitive; `?type=`×`?priority=` intersect (not union); empty
  intersection is an empty set that matches nothing; `min_priority`/
  `max_priority` remain pure numeric-rank filters, now documented as such
  in `docs/api-reference.md`.

## Test-inflation check

None found. The new tests are killing tests by construction: the adapter
band-reconstruction test asserts loaded values that `from_rank` produces
differently for every seeded rank; the type-set filter test's legacy-row and
empty-set assertions fail under both the old single-type filter and a
no-op-when-empty implementation; the server filter test's urgent-returns-none
and high-returns-rank-2-type assertions both fail under the removed
rank-band widening; the dispatcher test asserts entity band == routing band
for the 12 diverging types. `cargo test --all` (acceptance criterion) is
deferred to the verifier with the focused suites enumerated.

## Observations (non-blocking)

- The working tree carries an uncommitted executor staging of
  `specs/tasks/task-123.md` (progress `complete`, 5-commit list matching the
  assignment body); the candidate commit itself records the full 8-commit
  attribution including `58f0fae8`. Executor bookkeeping, not a code defect.
- `NotificationPriority::from_rank`/`rank_band` remain public helpers
  describing the numeric scale's own bands (documented, test-only callers).
- `SecurityFinding`/`SystemAlert` still have no emitting feature — correctly
  documented in the coverage row rather than faked (no scanner/system-alert
  source exists in the platform; owned by future tasks).
