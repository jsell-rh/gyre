# Review: TASK-110 — Implement tenant & workspace invitation flow

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-11-09 (round 13 — rebase onto base `33cd786c2`)
**Verdict:** needs-revision

**Candidate under review:** `d05c2cee9fff18876bd684f59a5d9269c0e558ff` (merge commit `fcd66acf3`, parents `c5c927604` + `33cd786c2`)
**Spec:** `user-management.md` §Tenant-Level User Onboarding, §Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations

**Verification performed:** read the full base→candidate diff (32 files, +7342/−32) and all new sources end to end (domain entities, ports, SQLite/Postgres/mem adapters, migrations, api/invitations.rs, auth.rs SSO link, lib.rs/main.rs wiring, ABAC mappings); ran `cargo check -p gyre-server` (exit 0, 8m58s — server test builds are blocked by the sandbox's 4 GiB build-budget wrapper and belong to the verifier); ran `cargo test -p gyre-adapters invitation` (5 passed / 0 failed: roundtrips, pending-scope unique index enforced by the database, migration 000061 raced-duplicate dedupe, transition_status source-status guard); ran all 17 relevant mechanical guard scripts (migration versions, ABAC route registry, ABAC exempt handlers, arch, mem port contracts, dead message kinds, scope literal defaults, fabricated scope defaults, inert enforcement, lossy secret conversion, forwarded header trust, in-memory state stores, unbounded external HTTP, migration SQL portability, forged scope fields, relative path defaults, byte slice truncation) — all PASS.

---

## Assessment

The invitation flow itself is real and well-built. Verified strengths:

- **Entities/ports/adapters:** `TenantInvitation`/`WorkspaceInvitation`/`InvitationPolicy` in gyre-domain with every spec field; port contracts (`TenantInvitationRepository`, `WorkspaceInvitationRepository`) document the pending-scope uniqueness and the conditional single-use `transition_status`; all three adapters enforce both — SQLite/Postgres via a conditional `UPDATE ... WHERE status = from` and partial unique indexes (migration `2026-11-08-000061`), mem via check-and-set under the store lock. Migration numbering is collision-free against the base (`check-migration-versions.sh` green; the 000058/000060/000061 renumber history is documented in-file).
- **Endpoints:** all five specced tenant endpoints plus bulk (partial success with per-entry errors), status-filtered list, revoke (conditional Pending→Revoked), magic-link accept/decline, the workspace invitation lifecycle, the invitee-facing surface (`GET /users/me/invitations`, authenticated `POST /workspaces/{id}/invite/{accept,decline}` keyed on caller identity), and the per-tenant policy endpoints. Magic-link routes are correctly registered outside the auth middleware chain (the 256-bit CSPRNG token is the auth factor; the optional-Bearer path enforces invitee-email/user match with 403).
- **Auth/containment:** per-handler TenantAdmin guard (real HTTP-level 403 tests with a Member API key), workspace Owner/Admin or TenantAdmin, tenant containment on every scoped handler (two-tenant HTTP fixtures), cross-tenant pre-assigned workspaces skipped at accept. ABAC mappings added for every new authenticated route; registry and exempt-handler guards green.
- **Tokens:** 32 bytes from ring `SystemRandom`, hex-encoded, stored only as SHA-256 (known-answer test against the `abc` vector). Single-use claim before side effects, with rollback to Pending on failure.
- **SSO link:** `find_or_create_user_tenant` claims an invitation-created local account on first SSO login only when `email_verified=true` and the tenant matches, via the sanctioned `claim_external_id` port transition; negative tests prove unverified emails and cross-tenant logins do not claim.
- **Expiry:** background job spawned in main.rs, interval env-overridable with a zero-guard (avoids the tokio interval panic), conditional transitions so a racing accept is never overwritten, expired rows kept for audit; accept/decline also mark expiry inline (409).
- **Rebase quality:** both merge conflicts (`mem.rs`, `warn-continue-creation-exemptions.txt`) resolved keeping both sides' surfaces; the migration collision with the base's `2026-11-08-000059_notification_entity_source` was correctly repaired by renumbering to 000061.

## Findings

- [ ] **F1 (code): the workspace-invitation notification to the invitee is typed `NotificationType::TrustSuggestion` instead of `NotificationType::InvitationReceived`** — `crates/gyre-server/src/api/invitations.rs:1255-1263` (`invite_to_workspace`). The spec's own Notification Entity taxonomy (user-management.md §Notification Entity, Workspace section) defines `InvitationReceived` as exactly this event ("A user was invited to a workspace"), and the base merged in this very rebase established that typing: task-123's coverage row records "`InvitationReceived` (invite_member, **replaces TrustSuggestion mis-typing**)" — the sibling `invite_member` handler (api/users.rs:488) was repaired to use `InvitationReceived`, and this candidate reintroduces the identical mis-typing in the new handler. User-visible consequence: the invitee's Inbox (`web/src/components/Inbox.svelte`) maps `InvitationReceived` → snake-case `invitation_received` with 'info' severity and the `decisions.type_labels.InvitationReceived` i18n label; a `TrustSuggestion`-typed row falls outside that mapping and renders as a trust-policy suggestion (en.json groups it under "Trust Suggestions"), so every workspace invitation surfaces to the invitee as a trust suggestion instead of an invitation. The priority band happens to be the same (rank 8 vs 10, both Low), which is why nothing functionally breaks — but the typed taxonomy is the contract, and the body's `"kind": "workspace_invitation"` does not repair the mis-typed enum. The task's own test `invite_notification_carries_accept_path` asserts only the body kind and the accept URL, never `notification_type` — the mis-typing is untested. Fix: use `NotificationType::InvitationReceived` (one line) and extend the test to assert the type (and that it is not `TrustSuggestion`). Reproduce: POST /api/v1/workspaces/{id}/invite as Owner, then GET the invitee's notifications — the row's `notification_type` is `TrustSuggestion`.

  Non-blocking observation (same handler): the notification is constructed via `Notification::new` + `notifications.create` directly rather than the codebase's `notify_rich` helper, so it forgoes the shared deep-link `action_url` resolution used by the sibling `invite_member` path; the body's `accept_url` covers the action, so this is cosmetic, but converging on `notify_rich` when fixing F1 would also pick up the deep link.

## Verifier-required probes (blocked in this sandbox by the 4 GiB build-budget wrapper)

The server test suite could not be compiled here (wrapper rejection recorded in `/tmp/stage/review-evidence/test-invitations.log`). The independent verifier must run:

1. `cargo test -p gyre-server --lib api::invitations` (26 tests claimed at this head)
2. `cargo test -p gyre-server --lib auth` (JWT email-link tests: `jwt_email_link_claims_invitation_account`, `jwt_unverified_email_does_not_claim_invitation_account`, `jwt_email_link_does_not_cross_tenants`)
3. `cargo test -p gyre-server --lib notification` (base notification surfaces against the merged mem.rs)
4. `cargo test -p gyre-adapters` (full suite)
5. `cargo test --all` (task acceptance criterion)

## Verdict

needs-revision — one blocking finding (F1). Everything else in the task contract is implemented for real and independently evidenced. F1 is a one-line production fix plus a test assertion; once it lands and the server suites above pass in the verifier, this candidate is approvable.
