# Review — task-114 (Public user profile page `/@{username}`)

Spec: `specs/system/user-management.md` §User Profile Page (`/@{username}`) (rescoped 2026-09-30: public profile only; no My Dashboard scope, nothing added to private `/profile`).
Candidate: `652bb027d11e87c2539cb086c24276c3a3f4904c` (base `81c24bf07b059128fe6019c1df6f1c6924299930`).
Verdict: **approved** — the round-1 blocking finding is fixed with real code and a production-identity test; every acceptance criterion is backed by production behavior.

## Round 2 (this review, post-rebase + repair)

Commits under review since the previously reviewed `8174c6bb`: `d05ad4c9`/`6509b2c1` (rebase onto main), `c60e741e` (fix: attribute profile spec approvals by canonical user Id), `2b73d849`/`652bb027` (process).

### Prior finding F1 (blocking, round 1) — FIXED and verified

Round 1 found `approval_identity_forms` queried only `user:{display_name}`/`user:{username}` while the production ledger writer (`approve_spec`, `crates/gyre-server/src/api/specs.rs:647-656`) records the raw canonical user Id for human principals, so `specs_approved` was always 0 for real approvals and the feed never showed spec events. Verified fixed:

- `approval_identity_forms` (`users.rs`) now leads with `user.id.to_string()` — exactly the form `approve_spec` writes (`auth.user_id` → `ledger_approver`), a form pinned in production by the specs.rs regression "ledger approver_id must be the canonical user Id" (specs.rs:3337). The legacy `user:{name}` forms are retained only for historical rows and are deduped by ledger id at both call sites.
- The test `public_profile_activity_and_stats` now seeds the approved ledger row under `owner.id.to_string()` (the production form) and the rejected row under the legacy form — it is no longer mirrored-logic; it fails if the canonical form is dropped from the reader (that was precisely the round-1 reproduction).

### Prior finding F2 (minor, mem ordering) — FIXED

`MemSpecApprovalRepository::list_by_approver` (and the sibling listers) now sort by id string descending, matching the SQL adapters' `ORDER BY id DESC` — verified `spec_approvals.id` is a TEXT column (`SpecApprovalRow.id: String`), so string ordering is parity, not approximation. No user-visible effect either way (the handler re-sorts by timestamp), but the port contract now holds in every adapter.

### Probes run (evidence: `/tmp/stage/review-evidence/`)

- `cargo check -p gyre-server` → pass (0 errors, 7m09s).
- `cargo check -p gyre-server --tests` → pass (type-checks the new test code; 3m22s).
- `cargo test -p gyre-adapters -- --skip postgres "list_by"` → 27 passed, including the three new port-method tests: `sqlite::agent::tests::list_by_spawner_filters_and_orders_newest_first`, `sqlite::review::tests::list_by_reviewer_filters_and_orders_newest_first`, `sqlite::spec_approval::tests::list_by_approver_returns_only_that_approver` (each proves filter + newest-first ordering with a foreign-row control).
- `cd web && npm ci` then `npx vitest run src/__tests__/PublicUserProfile.test.js src/__tests__/AppShell.test.js` → 85/85 passed.
- `cd web && npm test` (full suite) → 121 files, 2405 passed / 41 skipped / 0 failed.
- Invariant scripts, all exit 0: `check-abac-route-registry`, `check-arch`, `check-mem-port-contracts`, `check-in-memory-state-stores`, `check-dead-message-kinds`, `check-forged-scope-fields`.
- `scripts/check-task-commit-attribution.sh` — attempted; requires full-history traversal and timed out in this sandbox. Verified manually instead: all six frontmatter SHAs (`fc06e371`, `978faf1a`, `d813d79b`, `b25f1060`, `c60e741e`, `23ebbb9c`) are present in `base..candidate` and cover every task-labeled product/test/build commit on the branch (the rest are `process:`/`verify:`/merge commits).

### Verifier must run (blocked here by the 4 GiB build wrapper — wrapper rejection logged)

- `cargo test -p gyre-server --lib public_profile` — covers `public_profile_includes_teams`, `public_profile_activity_and_stats` (stats correctness with a foreign-reviewer control, feed ordering, pagination, same-tenant peer read, cross-tenant 404 on both `/activity` and `/stats`), plus the pre-existing profile containment tests.
- `cargo test --all` (acceptance criterion).

### Contract verification

