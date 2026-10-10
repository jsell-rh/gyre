---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["1f2b8cb475b1a5c6ba179e5f9fcdbebab4a3240d", "15905f88334a8be22d08fc4f8bd87edebffc2014", "ecb3a35a9d931f4988e9364a3e0bc9b4babf51d5", "1327be035ea026dc865f25f6e3a7539843ef9929"]
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

**Repairs made this round (previous assignment died mid-flight, exit 130).** Running the checkpoint's own tests surfaced two real defects in the saved implementation, both fixed: (1) `check_repo_scope`'s predicate `(right == "write" || (!require_spawn_right && right == "spawn") || right == "spawn")` had a dead `require_spawn_right` — the trailing `|| right == "spawn"` made the flag a no-op, so write-requirement and spawn-requirement were indistinguishable; (2) `check_repo_write_scope` and the cross-tier task tools passed `require_spawn_right: true` for plain write actions, rejecting workers' valid own-repo writes once the predicate actually worked. Both inverted/short-circuited conditions now enforce: spawn-tier actions need the `spawn` right; write actions accept `write` or `spawn`. The broken test setup for `mcp_scope_worker_create_mr_own_repo_passes_scope_gate` (worker registered without a worktree, contradicting TASK-216's every-spawned-agent-has-a-worktree invariant) was fixed by seeding a real `AgentWorktree` in `register_worker`, mirroring `spawn_agent_core`.

**Test evidence.** `cargo test -p gyre-server --lib mcp_scope` — 7/7 pass (worker cross-repo MR denied naming r-2; worker own-repo create_mr passes scope gate; worker cannot spawn workers; workspace orchestrator cannot repo-write; workspace tools allowed in own workspace, denied in ws-2 naming it; global token unaffected). Full `cargo test -p gyre-server --lib` run recorded under `/tmp/stage/review-evidence/`. Static gates: `check-task-commit-attribution.sh`, `check-scope-literal-defaults.sh`, `check-inert-enforcement.sh`, `check-mcp-write-tools.sh`, `check-mem-port-contracts.sh` all OK (exemption files unchanged in entry count — only line-number drift for existing task-099-owned entries).

Sandbox TCP listeners unsupported (errno 95) per `/tmp/stage/capabilities.json` — no live HTTP/browser probes; those belong to host verification and exact-head GitHub CI, as do the full workspace suite, all-target Clippy, and `cargo test --all`.
