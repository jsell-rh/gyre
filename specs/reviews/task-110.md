# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding, §Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate under review: `c5c927604` (base `3d87a1507`, merge commit `bc4bc1090` + process commit).
Verdict: **approved** — no findings. Evidence: `/tmp/stage/review-evidence/task-110-round13-review.md` (plus raw probe transcripts probe1–probe3 in that directory).

## What was reviewed

Diff `3d87a1507..c5c927604` (32 files, +7333/−30): domain entities (`gyre-domain/src/invitation.rs`), ports (`gyre-ports/src/invitation.rs`, plus `UserRepository::find_by_email`/`claim_external_id`), SQLite + Postgres adapters with three shared migrations (000058 invitations tables, 000059 pending-scope partial unique indexes with raced-duplicate dedupe, 000060 user tenant-scope index), mem adapters, all API handlers (`gyre-server/src/api/invitations.rs`), route registration (`api/mod.rs` + pre-auth magic-link router in `lib.rs`), ABAC mappings, auth.rs SSO email-link, background expiry job (`main.rs` spawn), docs, coverage matrix rows 5/6/7/25.

## Verification performed

**Focused tests (exit 0):** `cargo test -p gyre-server --lib api::invitations` 26/26; `-p gyre-adapters invitation` 5/5; `-p gyre-server --lib auth::` 55/55 (includes the verified-email-claim / unverified-no-claim / cross-tenant-no-link F2 tests).

**Mutation probes (restored immediately; tree clean after):**
- Removing the `require_tenant_admin` guard body → `non_admin_cannot_invite_via_http` FAILS.
- Disabling `TenantInvitation::is_expired` → `expired_invitation_cannot_be_accepted_and_job_marks_expired` FAILS.

The guards are load-bearing; the suite is not self-confirming.

**Live end-to-end probes against the SQLite production wiring** (not the mem test state; `SKIP_WEB_BUILD=1` debug build, `GYRE_DATABASE_URL=sqlite:...`, server on :3931):
- Tenant: invite (email lowercased, 64-hex CSPRNG token only in the create response; listing leaks no token material) → unauthenticated magic-link accept creates the user → token re-use 409 → unknown token 404.
- Workspace: invite by email/user_id (all three spec selectors), pending-duplicate 409, revoke 204, re-invite after revoke, token accept 204 (membership activated `accepted:true`), re-accept 409, already-member 409.
- Background expiry job: a Pending row backdated directly in SQLite flipped to `Expired` within one job interval, logged `count=1`; first cycle at startup; `GYRE_INVITE_EXPIRY_INTERVAL_SECS` honored; zero-value falls back to default instead of panicking.
- Bulk invite: partial success (`created_count`/`error_count`), no batch abort. Policy GET/PUT: spec defaults, partial update persists, `allow_re_invite:false` blocks re-invite after terminal state with 409; unauthenticated invite 401.

**Mechanical guards (all exit 0):** arch, ABAC route registry, ABAC-exempt handlers (83), migration versions (no duplicate Diesel versions despite the rebase renumbering saga), mem port contracts, in-memory state stores, forged scope fields, inert enforcement, migration SQL portability, forwarded-header trust, unbounded external HTTP, byte-slice truncation, relative-path defaults, fail-open ref resolution, fabricated scope defaults, lossy secret conversion, scope literal defaults, dead message kinds, MCP write tools, task commit attribution.

## Design points checked and found sound

- **Single-use acceptance:** `transition_status(id, from, to)` is a conditional write in every adapter (SQL `WHERE status = from` under one statement / mem check-and-set under the store lock); the accept path claims BEFORE side effects and compensates (rollback to Pending) on failure. Concurrent-claim test races two `tokio::join!` futures — exactly one winner.
- **Token security:** 32 bytes from ring `SystemRandom`, hex-encoded (256 bits), stored only as SHA-256 (known-answer test for the hash), unique index on `token_hash`. Magic-link routes sit outside `require_auth_middleware` by design (the token is the auth factor; the invitee may have no credentials), remain behind the rate-limit middleware, and when a Bearer identity IS presented it must match the invitee (email match on tenant side, user-id match on workspace side).
- **SSO link:** claims an invitation-created account only on `email_verified: true` and only within the tenant-namespaced external_id — no cross-tenant claim, no unverified-email takeover; `claim_external_id` is the sanctioned repoint and is port-contract-enforced in all three adapters.
- **Scope handling:** every management handler derives tenant from the authenticated caller, loads the scoped entity, and 403s on mismatch (revoke checks invitation.tenant_id; workspace routes check workspace.tenant_id; invitee resolution is tenant-filtered; cross-tenant pre-assigned workspaces are skipped at accept). No fabricated "default" scope identities.
- **Policy fails closed:** a corrupt stored policy row surfaces as an error (defaults are the permissive side), with PUT as the remediation path.
- **DB-level pending uniqueness:** partial unique indexes (migration 000059) back the check-then-insert port contract, with a dedupe pass so index creation cannot fail on pre-existing raced duplicates; portable SQL (portability check green).

## Minor observations (non-blocking, no action required)

- Invitation notifications reuse `NotificationType::TrustSuggestion` with `kind: workspace_invitation` in the body — cosmetic type reuse; the semantic kind is carried and tested.
- An authenticated caller whose stored user has no email cannot accept their own tenant invitation via the Bearer path (`require_matching_invitee` rejects); the unauthenticated token path remains available. Edge case only.
- The pre-invite duplicate guard lists all tenant invitations (O(n) per invite) — a performance nit, not a correctness issue; the unique index is the real arbiter.
- Server-log ERRORs during the live probe (transient r2r2 "database is locked" at startup, OTLP export failures with no collector) are environment noise, not task defects.

Test-inflation check: none found — every probed guard has a killing test, and the mutation probes confirm it. Scope check: no out-of-scope edits; web/dist untouched (no web source changed; remote rebuild owns publication). Full-workspace `cargo test --all` and all-targets Clippy left to the verification stage per review scope.
