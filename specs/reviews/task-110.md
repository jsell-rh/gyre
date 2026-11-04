# Review: task-110 — Implement tenant & workspace invitation flow

**Candidate:** `61814a2bfbc45fa3995d7bed6a6a2206a6946b18`
(base `63aa1486210dc579f2fcd4e8b5c6b91e6f66fd77`)
**Verdict:** NOT approved — 2 substantive findings, 2 minor.
**Evidence:** `/tmp/stage/review-evidence/task-110-review-round.md`
(+ test/check logs alongside it).

## What was verified as real and correct

The implementation is substantial and overwhelmingly genuine — every layer
was read, not just the shipped summary:

- **Domain/ports/adapters**: `TenantInvitation`, `WorkspaceInvitation`,
  `InvitationStatus`, `InvitationPolicy` match the user-management.md
  structs field-for-field. Ports implemented by SQLite (real diesel
  queries, temp-file DB tests through migration `000056`), PostgreSQL, and
  mem — the duplicate-pending port contract is enforced in code by all
  three (the migration's non-unique indexes alone would not enforce it).
  Migration numbering correct (`000056`/`000057` after `000055`), SQL
  portable across SQLite/PG.
- **API**: all 5 spec'd tenant endpoints functional plus decline, policy
  GET/PUT, and workspace invite/list/revoke. Tokens are 32-byte CSPRNG
  (ring), persisted only as SHA-256 (known-answer test included);
  acceptance single-use; expired-pending transitions persisted on touch;
  revoke keeps the row. Per-handler authorization is real
  (`require_tenant_admin`, workspace Owner/Admin-or-TenantAdmin, tenant
  containment on every lookup, cross-tenant pre-assigned workspaces
  skipped, invitee-without-tenant rejected rather than defaulted).
  Magic-link routes correctly live outside `require_auth_middleware`
  (token is the auth factor) with bearer-identity matching when an
  identity IS presented.
- **SSO email-link (auth.rs)**: invitation-created local accounts are
  claimed by a matching verified SSO email within the same tenant;
  unverified emails and cross-tenant logins are refused — each case has a
  killing test.
- **Repair F1**: `users.tenant_id`/`global_role` columns added with
  adapter round-trip tests — a real DB-mode bug (invitees 403ing on
  tenant containment) fixed, not papered over.
- **Background job**: `run_expiry_once` marks Pending-past-`expires_at`
  rows Expired (never deletes); spawned hourly with env override, first
  cycle at startup.
- **Probes** (candidate tree, exit codes in evidence): server invitation
  tests 14/0, adapters 2/0, domain 4/0. All 88 mechanical checks run at
  candidate AND base: identical failure sets (35, all pre-existing or
  sandbox awk-version errors) — **zero new violations**; exemption files
  only line-re-anchored, never grown. All 10 task commits are ancestors.

## Findings (why not approved)

1. **Invitee-side workspace acceptance is unreachable** (code,
   `api/invitations.rs`). The in-app notification carries no accept link;
   `WorkspaceInvitationRepository::list_by_user` — built precisely for
   "the invitee's pending invitations view" — is wired to no route (dead
   at the API layer); the workspace list endpoint requires membership the
   invitee does not yet have. Only the inviter's create-response
   `invite_url` can reach the accept endpoint. Spec §Workspace Invitation
   Flow steps 3→4 (notification → accept/decline) therefore do not close
   end-to-end for the invitee.
2. **Spec endpoint shape not honored nor amended** (spec,
   `user-management.md` §API). The spec defines
   `POST /api/v1/workspaces/{id}/invite/accept` (authenticated accept);
   the implementation replaced it with a token route and accepts invites
   by `user_id` only, where the spec says "by username or email". Goal
   rule 4: specs are the contract — implement the specced route or amend
   the spec via lifecycle. The specced authenticated accept route is
   exactly what would close finding 1's gap.
3. *Minor (code)*: check-then-act race on single-use acceptance —
   `update_status` has no `WHERE status = 'Pending'` precondition and no
   transaction; concurrent accepts of one token can double-create the
   user/membership. Low severity (no escalation; token holder racing
   itself).
4. *Minor (test)*: `non_admin_cannot_invite` calls the guard directly,
   not through HTTP; the 403 paths for non-admin tenant invites and
   non-member workspace invites are untested end-to-end.

Findings 1+2 share one root cause and one repair: give the invitee an
authenticated surface (wire `list_by_user` into e.g.
`GET /api/v1/users/me/invitations`, implement the spec's accept route or
amend the spec, include the accept URL in the notification), and the
lifecycle is genuinely closed.

## Transport note

No TCP listener is bindable in this sandbox (capabilities.json), so no
live-server smoke was run; findings are established by source inspection
and route-table greps, which need no listener. Host verification should
add the invitee-reachability smoke described in the evidence log.
