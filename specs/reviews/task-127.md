# Review — task-127 (Implement user management UI pages)

Spec: `user-management.md` §UI Pages (with §In-App Notification UI informing the drawer criterion).
Commits under review (candidate `70b5a286`, base `9fb371ac`): `1a32b4e6` (pages implementation), `0f127bc0` (i18n repair), `4de47315`/`fe84485b` (asset builds — generated, verifier-owned), `b6966093` (Session Management page), merge `737c37ee` + coverage/process commits. Task frontmatter at the candidate lists all five product-surface commits (attribution OK).
Verdict: **needs-revision**.

## Round 1

Test runs (see /tmp/stage/review-evidence):

- `cd web && npm ci` → exit 0.
- `cd web && npm test` → **119 files passed, 2399 passed / 41 skipped, 0 failed** (full authored suite, locked deps).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean (type-checks the Rust change).
- Deferred to verifier (build budget): `cargo test -p gyre-server --lib api::users::tests::invitee_can_decline_own_pending_invitation`, `cargo test --all`.

Verified working (no findings):

- **User Settings** (`/profile`, `UserProfile.svelte`): profile edit + preferences fieldset (theme/density/diff view/feed scope/font size) through the real `PUT /users/me` partial-update contract; notification-preferences tab now parses the real `{preferences: [{notification_type, enabled}], channels}` shape (the previous mis-parse is fixed) and writes it back; delivery-channel config (email digest/min-priority, webhook, slack) on the same PUT; Sessions tab (below); Invitations tab (accept/decline of my pending memberships).
- **Session Management** (Sessions tab): list over `GET /users/me/sessions` (device from User-Agent with explicit WS-session label, IP, started/last-active/expiry from the epoch-second `UserSessionResponse`), per-session revoke (`DELETE .../sessions/{id}`, row removed on 204), sign-out-everywhere (`POST .../sessions/revoke-all`).
- **User Profile** (`/@username`, `PublicUserProfile.svelte`): route parsing, history/popstate handling, and rendering all real; accepted-memberships-only filter (pending invitations correctly hidden), team memberships derived from per-workspace team listings with honest best-effort degradation, honest 404 state (cross-tenant == nonexistent per the endpoint).
- **Tenant User Management** (TenantSettings Users tab): real `GET /users` TenantAdmin-gated listing (403 renders a gated note, no fake list), client-side search, Deactivate with reason → `PUT /users/{id}/deactivate`, Provision modal → `POST /users` with the once-only API-key reveal; role option lists match `UserRole`/`WorkspaceRole` variants exactly.
- **Workspace Members** (WorkspaceSettings Members tab): member list with role badges + inline role change (ownership-transfer reason prompt, cancelled prompt aborts), remove, invite-by-username resolved via `GET /users/{username}` then `POST /invite`, pending-invitations table with revoke.
- **Team Management** (Teams tab): real team CRUD + membership management over the task-122 endpoints (the tab previously showed the member list — now genuine teams).
- **Invitation Accept** (`/invite/{workspaceId}` + `InvitationAccept.svelte`): workspace preview, Accept → `POST /invite/accept`, Decline → invitee self-decline; honest terminal states. The segment is the workspace id because task-124 models pending invitations as membership rows; `/invite/{token}` belongs to task-110 — honestly documented, acceptable.
- **Notification Drawer** (`NotificationDrawer.svelte`): slide-out from the bell, priority-band grouping matching the server's exact `priority_level` strings, per-item read/unread via the task-125 endpoints, mark-all-read, deep links via `action_url` with the `InvitationReceived` → `/invite/{ws}` fallback, honest no-link disabled state.
- **Backend change** (`revoke_invitation` in `api/users.rs`): invitee self-decline is correctly scoped — manager check retained, self-decline only for the caller's own membership row, foreign invitations masked as 404 for non-managers (no probe surface), distinct `workspace_invitation_declined` audit event, real test added. No escalation introduced (an invitee can still only delete their own pending row).
- **Inbox bug fix**: dismiss now calls the actual dismiss endpoint instead of mark-read.
- All api.js wrappers added in the diff map to registered routes with matching response shapes (verified route-by-route in `api/mod.rs` / `users.rs`).
- Tests are behavioral, not self-confirming: they assert endpoint calls with arguments, rendered content per state, and 404/none/terminal states; `npm test` green with the full suite.

Findings:

- [open] **F1 (moderate): the bell's unread badge — an explicit acceptance criterion — is not implemented, and the repurposed click contradicts canonical specs left unamended.** `user-management.md` §In-App Notification UI requires a "Notification bell in top nav with **unread count badge**", and the task's acceptance criteria list "Notification Drawer with **unread badge**". The shipped bell (`web/src/App.svelte:1748-1761`) opens the drawer but renders `decisionsCount` (loaded at line 215 from the legacy `GET /users/me/notifications/count` — active unresolved+undismissed, a different metric than unread), so a user can see `🔔 3`, open the drawer, and find a different unread pill. The spec-path wrapper `api.unreadNotificationCount` added in this very diff (`web/src/lib/api.js:624`) is referenced by no component — the badge wiring is unfinished dead code. Additionally, the click repurposing contradicts `specs/system/ui-navigation.md` (Top Bar: "Clicking navigates to the workspace home with the Decisions section expanded"; repo mode: "Clicking opens the Decisions tab") and `docs/ui.md` line 23 — the diff edited `docs/ui.md` but left the stale badge bullet, so docs and code now disagree. Per the repo rules ("Specs are the contract… amend the spec via spec lifecycle rather than shipping code that contradicts it"), the genuine conflict between the two specs over the single bell must be resolved by amendment or by a separate notification bell with the unread count — not by silently dropping one spec's behavior. No test covers the bell click in either direction.
- [open] **F2 (minor): the drawer lists dismissed/resolved notifications.** `NotificationDrawer.svelte` renders every row of `GET /notifications` without filtering, and the backing `list_for_user` query (`crates/gyre-adapters/src/sqlite/notification.rs:150-207`) has no `dismissed_at`/`resolved_at` filter — a notification dismissed in the Inbox reappears in the drawer with no visual distinction. Filter client-side (or server-side) and/or render dismissed rows distinctly.

Both findings are small, localized repairs; everything else in the task is real, working, tested code consuming the task-124/125/111/122 endpoints as required.
