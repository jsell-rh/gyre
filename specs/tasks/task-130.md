---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["bb64034093807386ea0b91a001db3f7ffc7b7444", "a2f36db7eefb9bf5e400240bb6d3a5ccba1cd077"]
---

## Spec Excerpt

From `platform-model.md` §1 Token Scoping:

Agent OIDC tokens encode the scope:

```json
{
  "sub": "agent:worker-42",
  "tenant_id": "tenant-acme",
  "workspace_id": "ws-gyre-platform",
  "repo_id": "repo-gyre-server",
  "task_id": "TASK-007",
  "scope": ["repo:gyre-server:write"],
  "stack_hash": "sha256:...",
  "persona": "security",
  "attestation_level": 3
}
```

A worker agent's token is scoped to its repo. The Workspace Orchestrator's token is scoped to the workspace. The MCP server validates scope on every tool call.

## Implementation Plan

1. **Extend `AgentJwtClaims` in `gyre-server/src/auth.rs`:**
   - Add `tenant_id: String` (required)
   - Add `workspace_id: String` (required)
   - Add `repo_id: Option<String>` (None for workspace orchestrators)
   - Add `persona: Option<String>` (persona slug)
   - Add `attestation_level: Option<u32>` (workload attestation confidence)
   - Change `scope: String` to `scope: Vec<String>` for fine-grained scopes (e.g., `["repo:gyre-server:write"]`)

2. **Update token minting in `auth.rs` `mint_with_workload()`:**
   - Accept tenant_id, workspace_id, repo_id, persona, attestation_level parameters
   - Populate claims from spawn context

3. **Update `spawn.rs` to provide hierarchy context at mint time:**
   - Look up agent's task → repo → workspace → tenant to populate all claims
   - Worker agents: scope = `["repo:{repo_name}:write"]`
   - Workspace orchestrators: scope = `["workspace:{workspace_id}:read", "workspace:{workspace_id}:spawn"]`
   - Repo orchestrators: scope = `["repo:{repo_name}:write", "repo:{repo_name}:spawn"]`

4. **Extend `AuthenticatedAgent` with hierarchy fields:**
   - Add workspace_id, repo_id fields derived from validated claims
   - Available to all downstream handlers

5. **MCP scope validation in `mcp.rs`:**
   - On every tool call, validate `scope` claims permit the requested action
   - `repo_id` claim must match the repo context for repo-scoped tools
   - `workspace_id` claim must match workspace context for workspace-scoped tools
   - Reject calls that exceed the token's scope with clear error

6. **Update existing tests:**
   - All test token creation must include new required fields
   - Add scope validation tests for MCP tool calls

## Acceptance Criteria

