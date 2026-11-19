# Review — task-265 (SCIM user lifecycle completion: PATCH, deactivation semantics, enterprise extension, production wiring)

Spec: `specs/system/identity-security.md` §User Identity / SCIM Provisioning (gap spec-31d3ae72e12e22c52442e309 / scim-users-lifecycle / b60edce5bc49).
Candidate under review: `25733fb00cf7c302f1b564d23815ce4ea3ef58c6` (base `bafc03531aec9c9ddd05c52497aa10d0d8856fe5`).
Product commits: `156d268f` (implementation), `d835ff67` (review repair: RFC 7644 `Operations` key), `ed7b5a88` (verification repair: persist `spawned_by` in SQL agent update); `3cf47643`/`d58ea0e4`/`65244c68`/`e3723c02`/`25733fb0` docs/process.
Verdict: **needs-revision** — one code finding (F1, audit/SIEM pollution on the production SCIM path); all five requirement areas otherwise verified real.

## Round 1

Probes run (evidence in `/tmp/stage/review-evidence/`; `cargo check`/adapters tests run at candidate HEAD):

- `bash scripts/check-abac-route-registry.sh` → OK (PATCH added to the existing `/scim/v2/Users/:id` route; resolver-exempt entries cover it).
- `bash scripts/check-abac-exempt-handlers.sh` → OK (86 handlers; every SCIM handler opens with `check_scim_auth(&headers)?` — 9 handlers, 9 call sites).
- `bash scripts/check-migration-versions.sh` → OK (`2026-11-13-000067_user_manager_id` is the next unused number after 000066).
- `bash scripts/check-migration-sql-portability.sh` → OK (plain `ALTER TABLE users ADD COLUMN manager_id TEXT`, portable to both backends).
- `bash scripts/check-mem-port-contracts.sh` → OK.
- `cargo test -p gyre-adapters sqlite::agent` → **19 passed, 0 failed**, including the new `update_persists_spawned_by_attribution_changes` regression (Some→None and None→Some round trips — this is the exact bug verification run `85ea51e0` caught, fixed in `ed7b5a88`).
- `cargo test -p gyre-adapters sqlite::user::tests::enterprise_manager_id_round_trips` → **1 passed**.
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → clean (the new `tests/scim_lifecycle.rs` type-checks; full test-binary compilation deferred to the verifier per the 4 GiB budget rule).

Verified working (no findings):

- **Production wiring (requirement 1) is real.** `require_auth_middleware` (lib.rs:850) passes `/scim/v2/*` through before the global token ladder; every handler still enforces `check_scim_auth` (fail-closed 403 when `GYRE_SCIM_TOKEN` is unset, `auth::tokens_equal` constant-time compare — the plain `!=` was replaced). ABAC passes the routes as resolver-exempt with per-handler auth, so both ABAC gates stay green (ran them). The new tests in `tests/scim_lifecycle.rs` drive the FULL `build_router()` stack (require_auth → last_seen → abac → handler) over a real SQLite temp-file DB — not a directly mounted `api_router()` — and assert the positive path (SCIM-only bearer → 200/201), the negative paths (wrong token 403, no header 403, global token 403, unset env 403), exactly the verification intent (a)–(c).
- **PATCH (requirement 2) is a real RFC 7644 PatchOp implementation.** `scim_patch_user` parses `Operations` (the exact RFC capital-O key — the d835ff67 repair; a missed key would silently no-op, and the code comments show the author found and fixed that class), validates `op` ∈ {add, replace, remove}, handles path targets (`active`, `displayName`, `emails`, enterprise URN paths incl. `:manager`/`:manager.value`), path-less object values, and a lenient bare-body form. ServiceProviderConfig advertises `patch.supported: true`; Schemas/ResourceTypes advertise the enterprise extension.
- **Deactivation lifecycle (requirement 3) is a real cascade, not a flag flip.** `deactivate_via_scim`: soft `User::deactivate` (row stays resolvable — DELETE no longer hard-deletes; the old unit test asserting 404-after-DELETE was correctly inverted to 200 + `active:false`), `sessions.revoke_all_for_user`, `impersonation::end_active_sessions_for_user`, agents' `spawned_by` cleared via `agents.update` (now actually persisted — both SQL adapters' `update()` SET lists gained `agents::spawned_by`, with adapter regression tests), `workspace_ownership::on_user_removed`, and a `user_deactivated` audit event with `source: "scim"`. Serialized `active` mirrors `!user.is_deactivated()`. The 401-on-replay proof is real: in auth.rs the `credential_revoked` check was hoisted above the deactivation branch in the API-key path so a session-revoking SCIM deactivation replays its pre-deactivation credential with 401 "Session has been revoked" (and an admin deactivation without revocation still 403s — both flavors keep accurate reasons). `assert_deactivated` in the tests checks stored state, serialized state, ALL sessions revoked with a non-empty control, the 401 replay through `/api/v1/users/me`, and `spawned_by == None`.
- **Enterprise extension (requirement 4).** `urn:ietf:params:scim:schemas:extension:enterprise:2.0:User` serialized on every resource with `organization` (tenant id) and `manager.value`; persisted via new `users.manager_id` column (migration 000067, SQLite + Postgres rows mapped in both adapters, round-trip adapter test); accepted on create/PUT/PATCH incl. whole-extension and sub-attribute path forms and `remove` ops.
- **Core-schema preservation (requirement 5).** userName/externalId immutable on PUT (pre-existing unit test `scim_update_user` still asserts it) and PATCH (new test `patch_cannot_rename_user_name_or_external_id`); idempotent create, emails, meta, no password fields unchanged. Create-side username sanitization and the reserved `system` displayName guard are preserved in the PATCH paths too.
- **Coverage row** (identity-security.md row 10) honestly stays `implemented`/partial with the groups half still missing and tracked by task-266; docs updated (api-reference PATCH row, server-config 403 fail-closed wording).
- Test-inflation check: none found. The lifecycle tests go through the production stack, assert controls (live credential pre-deactivation, non-empty session set) before the effects, and each would fail if the wiring exemption, session revocation, or spawned_by persistence were reverted.

