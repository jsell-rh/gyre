# Review: task-127 — Implement user management UI pages

**Candidate:** `c1b76f2de72700174c8feab2deedeff634ca05fe` (base `9f446d0e`, branch
`pipeline/task-127/6aa0d0fc1dc445ca959e876896030d94-1`)
**Verdict:** not approved — one new-code defect (repairable in one line), one
minor pre-existing observation. Everything else checked out.

## What was verified

Every §UI Pages row ships as a real Svelte 5 (runes) component over real
endpoints — no hardcoded data, no dead controls, no stub-backed flows:

- **User Settings / Session Management** (`UserProfile.svelte`, `/profile`):
  display name / timezone / locale via `api.updateMe`; theme / density /
  diff-view / feed-scope / font-size preferences; per-type notification
  toggles + email/webhook/Slack channel config via
  `api.updateNotificationPreferences`; sessions tab lists / revokes /
  revoke-all via `api.listMySessions` / `revokeMySession` /
  `revokeAllMySessions`.
- **User Profile `@{username}`**: `PublicUserProfile.svelte` (task-114,
  carried through the merges).
- **Tenant User Management** (`TenantSettings.svelte` Users tab,
  server-response-driven TenantAdmin gating, forbidden renders a note not an
  error): list/search, invite, deactivate (`PUT /users/{id}/deactivate`),
  inline global-role select wired to the new `PUT /api/v1/users/{id}/role`.
- **Tenant Invitation Management** (Invitations tab): status filter
  (server-side `?status=`), single + bulk invite (per-entry errors
  surfaced), one-time `invite_url` with copy, revoke for Pending rows.
- **Workspace Members / Invitations / Teams** (`WorkspaceSettings.svelte`):
  members table, invite modal (username resolved to user id), role change
  incl. ownership transfer (reason prompt → `updateMemberRole`), pending
  invitations with expiry countdown + revoke, team CRUD + membership.
- **Invitation Accept Page** (`InvitationAccept.svelte`): both flows —
  authenticated `/invite/{workspaceId}` (GET /users/me/invitations →
  accept/decline) and magic-link `/invite/{token}` (64-hex vs UUID
  disambiguation in `parseUrl`, tested incl. uppercase-hex negative), backed
  by the new read-only unauthenticated `GET /api/v1/invite/{token}/preview`
  (read-only verified: expired-Pending rows report Expired WITHOUT
  persisting the transition — asserted in a server test), accept/decline via
  the token endpoints with `skipAuth` so a mismatched logged-in identity
  cannot burn someone else's invitation; minted first API key shown once
  with copy.
- **Notification Drawer / Bell**: unread badge from
  `GET /notifications/unread-count` + WS push + 60s resync; drawer lists
  active-only (`?active=true` → new `list_active_for_user` port method
  implemented in SQLite / PostgreSQL / mem with contract tests), priority
  grouping, deep links routed through the app's `navigateToPath`, no
  fabricated targets (no-action_url rows are disabled, not linked to a
  made-up page). The bell supersession of the decisions badge is recorded
  as a dated spec amendment in `ui-navigation.md` §1 (spec-lifecycle
  compliant, goal rule 4).

Backend additions reviewed and found sound: `PUT /users/{id}/role`
(TenantAdmin-only per-handler check via stored-record `is_tenant_admin`,
same-tenant containment, self-change and deactivated-target 409s,
GlobalRole↔UserRole set in lockstep, audit-logged; ABAC
`RouteResourceMapping` entry added — no exemption-file growth);
`GET /invite/{token}/preview` (token-as-auth-factor trust model, strictly
read-only, no credential material).

## Probes (evidence: /tmp/stage/review-evidence/)

- `cd web && npm ci && npm test` → **2494 passed / 0 failed** (124 files),
  matching the implementer's claim.
- Mutation probe: removing the `api.acceptTenantInvitationByToken` call in
  `InvitationAccept.svelte` fails 2 tests — the new tests are behavioral,
  not self-confirming. Source restored and verified clean.
- `cargo check -p gyre-server --lib --tests` → clean (8m57s). Server test
  execution deferred to the verifier per the 4 GiB build wrapper; required
  commands recorded in the evidence file (preview ×4, update_user_role ×2,
  active-filter, adapter `list_active_for_user`).
- Test-quality spot audit: the new suites assert rendered outcomes and exact
  API call payloads (`toHaveBeenCalledWith`), including negative assertions
  (cross-flow endpoints not called, revoke absent for non-Pending rows).
  Weakest spots (vacuous empty-state test, untested revoke/ownership-transfer
  interactions in WorkspaceSettings) are coverage gaps, not fakery — the
  wired behaviors were verified by direct source inspection.

## Findings

1. **`web/src/components/WorkspaceSettings.svelte:205` (new code, blocking
   as shipped):** sub-hour expiry countdown passes raw seconds with
   `unit: 'm'` — a Pending invitation expiring in 59 minutes renders
   "Expires in 3540m" (`en.json`: "Expires in {count}{unit}"). The days and
   hours branches are correct; only the minutes branch is missing
   `Math.floor(secs / 60)`.
2. **`web/src/components/UserProfile.svelte` ~646-648 (pre-existing at
   base, non-blocking):** edit-form Save/Cancel buttons lack
   `type="button"` inside the `onsubmit`-guarded form, so Save fires
   `saveEdit()` twice (duplicate idempotent `PUT /users/me`) and Cancel can
   trigger a spurious save. One-line fix; the adjacent prefs-save button
   already does it correctly.

Both are one-line repairs; no structural, scope-handling, or fakery
findings. The task contract is otherwise genuinely met — after the
countdown fix this candidate should be approvable without further rounds.
