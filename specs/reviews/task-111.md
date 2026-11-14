# Review — task-111: Implement session management

**Candidate:** `a139b7766ba733d78ce04ef801c3f2df296bd8ac`
**Base:** `bfc36c9c03b5baddaec30f204feb18ac5981b7d2`
**Round:** independent review of the base-merge repair round (`fd4351b9` on merge `92ed0c5d`)
**Verdict:** APPROVED

## Scope reviewed

Full diff base..candidate (59 files, +3951/−348), focused on the
user-management.md §Session Management contract and the three merge-fallout
repairs described in the shipped notes. Probe evidence:
`/tmp/stage/review-evidence/review-summary.md`.

## Spec contract vs implementation

| Requirement | Evidence |
|---|---|
| `UserSession` entity, all spec fields | gyre-domain/src/user_profile.rs — exact field set; only SHA-256 of the API key is ever stored |
| `SessionRepository` port | gyre-ports/src/user_profile.rs — all planned methods plus `find_by_credential_and_device`; port contracts documented per-method |
| SQLite/PG/mem adapters | sqlite/postgres user_profile.rs implement every method with the same semantics; mem guards duplicate-id in code (check-mem-port-contracts PASS) |
| Migration | `2026-10-08-000064_user_sessions` — unique sequence (check-migration-versions PASS), portable SQL (check-migration-sql-portability PASS), indexes on user_id/token_hash/expires_at |
| Sessions created on auth, throttled `last_active_at` | `track_session` in auth.rs: find-or-mint keyed on (credential hash, IP, User-Agent); touch gated on `now − last_active_at ≥ 60s`; the throttled write also slides `expires_at` (sliding TTL) |
| Revoked sessions rejected in auth | Two layers, both enforced on the HTTP extractor AND the WS/TTY `authenticate_token` path: per-device (`track_session` returns false on a revoked device row) and credential-wide (`credential_revoked` — "sign out everywhere", including never-seen devices) |
| GET /users/me/sessions | Active-only filter (`is_active`), token hash never in the response (test asserts absence) |
| DELETE /users/me/sessions/{id} | Scoped: another user's session id → 404 (tested) |
| POST /users/me/sessions/revoke-all | Signs the credential out everywhere, including a device never seen before the revoke (dedicated stolen-key test) |
| TenantAdmin view/revoke any user's sessions | Admin role check + `ensure_same_tenant_session_target` tenant containment (fail-closed on unbound target); cross-tenant admin → 403 on all three verbs (tested); unknown user → 404 |
| Background cleanup | `spawn_session_cleanup` spawned in main.rs; daily; deletes only unrevoked rows expired >30 days — revoked rows are the durable sign-out record (tested: cleanup must not re-authorize a signed-out credential) |
| Routes + ABAC | Registered in api/mod.rs; resolver-exempt entries in abac_middleware.rs; per-handler authorization present (check-abac-route-registry and check-abac-exempt-handlers PASS) |
| IP extraction | Socket peer via ConnectInfo only; forwarded headers not consulted (check-forwarded-header-trust PASS) |

## Test quality

The gyre-server session tests drive the real router with real bearer keys
and assert HTTP status codes (401 after revoke, 403 cross-tenant, 404
cross-user) and store state — behavior-coupled, not mirrored logic. The
throttle test asserts both the write (stale session) and the non-write
(fresh session) sides. The WS-path tests have live-credential controls so
the rejection is attributable to revocation, not setup. Adapter tests
assert DB round-trips including the duplicate-id failure contract.

Server test *execution* is blocked in-sandbox by the 4 GiB build wrapper
(restriction, not defect); the exact required commands are recorded in
review-evidence. `cargo check -p gyre-server --lib` is clean;
`cargo test -p gyre-adapters --lib` (530 pass, includes all session and
analytics adapter tests) and `cargo test -p gyre-domain` (518 pass) were
run independently for this round.

## Merge-fallout repairs (this round)

1. **MergeRequest::new call sites** — all pass `workspace_id` at
   construction; production sites resolve it from the repository record
   (spawn.rs, mcp.rs refuse to create an MR whose scope cannot be resolved
   instead of fabricating one). Test fixtures adapted consistently.
2. **Migration renumber** — `user_sessions` moved to unique sequence
   000064; no collision (gate PASS).
3. **Analytics scope literals** — fixed for real: rows carry the storage
   handle's resolved `self.tenant_id` (same accepted form as the earlier
   merge_request/repository repairs); all six exemption entries deleted
   and FROZEN_EXEMPTION_COUNT lowered 6→0. The check script additionally
   gained a no-arg default scan of crates/ (previously a no-arg invocation
   scanned zero files vacuously) — a strengthening consistent with the
   file's documented fix-and-delete policy.

## Mechanical gates

All 90 check scripts run at the candidate and at the base (worktree). The
34 that fail do so identically at both trees (pre-existing/environmental:
missing args, node_modules, shell arithmetic quirks); output content is
identical for every source-scanning check. The branch introduces zero new
gate failures; the 13 gates most relevant to this diff all PASS.

## Non-blocking observations

- `track_session`/`credential_revoked` fail-open on session-store errors
  (log + allow) — documented in-code tradeoff: the credential was already
  validated against api_keys, and failing closed would turn a store outage
  into a platform lockout.
- `credential_revoked` scans all of a user's sessions per API-key request
  (performance, not correctness).
- `SessionRepository::find_by_token_hash` has no production caller; the
  task plan mandates the method and adapter tests exercise it.
- Generated `web/dist` not rebuilt by the agent — owned by the
  deterministic verifier; no web/ source changes are in the diff.
