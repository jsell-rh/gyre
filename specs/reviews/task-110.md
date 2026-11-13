# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding,
§Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate: `9a68f84a` (assigned base `d9c419a`, merge-base `33cd786c2`).
Task-labeled commits reviewed: `8f66d5a4`, `0f4fea9a`, `8c469c53`,
`c1de316b`, `797cf52f`, `ea366b15`, `6bef61b7` (all verified ancestors of
the candidate head).
Verdict: **needs-revision** — one structural finding (F1, integration
fitness against the assigned base); the task implementation itself is
verified clean against every acceptance criterion.

## Probes (evidence: /tmp/stage/review-evidence/task-110-review.md)

- `cargo test -p gyre-domain invitation` → 4 passed.
- `cargo test -p gyre-adapters invitation` → 5 passed against real SQLite,
  including DB-level pending-scope unique-index enforcement and the
  migration-000061 raced-duplicate dedupe.
- `cargo check -p gyre-server --lib --tests` → exit 0.
- All 19 mechanical invariant scripts → exit 0 (arch, ABAC route registry,
  ABAC exempt handlers, migration versions, migration SQL portability, mem
  port contracts, in-memory state stores, forged scope fields, inert
  enforcement, scope literal defaults, lossy secret conversion, fabricated
  scope defaults, byte-slice truncation, relative path defaults, forwarded
  header trust, dead message kinds, MCP write tools, unbounded external
  HTTP, warn-continue creation).