- **`GET /api/v1/users/{username}`** extended with `teams` drawn from accepted workspace memberships only (`TeamRepository::list_for_user` per accepted membership — a pending invitation cannot leak a team roster); registered in `api/mod.rs`; ABAC `RouteResourceMapping` entries added for all three routes (registry gate exit 0; nothing added to the frozen exemption files).
- **`GET /api/v1/users/{username}/activity`** — public actions only: `mr_review` (joined with the MR for `mr_title`/`repo_id`/`workspace_id` navigation), `spec_approved`/`spec_rejected`/`spec_revoked` from the durable ledger, `agent_spawned`. Newest first, `?limit=` (default 30, max 100) / `?offset=`, `{activity, limit, offset, total}`. Notifications/sessions/tokens never enter the feed. Attribution verified against the production writers: human reviews are recorded under the user id (enforced by `submit_review`'s own-identity check, merge_requests.rs:1128-1148); ledger rows under the canonical user Id (specs.rs:647-656); spawns under `auth.agent_id` = display name for user principals (orchestrator.rs:595, auth.rs:1625).
- **`GET /api/v1/users/{username}/stats`** — `{mrs_reviewed, specs_approved, agents_spawned}` from the same stores with the same dedup rules; rejected/revoked approvals are not counted as approved.
- **Tenant scoping** — all three handlers share `resolve_public_user` (cross-tenant indistinguishable from nonexistent, 404); test asserts 404 on both new endpoints cross-tenant.
- **New port methods** `ReviewRepository::list_by_reviewer`, `SpecApprovalRepository::list_by_approver`, `AgentRepository::list_by_spawner` implemented in SQLite + PostgreSQL + mem + the search-sync wrappers — real queries, no stand-ins.
- **Frontend** — `/@{username}` route in `parseUrl`/`urlFor` (+ history state, popstate restore, document title, breadcrumb with back arrow, URL-encoded handle round-trip tests); `PublicUserProfile.svelte` renders identity header (avatar/display name/@handle/timezone/joined), stats bar, workspace memberships with role badges, teams, activity feed with entity navigation; responsive single-column below 720px; 404 empty state with no follow-on API calls. Avatar-menu "Public profile" link to own handle; `goToUserProfile` context exposed for @mention/activity-feed links (no @mention-rendering surface exists in the UI yet to wire). Private `/profile` (`UserProfile.svelte`) untouched; HSI §12 NOT-list guarded by a test.
- **e2e locator change** (`hasText: /^Profile$/`) is a correct exact-match adjustment for the new sibling "Public profile" menu item.
- Coverage matrix row 23 → `implemented` with accurate evidence; no `specs/system/` files modified; commit attribution complete.

### Non-blocking observations (recorded, no action required for this task)

- Legacy `user:{display_name}`/`user:{username}` ledger-form matching is not tenant-scoped (the approval ledger has no tenant column), so a cross-tenant display-name collision can attribute another tenant's legacy approval rows (spec path/sha/timestamps) to this profile. Canonical-Id form is primary and globally unique; legacy rows only exist from the pre-canonical-Id writer. Suggested future hardening: drop the legacy forms once migration is complete, or tenant-scope the ledger.
- Agent-spawn attribution keys on display name/username (what `auth.agent_id` carries for user principals); a display-name change orphans prior spawns on the profile. Data-model limitation of `spawned_by`, not introduced by this task's reader.
- `AppShell.test.js` router cases exercise the file's pre-existing replicated `parseUrl`/`urlFor` copy (the file's established pattern since before this task), not the exported `App.svelte` functions; the App-level render path for `/@{username}` has no e2e navigation test (the component itself is fully tested with mocked API). Non-blocking.
- `PublicUserProfile` renders the not-found state for any profile-fetch error, conflating 5xx with 404. Minor UX.

Generated `web/dist/` observations are deferred to the deterministic verifier per the stage contract (dist regen is attributed on this branch via `23ebbb9c`).

Test-inflation check: none found. The backend test seeds a foreign reviewer's review as a control, drives the real router via `oneshot`, asserts exact newest-first event ordering and pagination totals, and exercises both a same-tenant peer and a cross-tenant 404; the adapter tests prove filter + ordering against real SQLite; the frontend tests assert rendered output and click-through navigation, and fail if the stats bar, teams, or feed are removed.

Scope check: no out-of-scope edits — no production code, scripts, or specs modified beyond the task's surface; `specs/system/` untouched; only `specs/coverage/`, `specs/tasks/task-114.md`, `docs/`, and the task's own source/test files changed.
