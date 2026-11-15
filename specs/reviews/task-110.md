# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `user-management.md` §Tenant-Level User Onboarding, §Workspace
Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Assignment base `6cacb4bf` → candidate `fca4e6a9` (branch
`pipeline/task-110/dec04fd9744d4783bccd45d07ebd14c5-1`, ~9,740 insertions
across 38 files; core surface `gyre-server/src/api/invitations.rs` +
`gyre-adapters/src/sqlite|postgres/invitation.rs` + migrations
`2026-11-13-000063..000066`). Attribution check passes at the candidate
(all 16 task-110 product commits are in the frontmatter `commits:` list;
the working-tree failure of the same script is the stage harness's reset
of the task file, verified separately).
Verdict: **approved**.

## Independent evidence (persisted under /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain invitation` → **4 passed, 0 failed**
  (status roundtrip, expiry-only-when-Pending for both kinds, policy
  defaults).
- `cargo test -p gyre-adapters --lib invitation` → **6 passed, 0 failed**
  against real SQLite: tenant/workspace roundtrips,
  `transition_status_rejects_wrong_source_status` (conditional UPDATE
  WHERE-clause precondition — the single-use guard),
  `pending_scope_unique_index_enforced_by_database` (partial-index race
  backstop), `migration_000064_dedupes_raced_pending_invitations_before_index`,
  `corrupt_preassigned_columns_fail_closed` (malformed JSON columns error,
  never silently dropped).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → clean (the
  6,000-line handler+test module type-checks under `cfg(test)`).
- Mechanical gates, all exit 0 at the candidate: `check-arch`,
  `check-abac-route-registry`, `check-abac-exempt-handlers`,
  `check-migration-versions`, `check-migration-sql-portability`,
  `check-mem-port-contracts`, `check-in-memory-state-stores`,
  `check-byte-slice-truncation`, `check-scope-literal-defaults`,
  `check-fabricated-scope-defaults`, `check-forged-scope-fields`,
  `check-lossy-secret-conversion`, `check-inert-enforcement`,
  `check-dead-message-kinds`, `check-forwarded-header-trust`,
  `check-unbounded-external-http`, `check-relative-path-defaults`,
  `check-fail-open-ref-resolution`, `check-mcp-write-tools`,
  `check-conditional-test-guards`, `check-aspirational-test-names`,
  `check-api-auth`, `check-task-commit-attribution` (at candidate
  fca4e6a9).
- Non-gating scripts that fail here fail identically at the base commit
  (`check-hardcoded-defaults`: pre-existing `default_branch: "main"`
  findings in files this candidate does not touch, diff-verified
  identical base↔candidate; `check-crypto-verify` /
  `check-cross-surface-parity`: mawk 1.3.4 syntax errors on gawk-only
  script syntax — environment, not code).

## Verified against the task contract

- **Domain entities (AC1):** `TenantInvitation` and `WorkspaceInvitation`
  carry every spec field; `InvitationStatus` has all five variants;
  `InvitationPolicy` matches the spec struct exactly with spec defaults
  (7/7/50/true).
- **Ports (AC2):** `TenantInvitationRepository` /
  `WorkspaceInvitationRepository` with CRUD + token-hash lookup +
  `transition_status(id, from, to, accepted_at) -> bool` whose `from`
  precondition is enforced *inside* the write in every adapter
  (SQLite/PG: conditional `UPDATE ... WHERE status = from`; mem:
  check-and-set under the store lock). `check-mem-port-contracts` green.
- **SQLite adapter + migrations (AC3):** migrations 000063–000066 hold
  unique versions (000063 renumbered off a colliding upstream version,
  documented), portable SQL, BIGINT timestamps with a PG-only
  `repair::ensure_invitation_timestamps_bigint` hook for databases that
  ran the pre-repair INTEGER variant, and a pending-scope partial unique
  index with raced-duplicate dedupe on apply. PG adapter mirrors the
  SQLite one field-for-field.
- **All 5 tenant endpoints (AC4) + workspace lifecycle (AC5):**
  single/bulk/list/filter/revoke + magic-link accept; workspace
  invite/list/revoke; invitee-facing `GET /users/me/invitations`,
  authenticated `POST /workspaces/{id}/invite/accept|decline`; token
  accept/decline for both invitation kinds. Magic-link routes are
  registered in `build_router` outside `require_auth_middleware` (token
  is the auth factor) with a per-handler invitee-match guard whenever a
  Bearer identity IS presented. Deactivated accounts are rejected on
  every accept path. Cross-tenant guards on every scoped route.
