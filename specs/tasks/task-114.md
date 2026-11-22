---
title: "Implement public user profile page (`/@{username}`)"
spec_ref: "user-management.md §User Profile Page (`/@{username}`)"
depends_on:
  - task-113
  - task-208
progress: complete
coverage_sections:
  - "user-management.md §User Profile Page (`/@{username}`)"
commits: ["c60e741e10695fda2884e7fc03a9e7aa88cd9d85", "23ebbb9c7aa80a3405ca92c7b33d868650958ea9", "b25f1060558b2e557a329efbda7eb9efcae231e8", "d813d79b46ef04732ccd88ffb83039a9e606293f", "978faf1a180b555eab24b739a2b5653f21bdcad3", "fc06e3713988b2ec3647e87e077da00b23ad52c1", "450e5841ebcd2e56a9b1d5e280b4e76e5664b1d0"]
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
  ledger entries matched against the canonical user Id `approve_spec`
  writes for human principals plus the legacy `user:{name}` forms for
  pre-canonical-Id rows, deduped by row id; spawns by the recorded
  principal id).
- All three handlers share `resolve_public_user`: tenant containment
  with 404 for cross-tenant handles (indistinguishable from
  nonexistent). ABAC resolver maps the two new routes to the "user"
  resource (route-registry check passes).
- New port methods `ReviewRepository::list_by_reviewer`,
  `SpecApprovalRepository::list_by_approver`,
  `AgentRepository::list_by_spawner` — implemented in SQLite,
  PostgreSQL, mem, and the search-sync indexing wrappers (real queries,
  deterministic id-desc ordering matching the SQL adapters; SQLite
  adapter tests prove filter + ordering).

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

### Review repair round (c60e741e)

Independent review (4cacdd86) found the spec-approval attribution read
the wrong identity key. Fixed in c60e741e:

- `approval_identity_forms` matched only legacy `user:{display_name}` /
  `user:{username}` ledger keys, but the production writer
  (`approve_spec`) records the raw canonical user Id for every human
  principal (pinned by the specs.rs regression "ledger approver_id must
  be the canonical user Id") — disjoint key sets, so
  `/stats` always returned `specs_approved=0` and the feed never showed
  spec events for real human approvals. The canonical Id is now the
  primary queried form; the legacy name forms remain so
  pre-canonical-Id rows still attribute (call-site dedup by ledger id
  unchanged). Residual, acknowledged: legacy name-form rows can
  cross-attribute within a tenant when two users share a display name
  (review noted this as minor/non-blocking; no discriminator exists on
  those rows, and current-writer rows attribute exactly by canonical
  Id). Agent-spawn attribution still matches the display-name principal
  id because that is what the spawn path records — changing that
  writer is out of this task's scope.
- The mirrored-logic test was re-seeded via the production identity:
  the approved ledger row now carries `owner.id.to_string()` (the form
  `approve_spec` writes), and the rejected row keeps the legacy
  `user:{display_name}` form to pin the historical fallback.
- `MemSpecApprovalRepository` list methods claimed "insertion order:
  newest first" but reversed arbitrary HashMap iteration; they now sort
  by id string descending, mirroring the SQL adapters' `ORDER BY id
  DESC` (the port contract's deterministic ordering).

### Test evidence

- In-sandbox (this round): `cargo check -p gyre-server --lib` and
  `cargo check -p gyre-server --all-targets` (covers the cfg(test)
  module with the re-seeded test) both exit 0. Evidence:
  /tmp/stage/review-evidence/review-repair.md.
- In-sandbox (prior rounds): full vitest suite 121 files / 2398 passed;
  focused `PublicUserProfile.test.js` (8 tests incl. the HSI §12
  NOT-list guard) and `AppShell.test.js` `/@{username}` routing tests;
  `cargo test -p gyre-adapters --lib list_by` (27 passed); the fast
  mechanical gate scripts (arch, ABAC registry/exempt-handlers,
  mem-port-contracts, task attribution) all pass.
- Deferred to the independent verifier (4 GiB build wrapper):
  `cargo test -p gyre-server --lib api::users::tests` — mem-backed
  tests `public_profile_includes_teams` and
  `public_profile_activity_and_stats` (production-identity spec
  attribution: approved row under the canonical user Id counts, legacy
  row attributes, rejected entry not counted as approval; another
  reviewer's rows excluded; newest-first ordering; `?limit=` pagination
  + total; same-tenant peer 200; cross-tenant 404) plus the
  pre-existing `get_user_by_username_is_tenant_scoped` regression — and
  the workspace-wide `cargo clippy --all-targets` diff gate. Required
  probe list in /tmp/stage/review-evidence/verifier-tests.md.

## Agent Instructions

Read `specs/system/user-management.md` §User Profile Page (`/@{username}`) — the "My Dashboard" section above it is superseded (see rescoping note). Public profile endpoints: add `GET /api/v1/users/{username}` (+`/activity`, `/stats`) in `crates/gyre-server/src/api/users.rs`; register routes in `crates/gyre-server/src/api/mod.rs`; add ABAC mappings in `crates/gyre-server/src/abac_middleware.rs`. The Svelte app entry is `web/src/App.svelte` (router is `parseUrl`/`urlFor`, App.svelte:183-270). User model: `crates/gyre-domain/src/user.rs`.

Note: `/profile` (private settings surface) already exists and is governed by HSI §12 — do not add activity-hub features there (no My Tasks/MRs/Agents, per the HSI §12 NOT-list). This task's `/@{username}` page is a separate, public tenant-scoped surface.
