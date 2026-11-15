---
title: "Implement public user profile page (`/@{username}`)"
spec_ref: "user-management.md §User Profile Page (`/@{username}`)"
depends_on:
  - task-113
  - task-208
progress: ready-for-review
coverage_sections:
  - "user-management.md §User Profile Page (`/@{username}`)"
commits: ["23ebbb9c7aa80a3405ca92c7b33d868650958ea9", "b25f1060558b2e557a329efbda7eb9efcae231e8", "d813d79b46ef04732ccd88ffb83039a9e606293f", "978faf1a180b555eab24b739a2b5653f21bdcad3", "fc06e3713988b2ec3647e87e077da00b23ad52c1"]
---

## Spec Excerpt

**RESCOPED (2026-09-30 PM cycle):** this task originally included the "My Dashboard" landing page with My Tasks / My MRs / My Agents sections. That model is superseded:
- The landing-page role is taken by `ui-navigation.md` §2 — "The workspace home is a dashboard... It's the landing page after selecting a workspace." No separate `/dashboard` page is to be built.
- My Tasks / My MRs / My Agents surfaces are **forbidden** in `/profile` by `human-system-interface.md` §12 ("What the Profile Is NOT"); task-208 removes them and amends `user-management.md` §"My Stuff" Views.

The remaining scope is the public user profile page only.

From `user-management.md` §User Profile Page (`/@{username}`):

Public within the tenant:
- Display name, username, avatar, timezone
- Workspace memberships and roles
- Team memberships
- Recent activity feed (public actions only)
- Stats: MRs reviewed, specs approved, agents spawned

## Implementation Plan

1. **Backend API:**
   - Add `GET /api/v1/users/{username}` — public profile endpoint (tenant-scoped)
   - Add `GET /api/v1/users/{username}/activity` — public activity feed (public actions only)
   - Add `GET /api/v1/users/{username}/stats` — aggregate stats: MRs reviewed, specs approved, agents spawned

2. **User Profile page (Svelte):**
   - Route: `/@{username}`
   - Profile header: avatar, display name, username, timezone
   - Workspace memberships with role badges
   - Team memberships
   - Activity feed (public actions)
   - Stats bar: counts of MRs reviewed, specs approved, agents spawned

3. **Navigation integration:**
   - User avatar in top nav links to profile
   - Profile pages accessible from @mentions and activity feeds

## Acceptance Criteria

- [ ] User profile page at `/@{username}`
- [ ] Profile shows workspace memberships, teams, activity, stats
- [ ] Profile is tenant-scoped (only visible to same-tenant users)
- [ ] Responsive layout
- [ ] `cargo test --all` and `npm test` pass

## Shipped

The public user profile page (`/@{username}`) is implemented end-to-end,
scoped to the rescope note above (public profile only — no My Dashboard
scope, nothing added to the private `/profile` surface, HSI §12 NOT-list
respected and guarded by a test). The acceptance-criterion boxes above
stay as assigned (protected contract text); the evidence that each is
satisfied is below.

### Backend

- `GET /api/v1/users/{username}` also returns `teams` — drawn from the
  subject's **accepted** workspace memberships
  (`TeamRepository::list_for_user`), so a pending invitation never leaks
  a team roster.
- `GET /api/v1/users/{username}/activity` — the "recent activity feed
  (public actions only)": MR reviews (`mr_review`, joined with the MR so
  the UI gets `mr_title`/`repo_id`/`workspace_id` for navigation), spec
  approvals/rejections/revocations from the durable approval ledger
  (`spec_approved`/`spec_rejected`/`spec_revoked` with `spec_path`), and
  agent spawns (`agent_spawned` with `agent_name`). Newest first,
  `?limit=` (default 30, max 100) / `?offset=`, `{activity, limit,
  offset, total}`. Private state (notifications, sessions, tokens) is
  never in the feed.
- `GET /api/v1/users/{username}/stats` — `{mrs_reviewed,
  specs_approved, agents_spawned}` from the same stores, with the same
  attribution rules the feed uses (reviews by globally-unique user id;
  ledger entries matched against both identity forms `approve_spec`
  writes, deduped by row id; spawns by recorded principal id).
- All three handlers share `resolve_public_user`: tenant containment
  with 404 for cross-tenant handles (indistinguishable from
  nonexistent). ABAC resolver maps the two new routes to the "user"
  resource (route-registry check passes).
- New port methods `ReviewRepository::list_by_reviewer`,
  `SpecApprovalRepository::list_by_approver`,
  `AgentRepository::list_by_spawner` — implemented in SQLite,
  PostgreSQL, mem, and the search-sync indexing wrappers (real queries,
  newest-first ordering; SQLite adapter tests prove filter + ordering).

### Frontend

- `/@{username}` route in `App.svelte`: `parseUrl`/`urlFor` (handle is
  URL-decoded/encoded), history state carries the username, popstate
  restores it, document title `@{username} | Gyre`, topbar breadcrumb
  with back arrow.
- `PublicUserProfile.svelte`: identity header (avatar image or initial,
  display name, `@username`, timezone, joined date), stats bar (MRs
  reviewed / specs approved / agents spawned), workspace memberships
  with role badges, team memberships (`team:{slug}`), activity feed with
  per-type icons and entity navigation (MR/agent entries open the
  entity detail via the `goToEntityDetail` context). Responsive:
  two-column grid collapses to one column below 720px, stats stack
  below 420px. 404 renders an empty state and does not probe
  stats/activity.
