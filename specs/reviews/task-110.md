# Review — task-110 (Tenant & Workspace Invitation Flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding, §Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate under review: `7e427038a79b6445f99f77d17e79e5b83d627bb5` (branch `pipeline/task-110/2e0f34d32a4c4835ae3e4e9ed6f23251-1`), diffed against assigned base `72ad2a128d87c0cfaed5d1f6d2ea8cb5f1ad5781`. Task commits `0f4fea9aa`, `8f66d5a43` match the frontmatter; the intervening history is the upstream merge `a481428a6` plus process-labeled commits. Working tree clean at the candidate SHA.

Verdict: **approved**.

## Round 4 (this review — independent re-verification at the merged head)

Prior rounds were consumed by an interrupted upstream merge (exemption-ledger conflicts) and sandbox compilation blocks; this round re-verified the full candidate from scratch at `7e427038a`. No resume artifacts existed (`/tmp/stage/review-resume.json` absent).

### Diff review

- **Domain** (`gyre-domain/src/invitation.rs`): `TenantInvitation`/`WorkspaceInvitation` with every spec field, `InvitationStatus` (Pending/Accepted/Declined/Expired/Revoked), `InvitationPolicy` with spec defaults (7/7/50, re-invite true).
- **Ports** (`gyre-ports/src/invitation.rs`): both repositories with the port contract documented (create fails on duplicate pending; `transition_status` is a conditional write returning `Ok(false)` on lost race — the single-use guard).
- **Adapters**: SQLite + Postgres implement both traits (conditional `UPDATE ... WHERE status = from` inside the write for transitions; check-then-insert duplicate guards); mem implements check-and-set under the store lock. Migration `2026-10-09-000057_user_tenant_scope` (repair F1: users table lacked `tenant_id`/`global_role` columns the domain already carried — DB-backed deployments silently dropped them) and `2026-10-10-000058_invitations`; both portable SQL, next unused sequence, down scripts present. User adapters round-trip the new columns with a legacy-row regression test (unknown role reads back as Member, never escalation).
- **Server**: all 5 specced tenant endpoints plus decline, per-tenant policy GET/PUT (kv-backed, partial-update semantics, zero-day rejected), workspace invite/list/revoke, token accept/decline, and the invitee surface (`GET /users/me/invitations`, authenticated `POST /workspaces/{id}/invite/accept|decline`). Magic-link routes registered on the outer router in `lib.rs` — outside `require_auth_middleware` — with `Option<AuthenticatedAgent>` so a presented identity must match the invitee (403 + not burned on mismatch, tested both directions). Background expiry job spawned from `main.rs` (`GYRE_INVITE_EXPIRY_INTERVAL_SECS`, first tick at startup), conditional Pending→Expired transitions only. Auth (`find_or_create_user_tenant`): SSO email-link claims an invitation-created account only when the JWT carries `email_verified=true` and the tenant-namespaced external_id matches — negative test `jwt_unverified_email_does_not_claim_invitation_account` present.
- **ABAC**: all new `api/mod.rs` routes have `RouteResourceMapping` entries; per-handler TenantAdmin/Owner-Admin enforcement additionally in the handlers (dev token must pass, so middleware alone cannot carry it).

### Probes run (evidence in `/tmp/stage/review-evidence/`)

- Focused tests: `cargo test -p gyre-server --lib api::invitations` → **18/18**; `--lib auth::` → **39/39**; `cargo test -p gyre-adapters invitation` → **3/3**, `user` → **17/17**; `cargo test -p gyre-domain --lib invitation` → **4/4**. Includes HTTP-layer negative authz (non-admin 403 tenant invite, non-member 403 workspace invite, both asserting no side effects), mismatched/matching bearer identity on magic links, concurrent-claim exactly-one-winner, expiry job, policy-configurability applying to real creation, invitee end-to-end (list → accept → membership active → single-use 404), decline-leaves-no-membership, CSPRNG token + SHA-256 known-answer.
- Mechanical invariants (18 scripts): ABAC route registry, mem port contracts, migration versions + SQL portability, arch, fabricated-scope defaults, scope-literal defaults, in-memory state stores, lossy secrets, forged scope fields, relative path defaults, inert enforcement, dead message kinds, byte-slice truncation, fail-open ref resolution, forwarded-header trust, unbounded external HTTP, task commit attribution — **all OK**. Exemption ledgers resolved to comments only (frozen counts not grown).
- **Live-server smoke through the real `build_router`** (loopback listener available this round; `SKIP_WEB_BUILD=1` debug binary, real SQLite via `GYRE_DATABASE_URL`, so migrations 000057/000058 ran at startup): health 200; `POST /api/v1/tenant/invite` (dev token) → 201 with `invite_url`; magic-link accept **without any Bearer** → 200 + user created (proves the routes genuinely bypass `require_auth_middleware` in the production router, which the in-process tests' hand-built routers cannot prove); re-accept → 409 (single-use through real SQLite); list `?status=Accepted` shows the terminal state; unauthenticated management invite → 401; policy endpoint returns spec defaults; workspace token routes reach their handlers (bogus token → 404 `invitation not found`); invitee routes correctly behind auth (401 unauthenticated, 404 no-pending with auth, 200 empty list). Router builds with no merge/panic.

### Findings

None blocking.

Minor (not blocking, recorded for completeness):

- The `000058_invitations/up.sql` header comment says "Unique constraints enforce one PENDING invitation per (tenant, email) / (workspace, user); the partial index keeps re-invite after terminal states possible" — but the SQL creates only the unique `token_hash` index and non-unique `(tenant_id, status)` indexes; no partial unique index exists. The pending-uniqueness is actually enforced by check-then-insert in all three adapters (the layer the port contract names; verified by `check-mem-port-contracts` and round-trip tests). Consequence of the missing DB-level constraint is a narrow create-race allowing a duplicate pending invitation — benign downstream (accept is idempotent per user, single-use per row, expiry reaps). Recommend fixing the comment (or adding the partial unique index) opportunistically.
- `require_matching_invitee` compares email only, not tenant; token possession (256-bit CSPRNG) is the primary auth factor, so this is defense-in-depth only.
- The workspace-invitation in-app notification reuses `NotificationType::TrustSuggestion`; the discriminating `kind: workspace_invitation` lives in the body JSON. Cosmetic.
