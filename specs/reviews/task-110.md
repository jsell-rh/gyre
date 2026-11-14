# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `user-management.md` §Tenant-Level User Onboarding, §Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate: `aa92dbe9` on assignment base `9afd66c7` (merge `f56a47df` carries the base's task-133 ABAC MCP permission resources; the only Rust diffs beyond the prior CI-tested tree `a8996690` come from that merged base — every invitation file, `auth.rs`, `users.rs`, and the migrations are byte-identical to `a8996690`, verified with `git diff a8996690..aa92dbe9 -- <invitation paths>` → empty).
Verdict: **approved** (independent review round; probes and evidence under `/tmp/stage/review-evidence/`).

## What was reviewed

Full diff base→candidate (~7.8k lines): domain entities (`gyre-domain/src/invitation.rs`), ports (`gyre-ports/src/invitation.rs`), SQLite + PostgreSQL + mem adapters, migrations `2026-11-13-000063/64/65` (+ carried task-124 `000066` renumber), `gyre-server/src/api/invitations.rs` (handlers + 28 tests), `auth.rs` SSO email-link, `users.rs` (task-124's membership-row invitation stubs correctly removed in favor of the real `WorkspaceInvitation` model), route registration, ABAC mappings, background expiry job, mem wiring (`store!` pattern), docs.

## Verified working (independent evidence)

- **Domain/ports/adapters are real.** `TenantInvitation`/`WorkspaceInvitation`/`InvitationStatus`/`InvitationPolicy` carry every spec field; both repository ports implemented in SQLite (Diesel), PostgreSQL, and mem with the port contracts enforced in all three: pending-scope duplicate guard as check-then-insert **plus** DB-level partial unique indexes (migration `000064`, with pre-index raced-duplicate dedupe), and `transition_status` as a conditional `UPDATE ... WHERE status = from` (mem: check-and-set inside one lock critical section) — the single-use acceptance guard. `cargo test -p gyre-adapters` (full): **533 passed, 0 failed** (6 ignored are credential-gated). `cargo test -p gyre-domain`: **539 + 8 passed**. `cargo check -p gyre-server`: clean.
- **Endpoints.** All five specced tenant endpoints plus policy GET/PUT, bulk invite (per-entry errors without batch abort), workspace invite/list/revoke, token + authenticated accept/decline, and the invitee's `GET /users/me/invitations` — registered in `api/mod.rs` behind auth+ABAC (magic-link accept/decline correctly on the public router with the 256-bit token as auth factor and per-handler identity matching when a Bearer is presented). `check-abac-route-registry.sh` and `check-abac-exempt-handlers.sh` pass. The 28 `api::invitations` tests exercise the full HTTP routing chain (not repo-level mirrors): lifecycle, single-use 409, revocation blocking accept, expiry (endpoint + `run_expiry_once`), bulk, non-admin 403, cross-tenant 403 fixtures, cap 429, concurrency (exactly one claim winner), CSPRNG/hash KAT (SHA-256("abc")), policy fail-closed, SSO link, local-mode key mint, username contract.
- **Token security.** 32-byte `ring::SystemRandom` CSPRNG, stored only as SHA-256 hex; token appears only in the create response (`invite_url`), never in listings.
- **SSO link.** `find_or_create_user_tenant` claims an invitation-created account only when `email_verified=true` and the tenant matches; `claim_external_id` is the sole sanctioned external_id transition, refusing taken ids; cross-tenant and unverified-email negative tests present.
- **Background expiry.** `spawn_invitation_expiry` wired in `main.rs`; conditional Pending→Expired transitions never overwrite terminal statuses; zero-interval env misconfig falls back instead of panicking.
- **Mechanical gates** (all pass; logs in evidence dir): arch, hierarchy (skipped by design), migration versions, migration SQL portability, mem-port contracts, ABAC registry, ABAC exempt handlers, inert enforcement, forged scope fields, scope-literal/fabricated defaults, lossy secrets, byte-slice truncation, relative path defaults, forwarded-header trust, unbounded external HTTP, dead message kinds, unwired port methods, in-memory state stores, fail-open ref resolution, task commit attribution (all nine frontmatter SHAs verified in-branch via `merge-base --is-ancestor`). `check-rustfmt-diff.py 9afd66c7`: 22 changed Rust files clean.
- **Durable CI finding 7ec83a8d resolved.** GitHub API: run `43610196702` (head `a8996690`) completed `conclusion: success` on attempt 2 — every job green, including `cargo nextest run --all` (Test), Development controller, Format, Web build. Attempt-1's failure did not reproduce on the identical tree. The two `test_pipeline*.py` failures that DO reproduce in this sandbox (`test_real_timeout_kills_check_and_records_infrastructure_code`, `test_crash_cleanup_stops_durable_agent_before_capturing_source`) fail **identically on the assignment base** (worktree probe, evidence saved) — sandbox `os.killpg` restriction, not a code defect.

## Verifier must run (blocked in-sandbox by the 4 GiB build wrapper)

- `cargo test -p gyre-server --lib api::invitations` — 28 tests.
- `cargo test -p gyre-server --lib auth::tests` — invitation email-link tests (`jwt_email_link_claims_invitation_account`, `jwt_unverified_email_does_not_claim_invitation_account`, `jwt_email_link_does_not_cross_tenants`).
- Full `cargo test --all` and exact-head GitHub checks (CI already green at `a8996690`, whose invitation tree is byte-identical).
- `python3 scripts/check-clippy-diff.py 9afd66c7` (clippy needs a workspace build; deferred).

## Non-blocking observations (recorded, not findings)

- `specs/coverage/SUMMARY.md` regeneration drift on rows owned by the merged base (abac-policy-engine committed 3/14 vs regenerated 1/16; agent-gates, analytics similar). Inherited from the base's own stale SUMMARY (base matrix already had 16 implemented vs SUMMARY's 6); this task's user-management row regenerates byte-identically. A mechanical `update-coverage-summary.sh` run at integration fixes it.
- Migration `2026-11-13-000065_user_tenant_scope/up.sql` retains an older renumber-note paragraph saying "renumbered to ... 000060" beneath the newer note explaining the 000065 move — cosmetic comment staleness only; the version collision handling itself is correct and `check-migration-versions.sh` passes.

## Verdict

Real implementations throughout — no stubs, no audit-only enforcement, no test inflation found; acceptance criteria 1–9 verified with independent evidence, criterion 10 (`cargo test --all`) CI-proven at the byte-identical product tree and deferred to the verifier at this exact head. **Approved.**

— Reviewer, 2026-11-14