- **Token crypto (AC6):** 32 bytes from ring `SystemRandom`, hex-encoded,
  only the SHA-256 hash persisted; tokens redacted from tracing spans
  AND the unauthenticated `/metrics` label values
  (`redact_token_paths`, regression test asserts the raw token never
  appears in `state.metrics.render()` and the redacted template does).
- **Expiry job (AC7):** `spawn_invitation_expiry` spawned from
  `main.rs`; interval via `GYRE_INVITE_EXPIRY_INTERVAL_SECS` with a
  zero/invalid → default guard (tokio interval panics on zero); first
  tick expires backlog; conditional Pending→Expired transitions never
  overwrite a racing accept; rows kept for audit. Per-tenant policy
  persisted in the kv store; `load_policy` fails CLOSED on a corrupt row.
- **Bulk (AC8):** accepts the spec's `{invitations: [...],
  expires_in_days}` shape; per-entry validation errors reported without
  aborting the batch.
- **Routes + ABAC (AC9):** all routes registered in `api/mod.rs` /
  `lib.rs` with resolver mappings; invitee accept/decline are
  `RouteResourceMapping::exempt` (correctly — the seeded
  `workspace-membership-required` builtin would 403 a non-member invitee
  before per-handler authz) with tenant containment +
  caller-must-be-the-invitee enforced visibly in each handler body;
  `invitee_accepts_and_declines_through_abac_middleware` proves
  reachability through the real api_router + abac_middleware chain with
  a 403 control case on a workspace-mapped route.
- **`cargo test --all` (AC10):** deferred to the independent verifier
  per the stage budget rules (gyre-server test codegen); recorded
  commands below. Layer evidence above (domain, adapters-on-real-SQLite,
  server + test code check) is green.

## Security-relevant design reviewed in depth

- **Single-use acceptance:** claim (conditional transition) happens
  BEFORE any side effect; failure after the claim compensates by rolling
  back to Pending, and a failed compensation is loudly logged with the
  row id. Concurrent claims provably produce exactly one winner
  (adapter + handler tests).
- **SSO link path:** `find_or_create_user_tenant` claims an
  invitation-created account only when the JWT email is
  `email_verified=true` AND the namespace record's tenant AND email
  match the claim (a foreign record planted on the
  `tenant:{t}:email:{e}` namespace via SCIM verbatim externalId is never
  linked); the re-point goes through the sanctioned `claim_external_id`
  port transition (plain `update()` rejects external_id changes).
- **Credential delivery:** local-mode acceptance mints the account's
  first API key (CSPRNG, SHA-256 stored, registered in the real
  `state.api_keys` store) ONLY for invitation-created accounts; a
  pre-existing independently-bound account (SSO subject / bootstrap
  `local:{username}`) gets `api_key: null` — token possession must not
  bypass an existing auth factor. Reserved system ABAC identity rejected
  on explicit and derived display names/usernames.
- **Removal of task-124's parallel membership-row invitation handlers**
  is a documented, reviewed rebase resolution: both branches registered
  the same spec paths, and task-110's `WorkspaceInvitation` model is the
  assigned superset (decline, expiry, per-tenant policy, cap, background
  job). Coverage rows 26/27/30 recount mechanically and credit both
  tasks. No behavioral hole: the four spec paths remain live with
  equivalent-or-stronger authorization (invitee routes now prove tenant
  containment + invitee identity per-handler).

## Non-blocking observations

- Invalid/expired Bearer credentials on the magic-link routes degrade to
  the anonymous path (axum `Option<Extractor>` maps rejections to
  `None`); the invitation token remains the sole auth factor and the
  invitee-match guard is simply not applicable — acceptable under the
  documented trust model.
- Duplicate/cap pre-checks in `create_tenant_invitation` /
  `invite_to_workspace` are read-then-write; the pending-scope partial
  unique index is the duplicate race backstop (surfaced as a port
  contract error), and the cap is advisory. Fine.
- `list_workspace_invitations` admits any workspace member (task-124's
  variant was Owner/Admin-only). Within-tenant exposure of invitation
  metadata; consistent with "visible in workspace settings", not a
  containment issue.

## Verifier must run (gyre-server test compilation deferred per stage rules)

- `cargo test -p gyre-server --lib api::invitations` (45 tests)
- `cargo test -p gyre-server --lib auth::` (JWT email-link claim tests)
- `cargo test -p gyre-server --lib middleware::` (token-redaction tests)
- `cargo test --all`
- clippy all-targets + frontend `cd web && npm ci && npm test` (no web
  source changed in this candidate; `web/dist` untouched in the diff —
  generated output owned by the verifier)

Generated-output note (non-blocking, per stage rules): the candidate
contains no `web/` changes at all; there is nothing to observe on the
bundle.

— Reviewer, 2026-11-15
