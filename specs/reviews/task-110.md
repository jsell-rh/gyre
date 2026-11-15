# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding, §Workspace
Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate: `073e47ca446d4125737c71a7169a0356a5e1e111` (assignment base `6cacb4bf`,
task-114). This round is a source-merge rebase only; the feature surface is cumulative
from the branch's 19 recorded commits.
Verdict: **needs-revision** — the task contract itself is genuinely and robustly
implemented (all acceptance criteria verified with real probes), but four targeted
defects remain in the accept/adapter paths. None require redesign; all are
point fixes.

## Probes run (exact candidate checkout)

- `cargo test -p gyre-domain invitation` → 4 passed.
- `cargo test -p gyre-adapters invitation --lib` → 5 passed, including
  `pending_scope_unique_index_enforced_by_database` (real SQLite, DB-level
  partial unique index), `migration_000064_dedupes_raced_pending_invitations_before_index`,
  `transition_status_rejects_wrong_source_status`.
- `cargo check -p gyre-server` → exit 0.
- Mechanical gates all exit 0: check-arch, check-abac-route-registry,
  check-abac-exempt-handlers, check-migration-versions,
  check-migration-sql-portability, check-mem-port-contracts,
  check-in-memory-state-stores, check-forged-scope-fields,
  check-fabricated-scope-defaults, check-inert-enforcement,
  check-lossy-secret-conversion.
- `check-task-commit-attribution.sh` timed out in the review sandbox (needs full
  history walk) — verifier must re-run.
- Evidence: `/tmp/stage/review-evidence/task110-final/` (logs + SUMMARY.md).
- gyre-server test compilation deferred to the verifier per the 4 GiB budget rule;
  the invitation test module was read and assessed instead (see below).

## Verified working (independently confirmed at this SHA)

- **All five tenant endpoints** plus policy GET/PUT, workspace invite/list/revoke,
  invitee-facing accept/decline (authenticated + token variants), and
  `GET /users/me/invitations` — registered in `api/mod.rs`/`lib.rs` with ABAC
  mappings; magic-link routes correctly outside the auth middleware chain (token is
  the auth factor) with per-handler token validation.
- **Single-use acceptance**: the conditional `transition_status(Pending→Accepted)`
  claim runs BEFORE any side effects and is genuinely atomic in all three adapters
  (SQLite/PG: `WHERE status = from` inside the UPDATE; mem: CAS under the store
  lock). Adapter tests pin this against real SQLite.
- **Token security**: 32-byte ring `SystemRandom` tokens, only SHA-256 hashes
  persisted, raw token only in the one-time create response, redacted from
  observability.
- **Tenant containment**: tenant derived from the auth extractor everywhere (never
  request bodies); revoke re-checks invitation tenant; workspace routes guard
  `workspace.tenant_id` before role gates; cross-tenant invitees rejected;
  `require_matching_invitee` fails closed for bearer identities that don't match
  the invited email.
- **Expiry**: `run_expiry_once` marks both invitation kinds via the same
  conditional claim (a racing accept is never overwritten); job spawned in
  main.rs, env-overridable interval, zero-value filtered.
- **Migrations**: 000063/000064/000065 portable plain SQL, pending-scope partial
  unique indexes with dedupe-before-index safety net; the base's
  `user_deactivated_at` correctly renumbered to 000066 to break a Diesel version
  collision (verified: base assigned version 20261113000063 to both files).
- **Local-mode credential delivery**: first API key CSPRNG-generated,
  hash-persisted in the auth-resolving store, minted only after all side effects
  succeed; reserved system ABAC identity guarded pre- and post-claim; deactivated
  accounts rejected on every accept/link path.
- **Policy**: fail-closed on corrupt rows; PUT admin-only with validation
  (zero expiry days rejected).
- **Test quality** (read; not executed here): the HTTP-level tests drive the
  production `api_router` + `abac_middleware` with control requests proving the
  middleware would 403 absent the exemption (`invitee_accepts_and_declines_through_abac_middleware`),
  isolate tenant containment with a caller that passes every role gate
  (`cross_tenant_guards_reject_foreign_callers`), pin the cap off-by-one both ways
  (`workspace_invite_cap_is_enforced`), and pin the single-use primitive
  (`concurrent_claims_produce_exactly_one_winner`). Not test-inflated.