Findings:

- [-] **F1 (major, code): every successful `/scim/v2/*` request through the production stack persists a false `AuthFailure` audit event — SIEM severity 6.** The `/scim/v2` exemption was added to `require_auth_middleware` (lib.rs:850) only, but `last_seen_middleware` (middleware.rs:146), layered over the same `api_router()` and running after require_auth, unconditionally attempts `AuthenticatedAgent::from_request_parts` with the SCIM bearer token. The extractor ladder has no `GYRE_SCIM_TOKEN` branch, so it falls through to the final fallback (auth.rs:1701) which records an `AuthFailure` audit event ("invalid token", token_kind "unknown", `source_ip` = the IdP's socket peer) before returning Err — an error `last_seen_middleware` discards while the handler succeeds. Each legitimate IdP provisioning call therefore writes a persisted, broadcast, CEF-severity-6 auth-failure row: continuous high-severity false positives from the IdP's IP, operationally indistinguishable from (and masking) a real credential attack. Pre-task, rejected SCIM calls logged one TRUE failure; post-task, successful calls log FALSE ones — the production wiring fix is incomplete. Full line-by-line trace and reproduction steps: `/tmp/stage/review-evidence/f1-audit-pollution-trace.md`. **Repair (mechanical):** early-return `next.run(req)` for `/scim/v2/` paths in `last_seen_middleware` (the tracking branch is already dead for them — no workspace id in a SCIM path), or add a SCIM-token branch to the extractor that does not record a failure; then assert zero `AuthFailure` events for authenticated SCIM traffic in the lifecycle tests.

Minor (non-blocking, recorded for completeness):

- PUT with the enterprise extension omitted leaves stored `organization`/`manager` untouched (PATCH-like partial semantics on a replace verb). The task text only requires "accept and persist", so this is noted, not charged.
- A PATCH body using a non-RFC lowercase `operations` key falls into the bare-body path where the key is ignored (no-op 200). RFC 7644 §3.5.2 mandates `Operations`; conformant IdPs are unaffected.
- Generated `web/dist/` not reviewed — verifier-owned per the stage contract.

Verifier must run (deferred from this sandbox per the build-budget rule):

- `cargo test -p gyre-server --test scim_lifecycle` (9 tests; the three formerly failing spawned_by cases are covered by the `ed7b5a88` adapter fix verified here).
- `cargo test -p gyre-server --lib scim::` (in-crate unit tests incl. PUT immutability and the inverted soft-DELETE assertions).
- `cargo test --all` (full suite).
- F1 probe: after any successful `scim_call` through `build_router()`, `state.audit` must contain zero `AuthFailure` events attributable to that request — currently one per call (see trace file).
