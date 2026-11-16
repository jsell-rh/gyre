# Review: task-127 — Implement user management UI pages

**Candidate:** `22945fe45bebc7a4073641060d7566def03f0c9c` (base `c66ee2ac9d0fff7e45cffc6dbeabdf90a3f3a70e`)
**Round:** 2026-11-15 rebase review (candidate merged onto main head c66ee2ac, task-123 notification entity)
**Verdict:** NOT APPROVED — shipped work is real and verified, but the task contract is incomplete (see Findings).

## What was verified (independently, this round)

- **Frontend suite:** `cd web && npm ci && npm test` — 124 files, **2470 passed / 41 skipped, exit 0**
  (evidence: `/tmp/stage/review-evidence/web-vitest.log`).
- **Mutation probes (tests are not self-confirming):**
  - Removing `active: true` from `NotificationDrawer.svelte` load params →
    `NotificationDrawer.test.js` "loads the active-only listing" FAILS.
  - Removing the `status === 'Pending'` guard from `InvitationAccept.svelte` →
    `InvitationAccept.test.js` "renders the none state for an expired invitation" FAILS.
  Both mutations restored; `git status` clean afterwards (evidence:
  `/tmp/stage/review-evidence/mutation-probes.md`).
- **Backend merge changes:** `cargo test -p gyre-adapters notification` — 14 passed
  (incl. `list_active_for_user_excludes_resolved_and_dismissed`,
  `list_for_user_type_set_filter_matches_persisted_column`);
  `cargo test -p gyre-common notification` — 12 passed;
  `cargo check -p gyre-server` — clean.
  `scripts/check-arch.sh` and `scripts/check-mem-port-contracts.sh` pass.
- **Page audit (source read, not progress labels):** User Settings + Session Management
  (`UserProfile.svelte`: PUT /users/me, GET/DELETE /users/me/sessions, revoke-all),
  public profile `/@username`, Tenant User Management (list/search/deactivate/provision,
  TenantAdmin-gated server-side), Workspace Members (role change with ownership-transfer
  reason, remove, invite-by-username, pending invitations with status badge + expiry +
  revoke), Team Management (full CRUD + membership over task-122 endpoints),
  Notification Drawer (bell unread badge via /notifications/unread-count, priority
  grouping, mark read/unread, mark-all-read, deep links, no fabricated targets),
  Invitation Accept at `/invite/{workspaceId}` (workspace preview, accept/decline,
  expiry, 404/load-error/none states). All Svelte 5 runes; i18n keys complete
  (`invite_accept.*` verified against en.json).
- **Honest bookkeeping confirmed:** coverage row 31 (§UI Pages) stays `task-assigned`
  with the open items listed — no fake completion.

## Findings (blocking)

1. **Tenant Invitation Management page missing** (§UI Pages row). Task-110's endpoints
   are in-tree and registered (`api/mod.rs:1047-1048`, `api/invitations.rs`) but no
   `web/src` surface consumes them; no other task owns the row. Implementable
   frontend work inside task-127's coverage section.
2. **Acceptance criterion "Invitation Accept page at `/invite/{token}`" unmet as
   specified.** Shipped page is `/invite/{workspaceId}` (authenticated
   workspace-invitation flow). The token magic-link endpoints
   (`lib.rs:930-943`) have no UI, and the spec row was not amended (contrast: the
   bell re-pointing got a dated `ui-navigation.md` amendment). The workspace-preview
   sub-requirement is backend-blocked (no unauthenticated token-preview endpoint —
   verified), but token accept/decline + first-credential display is implementable;
   otherwise amend the spec row via lifecycle.
3. **Inline global-role editing open** (Tenant User Management "role management"):
   no endpoint exists; backend-blocked. Needs follow-up decomposition or recorded
   deferral before the section can close.

## Verifier must run (sandbox 4 GiB budget deferred)

- `cargo test --all`
- Specifically: `cargo test -p gyre-server --lib api::users::tests` (new
  `spec_path_notifications_active_filter_excludes_resolved_and_dismissed` exercises
  the merged `?active=true` branch through the mem adapter and the broadcasting
  wrapper) and `cargo test -p gyre-server --lib api::invitations`.
- Full frontend suite re-run on the rebuilt `web/dist` (regeneration is verifier-owned).