- The workspace-invite role gate (workspace Admin may invite with role Owner) is
  CONSISTENT with the pre-existing `update_member_role` authorization model
  (workspace Owner/Admin may also grant Owner via role change) — not a defect.

## Findings (repair round)

- [-] **F1 (medium): accept case-1 trusts a globally-writable external_id
  namespace.** `api/invitations.rs:836-862`: case 1 of
  `accept_tenant_invitation_claimed` consumes a
  `find_by_external_id("tenant:{tid}:email:{email}")` hit without comparing the
  found user's `tenant_id`/`email` to the invitation's — unlike case 2
  (`find_tenant_user_by_email`), which explicitly tenant-scopes. The lookup is
  global (`gyre-ports/src/user.rs:16`), and the pre-existing SCIM surface
  (`api/scim.rs:276-330`, global token) persists caller-controlled `externalId`
  verbatim: a planted record with `externalId =
  "tenant:acme:email:bob@acme.com"` is claimed when Bob accepts Acme's invitation —
  `mint_credential = true` mints an API key for the planted tenant-less record and
  the workspace-activation loop guards only the workspace's tenant, never the
  user's, so the tenant's pre-assigned memberships attach to the foreign record.
  Sibling gap in `auth.rs:2155-2176` (SSO verified-email link, same namespace, no
  tenant re-check). Requires a malicious/compromised SCIM principal (trusted
  surface) and yields no direct privilege escalation (planted records are
  ReadOnly/tenant-less; API-key auth fails closed without tenant scope), but it
  breaks the tenant-isolation invariant case 2 enforces, and the current
  containment lives by accident in a different file. **Fix:** in case 1 require
  `existing.tenant_id == Some(invitation.tenant_id)` and email match (else fall
  through to case 2/3); mirror the check in the auth.rs claim.
- [-] **F2 (low-medium): silent fail-open on malformed stored JSON.**
  `gyre-adapters/src/sqlite/invitation.rs:19-21,29-31` and the identical
  `postgres/invitation.rs:19-21,29-31`: `json_to_ids`/`json_to_ws_roles` use
  `.unwrap_or_default()` — a corrupt `workspace_ids`/`workspace_roles` column maps
  to an invitation with zero pre-assigned workspaces and no error, silently
  dropping the invitee's grants on acceptance. Two lines away an unknown role
  string inside valid JSON errors; invalid JSON itself should too. Also no
  parallel-array length check between the independently parsed columns.
- [-] **F3 (low): re-invite does not apply the invited GlobalRole.** Case 1
  (`api/invitations.rs:847-862`) returns the existing local-mode account untouched
  while cases 2 and 3 apply `invitation.role`; a promotion/demotion re-invite after
  a terminal state silently no-ops, reported as `mode: "linked"`.
- [-] **F4 (low): swallowed rollback failures.** `api/invitations.rs:792, 1635,
  1830`: the Accepted→Pending compensation is `let _ =` with no logging — if both
  the side effect and the compensation fail, the invitation is stranded Accepted
  with nothing delivered and nothing records it. One-line `tracing::error!` fix
  naming the invitation id.

Minor (recorded for completeness, not blocking):

- mem adapter `create` (`mem.rs:7012`, `7105`) overwrites on a reused id and does
  not guard `token_hash` uniqueness (SQL adapters: PK/unique violation) —
  unreachable via the API (fresh UUIDs); parity divergence only.
- `is_plausible_email` (`invitations.rs:2049`) accepts whitespace/control
  chars/consecutive dots; its unit test mirrors the implementation.
- `list_my_invitations` scopes by user_id only and the token-path workspace accept
  does not re-check the invitee's current tenant (containment enforced at invite
  time only) — defense-in-depth.
- `accepted_at` is overwritten (possibly to NULL) on later transitions in all
  three adapters — audit-durability nit.

## Verifier must run (deferred from this sandbox)

1. `cargo test -p gyre-server --lib api::invitations::` (26 tests).
2. Full `cargo test --all`.
3. All-target clippy.
4. `bash scripts/check-task-commit-attribution.sh` (timed out here; needs full
   git history).
5. Confirm no frontend surface is involved (diff touches no `web/` sources).
