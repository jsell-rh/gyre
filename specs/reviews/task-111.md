# Review — task-111: Implement session management

Round: final (this round reviewed candidate `56a373e` — the fixture-repair round for
verification finding `f182c1c1`; prior rounds reviewed the session implementation
itself and are summarized in `specs/tasks/task-111.md` "Shipped").

Reviewer scope: `git diff 6f5a631..56a373e` (the task-111-authored delta; see
"Base lineage" below) plus the full candidate tree at HEAD `56a373e`. Evidence
persisted at `/tmp/stage/review-evidence/` (README.md indexes every probe, command,
exit code, and log).

## Verdict

**Approved.** The session-management implementation is real, production code
satisfying every acceptance criterion of the task and `user-management.md`
§Session Management; no fakery, no missing enforcement, no unsafe scope handling,
no weak tests found.

## What was independently verified

- **Domain entity** (`gyre-domain/src/user_profile.rs`): `UserSession` with all
  spec fields; `token_hash` holds only `hash_api_key` output (SHA-256) — plaintext
  never persisted; `is_active(now)` = !revoked && expires_at > now.
- **Port** (`gyre-ports/src/user_profile.rs`): `SessionRepository` with every
  planned method plus a justified `find_by_credential_and_device` (one session row
  per credential+device so revoking one device does not sign out every device).
  The port contract "create fails on duplicate id" is enforced by all three
  adapters — SQLite/PG via PK constraint, mem via explicit guard
  (check-mem-port-contracts.sh PASS).
- **Adapters**: SQLite and PostgreSQL implementations are real diesel queries
  (verified by reading and by running the 12 `sqlite::user_profile` tests against
  real SQLite storage — all pass, including duplicate-id failure, newest-match
  semantics, scoped revoke, sliding-expiry touch, and retention-never-deletes-
  revoked-rows). `MemSessionRepository` mirrors the contracts in code.
- **Migration** `2026-10-08-000063_user_sessions`: next unused sequence 000063,
  portable SQL (CREATE TABLE IF NOT EXISTS / BIGINT / INTEGER, no dialect-only
  constructs); check-migration-versions.sh and check-migration-sql-portability.sh
  both PASS.
- **Auth integration** (`auth.rs`): on successful API-key auth, both the HTTP
  extractor (ConnectInfo socket-peer IP, User-Agent header; no forwarded-header
  trust — CWE-348 compliant) and the WS/TTY `authenticate_token` path (empty
  device tuple) run `credential_revoked` (rejects a key whose every session is
  revoked — "sign out everywhere", including never-seen devices) then
  `track_session` (finds or mints the per-device session; throttled ≤1/min
  `last_active_at` write that also slides `expires_at`; a revoked per-device row
  rejects with 401; an expired-but-live row is re-minted, not zombified).
- **Endpoints** (`api/users.rs`): all three `/users/me/sessions` routes plus the
  three TenantAdmin variants. Listings are active-only and never expose
  `token_hash`. `/me` revokes are caller-scoped (foreign session id → 404).
  Admin variants enforce Admin role (= `GlobalRole::TenantAdmin` per the realized
  model at users.rs:1834), target-user existence (404), and tenant containment
  (`ensure_same_tenant_session_target`, fail-closed on missing tenant binding;
  system principal exempted like `create_user`). Routes registered in
  `api/mod.rs`; ABAC resolver entries present (exempt + per-handler authz,
  matching the notifications pattern); all 20 mechanical gate scripts PASS.
- **Background cleanup**: `spawn_session_cleanup` (daily, spawned in main.rs)
  deletes only unrevoked sessions expired >30 days; revoked rows are never
  deleted (they are the durable sign-out record).
- **Tests are killing tests, not inflation**: 17 gyre-server tests
  (per-device vs sign-out-everywhere on both HTTP and WS paths with live-control
  cases, listing filters with directly seeded revoked/expired rows, throttle
  window both sides, re-mint single-row invariant, cross-tenant Admin Forbidden
  on all three admin verbs, non-admin Forbidden, unknown user 404, foreign
  session id 404) + 8 adapter tests. Each carries the "removing X makes this
  fail" rationale in its doc comment.
- **This round's repair** (`db116ab7`, finding `f182c1c1`): 4 test-fixture lines
  adding `seed_repo(&state).await` to the four base-drifted merge_requests tests;
  no product code. Causal chain verified: `create_mr` 404s unknown repositories
  (merge_requests.rs:285, M34 repair 74749334), the tests panicked unwrapping
  `json["id"]`. The prior full-suite run at the same product tree passed all
  session tests (2055 passed / 4 failed — exactly these four).
- **Coverage matrix** row 12 update is justified by the above; task frontmatter
  records all 18 task-labeled commits (check-task-commit-attribution PASS at the
  committed tree — the working-tree failure is the controller's assignment reset
  of `specs/tasks/task-111.md`, restored untouched after probing).

## Probes run (see /tmp/stage/review-evidence/)

- All 20 `scripts/check-*.sh` gates: PASS.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib`: exit 0.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests`: exit 0.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib sqlite::user_profile`:
  12 passed / 0 failed.
- `rustfmt --edition 2021 --check` over the seven session files: clean.

## Base lineage (integration note — not a candidate defect)

The assigned base `6ffd6799` (task-130 token scoping) is NOT an ancestor of the
candidate; the branch's last merged base is `6f5a631c` (task-195), which is what
the previous verification round ran against. No candidate commit deletes task-130
work — the diff from the assigned base merely shows its absence. Integration with
`6ffd6799` is a controller/verifier stage; both lines modify `auth.rs` from the
common ancestor, so the merge needs conflict resolution that preserves BOTH
task-130's `AgentJwtClaims` hierarchy claims/scope list AND task-111's session
tracking on the API-key paths.

## Required in the independent verifier

1. Merge the candidate with assigned base `6ffd6799`; confirm task-130's
   `AgentJwtClaims { tenant_id, scope: Vec<String> }` hierarchy claims AND
   task-111's session tracking coexist in `auth.rs` (both compile and both test
   suites present).
2. `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::merge_requests` —
   the four repaired fixture tests of finding `f182c1c1` must pass.
3. `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib auth::` and
   `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::users` — the session
   tracking/endpoint suites.
4. `SKIP_WEB_BUILD=1 cargo test --all` — repo-wide acceptance criterion.
5. `bash scripts/check-*.sh` full gate suite at the integrated tree.
