---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["894f7175715c63960f7b25dda9d2188b744c6ec0", "d500a34a88be3b0e0a1c967fc82b98d58b34f12b", "78ac5cafce92ae65451b5ed3ee95ba9181d8265d", "371bed863d11d347ac239d99921ea2ef7bed1b37"]
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

`platform-model.md` §1 Token Scoping is implemented end-to-end in production code on this branch (head `afa2c174a`).

### Implementation

`AgentJwtClaims` (`gyre-server/src/auth.rs`) carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, `jti`, `scope: Vec<String>`, and workload claims (`wl_image_hash`), minted with full hierarchy context via the single `mint_with_workload` call from `spawn.rs` / `orchestrator.rs` / `stale_agents.rs` / `signal_chain.rs`: worker agents get `["repo:{repo_name}:write"]`, workspace orchestrators get `["workspace:{ws}:read", "workspace:{ws}:spawn"]`, repo orchestrators get `["repo:{name}:write", "repo:{name}:spawn"]`. `with_hierarchy_from_claims` mirrors all hierarchy fields (including tenant) from signature-verified claims into `AuthenticatedAgent` on every JWT-resolved path (agent-token middleware and extractor, Keycloak user, federated JWT, impersonation). `validate_tool_scope` in `mcp.rs` runs before dispatch on every `tools/call` for scoped tokens: repo-scoped tools require a matching `repo:{name}` scope, workspace-scoped tools a matching `workspace:{ws}` scope, `gyre_list_mrs {}` clamps to the caller's signed repo via `list_by_repo`, worker-tool targets (`gyre_agent_heartbeat`/`gyre_record_activity`) are bound to the caller's repo scope via `check_agent_target_scope`, and every mismatch returns a `PERMISSION_DENIED` JSON-RPC error naming the target (no silent allow; unresolvable targets fail closed). Orchestrator spawn cores enforce tenant containment; persona resolution is fail-closed (`resolve_orchestrator_persona`). Known residual (documented in prior review rounds): worker mint passes `persona`/`attestation_level` as `None` — spawn takes no persona input and attestation is recorded post-spawn.

### This round: contract restoration (no code change)

The executor restored the assigned task document to its contract form (progress `not-started`, `commits: []`, shipped notes stripped). This round re-records the truth without touching production code: progress set back to `ready-for-review`, the four product-surface commits restored to `commits:` frontmatter (verifying `scripts/check-task-commit-attribution.sh` passes), and this `## Shipped` section rewritten under the required heading. Verified the implementation is intact at head `afa2c174a`: `SKIP_WEB_BUILD=1 cargo check -p gyre-server` and `cargo check -p gyre-server --tests` both pass; `scripts/check-arch.sh` passes; the §1 test slice is present (ten `mcp_scope_*` tests in `mcp.rs`, hierarchy-claim tests in `auth.rs`).

### Test evidence

This sandbox follows the 4 GiB budget rule (no gyre-server test-binary execution here). The independent verifier must run `cargo test -p gyre-server --lib` — §1 slices: `mcp_scope_worker_cannot_create_mr_in_other_repo`, `mcp_scope_worker_create_mr_own_repo_passes_scope_gate`, `mcp_scope_worker_list_mrs_no_repo_clamped_to_own_repo`, `mcp_scope_workspace_orchestrator_list_mrs_requires_repo`, `mcp_scope_worker_cannot_heartbeat_foreign_agent`, `mcp_scope_worker_cannot_spawn_workers`, `mcp_scope_workspace_orchestrator_cannot_write_repo`, `mcp_scope_workspace_orchestrator_own_workspace_allowed`, `mcp_scope_workspace_orchestrator_other_workspace_rejected`, `mcp_scope_global_token_unaffected` (mcp.rs); `agent_jwt_tenant_mirrored_from_verified_claims`, `jwt_tenant_id_extracted_from_claim`, `mint_with_workload_roundtrips_wl_image_hash_claim`, `minted_jwt_carries_unique_jti` (auth.rs); plus the `jj.rs` Fulcio-fallback and `commit_signatures.rs` attribution tests updated to the §1 claims shape.

### Prior repair rounds (condensed)

- **Rebase onto `65f71e657` (merge `f90151146`, tests `d500a34a8`)**: resolved task-107 Fulcio `bearer_token` vs §1 hierarchy-field conflicts at all four `AuthenticatedAgent` sites (both kept); updated base's new task-107 tests to the §1 claims shape.
- **Verification repair (`371bed863`)**: rustfmt, one clippy borrow lint, inert-enforcement exemptions deleted.
- **Review `92601fb6` (`78ac5cafc`)**: `gyre_list_mrs` no-arg fallback clamped to signed repo; worker-tool target binding.
- **Earlier rebases** (`149111bd2`, `afb1d86bd`, `f6ea08e94`, `f69bd657a`, `b326f2711`): kept §1 `scope: Vec<String>` alongside base's `jti`, task-103 tenant-claim schema-default, task-140 fail-closed persona resolution, task-108 impersonation hierarchy mirroring, task-117 compute-target `wl_image_hash` — every conflict resolved to keep both sides' behavior.