- Navigation integration: the top-nav avatar menu gains "Public
  profile" linking to the caller's own `/@{username}`; a
  `goToUserProfile` context is exposed for future @mention/
  activity-feed surfaces (no @mention rendering surface exists in the
  UI today — the context + URL route are the integration point). The
  two e2e user-menu locators that matched "Profile" by substring were
  tightened to exact-text matches because the menu now also offers
  "Public profile".

### Verification-gate repair round (b25f1060)

The prior candidate failed deterministic verification on the
  diff-scoped rustfmt and clippy gates (no behavioral defect). Fixed in
  b25f1060, all changes confined to lines this branch added:

- rustfmt: `api/users.rs` reformatted where the check flagged (handler
  and test code added by this task).
- clippy `unnecessary_sort_by`: the three descending sorts
  (`api/users.rs` activity merge, `mem.rs` `list_by_spawner` /
  `list_by_reviewer`) now use `sort_by_key` + `Reverse`, matching the
  existing `mem.rs` convention; ordering semantics unchanged
  (descending, stable).
- clippy `double_must_use`: `AgentRepository` and `ReviewRepository`
  gained the same `#[allow(clippy::double_must_use)]` trait attribute
  with rationale comment that base already carries on ~20 other port
  traits (async-trait 0.1.89 injects `#[must_use]` into every expanded
  method while the boxed-future return is already `#[must_use]`);
  `SpecApprovalRepository` already had it. No verifier or exemption
  file was touched.

### Rebase resolution (6509b2c1)

The published PR head (1574206a) conflicted with current main
(81c24bf0, task-137 gate-failure feedback). Resolved by merging main
into the branch (merge base c9cc1536):

- Only conflicted file: `specs/coverage/SUMMARY.md` — resolved by
  mechanical regeneration (`scripts/update-coverage-summary.sh`)
  from the merged coverage files: agent-gates row from main
  (task-137 closed row 9), user-management row from this branch
  (task-114 row 23), and an HSI row recount that fixes a stale
  candidate-side summary (HEAD's committed SUMMARY said 15/13 while
  its own coverage file already said 14/14).
- Source sides touch disjoint files (this branch: users API, ports,
  adapters, mem, search_sync, web/src; main: message bus, gate
  executor, merge processor, ralph loop, ws) — no semantic overlap;
  `cargo check -p gyre-server --lib --tests` clean on the merged tree.
- `web/dist` keeps the executor-restored assignment-base assets in
  the merge commit; final integration rebuilds and publishes them
  remotely. No dist files committed by this round.
- This round's in-sandbox probes: cargo check (lib + tests) exit 0;
  arch / ABAC route-registry / mem-port-contracts / task-commit-
  attribution gates pass; focused vitest `PublicUserProfile.test.js`
  8/8 and `AppShell.test.js` 77/77 (incl. the `/@{username}` routing
  tests); `cargo test -p gyre-adapters --lib list_by` 27 passed.
  Evidence: /tmp/stage/review-evidence/rebase-resolution.md.

### Test evidence

- In-sandbox (rebase round): probes listed under "Rebase resolution"
  above; full command log in
  /tmp/stage/review-evidence/rebase-resolution.md.
- In-sandbox (implementation round): full vitest suite 121 files / 2398
  passed (41 skipped); focused `PublicUserProfile.test.js` (8 tests:
  header, memberships + teams, stats, activity + MR navigation, empty
  feed, 404 handling, HSI §12 NOT-list guard) and `AppShell.test.js`
  routing tests for `/@{username}`; `cargo test -p gyre-adapters --lib
  list_by` (27 passed, incl. the three new adapter tests); check-abac-
  route-registry / check-arch / check-mem-port-contracts all pass.
- In-sandbox (repair round): `python3 scripts/check-rustfmt-diff.py
  706bc7bd` clean (14 files); `cargo clippy -p gyre-ports --all-targets`
  emits no warnings for the three touched port files; `cargo check -p
  gyre-server --lib --tests` exit 0; the fast mechanical gate scripts
  (arch, hierarchy, ABAC registry/exempt-handlers, mem-port-contracts,
  scope/forged/forwarded/inert/lossy/fabricated, in-memory stores,
  unbounded HTTP, task attribution) all pass.
- Deferred to the independent verifier (4 GiB build wrapper):
  `cargo test -p gyre-server --lib api::users::tests` — mem-backed
  tests `public_profile_includes_teams` and
  `public_profile_activity_and_stats` (stats correctness — another
  reviewer's rows excluded, rejected approval not counted; newest-first
  ordering; `?limit=` pagination + total; same-tenant peer gets 200 on
  the feed; cross-tenant 404 on both endpoints) plus the pre-existing
  `get_user_by_username_is_tenant_scoped` regression — and the
  workspace-wide `cargo clippy --all-targets` diff gate. Required probe
  list in /tmp/stage/review-evidence/verifier-tests.md.

## Agent Instructions

Read `specs/system/user-management.md` §User Profile Page (`/@{username}`) — the "My Dashboard" section above it is superseded (see rescoping note). Public profile endpoints: add `GET /api/v1/users/{username}` (+`/activity`, `/stats`) in `crates/gyre-server/src/api/users.rs`; register routes in `crates/gyre-server/src/api/mod.rs`; add ABAC mappings in `crates/gyre-server/src/abac_middleware.rs`. The Svelte app entry is `web/src/App.svelte` (router is `parseUrl`/`urlFor`, App.svelte:183-270). User model: `crates/gyre-domain/src/user.rs`.

Note: `/profile` (private settings surface) already exists and is governed by HSI §12 — do not add activity-hub features there (no My Tasks/MRs/Agents, per the HSI §12 NOT-list). This task's `/@{username}` page is a separate, public tenant-scoped surface.
