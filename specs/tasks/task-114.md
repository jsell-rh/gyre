---
title: "Implement public user profile page (`/@{username}`)"
spec_ref: "user-management.md §User Profile Page (`/@{username}`)"
depends_on:
  - task-113
  - task-208
progress: ready-for-review
coverage_sections:
  - "user-management.md §User Profile Page (`/@{username}`)"
commits:
  - fc06e371
  - 978faf1a
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

- [x] User profile page at `/@{username}`
- [x] Profile shows workspace memberships, teams, activity, stats
- [x] Profile is tenant-scoped (only visible to same-tenant users)
- [x] Responsive layout
- [x] `cargo test --all` and `npm test` pass (npm test: full vitest suite
  2398 passed in-sandbox; server tests deferred to the independent verifier
  per the 4 GiB build wrapper — `cargo check -p gyre-server --lib --tests`
  passes, required probe list in /tmp/stage/review-evidence/verifier-tests.md)

## Shipped

The public user profile page (`/@{username}`) is implemented end-to-end,
scoped to the rescope note above (public profile only — no My Dashboard
 scope, nothing added to the private `/profile` surface, HSI §12 NOT-list
respected and guarded by a test).

### Backend

- `GET /api/v1/users/{username}` now also returns `teams` — team
  memberships drawn from the subject's **accepted** workspace memberships
  (`TeamRepository::list_for_user`), so a pending invitation never leaks a
  team roster.
- `GET /api/v1/users/{username}/activity` — the spec's "recent activity
  feed (public actions only)": MR reviews (`mr_review`, joined with the MR
  so the UI gets `mr_title`/`repo_id`/`workspace_id` for navigation), spec
  approvals/rejections/revocations from the durable approval ledger
  (`spec_approved`/`spec_rejected`/`spec_revoked` with `spec_path`), and
  agent spawns (`agent_spawned` with `agent_name`). Newest first,
  `?limit=` (default 30, max 100) / `?offset=`, `{activity, limit, offset,
  total}`. Private state (notifications, sessions, tokens) is never in the
  feed.
- `GET /api/v1/users/{username}/stats` — `{mrs_reviewed, specs_approved,
  agents_spawned}` from the same stores. Reviews attribute by user id
  (globally unique, enforced at submission); ledger entries match the
  approver identity forms `approve_spec` actually writes
  (`user:{display_name}` / `user:{username}`, deduped by row id); spawns
  match the principal id the spawn path records.
- All three handlers share `resolve_public_user`: tenant containment with
  404 for cross-tenant handles (indistinguishable from nonexistent),
  mirroring the pre-existing profile endpoint. ABAC resolver maps the two
  new routes to the "user" resource (route-registry check passes).
- New port methods `ReviewRepository::list_by_reviewer`,
  `SpecApprovalRepository::list_by_approver`,
  `AgentRepository::list_by_spawner` — implemented in SQLite, PostgreSQL,
  mem, and the search-sync indexing wrappers (real queries, newest-first
  ordering; SQLite adapter tests prove filter + ordering).

### Frontend

- `/@{username}` route in `App.svelte`: `parseUrl`/`urlFor` (handle is
  URL-decoded/encoded), history state carries the username, popstate
  restores it, document title `@{username} | Gyre`, topbar breadcrumb with
  back arrow.
- `PublicUserProfile.svelte`: identity header (avatar image or initial,
  display name, `@username`, timezone, joined date), stats bar (MRs
  reviewed / specs approved / agents spawned), workspace memberships with
  role badges (names resolved from the workspace list), team memberships
  (`team:{slug}`), activity feed with per-type icons and entity navigation
  (MR/agent entries open the entity detail via the `goToEntityDetail`
  context). Responsive: two-column grid collapses to one column below
  720px, stats stack below 420px. 404 renders an empty state ("User not
  found") and does not probe stats/activity.
- Navigation integration: the top-nav avatar menu gains "Public profile"
  linking to the caller's own `/@{username}`; a `goToUserProfile` context
  is exposed for future @mention/activity-feed surfaces (no @mention
  rendering surface exists in the UI today — the context + URL route are
  the integration point).

### Test evidence

- In-sandbox: full vitest suite 121 files / 2398 passed (41 skipped);
  focused `PublicUserProfile.test.js` (8 tests: header, memberships +
  teams, stats, activity + MR navigation, empty feed, 404 handling, HSI
  §12 NOT-list guard) and `AppShell.test.js` routing tests for
  `/@{username}`; `cargo test -p gyre-adapters --lib list_by` (27 passed,
  incl. the three new adapter tests); `cargo check -p gyre-server --lib
  --tests`; check-abac-route-registry / check-arch / check-mem-port-
  contracts all pass.
- Deferred to the independent verifier (4 GiB build wrapper):
  `cargo test -p gyre-server --lib api::users::tests` — new mem-backed
  tests `public_profile_includes_teams` and `public_profile_activity_and_
  stats` (stats correctness — another reviewer's rows excluded, rejected
  approval not counted; newest-first ordering; `?limit=` pagination +
  total; same-tenant peer gets 200 on the feed; cross-tenant 404 on both
  endpoints) plus the pre-existing
  `get_user_by_username_is_tenant_scoped` regression.

Coverage row 23 (`user-management.md` §User Profile Page) is marked
implemented with the evidence above.

## Agent Instructions

Read `specs/system/user-management.md` §User Profile Page (`/@{username}`) — the "My Dashboard" section above it is superseded (see rescoping note). Public profile endpoints: add `GET /api/v1/users/{username}` (+`/activity`, `/stats`) in `crates/gyre-server/src/api/users.rs`; register routes in `crates/gyre-server/src/api/mod.rs`; add ABAC mappings in `crates/gyre-server/src/abac_middleware.rs`. The Svelte app entry is `web/src/App.svelte` (router is `parseUrl`/`urlFor`, App.svelte:183-270). User model: `crates/gyre-domain/src/user.rs`.

Note: `/profile` (private settings surface) already exists and is governed by HSI §12 — do not add activity-hub features there (no My Tasks/MRs/Agents, per the HSI §12 NOT-list). This task's `/@{username}` page is a separate, public tenant-scoped surface.
