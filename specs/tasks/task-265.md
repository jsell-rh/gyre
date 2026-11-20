---
title: "SCIM user lifecycle completion: PATCH, deactivation semantics, enterprise extension, production wiring"
spec_ref: "Spec gap spec-31d3ae72e12e22c52442e309 / scim-users-lifecycle / b60edce5bc49"
depends_on: []
progress: complete
commits: ["117384c359678c880277190e2523c7f4b99f676f", "ed7b5a88115656eab7480444622586f57c8dd99f", "d835ff672057a17202f7c2902f7c958e9ea4b233", "156d268fde267a990c59a047da860d9b9725e77c"]
---

## Required behavior

Complete the SCIM 2.0 user-lifecycle half of the User Identity section so IdPs can push creates, updates, and deactivations automatically. (1) Fix the production wiring defect: require_auth_middleware (lib.rs) currently layers over all of api_router() including /scim/v2/* and has no GYRE_SCIM_TOKEN branch, so a request bearing only the dedicated SCIM bearer token is rejected 401 before check_scim_auth runs (the existing tests bypass this by mounting api_router() directly). Exempt /scim/v2/* from the global auth middleware (handlers keep enforcing check_scim_auth: fail-closed 403 when GYRE_SCIM_TOKEN is unset, constant-time bearer compare, separate from agent JWTs), keeping the ABAC-exempt-handler and route-registry gates green. New tests must go through the full build_router() stack. (2) Implement PATCH /scim/v2/Users/{id} (RFC 7644 partial update) including the deactivation path active:false, and update ServiceProviderConfig to advertise patch supported. (3) Deactivation lifecycle per spec: active:false deactivates the user, revokes all active sessions (revoke_all_for_user — never currently invoked from SCIM), and removes the deactivated user from active agents' spawned_by tracking; SCIM DELETE deprovisions by marking the user inactive (no hard store delete — the current handler hard-deletes); the serialized resource's active field reflects the stored user state instead of the hardcoded true. (4) Enterprise extension (RFC 7643 §4.3): serialize and accept urn:ietf:params:scim:schemas:extension:enterprise:2.0:User with organization (tenant-id) and manager.value on create/PUT/PATCH. (5) Preserve the existing core-schema behavior: userName immutable after creation, externalId↔external_id mapping, idempotent create, emails, meta, no password fields.

## Acceptance Criteria

- [ ] Through the production build_router() stack, a request bearing only Authorization: Bearer $GYRE_SCIM_TOKEN reaches the SCIM handlers and succeeds; with the token unset or wrong, every /scim/v2 request is rejected (403/401) — verified end-to-end, not via a directly mounted api_router().
- [ ] PATCH /scim/v2/Users/{id} with {"active": false} deactivates the user: the stored record is inactive, the serialized SCIM resource reports active:false, all of that user's active sessions are revoked (a subsequent authenticated request with a pre-deactivation session token gets 401), and active agents' spawned_by tracking no longer attributes to the deactivated user.
- [ ] SCIM DELETE marks the user inactive (deprovision) — the user row remains resolvable (not a hard delete), with the same session-revocation and spawned_by effects as active:false.
- [ ] User resources include the enterprise extension schema in schemas and carry organization (tenant id) and manager.value; PUT/PATCH accept and persist them.
- [ ] PUT/PATCH attempting to change userName or externalId is rejected/ignored (immutable), and ServiceProviderConfig advertises patch as supported.

## Verification intent

Through build_router(): (a) send GET /scim/v2/Users with only the SCIM bearer token — against current code this gets 401 'Invalid token' from require_auth_middleware before check_scim_auth ever runs, so the positive path failing pre-task proves the wiring fix is real; (b) with GYRE_SCIM_TOKEN unset, the same request must be rejected 403 — proving the exemption did not open an unauthenticated hole; (c) PATCH active:false on a user holding a live session, then replay that session — must 401, proving deactivation is a lifecycle event (sessions revoked), not a serialized flag flip (today active is hardcoded true and the parsed field is never read).

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/identity-security.md`. Source: `fb1424fbac55cd23cb08606f4cd667a96f4dbc43`; plan: `8e132f42330e3bc1b1fb491a64d1396c0f3190a1457ecee8ef489f18b26cea9e`.

[
  {
    "id": "e88b0643650de3d8f271012a",
    "decision": "gap",
    "requirement_quote": "SCIM 2.0 (RFC 7643 / RFC 7644) is the standard protocol for enterprise IdP-driven user lifecycle management. Keycloak, Okta, Entra ID, and most enterprise IdPs support SCIM as a provisioning protocol — they push user creates, updates, and deactivations to Gyre automatically.",
    "detail": "SCIM 2.0 user lifecycle support is only partially implemented, so the section's core promise — IdPs push creates, updates, and deactivations automatically — is not fully delivered. What exists: /scim/v2 base path with Users CRUD (list/get/create/update/delete) plus ServiceProviderConfig, Schemas, ResourceTypes discovery endpoints, GYRE_SCIM_TOKEN bearer auth checked per-handler (fail-closed when unset), externalId mapped to external_id with idempotent create, and immutable userName. What is missing (detailed in the child sections): all six /scim/v2/Groups endpoints (grep for /scim/v2/Groups and ScimGroup across crates/ returns zero matches — no group type exists anywhere), PATCH /scim/v2/Users/{id} (explicitly disclaimed 'supported: false'), the deactivation lifecycle (active is parsed but never read; DELETE is a hard delete with no session revocation or spawned_by cleanup), the enterprise schema extension, the group-to-workspace mapping endpoint and behavior, and a production wiring defect: the global require_auth_middleware layered over api_router() does not accept the SCIM token, so a request bearing only GYRE_SCIM_TOKEN is rejected 401 before the handler's own check runs (the SCIM tests bypass this by mounting api_router() directly). Endpoint, schema, and mapping specifics are accounted once under their child sections.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "// ── SCIM 2.0 Provisioning ─────────────────────────────────────────\n        .route(\n            \"/scim/v2/Users\",\n            get(scim::scim_list_users).post(scim::scim_create_user),\n        )",
        "detail": "The complete production SCIM route set: Users CRUD, ServiceProviderConfig, Schemas, ResourceTypes — 8 of the 15 specced endpoints. There are no Group routes and no PATCH anywhere in the router, so the IdP-driven group lifecycle (the section's 'Group-to-workspace-role mapping' motivation) has no receiving surface."
      }
    ]
  },
  {
    "id": "2cf8a1c0fbde5c0d82e85317",
    "decision": "gap",
    "requirement_quote": "**Authentication:** SCIM endpoints accept a dedicated SCIM bearer token (configured via `GYRE_SCIM_TOKEN` env var). This is separate from the agent JWT — SCIM clients are server-to-server, not agent-to-server.",
    "detail": "Endpoint inventory: 8 of the 15 specced endpoints are implemented (GET/POST /scim/v2/Users; GET/PUT/DELETE /scim/v2/Users/{id}; GET ServiceProviderConfig, Schemas, ResourceTypes). Missing: PATCH /scim/v2/Users/{id} (ServiceProviderConfig explicitly declares patch unsupported), and all six /scim/v2/Groups endpoints (grep for '/scim/v2/Groups' and 'ScimGroup' across crates/ returns zero matches — no group handler, type, or storage exists). Authentication is implemented per-handler and is fail-closed and separate from agent JWTs (constant-time compare against GYRE_SCIM_TOKEN; unset token => 403 'GYRE_SCIM_TOKEN not configured'; routes are ABAC-exempt). BUT there is a production wiring defect that defeats it: build_router layers require_auth_middleware over the whole api_router() — including /scim/v2/* — and that middleware only exempts /api/v1/version, accepting only the global auth token, impersonation JWTs, agent tokens, API keys, upstream OIDC JWTs, or federation JWTs. A request bearing only the SCIM bearer token is rejected 401 'Invalid token' before check_scim_auth ever runs; GYRE_SCIM_TOKEN is not referenced anywhere in lib.rs. The SCIM integration tests never catch this because they build api_router().with_state(...) directly, bypassing the middleware stack. The heading's own '(stub — to be implemented)' label does not close the gap; the goal requires real implementation.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/scim.rs",
        "quote": "fn check_scim_auth(headers: &HeaderMap) -> Result<(), ApiError> {",
        "detail": "Per-handler SCIM auth exists and is fail-closed ('GYRE_SCIM_TOKEN not configured' => Forbidden when the env var is unset; constant-time Bearer compare otherwise), separate from the agent JWT chain — the handler-level half of the requirement is implemented."
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "// /api/v1/version is intentionally public.",
        "detail": "require_auth_middleware's only path exemption is /api/v1/version. Its token ladder (global token, impersonation JWT, agent_tokens KV, API keys, upstream OIDC, federation JWT) has no GYRE_SCIM_TOKEN branch — grep for 'scim' in lib.rs returns zero matches."
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "let api = api::api_router()",
        "detail": "The production router layers abac_middleware, last_seen_middleware, and require_auth_middleware over all of api_router() — which contains the /scim/v2 routes. A production SCIM client presenting only the dedicated SCIM bearer token is therefore rejected 401 by the global gate before the handler-level check_scim_auth runs; the specced authentication model does not work end-to-end in the shipped server."
      },
      {
        "file": "crates/gyre-server/src/api/scim.rs",
        "quote": "\"patch\": { \"supported\": false }",
        "detail": "ServiceProviderConfig explicitly disclaims PATCH support, confirming PATCH /scim/v2/Users/{id} (specced for partial update e.g. deactivate via active:false) is not implemented."
      }
    ]
  },
  {
    "id": "b2b37ae05e75a44a9117ceca",
    "decision": "gap",
    "requirement_quote": "Gyre implements a subset of the SCIM core schema (RFC 7643 §4.1) plus enterprise extension (RFC 7643 §4.3):",
    "detail": "The core-schema subset is faithfully implemented: schemas, id, externalId (mapped to external_id, the Keycloak sub), userName (immutable after creation — the PUT handler never writes username or external_id), displayName, emails with value/primary/type, active field on the wire, and meta with resourceType/created/lastModified/location ('/scim/v2/Users/{uuid}'); no password fields are exposed. But the enterprise extension is entirely absent: the schemas array contains only the core User schema — grep for 'enterprise:2.0' across crates/ returns zero matches — so organization (tenant-id) and manager.value do not exist in the serialized resource. Worse, the specced Gyre-specific deactivation behavior is not implemented: the response hardcodes active: true ('deactivation tracked via future `active` field on User'), the request's parsed active field is never read by any handler, and SCIM DELETE is a hard store delete rather than the specced deactivation — no session revocation (sessions.revoke_all_for_user is never invoked from the SCIM path) and no removal from active agents' spawned_by tracking (grep for spawned_by in scim.rs: zero matches).",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/scim.rs",
        "quote": "schemas: vec![\"urn:ietf:params:scim:schemas:core:2.0:User\".to_string()]",
        "detail": "Only the core schema is advertised/serialized; the specced 'urn:ietf:params:scim:schemas:extension:enterprise:2.0:User' extension (organization -> tenant-id, manager.value) appears nowhere in the codebase, so the 'plus enterprise extension' half of the schema requirement is absent."
      },
      {
        "file": "crates/gyre-server/src/api/scim.rs",
        "quote": "active: true, // deactivation tracked via future `active` field on User",
        "detail": "active is hardcoded true in the serialized resource and the request-side active: Option<bool> is never read by any handler, so the specced 'active: false → Gyre deactivates user, revokes all active sessions, removes from active agents' spawned_by tracking' lifecycle cannot be driven via SCIM at all."
      },
      {
        "file": "crates/gyre-server/src/api/scim.rs",
        "quote": "// SCIM DELETE = deactivate, not hard delete. We delete from store here;",
        "detail": "Full comment admits the deviation: 'SCIM DELETE = deactivate, not hard delete. We delete from store here; production deployments with SSO should soft-delete via active=false.' The handler performs a hard users.delete plus workspace-ownership cleanup only — no session revocation, no spawned_by cleanup (the machinery for both exists elsewhere, e.g. state.sessions.revoke_all_for_user on the admin deactivate path, but is never invoked from SCIM)."
      }
    ]
  }
]

## Shipped

All five requirement areas are implemented in production code on this branch: implementation commit `156d268f`, review-repair commit `d835ff67`, verification-repair commit `ed7b5a88`, and round-2 review-repair commit `117384c3`.

### Production wiring (requirement 1)

`require_auth_middleware` (crates/gyre-server/src/lib.rs) passes `/scim/v2/*` through to the handlers instead of running the global agent/API-key/JWT token ladder (which rejected a SCIM-only bearer 401 "Invalid token" before `check_scim_auth` could run). The exemption is fail-closed: every `/scim/v2` handler still enforces `check_scim_auth` — 403 when `GYRE_SCIM_TOKEN` is unset, constant-time bearer compare (`auth::tokens_equal`). The routes are ABAC resolver-exempt with per-handler auth, so the ABAC-exempt-handler and route-registry gates stay green (both re-run on this branch: OK — all routes resolve, 86 handlers checked).

### PATCH + deactivation lifecycle (requirements 2–3)

`PATCH /scim/v2/Users/{id}` (`scim_patch_user`, api/scim.rs) implements RFC 7644 PatchOp (`Operations` with `op`/`path`/`value`, including enterprise-extension URN paths) plus a simplified bare-body form (`{"active": false}`) for IdPs that send a fragment. `active:false` (PATCH, PUT, or DELETE) runs `deactivate_via_scim`: soft `User::deactivate` (the row remains resolvable — SCIM DELETE no longer hard-deletes), `sessions.revoke_all_for_user`, `impersonation::end_active_sessions_for_user`, agents' `spawned_by` attribution cleared, `workspace_ownership::on_user_removed`, and a `user_deactivated` audit event with `source: "scim"`. The serialized `active` field mirrors `!user.is_deactivated()` instead of the hardcoded `true`. `ServiceProviderConfig` advertises `patch.supported: true`. In auth.rs, the `credential_revoked` check was hoisted above the deactivation check in the API-key branch so a session-revoking SCIM deactivation replays its pre-deactivation credential with 401 "Session has been revoked".

### Enterprise extension (requirement 4) + core-schema preservation (requirement 5)

User resources serialize `urn:ietf:params:scim:schemas:extension:enterprise:2.0:User` with `organization` (the tenant id) and `manager.value`, persisted via the `users.manager_id` column (migration `2026-11-13-000067_user_manager_id`, SQLite + Postgres). Create/PUT/PATCH accept and persist both attributes. `userName`/`externalId` remain immutable on PUT and PATCH (ignored, not 409, to keep IdP sync loops working); idempotent create, emails, meta, and no-password-fields behavior are unchanged. Schemas/ResourceTypes discovery advertises the extension.

### Verification repair (ed7b5a88)

Verification run `85ea51e0` (cargo test --all) failed 3 of 9 tests in `tests/scim_lifecycle.rs`, all at the same assertion: after SCIM deactivation the agent's `spawned_by` still held the user id. Root cause: `deactivate_via_scim` clears the field in memory and calls `AgentRepository::update`, but both SQL adapters' `update()` omitted `agents::spawned_by` from the diesel SET list, so the clearance was never persisted (the mem adapter stores the whole struct, so only the SQL-backed integration tests could catch it). Fixed in `sqlite/agent.rs` and `postgres/agent.rs` (update now sets `spawned_by`, matching the create upsert); the port contract now documents that `update` persists every mutable field including `spawned_by`; regression test `update_persists_spawned_by_attribution_changes` (Some→None and None→Some round trip) added to the sqlite adapter suite.

### Round-2 review repair: no false AuthFailure audit rows for authorized SCIM traffic (117384c3)

The round-1 wiring exemption left one defect: `last_seen_middleware` (layered inside `require_auth_middleware` in `build_router`) still ran its `AuthenticatedAgent` extraction on `/scim/v2/*` requests. That ladder does not accept the SCIM bearer, and its final fallback in auth.rs PERSISTS an AuthFailure audit event ("invalid token", CEF severity 6 in the SIEM export) — one false high-severity row per SUCCESSFUL IdP provisioning call, indistinguishable from a credential attack. `last_seen_middleware` now skips `/scim/v2/*` before the extraction (last-seen tracking was already dead for these paths: no `/api/v1/workspaces/<id>` segment to extract). Regression test `successful_scim_requests_do_not_record_auth_failures` drives four successful SCIM calls (list/create/PATCH/GET) through the full `build_router()` stack and asserts zero `auth_failure` audit rows, with a live control: a genuinely invalid bearer on `/api/v1/users/me` records exactly one TRUE row, so the zero is not a vacuous pass. SCIM auth semantics are unchanged: every handler still enforces `check_scim_auth` (fail-closed 403 on unset token, wrong token, or missing header — `scim_wrong_or_missing_token_rejected`, `scim_unset_token_fails_closed`).

### Test evidence

- This sandbox (evidence in /tmp/stage/review-evidence, head `117384c3`): `cargo check -p gyre-server --tests` — clean, zero errors (8m49s cold; log `cargo-check-gyre-server.log`); `scripts/check-abac-route-registry.sh` and `scripts/check-abac-exempt-handlers.sh` — both OK. gyre-server test EXECUTION is deferred to the independent verifier (sandbox build budget), which must run: `cargo test -p gyre-server --test scim_lifecycle` (10 tests, including the round-2 `successful_scim_requests_do_not_record_auth_failures`), `cargo test -p gyre-adapters sqlite::agent` (19 tests, includes `update_persists_spawned_by_attribution_changes`), and the full `cargo test --all`.
- `tests/scim_lifecycle.rs` (added in `156d268f`, extended in `117384c3`) drives every request through the full production `build_router()` stack over a real SQLite temp-file database: SCIM-only bearer reaches the handlers and succeeds; wrong/missing/global token rejected 403; unset `GYRE_SCIM_TOKEN` fails closed 403; PATCH `active:false` (PatchOp and bare-body) and SCIM DELETE each deactivate the stored record, serialize `active:false`, revoke every session (pre-deactivation credential replays 401), and clear agents' `spawned_by`; DELETE leaves the row resolvable; the enterprise extension round-trips through create/PUT/PATCH; userName/externalId immutability and patch-advertised ServiceProviderConfig asserted.
