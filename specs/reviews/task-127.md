# Review — task-127 (Implement user management UI pages)

Spec: `specs/system/user-management.md` §UI Pages (row 31 of `specs/coverage/system/user-management.md`).
Candidate under review: `85301f66` (product commit `1a32b4e6` + process commits `2f6dc63e`/`85301f66`), base `5984177c` (task-124 tip).
Verdict: **approved** — every acceptance criterion is backed by real production code consuming the real task-124/125 endpoints, with meaningful tests; all frontend evidence reproduced independently in this review. Server test execution is deferred to the independent verifier (build-wrapper 4 GiB limit — infrastructure, not a code defect); exact commands recorded below.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cd web && npm ci && npm test` → **116 files / 2339 passed, 41 skipped** (run twice; identical).
- `cargo check -p gyre-server` → **pass** (8m 23s, cold).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → **pass** (type-checks the new `#[cfg(test)]` backend test; helpers `seed_workspace_with_owner`/`seed_keyed_user` exist in-module).
- `bash scripts/check-arch.sh`, `check-abac-route-registry.sh`, `check-inert-enforcement.sh` → **all pass** (no new routes; mod.rs untouched).
- **Mutation probe:** with `NotificationDrawer.svelte` grouping mutated to force every notification into the "Low" band, `npx vitest run src/__tests__/NotificationDrawer.test.js` fails exactly the "groups notifications by priority band" test (1 failed | 7 passed). The grouping test is real, not self-confirming. File restored; tree clean.
- **Contract verification by source inspection** — every new frontend API call was checked against the actual handler at the candidate: `GET /notifications` shape (`{notifications, limit, offset}` with `priority_level`/`read`/`action_url`/`notification_type`/`workspace_id`), `POST /notifications/{id}/read|unread`, `mark-all-read`, `GET/PUT /notifications/preferences` (`{preferences: [{notification_type, enabled}], channels}`), `PUT /users/me` preferences patch semantics, `GET /users/{username}` tenant-containment 404, `GET /workspaces/{id}` tenant-only scope (invitee can preview), `DELETE /workspaces/{id}/invitations/{id}` self-decline authorization, teams CRUD/membership routes, `GET /users` + deactivate + provision. All match.

## Verified working (no findings)

