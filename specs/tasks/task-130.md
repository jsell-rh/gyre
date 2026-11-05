---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["a2f36db7eefb9bf5e400240bb6d3a5ccba1cd077"]
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

Implemented `platform-model.md` §1 Token Scoping end-to-end. This round repairs the review 7be3363d finding (m18 regression + unscoped-token bypass, previously reported unremedied as 4e8e1b2).

### Claims and minting

`AgentJwtClaims` (auth.rs) carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, and `scope: Vec<String>`. Workers mint `["repo:{name}:write"]`, workspace orchestrators `["workspace:{id}:read","workspace:{id}:spawn"]`, repo orchestrators `["repo:{name}:write","repo:{name}:spawn"]` — all resolved from real hierarchy context (repo → workspace → tenant) in spawn.rs / orchestrator.rs / stale_agents.rs. `AuthenticatedAgent` exposes `workspace_id`/`repo_id`/`scope` derived from signature-verified claims on every JWT-resolved auth path (agent-token, ws-ticket, Keycloak, federated JWT). Residual (unchanged): worker mint passes persona/attestation_level as None — the spawn request carries no persona input and attestation is recorded post-spawn (workload_attestations KV).

### MCP scope gate

`validate_tool_scope` runs before dispatch on every `tools/call` for scoped (Gyre-minted) tokens: worker tools (heartbeat/complete/record_activity/create_mr/list_mrs) require a matching `repo:` scope; spawn tools additionally the `spawn` right; workspace tools a matching `workspace:` scope; cross-tier task tools resolve the target task's repo/workspace from state and check both. Mismatch returns a `PERMISSION_DENIED` JSON-RPC error naming the target repo/workspace. Non-scope-list callers (global token, API key, Keycloak JWT, legacy UUID tokens) keep the pre-existing ABAC/RBAC path. Earlier review defects (gyre_update_task/gyre_decompose_spec target resolution, gyre_search workspace arg, stale decompose_spec assertion shape, dead check_repo_read_scope) were fixed in prior rounds and verified present-correct at this head.

### This round: worker spawn fails closed on unresolvable workspace

`spawn_agent_core` previously skipped the JWT pre-mint and issued a legacy UUID token whenever the repo's workspace_id had no workspace row — a state reachable via `POST /api/v1/repos` (which accepts any workspace_id). That UUID token is classified non-scoped by `validate_tool_scope` and bypassed the entire §1 gate; it also broke m18_oidc_integration deterministically (3 of 8). Now the workspace lookup at the start of the spawn fails closed: a missing row returns 404 naming the workspace and repo ("the tenant scope for the agent token cannot be determined — fix the repo's workspace reference before spawning") BEFORE any agent record is created, and a mint failure returns 500 — an agent that cannot receive a scoped token is never downgraded to an unscoped one. This mirrors the orchestrator spawn path and the stale-restart path, which already failed closed. Remedy choice per the review's disjunction ("fail the spawn closed OR validate workspace existence at repo creation"): the fail-closed gate is implemented because it closes the security dimension with minimal blast radius — repo-creation validation additionally breaks ~85 pre-existing fixtures across 14 modules unrelated to this task (agent_tracking, jj, mr_timeline, compose, specs_assist, ...) and deserves its own task.

### Test evidence

- `cargo test -p gyre-server` (lib + integration binaries) at head 4ad379a7: lib 1262 passed / 0 failed; api_integration 66, auth_integration 21, git_integration 16, graph_integration 37, m18_oidc_integration **8/8** (the 3 previously failing tests now pass), explorer_ws 8, conversation/dependency/e2e/search green. Full output: /tmp/stage/review-evidence/full-server-output.txt.
- m18 fixtures now create real workspaces (spawn needs the tenant-scope source row); `token_info_for_jwt_agent_token` strengthened to assert the §1 claims: scope array contains `repo:info-repo:write`, `repo_id`/`workspace_id` match the fixture, `tenant_id` non-empty.
- New discriminating test `spawn_unresolvable_workspace_fails_closed` (spawn.rs): a dangling-workspace repo seeded directly in the store → spawn rejected 404 naming ws-ghost, no agent record created, no container dispatched (no env dump — the F3 LEAK secret can never be delivered). It replaces `spawn_unresolvable_workspace_skips_secret_resolution`, whose premise (spawn succeeds with a UUID token) is exactly the bypass this round removes.
- Static gates OK at head (logs in /tmp/stage/review-evidence/gate-*.log): check-arch, check-mcp-write-tools, check-fabricated-scope-defaults, check-lossy-secret-conversion, check-inert-enforcement, check-relative-path-defaults, check-scope-literal-defaults, check-migration-versions, check-task-commit-attribution; `cargo check -p gyre-server` warning-free. The relative-path-defaults exemption entry for the spawn.rs prose comment was re-pointed :597 → :631 (same pre-existing site, line shifted by this diff; entry count unchanged). All other failing workspace gates fail identically at the pre-change base (verified by stash-diff — no new findings).
- Known pre-existing flake, untouched by this task: `api::scim::tests::scim_delete_user` can fail under full-suite parallelism because scim tests mutate the process-global `GYRE_SCIM_TOKEN` env var with different values (scim.rs:451/574, introduced in M23 #296); passes in isolation and in 2 of 3 full runs here.

Full workspace suite, all-target Clippy, and exact-head GitHub CI remain with verification/publication.
