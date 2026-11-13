# Review: task-111 — Implement session management

**Candidate:** `ece2bbf113003052d3920741dd890d980b14026d`
**Base:** `6f5a631c9fc09664a3e766cd10536f8b56d9ce2f`
**Spec:** `specs/system/user-management.md` §Session Management
**Verdict:** APPROVED (round 1)
**Evidence:** `/tmp/stage/review-evidence/evidence.md`

## Scope reviewed

Diff base→candidate: `UserSession` domain entity, `SessionRepository` port +
SQLite/PostgreSQL/mem adapters, migration `2026-10-08-000063_user_sessions`,
auth-middleware session tracking + revocation enforcement (HTTP and WS/TTY
paths), six session API endpoints with per-handler authorization, daily
retention cleanup, and the verification-repair changes absorbed from prior
rounds (7-arg `MergeRequest::new` with real workspace ids, tenant-scoped
`RepoRepository::create`/`MergeRequestRepository::create`, migration
renumbering, exemption-count reduction 12→6).

## What was independently verified

- **Compilation of the exact prior-failure target:** `SKIP_WEB_BUILD=1 cargo
  check -p gyre-server --lib --tests` passes (cold build, 4m25s) — durable
  finding `08f72f21` (E0061/E0277 in gate_executor.rs test fixtures) is
  repaired. gyre-adapters lib+tests also type-check (covers the PostgreSQL
  session implementation — the module is not feature-gated).
- **Real storage behavior:** `cargo test -p gyre-adapters --lib session` —
  8 SQLite session-repository tests pass against the real embedded Diesel
  migration (roundtrip, duplicate-id create fails, device-tuple match,
  newest-match, revoke scoped to owner, revoke-all, retention never deletes
  revoked rows, touch slides expiry).
- **Exemption-freeze repairs are real, not paperwork:** the killing tests
  `create_is_scoped_to_the_handles_tenant` (repository.rs) and
  `create_persists_under_the_handles_tenant_scope` (merge_request.rs) pass —
  a tenant-scoped handle sees its own creations, the default-scoped handle
  does not. The frozen exemption count went 12→6 with the underlying sites
  fixed (the documented allowed direction); production `MergeRequest::new`
  call sites pass looked-up `repo.workspace_id`.
- **Mechanical gates:** all 20 AGENTS.md-enforced check scripts pass at head.
  `check-task-commit-attribution` fails only against the working tree because
  the executor restored the assigned task contract (`commits: []`); every
  flagged commit is present in the candidate commit's frontmatter. Other
  failing advisory scripts were spot-checked against base — flagged sites
  exist verbatim at base (pre-existing, not candidate-introduced).
- **Auth-middleware integration (source inspection):** sessions are created
  on successful API-key auth with IP from the `ConnectInfo` socket peer only
  (no forwarded-header trust — CWE-348 invariant respected); `last_active_at`
  writes throttled to ≥60s and slide `expires_at`; a revoked device session
  rejects auth with 401; `credential_revoked` enforces "sign out everywhere"
  including devices never seen before; both the HTTP extractor and the
  WS/TTY `authenticate_token` path enforce both checks.
- **Endpoints:** handlers scope by caller (foreign session id → 404, not
  silent success); TenantAdmin variants check Admin role AND tenant
  containment (fail-closed on missing tenant binding; system principal
  exempt like `create_user`); unknown user id → 404 (no existence probing);
  listings are active-only and never expose `token_hash`.
- **Durability:** `sessions` is wired through the port-backed `store!` macro
  (SQLite/PG in DB mode), not a bare in-memory map; daily
  `spawn_session_cleanup` deletes only unrevoked rows expired >30 days, so a
  signed-out credential can never be re-authorized by cleanup.
- **Test quality:** the gyre-server session tests (reviewed in source; cannot
  execute under the review-stage build wrapper) are behavioral — real router
  + real auth extractor + real session store — covering per-device vs
  sign-out-everywhere semantics, cross-device isolation, listing filters,
  WS-path enforcement with live-credential controls, cross-tenant admin
  containment, non-admin 403, and unknown-user 404. Adapter-level killing
  tests were executed here and pass.

## Non-blocking observations

1. **Fail-open on session-store errors** (`track_session`/`credential_revoked`
   log and allow): a documented availability tradeoff; the credential itself
   is validated against the separate `api_keys` store, and in both DB modes
   the session store and api_keys share the same database, so the window
   where one fails and the other succeeds is negligible. Acceptable.
2. **API-key sessions only:** JWT (Keycloak) and agent-JWT auth paths do not
   mint user sessions. The spec's `token_hash` = SHA-256 of a stored session
   token maps to API keys (the user credential mechanism); JWTs are rotating
   signed assertions with nothing to hash-store. Defensible reading, noted
   for the record.
3. **Generated output:** `web/dist/` untouched by the candidate; no web
   source changes (the spec section is API-only). Verifier owns the bundle.

## Deferred to the independent verifier

gyre-server test execution exceeds the review sandbox's 4 GiB build wrapper:

1. `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib`
2. `SKIP_WEB_BUILD=1 cargo test --all`
3. `bash /tmp/stage/checks.sh` (the previously failing verification command)
4. `cargo test -p gyre-adapters --lib` (full; session/scoping subsets already
   executed in review, see evidence)

Approval is based on: focused executed tests against real storage, clean
type-check of the full server lib+test target, all mechanical gates passing,
and source-level verification of the enforcement paths above. The deferred
commands must still be run by the verifier before publication.