- Commit attribution gate → OK at HEAD (the working-tree failure is the
  harness's reset of task-110.md to its assigned state, not the candidate).
- F1 probe: `git worktree add /tmp/merge-test d9c419a && git merge --no-commit
  --no-ff 9a68f84a && bash scripts/check-migration-versions.sh` → exit 1
  (duplicate Diesel migration versions; details below). Worktree removed
  afterwards; candidate tree untouched.

## Verified working (no findings)

- **Domain/ports/adapters are real.** `TenantInvitation` /
  `WorkspaceInvitation` / `InvitationStatus` / `InvitationPolicy` carry
  every spec field; `TenantInvitationRepository` /
  `WorkspaceInvitationRepository` port contracts (pending-scope uniqueness,
  atomic conditional `transition_status` as the single-use claim) are
  enforced by all three adapters — SQLite and PostgreSQL via
  check-then-insert plus the partial unique indexes from migration
  `2026-11-08-000061` (violations mapped to the port-contract error), mem
  via check-and-set under the store lock.
- **All five specced tenant endpoints plus decline, policy GET/PUT,
  workspace invite/list/revoke, invitee-facing accept/decline, and
  `GET /users/me/invitations` are registered** (api/mod.rs) with ABAC
  mappings (abac_middleware.rs) and per-handler TenantAdmin / Owner-Admin
  guards plus tenant containment on every handler. The magic-link routes
  correctly sit outside `require_auth_middleware` (the 256-bit CSPRNG token
  is the auth factor), and a presented Bearer identity must match the
  invitee (email match for tenant invitations, user-id match for workspace
  invitations).
- **Token handling is real crypto.** 32 bytes from ring `SystemRandom`,
  hex-encoded; only the SHA-256 hash is persisted; the raw token appears
  only in the create response. Known-answer test pins the hash function.
- **Acceptance is single-use and race-safe.** The Pending→Accepted claim
  is a conditional write; concurrent accepts produce exactly one winner
  (tested); failure after the claim rolls the invitation back to Pending;
  expiry/revocation/decline transitions are equally conditional so a
  racing accept is never overwritten.
- **SSO/local dual mode works end to end.** Local accept mints a
  first-class tenant-scoped user (username contract: `validate_username`
  before the claim, deterministic unique-suffix derivation on collision);
  SSO-provisioned accounts are linked rather than shadowed; first SSO
  login claims a local-mode invitation account via `claim_external_id`
  only when the JWT email is verified and tenant-scoped (unverified-email
  and cross-tenant takeover tests are hard and negative).
- **Expiry is real.** `spawn_invitation_expiry` starts in main.rs (hourly,
  `GYRE_INVITE_EXPIRY_INTERVAL_SECS` override, zero falls back instead of
  panicking), marks past-due Pending rows Expired via conditional
  transitions, never deletes; expired tokens are rejected at accept and
  transitioned to Expired immediately. Per-tenant `InvitationPolicy`
  (7/7/50/allow_re_invite defaults) is persisted via the kv store, applied
  on every creation, and fails closed on a corrupt row.
- **Bulk invite** accepts an array, applies per-entry validation without
  aborting the batch, and reports `{created, errors, created_count,
  error_count}`.
- **`max_pending_invitations` cap (429) and `allow_re_invite: false`
  (409)** are enforced on both tenant and workspace surfaces, with tests
  that prove the cap and that revocation frees capacity.
- **Cross-tenant containment** is proven through HTTP with real API-key
  principals in two tenants (invite/list/revoke/accept all 403, invitation
  not burned).
- **Docs and coverage** updated: api-reference rows for every new route,
  server-config documents the expiry interval env var, coverage rows 5/6/7/25
  marked implemented with accurate evidence.
- Test-inflation check: none found — the suite drives real routers with
  real stores, asserts observable HTTP/storage outcomes, and includes
  negative tests (mismatched bearer, non-admin, foreign tenant, revoked,
  expired, single-use, cap, unverified email) that fail if the behavior
  regresses.

## Findings

- [-] **F1 (structural, blocking): the candidate is not based on the
  assigned base, and integrating it there collides Diesel migration
  versions.** The assigned base is `d9c419a`; the candidate's merge-base
  with it is `33cd786c2` (the round-13 rebase target), and the base has
  advanced 1433 commits since. A trial merge of the candidate into
  `d9c419a` (temporary worktree, removed afterwards) yields 6 content
  conflicts (postgres/mod.rs, schema.rs, gyre-domain/lib.rs,
  api/mod.rs, docs/api-reference.md, coverage user-management.md) and —
  the mechanical failure — a union migrations directory in which
  `2026-10-10-000058_invitations` duplicates the base's
  `2026-10-10-000058_budget_enforcement_action` (version `20261010000058`)
  and `2026-11-08-000061_invitation_pending_unique` duplicates the base's
  `2026-11-08-000061_spec_links` (version `20261108000061`).
  `scripts/check-migration-versions.sh` fails on the merged tree, and if
  bypassed, only the alphabetically-first migration per version runs:
  fresh databases would silently lose the invitation tables (the exact
  task-095 F5 failure class this check exists to prevent). This is the
  same situation as the task's own rounds 4/5/6/8/10/12/13, each resolved
  by a rebase repair round.

  **Repair path (owned by task-110's next revision round):** rebase (or
  merge) onto assigned base `d9c419a`; renumber the task's migrations to
  the next unused sequence numbers at that base (base tail is
  `2026-11-13-000062_team_slug_lead_membership`, so the invitations pair
  becomes e.g. 000063/000064, preserving the order invitations →
  invitation_pending_unique, with user_tenant_scope renumbered in
  between or after as its dependencies allow); resolve the six conflicts;
  re-run `bash scripts/check-migration-versions.sh`,
  `bash scripts/check-migration-sql-portability.sh`, and the focused test
  set; record the repair commit in the task's `commits:` frontmatter.

Non-blocking, recorded for the verifier/controller:

- gyre-server test execution is deferred (build exceeds this sandbox's
  4 GiB budget): `cargo test -p gyre-server --lib api::invitations` (26
  tests), `cargo test -p gyre-server --lib auth` (three task-110 JWT
  email-link tests), then `cargo test --all`, and the full CI check set on
  the integrated tree.
- Generated-output observation: no web/src files were authored by this
  task; the branch's web/dist changes came from checkpoint `6bef61b7` at a
  merged base. The committed bundle may be stale relative to the final
  integrated tree — the deterministic verifier owns rebuild and
  publication of dist assets.
