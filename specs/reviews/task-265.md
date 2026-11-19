# Review — task-265 (SCIM user lifecycle: PATCH, deactivation semantics, enterprise extension, production wiring)

Spec: `specs/system/identity-security.md` §SCIM Provisioning / §SCIM User Schema / Gyre-specific SCIM behavior.
Commits under review: `156d268f` (implementation), `d835ff67` (round-1 repair: RFC 7644 `Operations` serde key), `ed7b5a88` (verification repair: persist `spawned_by` in SQL agent update), `117384c3` (round-2 repair: `last_seen_middleware` false AuthFailure rows). Candidate `8fdfaede` (all four are ancestors).
Verdict: **approved** — all five requirement areas verified against the diff and the canonical spec; no blocking findings. Runtime evidence for the adapter layer and all mechanical gates gathered in this sandbox; gyre-server test execution deferred to the independent verifier (commands recorded below and in `/tmp/stage/review-evidence/`).

## Probes run (this sandbox, candidate HEAD)

- `cargo check -p gyre-server --tests` → clean, exit 0 (7m 18s cold; log `review-evidence/cargo-check-gyre-server.log`).
- `cargo test -p gyre-adapters sqlite::agent` → 19 passed, incl. `update_persists_spawned_by_attribution_changes` (the verification-repair regression: both SQL adapters' `update()` SET list now includes `agents::spawned_by`; Some→None and None→Some round-trip).
- `cargo test -p gyre-adapters sqlite::user` → 37 passed, incl. `enterprise_manager_id_round_trips` (users.manager_id persists through create/update/clear).
- `scripts/check-abac-route-registry.sh` → OK; `scripts/check-abac-exempt-handlers.sh` → OK (86 handlers; every `/scim/v2` handler enforces `check_scim_auth` in-body); `check-migration-versions.sh` → OK (000067 next unused); `check-migration-sql-portability.sh` → OK (plain `ALTER TABLE ... ADD COLUMN`, both dialects); `check-arch.sh` → OK; `check-forged-scope-fields.sh`, `check-scope-literal-defaults.sh`, `check-inert-enforcement.sh`, `check-in-memory-state-stores.sh` → OK.

## Verified per requirement

- **(1) Production wiring.** `require_auth_middleware` passes `/scim/v2/*` through before the global token ladder (which rejected a SCIM-only bearer 401 before `check_scim_auth` could run); `last_seen_middleware` skips the same prefix before its `AuthenticatedAgent` extraction — round-2 repair, since that ladder's fallback persisted one false AuthFailure audit row (CEF severity 6) per *successful* IdP provisioning call; `abac_middleware` passes the resolver-exempt SCIM routes straight through. The exemption is fail-closed: every handler enforces `check_scim_auth` (403 when `GYRE_SCIM_TOKEN` unset, `auth::tokens_equal` constant-time compare, separate from agent JWTs), and both ABAC gates stay green. `tests/scim_lifecycle.rs` drives every request through the full `build_router()` stack over a real SQLite temp-file DB — the positive wiring test (`scim_bearer_token_reaches_handlers_through_production_stack`) fails pre-task by construction, and `scim_unset_token_fails_closed` / `scim_wrong_or_missing_token_rejected` (wrong, missing, and global-token-as-SCIM-credential all 403) prove the exemption opened no unauthenticated hole.
- **(2) PATCH.** `.patch(scim_patch_user)` on `/scim/v2/Users/:id`; RFC 7644 PatchOp with the spec's capital-`Operations` key (serde `alias`, round-1 repair — without it every PatchOp body silently deserialized to a no-op 200) plus a lenient bare-body form; ServiceProviderConfig advertises `patch.supported: true`; Schemas/ResourceTypes advertise the enterprise extension.
- **(3) Deactivation lifecycle.** `deactivate_via_scim` performs soft `User::deactivate` (row resolvable), `sessions.revoke_all_for_user`, `impersonation::end_active_sessions_for_user`, agents' `spawned_by` clearance (persisted in both SQL adapters after the verification repair, regression-tested at the adapter layer), `workspace_ownership::on_user_removed`, and a `user_deactivated` audit event with `source: "scim"` (same event type as the admin path). SCIM DELETE now deprovisions via this cascade instead of hard-deleting; serialized `active` mirrors `!is_deactivated()`; `assert_deactivated` in the integration tests checks each effect with live controls (credential 200 pre-deactivation; non-empty session rows pre-revocation; pre-deactivation API key replays 401). The auth.rs reorder (credential-revoked check before deactivation check in the API-key branch) only changes the rejection *reason* (401 vs 403); both orderings reject, and the admin no-revocation deactivation flavor keeps its 403.
- **(4) Enterprise extension.** Both schema URNs serialized; `organization` → `users.tenant_id`, `manager.value` → `users.manager_id` (migration `2026-11-13-000067_user_manager_id`, SQLite + PostgreSQL rows/records/update lists, mem adapter struct-store); create/PUT/PATCH accept and persist both attributes; round-trip covered in integration and adapter tests.
- **(5) Core-schema preservation.** `userName`/`externalId` immutable (ignored on PUT/PATCH, not 409, keeping IdP sync loops working); idempotent create via `find_by_external_id`; reserved system ABAC identity rejected as displayName on every path including PATCH; no password fields; SCIM create defaults to ReadOnly (`User::new_sso`) — no privilege escalation through provisioning.

## Test-quality check

No inflation found. The integration suite mounts the production `build_router()` (the exact bypass that hid the original wiring defect is gone from the tests), the deactivation assertions each kill a distinct bug class (stored state, wire state, session revocation, attribution), and the round-2 audit regression has a true-positive control (a genuinely invalid bearer records exactly one row), so its zero-row assertion is not vacuous.

## Deferred to the independent verifier

gyre-server test execution (sandbox build budget): `cargo test -p gyre-server --test scim_lifecycle` (10 tests), `cargo test -p gyre-server --lib scim`, and the full `cargo test --all`. Adapter-layer subsets already green here.

## Non-blocking observations (follow-up suggestions, not defects against this task's contract)

- `apply_enterprise` does not validate that `organization` resolves to an existing tenant, unlike the admin `POST /api/v1/users` path (its "F1" invariant). The failure is fail-closed downstream (ABAC tenant isolation denies a bogus binding); the spec maps `organization` → tenant-id without requiring existence validation. Worth hardening in a follow-up.
- `tokens_equal` early-exits on length mismatch — pre-existing platform primitive shared with the global token ladder, unchanged by this task.
- PATCH `remove` on path `active` returns 400 (null value is not boolean); defensible strictness for a semantically undefined operation.
