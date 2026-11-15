# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding, §Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Base: `81c24bf07b059128fe6019c1df6f1c6924299930` — Candidate: `9de867d260930a6bd02ba6cb864947d3bc3754b8`.
Verdict: **approved** — no blocking findings. Independent evidence supports every acceptance criterion; the full workspace suite (`cargo test --all`) remains assigned to the independent verifier per the 4 GiB build-budget protocol (commands recorded below).

## What was reviewed

The full diff base→candidate (~9,200 insertions across 38 files): domain entities (`gyre-domain/src/invitation.rs`), ports (`gyre-ports/src/invitation.rs`, `user.rs` additions), all three adapters (SQLite `sqlite/invitation.rs`, PostgreSQL `postgres/invitation.rs`, mem `server/src/mem.rs`), migrations 000063–000066, the API layer (`server/src/api/invitations.rs`, 5,673 lines incl. 37 tests), route registration (`api/mod.rs`, `lib.rs` public magic-link router), ABAC mappings and exemptions (`abac_middleware.rs`), the SSO email-link claim path (`auth.rs`), token redaction (`middleware.rs`), the expiry job (`main.rs` spawn), user-repository extensions (`find_by_email`, `claim_external_id` with immutability-preserving semantics), docs, and coverage-matrix updates.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain invitation` → 4/4 passed.
- `cargo test -p gyre-adapters invitation` → 5/5 passed (real SQLite storage: roundtrips, pending-duplicate contract, single-use transition guard, DB-level partial-unique-index enforcement incl. the token-hash-misclassification negative, migration-000064 raced-duplicate dedupe).
- `cargo test -p gyre-adapters user::` → 15/15 passed (`claim_external_id_is_the_sanctioned_repoint`, `find_by_email_is_case_insensitive_and_returns_all_matches`, username immutability).
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → both clean (lib and full test module type-checked).
- All 24 relevant repo invariant scripts → pass. `check-task-commit-attribution.sh` fails only against the stage-reset working tree; on the exact candidate commit (verified in a temp worktree) it passes. `check-non-atomic-creation.sh` reports one violation in `git_http.rs::process_persona_changes` that is pre-existing at the assigned base (task-137 lineage, file untouched by this candidate) — not a candidate finding.
- **Mutation probes (temp worktree at the candidate commit, restored afterwards):**
  1. Removing the `status = from` precondition from the tenant `transition_status` UPDATE → `tenant_invitation_roundtrip` FAILS ("a claimed invitation cannot be re-claimed"). The single-use acceptance guard is real and test-covered.
  2. Making `is_tenant_pending_scope_violation` match any unique violation → `pending_scope_unique_index_enforced_by_database` FAILS (token-hash misclassification caught). The discriminator regression the tests warn about is actually detected.

## Verified

- **Contract completeness.** `TenantInvitation`/`WorkspaceInvitation`/`InvitationStatus`/`InvitationPolicy` match the spec structs field-for-field; both repository ports exist; SQLite + PostgreSQL + mem all implement them with the same duplicate-pending and conditional-transition contracts (DB-level partial unique indexes as race backstop in both SQL adapters, check-and-set under lock in mem).
- **All five specced tenant endpoints functional** plus decline, policy GET/PUT, workspace invite/list/revoke, invitee token accept/decline, authenticated invitee accept/decline, and `GET /users/me/invitations`. Routes registered in `api/mod.rs` (authed) and `lib.rs` (magic-link, token as auth factor, `Option<AuthenticatedAgent>` self-extracting) with ABAC mappings; invitee routes are resolver-exempt with genuine per-handler authorization (tenant containment + caller-must-be-invitee), proven through the production middleware chain by a test that first establishes the 403 control on a workspace-mapped route.
- **Token security.** 32-byte ring `SystemRandom` CSPRNG, SHA-256 known-answer test, only the hash persisted, token redacted from logs and (unauthenticated) `/metrics` path labels.
- **Expiry.** `run_expiry_once` + `spawn_invitation_expiry` (hourly, env-overridable, zero-value fallback) with conditional Pending→Expired transitions that never overwrite terminal statuses; accept paths also expire past-due tokens eagerly.
- **Accept flow correctness.** Single-use claim precedes side effects with compensating rollback to Pending; local-mode user creation with username contract (validated pre-claim, 400/409), tenant-namespaced external_id; SSO link of a same-tenant same-email account; deactivated-account rejection on every accept path; first-API-key credential delivery only for invitation-owned accounts (null for independently-bound ones — token possession must not bypass an existing IdP factor); cross-tenant pre-assignment guarded.
- **Policy enforcement is real.** `load_policy` fails closed on corrupt rows; `allow_re_invite: false` blocks terminal-state re-invites on both surfaces (tested both); `max_pending_invitations` cap enforced (429); per-request expiry override validated.
- **Tests are not inflated.** HTTP-layer tests drive real routers via `tower::ServiceExt::oneshot` with real API keys and seeded builtin ABAC policies including negative controls (non-admin 403, non-member 403, cross-tenant 403, mismatched-bearer 403, revoked-blocks-accept, concurrent-claims exactly-one-winner). Both of my independent mutations were killed.
- **Docs and coverage** updated coherently (`api-reference.md`, `server-config.md`, coverage rows 5/6/7/25 → implemented with honest "pending independent review" notes; row 27 records the task-124 parallel-implementation removal and its justification — axum duplicate route registration).

## Generated-output observation (non-blocking, verifier-owned)

`web/dist/` was not regenerated in this diff. This task changes no authored web source (no `web/` files in the diff), so there is no source/bundle drift introduced by the candidate; bundle regeneration is owned by the deterministic verifier, which rebuilds and commits assets after integration.

## Verifier must run (deferred per 4 GiB protocol)

```
cargo test --all                                  # acceptance criterion
cargo test -p gyre-server --lib invit             # 37 api::invitations tests + auth SSO-link tests
cargo test -p gyre-server --lib invitee           # ABAC-middleware invitee-route exemption test
cargo test -p gyre-server --lib redact            # token-path redaction in middleware
```

All other acceptance criteria were independently verified in this review.