- [ ] `AgentJwtClaims` includes `tenant_id`, `workspace_id`, `repo_id`, `persona`, `attestation_level`
- [ ] `scope` field is `Vec<String>` with fine-grained permissions
- [ ] Token minting populates hierarchy claims from spawn context
- [ ] Worker agent tokens scoped to their repo
- [ ] Workspace orchestrator tokens scoped to their workspace
- [ ] MCP server validates scope on every tool call
- [ ] Scope mismatch returns clear error (not silent allow)
- [ ] `AuthenticatedAgent` exposes workspace_id and repo_id
- [ ] All existing tests updated for new claim fields
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/platform-model.md` §1 Token Scoping for the full spec. Current JWT implementation: `gyre-server/src/auth.rs` (AgentJwtClaims at line ~46, mint_with_workload at ~176). Token validation: `auth.rs` ~216. MCP server: `gyre-server/src/mcp.rs`. Agent spawn: `gyre-server/src/api/spawn.rs`. The `scope` field is currently a `String` hardcoded to `"agent"` — it needs to become `Vec<String>`. This is a breaking change to the JWT format, so update ALL token creation paths (search for `AgentJwtClaims` usage). Check migration numbering: `ls crates/gyre-adapters/migrations/ | tail -5` — currently at 000049.

## Shipped

Implemented `platform-model.md` §1 Token Scoping end-to-end. This round repairs the review 29e132008c6748508f2c52ec982d78d6 finding (tenant containment consumed the extractor's fabricated "default" tenant, breaking the workspace-orchestrator spawn flow and failing a deterministic lib test at the prior candidate).

### Claims, minting, and principal resolution

`AgentJwtClaims` (auth.rs) carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, and `scope: Vec<String>`. Workers mint `["repo:{name}:write"]`, workspace orchestrators `["workspace:{id}:read","workspace:{id}:spawn"]`, repo orchestrators `["repo:{name}:write","repo:{name}:spawn"]` — all resolved from real hierarchy context (repo → workspace → tenant) in spawn.rs / orchestrator.rs / stale_agents.rs. Worker spawn fails closed when the repo's workspace row is missing (no unscoped UUID downgrade). Residual (unchanged): worker mint passes persona/attestation_level as None — the spawn request carries no persona input and attestation is recorded post-spawn (workload_attestations KV).

### This round: caller tenant derived from signature-verified claims

`with_hierarchy_from_claims` previously mirrored `workspace_id`/`repo_id`/`scope` from the verified claims but NOT `tenant_id`, so `AuthenticatedAgent.tenant_id` stayed the extractor's fabricated `"default"` (pre-existing task-099 F1 site) on the per-agent-token path. Tenant-containment handlers (`spawn_repo_orchestrator_core`, `spawn_workspace_orchestrator_core`, explorer/git_http scope checks) compare that value against the target entity's real tenant — a workspace orchestrator with a correctly minted scoped token (tenant t1, workspace ws-1) was denied its own happy path ("repo r-1 belongs to tenant t1, not the caller's tenant default"), deterministically failing `mcp::tests::mcp_spawn_repo_orchestrator_happy_and_wrong_workspace`. Now `with_hierarchy_from_claims` mirrors `tenant_id` from the verified claims when present (tokens without a tenant claim keep the extractor-resolved identity), on every JWT-resolved auth path: the extractor's per-agent-token arm, `authenticate_token` (WebSocket), Keycloak JWT, and federated JWT — so REST handlers reading `auth.tenant_id` get the real claims-derived tenant too. This is fail-closed direction only (a correct tenant now matches where the fabricated "default" mismatched); cross-tenant/wrong-workspace denials keep firing.

### MCP scope gate

`validate_tool_scope` runs before dispatch on every `tools/call` for scoped (Gyre-minted) tokens: worker tools (heartbeat/complete/record_activity/create_mr/list_mrs) require a matching `repo:` scope; spawn tools additionally the `spawn` right; workspace tools a matching `workspace:` scope; cross-tier task tools (gyre_create_task/gyre_update_task/gyre_decompose_spec) resolve the target task's repo/workspace from state and check both; gyre_search validates its workspace_id arg. Mismatch returns a `PERMISSION_DENIED` JSON-RPC error naming the target repo/workspace. Non-scope-list callers (global token, API key, Keycloak JWT, legacy UUID tokens) keep the pre-existing ABAC/RBAC path.

### Test evidence

- `cargo test -p gyre-server` (lib + integration binaries) at head bb640340: lib **1267 passed / 0 failed** (the previously failing `mcp_spawn_repo_orchestrator_happy_and_wrong_workspace` passes); api_integration 66, auth_integration 21, git_integration 16, graph_integration 37, m18_oidc_integration 8/8, explorer_ws 8, conversation/dependency/e2e/search green. Logs: /tmp/stage/review-evidence/lib-test-full.log, integration-test.log.
- New discriminating test `agent_jwt_tenant_mirrored_from_verified_claims` (auth.rs): a workspace-orchestrator JWT (tenant t1, ws-1) through the extractor resolves `tenant_id`/`workspace_id`/`scope` exactly from the verified claims — fails if the fabricated "default" regresses.
- New test `mcp_spawn_repo_orchestrator_denies_tenant_mismatch` (mcp.rs): a token whose tenant claim (t-other) mismatches the target workspace's row (t1) is denied with an error naming both tenants — containment keeps firing with the claims-derived tenant, so the happy path passes for the right reason.
- Static gates OK at head: check-arch, check-abac-exempt-handlers, check-mcp-write-tools, check-fabricated-scope-defaults, check-lossy-secret-conversion, check-inert-enforcement, check-relative-path-defaults, check-scope-literal-defaults (also with the `crates` argument, stricter than the CI invocation), check-forged-scope-fields, check-forwarded-header-trust, check-in-memory-state-stores, check-unbounded-external-http, check-migration-versions, check-byte-slice-truncation, check-dead-message-kinds, check-task-commit-attribution; `cargo check -p gyre-server` warning-free. Exemption entries re-pointed for this diff's line drift (fabricated-scope-defaults auth.rs:922→936) and the scope-literal-defaults auth.rs/lib.rs entries were re-pointed to the actual current violation sites (they were stale from earlier drift; entry counts unchanged, no new entries).
- Known pre-existing flake, untouched by this task: `api::scim::tests::scim_delete_user` can fail under full-suite parallelism because scim tests mutate the process-global `GYRE_SCIM_TOKEN` env var with different values (scim.rs:451/574, introduced in M23 #296); passes in isolation here.

Full workspace suite, all-target Clippy, and exact-head GitHub CI remain with verification/publication.
