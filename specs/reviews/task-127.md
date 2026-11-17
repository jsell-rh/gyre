# Review — task-127 (Implement user management UI pages)

Spec: `specs/system/user-management.md` §UI Pages (row 31).
Candidate under review: `7a2d33b1` (branch `pipeline/task-127/58fadfd37cbe43b09dee70c3c20096d3-1`, ready-for-review), base `e69fd990`.
This is the post-merge re-review after the base merge `b3632aa9` and the
review-repair round `72a4bd09` (expiry minutes division), on top of the
previously approved task-127 UI work.
Verdict: **approved** — no findings.

## Probes (evidence: /tmp/stage/review-evidence/probe-summary.md and logs)

- `cd web && npm ci` → exit 0.
- Focused vitest suites (InvitationAccept, InvitationDeclineNavigate,
  WorkspaceSettings, TenantSettings, UserProfile, NotificationDrawer,
  AppShell, api, Inbox) → 16/16 and 373/373 passed.
- Full `npx vitest run` with a deliberate mutation re-introducing the
  expiry-division defect in `InvitationAccept.svelte` → 2493 passed /
  2 failed / 41 skipped; the only failures were the two expiry assertions
  (`'expires in 1800m'` vs `'expires in 30m'`). Mutation reverted, file
  restored byte-identical, focused suite 15/15 green. The repair-round tests
  are real regression guards, and the rest of the candidate tree's suite
  passes.
- `/tmp/stage/bin/cargo check -p gyre-server --lib` → clean.
- `/tmp/stage/bin/cargo test -p gyre-adapters --lib sqlite::notification` →
  10/10 passed, including `list_active_for_user_excludes_resolved_and_dismissed`
  against real SQLite storage.
- `scripts/check-abac-route-registry.sh` and `scripts/check-arch.sh` → both OK.
- Coverage matrix recount matches the header (0 not-started, 3 task-assigned,
  22 implemented, 11 n/a); row 31 implemented with an accurate note.
- Commit attribution verified manually over the base..candidate range
  (script too slow in this sandbox; recorded as a verifier check): every
  non-merge product-surface task-127 commit is in the frontmatter list;
  `verify:`/`build(web):` commits touch no scannable product surface.

## Verified working

- **All §UI Pages rows** ship as real Svelte 5 (runes) components: User
  Settings + Session Management + Notification Preferences
  (`UserProfile.svelte`, `/profile`), User Profile `/@{username}`
  (`PublicUserProfile.svelte`, retained from task-114), Tenant User
  Management (Users tab, server-gated TenantAdmin listing, deactivate,
  provisioning, inline global-role select), Tenant Invitation Management
  (Invitations tab: status filter, single + bulk invite, revoke, one-time
  invite_url with copy), Workspace Members/Invitations (role select incl.
  Owner transfer with mandatory reason, invite by username, pending
  invitations with expiry countdown and revoke for Pending only), Team
  Management (create/edit/delete team, add/remove members), Notification
  Drawer (bell with unread badge + 60 s resync, active-only listing,
  priority grouping, deep links, mark read/unread, unread pill), and the
  `/invite/{token}` accept page. "My Dashboard" is correctly absent — the
  canonical spec supersedes it (§"My Stuff" Views note).
- **`PUT /api/v1/users/{id}/role`** (users.rs): per-handler TenantAdmin gate
  with RBAC-denied recording, tenant containment (403 cross-tenant), self
  change 409, deactivated target 409, unknown role 400, idempotent no-op,
  GlobalRole↔UserRole set in lockstep, audit-logged
  `user_global_role_changed` with previous/new role and reason. Route
  registered in api/mod.rs with the ABAC resolver entry mirroring the
  deactivation split (registry script passes). Guard tests drive the real
  router.
- **`GET /api/v1/invite/{token}/preview`** (invitations.rs): unauthenticated
  by design — the 256-bit CSPRNG token is the auth factor, same trust model
  as the pre-existing token accept/decline routes. Strictly read-only: an
  expired-but-Pending row reports Expired without persisting the transition
  (covered by `preview_reports_expired_pending_without_persisting`), no
  claim, no credential material; response carries only what the accept page
  renders. Unknown token 404s.
- **`list_active_for_user`** port method implemented consistently in all
  four implementers (SQLite, PostgreSQL, mem, BroadcastingNotificationRepo)
  with the empty-type-set-matches-nothing contract guarded in code in the
  mem adapter; the `?active=true` handler branch shares the type-filter
  derivation with the default listing. Real-storage test passes.
- **Routing**: `/invite/{token}` (64-hex) vs `/invite/{workspaceId}` (UUID)
  disambiguated by shape in parseUrl; urlFor round-trips the token form
  byte-for-byte; popstate/history restore both forms; the magic-link calls
  skip the Bearer header (api.js `skipAuth`, covered by api.test.js
  assertions that no Authorization header is sent) so a logged-in identity
  cannot burn someone else's invitation.
- **Drawer deep links** route through the app's `navigateToPath` context;
  items without an action_url and without an InvitationReceived workspace
  are disabled with an explanatory title — no fabricated targets.
- **Spec amendments are coherent**: ui-navigation.md's bell amendment
  (unread count + drawer, superseding the decisions-count badge click) is
  documented with rationale and reconciles the conflict with
  user-management.md §In-App Notification UI / §UI Pages; decisions remain
  one click away (workspace home Decisions section retains its count;
  drawer items deep-link to acting surfaces). docs/api-reference.md and
  docs/ui.md updated for both new endpoints and the bell.

## Test-inflation check

None found. The mutation probe proves the expiry tests kill the exact
defect class they target; the api.test.js token-flow tests assert header
absence (a stubbed or auth-leaking client fails them); TenantSettings tests
assert endpoint calls, payloads, i18n resolution and the not-admin state;
WorkspaceSettings tests cover role-change endpoint invocation, invite
resolution by user id, revoke-pending-only, and team rows; server tests
drive the real router with oneshot requests and assert status codes and
persisted state.

## Deferred to verifier

`cargo test --all` (incl. the new invitations preview ×4, update_user_role
×2, and active-filter server tests), web dist regeneration, and the
full-history `scripts/check-task-commit-attribution.sh` run — commands
recorded in /tmp/stage/review-evidence/probe-summary.md.
