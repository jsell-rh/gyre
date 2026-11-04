---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["7da23e046ba59c68e07a6a45baa2519e046e5170", "51448dd92a7bed486430db146f9d0f743ed509ba", "d9d4d179531d1216ad032a91876ee9d328786034", "c0df707745bfe1cb3e81f44f8c7aa94b937fc4c2", "56659937fda0e141f606c328921a967c99b89e75", "e3782971046f04605200ee96015cf7d2b063b635", "1f2b8cb475b1a5c6ba179e5f9fcdbebab4a3240d", "15905f88334a8be22d08fc4f8bd87edebffc2014", "ecb3a35a9d931f4988e9364a3e0bc9b4babf51d5", "1327be035ea026dc865f25f6e3a7539843ef9929"]
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

Implemented §1 Token Scoping end-to-end on top of the recovered checkpoint lineage (base `73a31e0b`).

**Claims (`auth.rs`).** `AgentJwtClaims` now carries `tenant_id: String`, `workspace_id: Option<String>`, `repo_id: Option<String>` (None for workspace orchestrators), `persona: Option<String>`, `attestation_level: Option<u32>`, and `scope` changed from a hardcoded `String` to `Vec<String>` fine-grained scopes. All token-creation paths updated (spawn, orchestrator, stale-agent restart, jj/merge_deps/mcp tests).

**Minting from hierarchy context.** `spawn_agent_core` resolves repo → workspace → tenant and mints worker tokens with `scope=["repo:{name}:write"]`. `spawn_orchestrator` (and `stale_agents::restart_orchestrator`) resolve workspace → tenant and repo name; `mint_orchestrator` mints workspace tier `["workspace:{id}:read","workspace:{id}:spawn"]` and repo tier `["repo:{name}:write","repo:{name}:spawn"]` (repo_name required for repo tier, else mint fails). Persona + attestation parameters exist on `mint_with_workload`; orchestrator mints set persona to the tier slug. Worker mint passes persona/attestation as None — the spawn request carries no persona input and workload attestation is recorded post-spawn (KV `workload_attestations`, G10 claims available on the same struct); noted as residual in the coverage row.

**`AuthenticatedAgent`** exposes `workspace_id`/`repo_id`/`scope` derived from signature-verified claims via `with_hierarchy_from_claims`, called on every JWT-resolved auth path (agent-token, ws-ticket, Keycloak validate_jwt, federated JWT).

**MCP enforcement on every tool call.** `validate_tool_scope` runs in `tools/call` before dispatch for every tool; Gyre-minted agent JWTs (non-empty scope list) are gated by tier: worker tools (heartbeat/complete/record_activity/create_mr/list_mrs) require a matching `repo:` scope (write, or spawn which implies write); spawn tools (`gyre_spawn_worker`, `gyre_spawn_repo_orchestrator`) additionally require the `spawn` right — a worker's write-only token is rejected; workspace tools (`gyre_list_repo_orchestrators`, `gyre_cross_repo_task`) require matching `workspace:` scope; cross-tier task tools accept matching repo or workspace scope; a claims-array-with-empty-derived-scope fails closed. Scope mismatch returns a `PERMISSION_DENIED` JSON-RPC error naming the target repo/workspace. Non-scope-list callers (global token, API key, Keycloak JWT, legacy UUID tokens) keep the pre-existing ABAC/RBAC path. Handler-level `orchestrator_type` tier checks remain as defense in depth.

**Repairs made this round (review 6403a8e6).** (1) The pre-existing `mcp_decompose_spec_creates_subtasks_in_repo_scope` test asserted the OLD denial shape (`json["result"]["isError"]`) for a foreign-parent `gyre_decompose_spec` call, but the pre-dispatch `validate_tool_scope` gate correctly returns a JSON-RPC `PERMISSION_DENIED` error — `json["result"]` was Null and the unwrap panicked, making the full lib suite fail deterministically (1219 passed / 1 failed at the candidate). The assertion now checks `json["error"]["message"]` contains `PERMISSION_DENIED` and names the target task (parent-2) and repo (r-x), matching the `mcp_scope_*` tests. (2) Deleted dead `check_repo_read_scope` (never called — repo-read targets go through `check_repo_read_target`; it was the suite's only compiler warning). Earlier review defects (gyre_update_task vacuous gate, gyre_search cross-workspace read leak) were verified already fixed at this head: `gyre_update_task`/`gyre_decompose_spec` resolve the target task's repo/workspace from `state.tasks` and check them against the caller's scope; `gyre_search` validates its explicit `workspace_id` arg against the caller's own workspace claim; graph/spec-assist read tools go through `check_repo_read_target`.

**Merge of base ed57c723 (task-201 + task-097 revision round).** Resolved three conflicts: spawn.rs keeps mint-time `tenant_id` resolution (§1 Token Scoping) while adopting the non-lossy UTF-8 `GYRE_CRED_*` handling (skip-and-warn on non-UTF-8 secrets instead of `from_utf8_lossy` mangling); the fabricated-scope-defaults exemptions registry takes main's drained 5-entry state (the constraint_check site was fixed identically on main — exemption removed, `FROZEN_EXEMPTION_COUNT=5` restored); coverage SUMMARY totals recomputed from the merged per-file rows (802 total / 219 n/a / 310 assigned / 110 implemented / 163 verified, 46%).

**Test evidence.** `cargo test -p gyre-server --lib` — 1249 passed / 0 failed at head 7da23e04 (includes the previously failing decompose_spec test and the 7 `mcp_scope_*` tests: worker cross-repo MR denied naming r-2; worker own-repo create_mr passes scope gate; worker cannot spawn workers; workspace orchestrator cannot repo-write; workspace tools allowed in own workspace, denied in ws-2 naming it; global token unaffected). Output recorded under `/tmp/stage/review-evidence/full-lib-output.txt`. Static gates at head: `check-arch.sh`, `check-mcp-write-tools.sh`, `check-fabricated-scope-defaults.sh`, `check-lossy-secret-conversion.sh`, `check-inert-enforcement.sh` all OK (gate logs in `/tmp/stage/review-evidence/gate-*.log`); `cargo check -p gyre-server` clean of warnings after the dead-code removal.

Sandbox TCP listeners unsupported (errno 95) per `/tmp/stage/capabilities.json` — no live HTTP/browser probes; those belong to host verification and exact-head GitHub CI, as do the full workspace suite, all-target Clippy, and `cargo test --all`.
