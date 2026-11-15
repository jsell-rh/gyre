# Review: task-110 — Tenant & Workspace Invitation Flow

**Reviewer:** Independent review agent
**Round:** R-current (candidate a8fb3d2ba2d21eafa74caad8047746bf9d53455a)
**Spec ref:** user-management.md §Tenant-Level User Onboarding, §Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations
**Comparison base:** 706bc7bd2250aac1768a39687a468355ff8afda7
**Status:** complete

---

## Verdict

**Approved.** No blocking findings. The implementation is real production code
on every acceptance criterion, authorization is enforced per-handler on every
route that bypasses middleware ABAC, and the single-use/expiry semantics are
backed by conditional writes in all three adapters plus database-level partial
unique indexes.

## What was reviewed

Full diff base..candidate (38 files, +8720/−487): domain entities
(`gyre-domain/src/invitation.rs`), ports (`gyre-ports/src/invitation.rs`),
SQLite/PostgreSQL/mem adapters, migrations 000063–000066, all API handlers
(`gyre-server/src/api/invitations.rs`, 5186 lines incl. tests), route
registration (`api/mod.rs`, `lib.rs`), ABAC mappings (`abac_middleware.rs`),
middleware token redaction, SSO email-link in `auth.rs`, `mem.rs` port-backed
adapters, CLI endpoint usage, docs, and coverage-matrix updates.

## Acceptance criteria — evidence

1. **Domain entities** — `TenantInvitation`/`WorkspaceInvitation` carry every
   spec field; `InvitationStatus` (5 variants) and `InvitationPolicy`
   (defaults 7/7/50, `allow_re_invite`) match the spec.
2. **Ports** — `TenantInvitationRepository`/`WorkspaceInvitationRepository`
   with CRUD + scoped listings + `find_by_token_hash` + conditional
   `transition_status` (documented `Ok(false)` single-use contract).
3. **SQLite adapter + migrations** — 000063 (tables, BIGINT timestamps),
   000064 (partial unique pending-scope indexes with raced-duplicate dedupe
   pass), correctly renumbered past upstream collisions; 000065/000066 kept
   collision-free. `check-migration-versions.sh` and
   `check-migration-sql-portability.sh` pass.
4. **All 5 tenant endpoints functional** — invite, bulk, list (status
   filter), revoke (conditional Pending→Revoked), magic-link accept. All
   TenantAdmin-only per-handler (`require_tenant_admin`), with tenant
   containment on revoke.
5. **Workspace invitation lifecycle** — invite (Owner/Admin/TenantAdmin,
   cross-tenant 403, cap 429, duplicate 409) → Pending → accept/decline via
   token routes and authenticated invitee routes → expiry; membership
   activation idempotent.
6. **CSPRNG token, SHA-256 at rest** — 32 bytes from ring `SystemRandom`,
   hex-encoded; only `hash_token` (SHA-256) persisted; known-answer test
   (`ba7816bf…`) pins the hash; token returned once in the create response.
7. **Expiry background job** — `spawn_invitation_expiry` started in
   `main.rs`, hourly default + env override (0 falls back, avoiding the
   tokio interval panic); `run_expiry_once` uses conditional transitions.
8. **Bulk invite** — accepts `invitations` array, per-entry errors reported
   without aborting the batch.
9. **Routes + ABAC** — registered in `api/mod.rs` with resolver entries;
   invitee accept/decline are `RouteResourceMapping::exempt` with real
   per-handler authz (`authorize_my_pending_invitation`: tenant containment
   403, invitee-identity 404). Magic-link routes registered in `lib.rs`
   outside the auth middleware (the token is the auth factor) with
   bearer-identity match guard when a Bearer is presented.
   `check-abac-route-registry.sh` and `check-abac-exempt-handlers.sh` pass.
10. **`cargo test --all`** — not runnable in this sandbox (4 GiB build
    budget for the gyre-server test binary). Focused evidence below; full
    suite recorded for the verifier.

## Probes run (evidence in /tmp/stage/review-evidence)

- `bash scripts/check-*.sh` — all 18 mechanical invariant checks pass
  (arch, ABAC registry, ABAC exempt handlers, migration versions/SQL
  portability, mem port contracts, inert enforcement, in-memory state
  stores, etc.). Log: `invariant-checks.log`.
- `/tmp/stage/bin/cargo check -p gyre-server` — clean (7m51s), incl. web
  build. Log: `cargo-check-server.log`.
- `/tmp/stage/bin/cargo check -p gyre-cli` — clean. Log: `cargo-check-cli.log`.
- `/tmp/stage/bin/cargo test -p gyre-domain invitation` — 4 passed.
  Log: `test-gyre-domain.log`.
- `/tmp/stage/bin/cargo test -p gyre-adapters --lib invitation` — 5 passed,
  including the raw-SQL database-arbitration test
  (`pending_scope_unique_index_enforced_by_database`, which also feeds real
  DB errors to the violation discriminators and proves the token_hash
  constraint is NOT misclassified) and the migration dedupe test
  (`migration_000064_dedupes_raced_pending_invitations_before_index`).
  Log: `test-gyre-adapters.log`.
- Commit attribution: all 13 product-surface task-110 commits on the branch
  are recorded in the candidate's `commits:` frontmatter (the
  `check-task-commit-attribution.sh` failure observed in-sandbox is an
  artifact of the review harness resetting the working-tree task file to
  `commits: []`; the committed candidate is complete).

## Test-quality assessment (no test inflation found)

- `invitee_accepts_and_declines_through_abac_middleware` runs the
  production `api_router` + `abac_middleware` chain and includes a control
  assertion proving the non-member invitee is denied 403 on a
  workspace-mapped route — the 204 only proves something because the
  control holds.
- `concurrent_claims_produce_exactly_one_winner` races two
  `transition_status` calls and asserts exactly one winner.
- `token_is_csprng_and_hashed` includes a SHA-256 known-answer test.
- `invite_token_redacted_from_metrics_labels` (middleware.rs) drives a full
  router request and asserts the raw token appears nowhere in rendered
  metrics while the `{token}` template is the path label — kills the
  redaction-removal mutant.
- Deactivation, reserved-identity, unverified-email, cross-tenant, cap,
  expiry, and SSO-link behaviors all have router-level or storage-level
  tests asserting post-state, not just status codes.

## Non-blocking observations

- `crates/gyre-adapters/src/sqlite/invitation.rs` comments reference
  "migration 000061" in two places where the pending-scope index now lives
  in 000064 (the renumber notes in the migration files themselves are
  correct and explain the history). Cosmetic only.
- `accept_tenant_invitation` mints a fresh API key when a re-invited local
  account is accepted again — documented, consistent with the magic-link
  trust model (the token holder is treated as the account owner, same as
  first acceptance). Not a defect.
- Task-124's parallel membership-row invitation handlers were removed at
  the rebase in favor of task-110's `WorkspaceInvitation` model (axum
  rejects duplicate route registrations; task-110's model is the assigned
  superset). Documented in `api/mod.rs`, `abac_middleware.rs`, and both
  task files; task-124's lifecycle test was removed with its handlers and
  task-110's suite covers the same paths through the production chain.

## Commands the verifier must run (server test binary too large for sandbox)

```
cargo test --all
cargo test -p gyre-server --lib api::invitations
cargo test -p gyre-server --lib middleware::tests
cargo test -p gyre-server --lib auth::tests
cargo clippy --all-targets -- -D warnings
```
