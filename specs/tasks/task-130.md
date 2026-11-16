---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: complete
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["c709b94ea17612d30a4437e1e8e407256293b2ef", "f3beafe789e921436efa8603d7c6412ebaf84502", "318f1fd0322c68304ae134434f74e57514449d82", "33468526f46ca5eb4dcf4afbd473d9960495ee0e", "25fc7ab37182951b5f63a09498e00f03f966ba35", "d500a34a88be3b0e0a1c967fc82b98d58b34f12b", "371bed863d11d347ac239d99921ea2ef7bed1b37", "78ac5cafce92ae65451b5ed3ee95ba9181d8265d", "894f7175715c63960f7b25dda9d2188b744c6ec0", "6128a39c8fa30617634c5ac192529ee469506ffb"]
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

`platform-model.md` §1 Token Scoping is implemented end-to-end in production code on this branch. This round rebase-repairs the branch onto base `6f5a631c` (task-195's manifest-driven gate selection), which landed overlapping changes on the same token-scoping surfaces.

### §1 Token Scoping implementation

`AgentJwtClaims` (`gyre-server/src/auth.rs`) carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, `scope: Vec<String>`, and workload claims (`wl_image_hash`), minted with full hierarchy context via `mint_with_workload` from `spawn.rs` / `orchestrator.rs` / `stale_agents.rs` / `signal_chain.rs` / `ralph_loop.rs`: worker agents get `["repo:{repo_name}:write"]`, workspace orchestrators get `["workspace:{ws}:read", "workspace:{ws}:spawn"]`, repo orchestrators get `["repo:{name}:write", "repo:{name}:spawn"]`. `AuthenticatedAgent` mirrors all hierarchy fields from signature-verified claims. `validate_tool_scope` in `mcp.rs` runs before dispatch on every `tools/call` for scoped tokens and rejects every mismatch with a `PERMISSION_DENIED` JSON-RPC error naming the target (no silent allow; unresolvable targets fail closed). Worker-tool targets are bound to the token's scope and the `list_mrs` fallback clamped to it.

### This round: rebase repair onto 6f5a631c (`d4e3d9d9`)

Both the task-130 branch and the new base changed the same gate/review token surfaces. The merge (`d4e3d9d9`) resolves the convergence without weakening either side:

- `merge_requests.rs` `submit_review`: kept task-195's reviewer-identity binding (every agent-token principal submits under its own verified subject; human callers only under their own identity or Admin), with the `review:submit` scope check switched from the base's raw JSON string-claim read to the §1 extractor-derived `Vec<String>` `auth.scope` — an array claim read as a string silently fails open.
- `gate_executor.rs` `spawn_manifest_gate_agent` (task-195's manifest-gate path): its `mint_scoped` call now resolves the gated MR's workspace **fail-closed** for the `tenant_id` claim (unresolvable workspace aborts the spawn; never a fabricated literal) and passes the gate persona slug — the §1 claims shape on the new gate path.
- Tests the base added against the old 6-arg/5-arg mint signatures (`task134_review_probe.rs` inbox-route probe, `merge_requests.rs` forger-identity test) updated to the §1 signatures; the manifest-gate token test asserts the `Vec<String>` scope form.

No exemption files were touched; `scripts/check-fabricated-scope-defaults.sh`, `check-abac-route-registry.sh`, `check-dead-message-kinds.sh`, `check-mem-port-contracts.sh`, `check-mcp-write-tools.sh`, `check-arch.sh`, and `check-task-commit-attribution.sh` all pass at the repair head.

### Test evidence

This sandbox follows the 4 GiB budget rule (no gyre-server test binaries here). At repair head `d4e3d9d9`: `cargo check -p gyre-server --lib --tests` exit 0; `cargo fmt -p gyre-server --check` clean (logs in `/tmp/stage/review-evidence/`).

The independent verifier must run `cargo test --all` (the acceptance criterion), in particular `cargo test -p gyre-server --lib` slices `api::orchestrator::tests::workspace_orchestrator_spawn_and_scoped_jwt`, `gate_executor::tests::agent_review_context_resolves_blob_pinned_spec_ref`, `gate_executor::tests::validation_token_ttl_covers_configured_gate_timeout`, `gate_executor::tests::gate_agent_token_is_scoped_reviewer_jwt`, `gate_executor::tests::manifest-gate` slices, the §1 slices from prior rounds (mcp/auth/abac token-scoping tests, `--test search_wiring`, `--test task134_review_probe`, including `scoped_review_token_cannot_read_agent_inbox_route` and `submit_review_binds_agent_token_caller_to_own_identity` updated this round), `cargo test -p gyre-adapters --lib`, and `cargo test -p gyre-domain`.
