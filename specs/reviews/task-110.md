# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding, §Workspace
Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate under review: `8ae782b1d450f85d628acde7f46fb7fc692e0af4` (assignment base
`b8d63b0d115c1b7623480f69010f43d718e336bc`, merged at `1ada3106`; the branch carries
multiple prior review/repair rounds plus a process commit recording the base merge and
frontmatter restoration). All 14 frontmatter commits are present on the branch and the
attribution gate passes.

Verdict: **approved** — no blocking findings. Deferred verifier probes listed at the
end (sandbox build-wrapper limit, not a code defect).

## Probes run (independent, this round)

All from the checkout at `8ae782b1`, working tree clean before and after:

- `cargo test -p gyre-domain -p gyre-ports -p gyre-common` → **668 passed, 0 failed**
  (543 domain incl. the invitation entity suite, 125 common).
- `cargo test -p gyre-adapters` → **545 passed, 0 failed** (incl.
  `sqlite::invitation::tests`: roundtrips, `transition_status_rejects_wrong_source_status`,
  `pending_scope_unique_index_enforced_by_database`,
  `migration_000064_dedupes_raced_pending_invitations_before_index` — real SQLite, real
  conditional UPDATE, real partial unique index).
- `cargo test -p gyre-cli invitation` → **1 passed** (`cli_tenant_invitations_subcommands_parse`).
- `cargo check -p gyre-server` → clean (type checking; full server test compilation is
  wrapper-blocked in this sandbox, deferred below).
- Mechanical gates, all exit 0: `check-arch`, `check-abac-route-registry`,
  `check-abac-exempt-handlers`, `check-migration-versions`, `check-mem-port-contracts`,
  `check-mcp-write-tools`, `check-dead-message-kinds`, `check-byte-slice-truncation`,
  `check-relative-path-defaults`, `check-fabricated-scope-defaults`,
  `check-lossy-secret-conversion`, `check-scope-literal-defaults`,
  `check-inert-enforcement`, `check-migration-sql-portability`,
  `check-forged-scope-fields`, `check-forwarded-header-trust`,
  `check-in-memory-state-stores`, `check-unbounded-external-http`,
  `check-fail-open-ref-resolution`, `check-task-commit-attribution`.
- **Route-table probes** (standalone crate with the repo's exact axum 0.7.9 /
  matchit 0.7.3, since `cargo check` cannot prove router-construction or extraction
  behavior; source and output in `/tmp/stage/review-evidence/route-probe-*`):
  1. `/api/v1/workspaces/:id/invitations/:id` (duplicate `:id` names, api/mod.rs:1065)
     registers without panic.
  2. The outer-router magic-link routes (`/api/v1/invite/:token/{accept,decline}`,
     `/api/v1/workspaces/invitations/:token/{accept,decline}`) merge with the
     api_router's workspace-invitation routes without matchit conflict.
  3. `DELETE /api/v1/workspaces/ws-cap/invitations/inv-123` through
     `Path<(String,String)>` extracts `("ws-cap","inv-123")` positionally —
     `revoke_workspace_invitation`'s tuple is populated correctly despite the duplicate
     param names.

Evidence: `/tmp/stage/review-evidence/` (test summaries, gate outputs, route-probe
source + output).

## Verified against the task contract

- **Domain/ports/adapters**: `TenantInvitation` and `WorkspaceInvitation` carry every
  spec field (incl. `role: GlobalRole`, parallel `workspace_ids`/`workspace_roles`,
  `token_hash`, `accepted_at`); `InvitationStatus` matches the spec enum exactly;
  `InvitationPolicy` matches the spec struct with the spec defaults (7/7/50/true).
  Both repository ports are implemented by SQLite, PostgreSQL, and mem; the
  `transition_status(id, from, to)` single-use contract is enforced **inside the
  write** in all three (conditional UPDATE with the `from` status in the WHERE clause;
  check-and-set under the store lock in mem), and the pending-scope duplicate contracts
  are guarded in mem code plus DB-level partial unique indexes (migration 000064, with
  raced-duplicate dedup before index creation so index creation cannot fail on legacy
  duplicates). Portability gate passes for the shared migrations (BIGINT timestamps,
  plain SQL); the PG-only `repair::ensure_invitation_timestamps_bigint` hook repairs
  deployments that ran an earlier INTEGER variant and is idempotent.
- **All five tenant endpoints functional** and registered: single invite, bulk (array
  of invitations, per-entry errors without batch abort), list with status filter,
  revoke (conditional Pending→Revoked; concurrent accept wins), magic-link accept.
  Per-handler `require_tenant_admin` (UserRole::Admin) complements the ABAC
  `tenant`/`write` mappings so the system token passes while plain Members get 403
  (HTTP-level negative test asserts nothing is created).
- **Workspace invitation lifecycle**: invite by `user_id`/`username`/`email`
  (multi-selector ambiguity rejected), pending → accept/decline/expire/revoke, cap
  (`max_pending_invitations` → 429, freed by revoke), per-tenant expiry policy,
  `allow_re_invite=false` enforced on both surfaces, cross-tenant invites rejected
  (two-tenant HTTP fixtures, both directions). Acceptance activates a real
  `WorkspaceMembership` (idempotent on re-accept of a pending membership); decline
  leaves none.