- **User Settings** (`/profile`): preferences fieldset (theme/density/diff view/feed scope/font size) persisted via `PUT /users/me` with real partial-patch semantics server-side; Notification Preferences tab fixed to the real `{preferences, channels}` contract (the previous flat type→bool parse never round-tripped — a genuine bug fixed here, with the test updated to assert the real payload) plus delivery-channel config (email digest/min-priority, webhook, Slack; in-app locked) matching `validate_channel_config`; new Invitations tab with accept/decline.
- **User Profile `/@username`**: new route parsing (`/@handle`), `PublicUserProfile.svelte` with profile fields, accepted-only workspace memberships with role badges, team memberships derived from per-workspace team listings, honest 404 state for cross-tenant/nonexistent handles. The spec's "activity feed" has no backing endpoint (no per-user activity API exists) — honestly left out and recorded in the coverage note, not faked.
- **Tenant User Management** (TenantSettings Users tab): real `GET /users` listing (TenantAdmin-gated; 403 renders an honest "requires TenantAdmin" note, tested), client-side search, deactivate with reason via `PUT /users/{id}/deactivate` (self-deactivation hidden), `POST /users` provisioning with once-shown API key. Inline global-role editing has no backend endpoint — recorded in the coverage note.
- **Workspace Members** (new Members tab in WorkspaceSettings): member list with role badges/status, role-change dropdown via `PUT .../members/{user_id}/role` incl. ownership-transfer reason prompt (cancelled prompt aborts), remove member, invite-by-username modal (resolves handle → user id via tenant-public profile, then `POST /invite`), pending invitations table with revoke.
- **Team Management** (Teams tab): reworked from a member-list display to real team CRUD + membership management over the task-122 endpoints (create/edit/delete, add/remove member by username, lead badge).
- **Invitation Accept page** at `/invite/{workspaceId}`: workspace preview, invited role/date, accept (`POST /workspaces/{id}/invite/accept`) and decline (invitee self-decline). The `{token}` form of the spec row belongs to the tenant-invitation magic-link flow (`POST /api/v1/invite/{token}/accept`), which the spec's own completeness table marks "not yet implemented" (task-110); task-124 models workspace invitations as membership rows with no token. The adaptation is documented in-code and in the coverage note — the honest choice over fabricating a token flow.
- **Notification Drawer**: bell opens a slide-out dialog; priority-band grouping (Urgent/High/Medium/Low), unread pill, mark read/unread and mark-all-read via the task-125 endpoints, deep links via `action_url` (and `InvitationReceived` → `/invite/{ws}`), Escape/backdrop close. The bell badge keeps the global active-decisions count per `ui-navigation.md` principle 4 ("decisions are always visible") — the two specs tension is documented in-code; the drawer itself carries the unread badge the task requires.
- **Inbox dismissal bug fix**: `handleDismiss` previously called `markNotificationRead` (which used to hit the dismiss endpoint by old-api misnomer); api.js now names `read`/`unread`/`dismiss` correctly and Inbox uses `dismissNotification`.
- **Backend self-decline change** (`revoke_invitation`): managers keep revocation; the invitee may decline their own pending invitation; third parties get 404 for pending invitations (masked, same as nonexistent); distinct `workspace_invitation_declined` audit event. The new test drives the real router: third-party DELETE → 404, invitee DELETE → 204, membership row verified gone, subsequent accept → 404. Authorization ordering, tenant scoping (`load_workspace_scoped`), and the workspace-match check are all correct.
- **Svelte 5 runes** throughout (`$state`, `$derived`, `$props`, `$bindable`, `$effect`, `untrack` guards against reload loops).
- **Honest coverage reporting**: coverage row 31 stays `task-assigned` (not falsely `implemented`) with a precise enumeration of the rows that have no backing API (Session Management → task-111, tenant invitations/magic links/bulk → task-110, expiry countdown → no expiry field on WorkspaceMembership, activity feed/stats, inline global-role editing).

## Tests deferred to the verifier (build wrapper rejected gyre-server test builds; commands recorded in /tmp/stage/review-evidence/cargo-test-wrapper-rejection.txt)

- `cargo test -p gyre-server --lib api::users::tests::invitee_can_decline_own_pending_invitation`
- `cargo test -p gyre-server --lib api::users`
- `cargo test --all` (task acceptance criterion)

## Generated-output observation (verifier-owned, non-blocking)

The committed `web/dist/` predates this candidate's web source (a build.rs-triggered rebuild during review produced different hashed asset names). Per stage rules the deterministic verifier rebuilds, tests, and commits the bundle — not a source defect, recorded separately.

## Minor (non-blocking, recorded for completeness)

- `revoke_invitation` (users.rs): the `accepted → 409 Conflict` check runs before the manager/self-decline authorization check, so a same-tenant non-manager probing an *accepted* membership id gets 409 vs 404 for pending/nonexistent. The in-code comment claims full indistinguishability, which strictly holds only for pending invitations. Zero information gain in practice (accepted-ness and ids are already returned by the tenant-public `GET /users/{username}` memberships, and ids are unguessable), but hoisting the authorization check above the accepted-state check (or amending the comment) would make the code match its claim.
- Pre-existing (task-124, outside this diff): `GET /api/v1/users/{username}` returns pending (unaccepted) memberships of other users to any same-tenant caller; this candidate's UI filters them client-side for the public profile. Worth a follow-up server-side filter when task-110 revisits invitations.

Test-inflation check: none found. The drawer grouping test was mutation-killed in this review; the backend decline test asserts router-level status codes and post-state; the UserProfile prefs test asserts the exact real request payload shape; the TenantSettings not-admin test asserts the gated rendering; the invite tests assert exact endpoint calls and rendered outcome states.
