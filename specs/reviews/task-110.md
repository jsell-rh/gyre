# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding, §Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Assignment: base `103d665e`, candidate `f0c7b94d` (branch `pipeline/task-110/9f4b3f712c8745d3aaccd1c908f67627-1`).
Verdict: **approved** — no findings. All three prior-round findings (review `4cf9e027`) are fixed with regression tests; the full diff holds up under independent inspection and focused probes.

## Round for assignment 9f4b3f71 (2026-11-14)

### Prior findings (4cf9e027) — verified fixed

1. **auth.rs SSO email-link claim branch let deactivated accounts re-authenticate** → fixed at `auth.rs:2142`: the claim branch bails when `invited.is_deactivated()` before `claim_external_id`, mirroring the sibling auth paths. Regression: `jwt_email_link_does_not_claim_deactivated_account` (auth.rs:4379) asserts 401, no claim, and no account minted under the SSO subject.
2. **Migration 000063 declared INTEGER timestamps (int4) that break every PG read** → fixed: `up.sql` declares BIGINT for all six timestamp columns, and the PG-only init hook `postgres::repair::ensure_invitation_timestamps_bigint` (wired at `postgres/mod.rs:101`) ALTERs already-migrated int4 columns in place, idempotently.
3. **Invitation acceptance skipped the deactivation contract** → fixed at `invitations.rs:830` (local-mode retry arm), `:843` (SSO-link arm), `:1556` (workspace token-accept arm). Regression trio: `deactivated_local_account_cannot_accept_invitation`, `deactivated_sso_account_cannot_accept_invitation`, `deactivated_invitee_cannot_accept_workspace_invitation_by_token` — each asserts 403, invitation rolled back to Pending, and no role/membership mutation.

### Probes run (evidence: `/tmp/stage/review-evidence/`)

- Mechanical invariant gates, all exit 0: check-arch, abac-route-registry, migration-versions, migration-sql-portability, mem-port-contracts, scope-literal-defaults, fabricated-scope-defaults, inert-enforcement, lossy-secret-conversion, in-memory-state-stores, forged-scope-fields, abac-exempt-handlers.
- `cargo test -p gyre-domain` — pass, including the four invitation entity tests (expiry only when Pending; policy defaults).
- `cargo test -p gyre-adapters invitation` — 5/5 pass against real SQLite: roundtrips, conditional `transition_status` rejects wrong source status, the pending-scope partial unique index enforced at the database level via a raw-SQL bypass probe (fails if the index is missing), and migration 000064's raced-duplicate dedupe before index creation.
- `cargo check -p gyre-server` — clean.

Deferred to the independent verifier (the 4 GiB build wrapper blocks server test compilation; recorded in evidence): `cargo test --all`, plus focused `cargo test -p gyre-server invitations` (31 tests: production-chain ABAC exemption test with a denied-control, deactivation trio, concurrent-claim race, cross-tenant guards, cap enforcement), `cargo test -p gyre-server jwt_email_link`, and the users deactivate-cascade suite.

### Verified clean (no findings)

- **Domain**: `TenantInvitation`/`WorkspaceInvitation`/`InvitationStatus`/`InvitationPolicy` match the spec structs field-for-field; `InvitationPolicy` carries the specced configurable knobs (7/7/50 defaults).
- **Ports/adapters**: `transition_status` is a conditional write in every adapter (UPDATE … WHERE status = from in SQLite/PG; check-and-set under the store lock in mem), so the single-use acceptance claim is race-safe — proven by `concurrent_claims_produce_exactly_one_winner` and the adapter-level wrong-source-status test. The "one pending per (tenant,email)/(workspace,user)" port contract is enforced in code and backstopped by partial unique indexes (migration 000064) with raced-duplicate dedupe; terminal statuses never conflict, so re-invite after revoke/expiry works with a fresh token. Migrations use next-unused sequence numbers and portable SQL only.
- **API surface**: all five specced tenant endpoints plus bulk (per-entry errors without aborting the batch), the workspace invitation lifecycle (invite → pending → accept/decline/expire/revoke), the invitee-facing authenticated accept/decline, `GET /users/me/invitations`, and per-tenant policy GET/PUT (fails closed on a corrupt stored row). Tokens are 32 CSPRNG bytes (ring `SystemRandom`), stored only as SHA-256 (known-answer test). Acceptance claims the invitation before side effects and rolls back to Pending on failure; pre-assigned memberships activate cross-tenant-guarded and idempotently.
- **Credential delivery**: local-mode acceptance mints the account's first API key (hash persisted in the auth-resolving store) only for invitation-created local accounts; pre-existing SSO/bootstrap accounts get `api_key: null` so token possession cannot bypass their existing auth factor.
- **SSO link**: the auth.rs claim branch requires `email_verified=true`, a tenant-scoped match on the invitation namespace, refuses deactivated accounts, and uses the sanctioned `claim_external_id` port transition (plain `update()` keeps rejecting external_id changes). Cross-tenant claim is refused (`jwt_email_link_does_not_cross_tenants`).
- **AuthZ**: tenant-admin and workspace Owner/Admin guards are per-handler with tenant containment on every route; the ABAC-exempt invitee routes carry per-handler authorization (tenant containment 403, caller-must-be-invitee 404) and are guarded by the production-chain regression test with a control proving the seeded membership builtin would otherwise deny the invitee. Magic-link token routes are registered outside `require_auth_middleware` (token is the auth factor; a presented Bearer identity must match the invitee).
- **Expiry job**: `spawn_invitation_expiry` (started in main.rs) marks past-due Pending invitations Expired via conditional transitions (a racing accept is never overwritten), interval overridable via `GYRE_INVITE_EXPIRY_INTERVAL_SECS` with a zero-guard against tokio's panic on zero durations.
- **Docs/coverage**: api-reference rows for every endpoint; server-config row for the env var; coverage rows 5/6/7/25 recount is mechanically consistent; task-124's migration renumber is documented on both sides.

### Scope notes

- The working-tree modification of `specs/tasks/task-110.md` (progress reset to the assigned contract) is the executor's checkout restore per durable finding `921b6673`, not candidate content; the candidate commit itself carries the full 13-SHA attribution list.
- `scripts/check-task-commit-attribution.sh` was not completed in-sandbox (full-history walk timed out; the working tree holds the restored assigned contract). The verifier should run it on the published tree.
