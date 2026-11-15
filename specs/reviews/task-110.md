# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding, §Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate under review: `45129cf9fd1ede224059a2b8694c183cae23c3b5` (base `9fb371ac`).
Verdict: **needs-revision** — two enforcement/defect findings (F1, F2) plus one same-family gap (F3); the remainder of the implementation is genuinely strong and verified clean by focused probes.

## Round N (this review)

Probes run (evidence: `/tmp/stage/review-evidence/`):

- `/tmp/stage/bin/cargo test -p gyre-domain invitation` → 4/4 passed.
- `/tmp/stage/bin/cargo test -p gyre-adapters invitation` → 5/5 passed (real SQLite DB, embedded shared migrations; includes the 000064 dedupe + partial-unique-index tests).
- `/tmp/stage/bin/cargo check -p gyre-server` → clean.
- Mechanical gates: check-arch, check-abac-route-registry, check-abac-exempt-handlers, check-migration-versions, check-mem-port-contracts, check-migration-sql-portability, check-forged-scope-fields, check-in-memory-state-stores, check-unwired-port-methods, check-inert-enforcement, check-scope-literal-defaults and the rest of the standing set → all pass. (Infra note: check-tenant-filter / check-transactional-side-effects / check-crypto-verbose use gawk-only regex `match($0, /re/, arr)` and error under this sandbox's mawk — verifier should re-run them; not a code defect. check-non-atomic-creation flags `git_http.rs::process_persona_changes`, which is untouched by this diff — pre-existing on base.)
- Diff inspection of all 37 changed files (domain, ports, sqlite/postgres/mem adapters, migrations 000063-000066, api/invitations.rs in full, api/mod.rs, abac_middleware.rs, auth.rs, users.rs, lib.rs, main.rs, mem.rs, docs).

Verified working (no findings):

- **Domain/ports**: `TenantInvitation`/`WorkspaceInvitation`/`InvitationStatus`/`InvitationPolicy` match the spec structs exactly (all fields, all five status variants, defaults 7/7/50); `is_expired` correctly gates on Pending and treats `now == expires_at` as expired; hexagonal boundaries clean.
- **Adapters**: all three adapters (SQLite, Postgres, mem) implement the full port traits with matching semantics — same filters, same newest-first ordering, conditional `transition_status` writes (atomic single-use claim; `Ok(false)` on wrong source status), pending-scope duplicate enforcement everywhere (check-then-insert + partial unique index `WHERE status='Pending'` in SQL, in-lock check in mem). Migration numbering 63-66 is the next unused sequence; the task-124 `user_deactivated_at` renumber to 000066 is correctly documented.
- **API**: all five tenant endpoints plus workspace invite/list/revoke and invitee accept/decline are implemented and registered; ABAC mappings present with the two invitee routes deliberately resolver-exempt (frozen exemption files untouched) and carrying real per-handler authorization (`authorize_my_pending_invitation`: tenant containment 403 + caller-must-be-invitee 404). Tokens are 32 bytes from `ring::rand::SystemRandom`, persisted only as SHA-256, returned only in the create response, never logged; acceptance is single-use via the conditional transition with best-effort rollback to Pending. `require_tenant_admin` + caller-derived tenant scoping on every admin route; cross-tenant guards tested. Expiry: background job spawned at main.rs:156 (hourly interval, env-overridable, real repository calls, conditional transitions) plus lazy expiry on every accept/decline path; expired invitations cannot be accepted (409, status persisted). `max_pending_invitations` enforced on workspace invite (429) with per-tenant policy endpoints (fail-closed on corrupt policy rows). Local-mode accept mints a real authenticating API key (hash-persisted); SSO-mode links the existing account instead of duplicating, and suppresses key minting for independently-bound accounts.
- **Tests**: behavioral and adversarial — HTTP-level negative authz probes with state assertions, replay/single-use 409s, token non-leak, concurrency (exactly one claim winner), the ABAC-middleware-chain regression includes a control request proving the middleware deny actually fires before demonstrating the exemption path, cross-tenant scenarios, SSO-link vs parallel-account. No test inflation found.

Findings:

- [-] **F1 (major, auth.rs ~2130-2160): deactivated account re-authenticates via the SSO email-link claim branch.** The candidate-introduced claim branch of `find_or_create_user_tenant` claims an invitation-created local account when a verified JWT email matches, without the `is_deactivated()` check every sibling auth path enforces (auth.rs:1513, :1811, :2086 — the last with the comment "deactivation ... outlives the user's IdP session"). Repro: invite → local-mode accept → admin deactivates → SSO login with matching verified email → live authenticated session. Fix is two lines mirroring :2086. Evidence: `deactivated-claim-path.txt`.
- [-] **F2 (major, migration 000063): INTEGER timestamp columns vs BigInt schema — the PostgreSQL read path fails at runtime.** Both invitation tables declare `expires_at/created_at/accepted_at INTEGER`; schema.rs and both adapters' rows use `BigInt`/`i64`. PG documents int4 as a 4-byte binary value; diesel 2.3.7's `FromSql<BigInt, Pg> for i64` rejects <8 bytes ("Was an Integer expression accidentally marked as BigInt?"), so on the documented `postgres://` backend every invitation read errors while SQLite tests stay green — the repo's own shared-migration failure class (its convention, including this diff's 000066, is BIGINT for timestamps). Note the same mismatch pre-exists on main for `spec_links` — pre-existing condition, not a candidate regression, but the candidate replicates it for new tables. Fix: BIGINT columns (+ repair consideration for already-migrated PG databases). Evidence: `pg-integer-mismatch.txt`.
- [-] **F3 (minor, same family as F1, api/invitations.rs): acceptance never checks deactivation.** `accept_tenant_invitation_claimed` re-applies invited roles and activates memberships on a deactivated account and mints a key for it (the key dead-ends at the extractor, but account state is mutated; combined with F1 the invitee reaches a live session). Reject when the matched account `is_deactivated()`.

Non-blocking observations (recorded for the revision round, none gate approval):

- Pre-assigned `workspace_ids` are validated only at accept time (skipped + warned on tenant mismatch — fail-closed, but a 400 at invite time would be tighter).
- SSO-link acceptance applies the invited role to an existing account (within the inviting TenantAdmin's power, but the accepting token holder triggers it — deliberate product decision recommended).
- The workspace invite cap counts expired-but-unmarked Pending rows until the hourly job runs (availability-only, self-healing).
- Bulk invite has no batch-size bound and is O(n²) over `list_by_tenant` (TenantAdmin-only).
- Stale comments cite "migration 000061" for the partial unique index (actual: 000064) in invitations.rs:315, sqlite/invitation.rs:183/408/795, postgres/invitation.rs:181/408.
- The four token-based magic-link routes are registered in lib.rs outside `require_auth`/ABAC (possession of the 256-bit token is the auth factor, per spec); they are invisible to the mechanical route-registry scans by construction — their security was verified here by direct code inspection (token hashing, single-use claim, expiry, invitee-match guard when a Bearer identity is presented).
- No PostgreSQL test exists anywhere in the repo (how F2 ships invisibly); a PG smoke test would be the durable fix.

Test-inflation check: none found — the strongest tests drive the real router/middleware chain with control requests and state assertions.

Deferred to verifier (4 GiB budget): `cargo test -p gyre-server invitation`, `cargo test --all`, and a PG-backed invitation read probe to confirm F2 empirically (expected deserialize failure per diesel 2.3.7 source).
