# Review — task-127 (Implement user management UI pages)

Spec: `user-management.md` §UI Pages (§In-App Notification UI for the drawer).
Candidate `94a6c5e3` against base `8d07a41a` (branch
`pipeline/task-127/cfc5e877335f43c9b5edec8236c9afe3-1`; this round is the
post-merge-repair re-review — the branch merged its base via `67eb6ac1`,
sole conflict the coverage-matrix rows 30/31, resolved keeping both sides'
notes; no conflict markers remain anywhere in the base→candidate diff).
Product commit `1a32b4e6` (+ verified asset build `4de47315`), both recorded
in the task's `commits:` frontmatter at the candidate.
Verdict: **pass** (round 1 of this checkpoint; a prior approval existed on
the pre-merge tree and nothing in the merge changed product bytes beyond
the documented coverage-row resolution).

## What ran (evidence: /tmp/stage/review-evidence/)

- `cd web && npm ci && npm test` → **2382 passed, 41 skipped, 0 failed**
  (118 files) — acceptance criterion met; matches the task's claimed numbers.
- `/tmp/stage/bin/cargo check -p gyre-server` → clean (6m57s). Server *test*
  compilation is deferred to the independent verifier (4 GiB budget);
  required commands recorded in the evidence README.
- Mutation probe: disabling `NotificationDrawer.targetFor` (deep links) in
  the production source fails 2 of 8 drawer tests → the new tests are not
  self-confirming. Source restored byte-identical (git status clean).
- Contract greps: every frontend call site in `web/src/lib/api.js` matched
  against the registered routes and handler payload/response shapes
  (evidence: route-verification.txt).

## Verified real (acceptance criteria)

- **User Settings** (`/profile`, UserProfile.svelte): profile edit via
  `PUT /users/me`; preferences fieldset (theme/ui_density/diff_view/
  activity_feed_scope/code_font_size — all real `UserPreferencesPatch`
  fields); notif-prefs tab fixed to the actual `{preferences, channels}`
  contract (server shape verified field-for-field, incl. webhook `secret`
  and slack `channel: Option`) plus email/webhook/Slack channel config; new
  Invitations tab (accept/decline own pending memberships).
- **User Profile `/@username`** (PublicUserProfile.svelte + App.svelte
  routing, history/popstate/title handling): `GET /users/{username}`
  (tenant-contained 404), accepted memberships with role badges, team
  memberships via per-workspace team lists, honest not-found state.
- **Tenant User Management** (TenantSettings Users tab): real TenantAdmin
  listing (`GET /users` → `{users,total}`), client-side search, deactivate
  with reason (`PUT /users/{id}/deactivate`), provisioning modal
  (`POST /users` `{username,tenant_id,roles}` → one-time API key banner),
  self-deactivation hidden, 403 renders an honest gated note (no fake list).
- **Workspace Members** (WorkspaceSettings Members tab): member list with
  role badges + inline role change (`PUT .../members/{id}/role`, ownership
  transfers prompt for an audit reason and abort on cancelled prompt),
  remove, invite-by-@handle (resolved via `GET /users/{username}` then
  `POST /invite`), pending invitations table with revoke.
- **Invitation Accept page** (`/invite/{workspaceId}` +
  InvitationAccept.svelte): workspace preview, accept
  (`POST /invite/accept`), decline via the new invitee self-decline; honest
  none/not-found/terminal states.
- **Notification Drawer** (NotificationDrawer.svelte): bell opens a
  slide-out grouped Urgent/High/Medium/Low, per-item read/unread
  (`POST /notifications/{id}/read|/unread` — both routes verified
  registered), mark-all-read, unread pill, deep links via server-resolved
  `action_url` (derived server-side when absent, notifications.rs:113) and
  `InvitationReceived` → `/invite/{workspace_id}`.
- **Team Management** (WorkspaceSettings Teams tab): real team CRUD +
  membership management over the task-122 endpoints (the tab previously
  showed the member list).
- **Backend addition** (users.rs `revoke_invitation`): invitee self-decline
  per §Workspace Invitation Flow step 4 — scoped to the caller's own
  membership row, foreign invitations 404-masked (test asserts the
  third-party 404), distinct `workspace_invitation_declined` audit event,
  no new route (no ABAC registry change). `load_workspace_scoped` gates
  tenant only, so a pending invitee legitimately reaches the handler.
- Svelte 5 runes throughout (`$state`, `$derived`, `$props`, `$bindable`,
  `$effect`, `untrack`). Tests assert real API call shapes (mocked
  transport only) — spot-checked WorkspaceSettings/TenantSettings/
  NotificationDrawer/InvitationAccept diffs.
- Coverage row 31 honestly stays **task-assigned** with the blocked gaps
  enumerated (session page → task-111, tenant invitations/magic links →
  task-110, expiry countdown, activity feed, inline global-role editing) —
  no fake completion.

## Non-findings (checked, judged acceptable — recorded for follow-up)

- **Bell repurposing vs ui-navigation.md.** The top-bar bell now opens the
  NotificationDrawer (required verbatim by the assigned spec:
  §In-App Notification UI "Notification drawer slides out" / §UI Pages
  "Slide-out panel from bell icon"), which literally contradicts
  ui-navigation.md §Top Bar ("Clicking navigates to the workspace home
  with the Decisions section expanded") and §Repo Mode ("Clicking opens
  the Decisions tab") — an inherent conflict in the spec corpus that any
  implementation of this task must resolve in favor of the assigned,
  newer spec. Judged non-blocking because principle 4's substance holds:
  decisions remain visible on the workspace home and reachable via the
  drawer section nav, `g 3`, and the repo Decisions tab, and the drawer's
  notifications (the decisions data source per ui-navigation.md itself)
  deep-link to the underlying entities. **Follow-up:** amend
  ui-navigation.md §Top Bar/§Repo Mode via spec lifecycle to record the
  supersession, mirroring the My-Dashboard note pattern in
  user-management.md.
- **`revoke_invitation` ordering:** the `accepted` → 409 check precedes the
  manager/self-decline authorization check, so a same-tenant caller holding
  a known membership id can distinguish accepted (409) from pending/
  nonexistent (404) — a narrow pre-auth state oracle the candidate
  introduced (previously non-managers were rejected before the lookup).
  Low severity (ids are unguessable; only the accepted bit leaks).
  **Follow-up:** hoist the `!is_manager && !self_decline` check above the
  `accepted` check.
- **`/invite/{workspaceId}` vs the spec's `/invite/{token}`:** documented,
  dependency-constrained (task-124 models pending invitations as membership
  rows with no token; the token form is the task-110 tenant flow). No
  security impact: the page resolves only the caller's own pending
  membership, and both accept and decline are scoped server-side to it.
- AppShell.test.js parseUrl/urlFor cases are mirrored-logic (pre-existing
  file pattern); the component tests carry the real behavioral coverage,
  and the mutation probe confirms they bind to production behavior.
- Web bundle (`web/dist/`) observations are out of review scope (verifier-
  owned); authored source and tests reviewed here.

## Verifier must run

`cargo test --all`; `cargo test -p gyre-server
invitee_can_decline_own_pending_invitation`; `cargo test -p gyre-server
workspace_invitation_lifecycle`; `bash
scripts/check-task-commit-attribution.sh` (full-history walk; timed out
at 120s in-sandbox); re-run `cd web && npm ci && npm test` on the
integrated tree.

— Reviewer, 2026-11-14 (candidate 94a6c5e3)
