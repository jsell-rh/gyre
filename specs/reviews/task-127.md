# Review — task-127 (Implement user management UI pages)

Spec: `user-management.md` §UI Pages (coverage row 31).
Candidate under review: `6b562ecb6a2441e2f0c6f99d9264c975b8aed0e0` (assignment base `876c77b7`, task-077 HSI Trust Gradient). Rebase-repair round: merge `59770348` (conflict in `api/users.rs` `get_notifications` only), fix `94b10619` (dropped a duplicated `let disabled` binding left by the manual resolution), plus process commits.
Verdict: **approved** (round 1 of this checkpoint; earlier rounds' repairs verified in-tree).

## What ran (evidence in `/tmp/stage/review-evidence/`)

- `cd web && npm ci && npm test` → **Test Files 127 passed (127), Tests 2566 passed | 41 skipped**, exit 0 (`probe-frontend-tests.md`).
- Mutation probe: reversed the drawer's priority-group order in `NotificationDrawer.svelte` → `NotificationDrawer.test.js` "groups notifications by priority, Urgent first" **fails** (1 failed | 11 passed). Test kills a real regression; mutation reverted via `git checkout`, tree verified clean (`probe-mutation-drawer.md`).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib` → clean, no warnings, 6m27s (`probe-rust-check.md`). gyre-server test compilation exceeds this sandbox's 4 GiB budget — deferred to verifier (commands recorded below).
- `bash scripts/check-abac-route-registry.sh` → OK. `bash scripts/check-abac-exempt-handlers.sh` → OK (85 handlers).

## Acceptance criteria — verified

- **User Settings + Session Management** (`UserProfile.svelte`, `/profile`): real editing of display name/timezone/locale via `PUT /users/me`, theme/density/diff-view/feed-scope/font-size preferences, sessions via `GET /users/me/sessions` with per-session revoke and revoke-all. All `$state`/`$derived` runes.
- **User Profile `/@username`** (`PublicUserProfile.svelte`): parseUrl handles `@handle` segments; workspace/team memberships, public-action activity feed (`api.publicUserActivity`), stats. Rendered from App.svelte:2174.
- **Tenant User Management** (TenantSettings Users tab): TenantAdmin-gated (`usersNotAdmin` state from the 403 path), list/search, deactivate with reason + confirm, provisioning, inline global-role select over the new `PUT /users/{id}/role`.
- **Tenant Invitation Management** (Invitations tab): status filter over `GET /tenant/invitations`, single + bulk invite modals, revoke for Pending rows, one-time `invite_url` with copy.
- **Workspace Members/Invitations/Teams** (WorkspaceSettings): member list with role badges, invite, role-change dropdown with audit reason, ownership transfer (Owner-role change requires a reason and is aborted on cancelled prompt), pending invitations with expiry countdown (`expires in Nd/Nh/Nm`) and revoke, full team CRUD + membership.
- **Invitation Accept `/invite/{token}`** (`InvitationAccept.svelte`): parseUrl disambiguates 64-hex magic-link tokens from UUID workspace ids (shapes disjoint); token flow previews via read-only unauthenticated `GET /invite/{token}/preview`, accepts/declines via the tenant/workspace token endpoints with **no Bearer header** (`api.js` `skipAuth`, asserted by api.test.js — a logged-in identity cannot burn someone else's invitation); local-mode first API key displayed once with copy. Authenticated `/invite/{workspaceId}` flow resolves the caller's Pending WorkspaceInvitation via `GET /users/me/invitations`; non-Pending statuses render the none state.
- **Notification Drawer**: bell with unread badge (`GET /notifications/unread-count`), slide-out grouped Urgent→Low by `priority_level` (mutation-verified), deep links replaying popstate routing against server `action_url`, per-item and bulk mark read/unread, `?active=true` listing excluding resolved/dismissed rows.
- **Endpoints consumed**: all task-124/125 surfaces wired through `web/src/lib/api.js` (sessions, role, invitations, teams, notifications spec paths).

## Merge resolution (`get_notifications`) — verified coherent

The `?active=true` drawer branch applies no exclusion set and is paired with `count_unread` (which applies none); the default Inbox branch applies preference-disabled + HSI §2 trust-excluded types and is paired with `count_unresolved` (which applies the same combined set). Each surface agrees with its own badge; both branches share one `type_filter` derivation. The base's trust machinery (`trust_inbox_excluded_types`) is preserved intact — the only deletions vs base are the refactored comment block and the restructured `list_for_user` call site. `list_active_for_user` is implemented in all three adapters (sqlite/postgres/mem — mem guards in code, empty type set matches nothing, matching SQL semantics) plus the broadcasting wrapper.

## Server-side additions — verified real

- `PUT /api/v1/users/:id/role` (`update_user_role`): TenantAdmin per-handler gate with RBAC-denied recording, role vocabulary validation, same-tenant containment (system principal exempt), self-change 409, deactivated-target 409, idempotent no-op, GlobalRole↔UserRole lockstep, audit event. Route registered; ABAC registry + exempt-handler checks pass.
- `GET /api/v1/invite/:token/preview` (`preview_invitation`): strictly read-only — token-hash lookup in tenant then workspace invitation stores, `effective_status` reports expiry without persisting a transition, workspace names resolved for the preview, 404 otherwise. Registered outside `require_auth_middleware` with the other token-as-auth-factor routes.

## Test quality

Spot-checked the new suites: they assert endpoints called, headers absent (skipAuth), DOM state per outcome (accepted card with minted key, none state for expired/terminal links, load-error for non-404), grouping order, and navigation side effects — not mirrored logic. Mutation probe confirms the drawer grouping test fails when the behavior breaks.

## Non-blocking observations

- **Generated output (verifier-owned):** `web/dist` is resolved verbatim to the assignment base `876c77b7` — intentional; the verifier rebuilds remotely and binds publication to the tested tree. Not a finding.
- The drawer deliberately applies no preference/trust exclusion set (documented in-code): it is paired with `count_unread`, which counts every active notification; the Inbox and its `count_unresolved` badge apply the combined exclusion set. Surfaces are internally consistent; the task's spec excerpt imposes no trust filter on the drawer.
- Coverage-matrix note in row 31 cites "web suite 2494 passed" from the earlier round; the merged tree now runs 2566 (base added tests). Stale count in prose only, not a status inaccuracy — row 31's `implemented` status is backed by the shipped pages verified above.

## Verifier must run (recorded per 4 GiB budget)

- `cargo test --all`
- `cargo test -p gyre-server --lib inbox_trust_filter`
- `cargo test -p gyre-server --lib spec_path_notifications_active_filter_excludes_resolved_and_dismissed`
- `cargo test -p gyre-server --lib spec_path_notifications_list_with_filters`
- `cargo test -p gyre-server --lib spec_path_unread_count_and_mark_all_read`
- `cargo test -p gyre-server --lib disabled_preference_excludes_type_from_inbox_and_count`
- `cargo test -p gyre-server --lib update_user_role` (lockstep + cross-tenant/deactivated guards)
- `cd web && npm ci && npm test` (rebuild web/dist from current source)

— Reviewer, 2026-11-16 (candidate `6b562ecb`)
