# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding, §Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate under review: `e1bab69e7` (rebase merge `cfd42842d2` of the retained task-110 checkpoint onto base `cd752300c`, plus two narrative-only process commits).
Verdict: **approved** — no findings.

## Round 12 (rebase verification)

This round re-verified the previously reviewed implementation after the
requirements-changed rebase onto base `cd752300c` (which carried the
task-208 "My Stuff" supersession deleting the `/users/me/{agents,tasks,mrs}`
routes). Full evidence: `/tmp/stage/review-evidence/task-110-round12.md`.

### Probes run

- `cargo test -p gyre-server --lib api::invitations` → 26/26 passed.
- `cargo test -p gyre-adapters invitation` → 5/5 passed (incl. DB-level
  pending-scope unique index and migration 000059 dedupe).
- `cargo test -p gyre-server --lib` (full crate, merge validation) →
  1630/1630 passed.
- All mechanical guards green (arch, ABAC registry, ABAC-exempt handlers,
  migration versions, MCP write tools, dead message kinds, mem port
  contracts, fabricated scope defaults, inert enforcement, in-memory
  state stores, migration SQL portability, forged scope fields, task
  commit attribution).
- **Independent real-router probe** (external crate at /tmp/router-probe,
  in-process `build_router` — the in-repo tests use a hand-built router,
  so registration/middleware-layering defects were unproven; a live HTTP
  probe was blocked by the sandbox transport policy, an infrastructure
  restriction, not a code defect):
  - magic-link accept reachable **without** Authorization (bogus token →
    404, not 401) — the routes are genuinely outside
    `require_auth_middleware`;
  - tenant invite / list / policy PUT through the real auth+ABAC chain
    (201/200/200); unauthenticated list → 401;
  - unauthenticated magic-link accept creates the local-mode user;
    second accept → 409 (single-use); revoke-after-accept → 409;
  - bulk invite partial success (created_count/error_count);
  - **SQLite pass on a fresh database**: migrations 20261010000058 /
    20261108000059 / 20261108000060 all ran; invitation row `Accepted`,
    user created with tenant + invited GlobalRole; pending-scope partial
    unique indexes and `idx_users_tenant` present.

### Verified

- **Spec contract complete**: `TenantInvitation`/`WorkspaceInvitation`
  domain entities with every spec field; port traits; SQLite + Postgres +
  mem adapters (port contracts guarded in code where there is no schema);
  all five tenant-invitation endpoints plus bulk, revoke, decline,
  magic-link accept; workspace invite/list/revoke + token and
  authenticated invitee accept/decline surfaces; per-tenant
  `InvitationPolicy` (expiry days, max_pending_invitations,
  allow_re_invite) with fail-closed corrupt-row handling; background
  expiry job (env-configurable interval, first tick at startup, never
  deletes rows, uses the same conditional `transition_status` claim as
  accept so a racing accept is never overwritten).
- **Token handling**: 32-byte ring SystemRandom tokens, stored only as
  SHA-256 (known-answer test in suite); single-use enforced by
  conditional Pending→X transitions at the store (SQLite/Postgres UPDATE
  WHERE status=from; mem under one lock); expired tokens rejected on
  accept (409, persisted Expired).
- **Authorization**: TenantAdmin-only tenant management enforced
  per-handler (non-admin 403 test through the real extractor chain);
  workspace invite/revoke restricted to Owner/Admin member or
  TenantAdmin with tenant containment (cross-tenant 403s proven over
  HTTP fixtures on both surfaces); magic-link routes enforce
  matching-invitee when a Bearer identity is presented; the SSO email
  link in `find_or_create_user_tenant` requires `email_verified=true`
  and never crosses tenants (both proven by dedicated tests).
- **Rebase correctness**: the task-208 me-route removals are preserved
  (no dead handlers/registrations/ABAC mappings survive) while the full
  task-110 surface is intact; `scripts/warn-continue-creation-exemptions.txt`
  change is comments-only; coverage matrix rows 5/6/7/25 updated to
  implemented with accurate narratives; the two head process commits
  touch only the task narrative.
- **Migration numbering cleared**: new migrations share the 000058-000060
  sequence numbers with differently-dated base migrations, but Diesel
  versions include the date prefix, so there is no collision
  (check-migration-versions.sh legitimately green; fresh-DB SQLite probe
  confirms all migrations actually ran).

No structural defects, missing enforcement, unsafe scope handling, fake
implementations, or weak tests found. Approval is based on the probes
above, not on the implementation agent's progress labels.