- **Invitee-facing surface**: `GET /users/me/invitations` (no token material
  exposed), authenticated `POST /workspaces/{id}/invite/{accept,decline}` —
  ABAC-exempt because the invitee is a non-member by definition (the seeded
  `workspace-membership-required` builtin would 403 them before the handler);
  per-handler `authorize_my_pending_invitation` enforces tenant containment (403) and
  caller-must-be-the-invitee (404, no cross-user existence leak). The production-chain
  regression test layers the real `api_router` + `abac_middleware` + seeded builtins
  and includes a **control probe** proving the same invitee is denied 403 on a
  workspace-mapped route — so the 204 on the exempt route genuinely proves the
  exemption, not a broken middleware.
- **Token handling**: 32 bytes from ring `SystemRandom` (CSPRNG), hex-encoded,
  stored only as SHA-256 (known-answer test pins the hash); token visible exactly once
  in the create response (`invite_url`); lookup by hash; terminal statuses reject
  reuse; single-use claim precedes all side effects with rollback to Pending on
  failure (username validation happens **before** the claim so bad input cannot burn
  the invitation).
- **Bearer-identity guards on token routes**: a presented Bearer identity must match
  the invited email (tenant) / invited user (workspace); identity-less bearers are
  rejected fail-closed; mismatch leaves the invitation Pending (tested).
- **Local vs SSO modes**: local-mode acceptance creates the user
  (`tenant:{t}:email:{e}` namespace external_id) and mints a first API key — the
  documented rationale is sound (the platform has no password-login path; API keys are
  the local credential; same SHA-256-hash contract as the POST /users bootstrap key,
  and the test proves the minted key authenticates through the real extractor).
  SSO-provisioned accounts are linked (no parallel account, invited role and
  pre-assigned memberships attach to the real account, no key minted — token
  possession must not bypass the IdP). The auth.rs `find_or_create_user_tenant`
  email-link claim requires `email_verified=true` and uses the sanctioned
  `claim_external_id` port transition (update() rejects external_id changes; claim
  refuses a taken external_id — adapter tests cover all three arms).
- **Deactivation contract**: all acceptance paths (local, SSO-link, workspace token,
  workspace authenticated, JWT email-link) reject deactivated accounts with 403
  before side effects, rolling the claim back to Pending — five dedicated regression
  tests.
- **Expiry**: `spawn_invitation_expiry` spawned from main.rs (interval via
  `GYRE_INVITE_EXPIRY_INTERVAL_SECS`, zero/unparsable falls back — avoids the tokio
  interval panic), first tick at startup, conditional Pending→Expired transitions so a
  racing accept is never overwritten, rows kept for audit. Lazy expiry on touch
  persists the terminal state immediately.
- **Merge handling**: task-124's parallel membership-row invitation handlers on the
  same routes were removed in favor of task-110's `WorkspaceInvitation` model
  (duplicate axum registrations are impossible; the superset is the assigned scope),
  the task-124 migration was renumbered to 000066 to avoid a Diesel version collision
  with 000063, and the renumber notes document the Diesel first-wins semantics.
  `check-migration-versions` and the ABAC registry/exempt gates all pass on the merged
  tree.
- **Docs/coverage**: `docs/api-reference.md` documents every new route (including
  error semantics and the credential-delivery contract), `docs/server-config.md` the
  expiry env var; coverage matrix rows 5/6/7/25 marked implemented with accurate notes
  ("Pending independent review"), user-management.md has 0 not-started rows, and the
  counts match a mechanical recount.

## Test-quality assessment

No test inflation found. The suite drives the real handlers through the HTTP layer
(mem AppState backed by the same port traits), asserts on stored state after negative
outcomes (403/409 must not create/burn/activate anything), includes a production-chain
ABAC regression with a control, a concurrency test proving exactly one claim winner,
and deactivation/username-collision/SSO-link/cap/cross-tenant behaviors that would
each fail if the corresponding production check were removed. The adapter tests run
against real SQLite storage including the DB-level unique index.

## Minor observations (non-blocking, recorded for completeness)

- `create_tenant_invitation` loads all of the tenant's invitations per invite
  (O(pending) scan) rather than a filtered lookup; acceptable at spec scale, and the
  DB index remains the race backstop.
- The best-effort claim rollback (`let _ = transition_status(...)`) in the accept
  failure paths is unlogged if it itself fails; the invitation would stay Accepted
  with undelivered side effects. Edge case only (both writes are conditional updates
  on the same row).
- Cross-tenant revoke probes distinguish 403 (exists) from 404 (missing) by invitation
  id; ids are unguessable `Id`s, so the existence signal is negligible.

## Generated output note (verifier-owned, non-blocking)

The diff does not touch `web/` at all (0 files); no frontend source shipped with this
task, so there is no authored-frontend/stale-bundle divergence to record. `web/dist/`
is owned by the deterministic verifier per the standing instruction.

## Deferred verifier probes (sandbox build wrapper; not code defects)

- `cargo test -p gyre-server` — full suite, in particular `api::invitations::*`
  (30+ tests incl. the production-chain ABAC regression), `auth::*` JWT email-link
  tests, and `abac_middleware::*` build_router tests.
- `cargo test --all` — the task's final acceptance criterion.
- `cargo clippy --all-targets` if part of the standard verification pass.
