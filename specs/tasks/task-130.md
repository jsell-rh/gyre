---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["78ac5cafce92ae65451b5ed3ee95ba9181d8265d", "894f7175715c63960f7b25dda9d2188b744c6ec0"]
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

Implemented `platform-model.md` §1 Token Scoping end-to-end. This round repairs the two findings of review 92601fb6d780421cbda4ca4b576d31ac (candidate 15798cf5): the `gyre_list_mrs` no-target fallback silently listed every MR across all tenants, and the worker-tool scope arm never bound the caller-chosen `agent_id` target to the caller's repo.

### This round: list_mrs fallback clamped, worker-tool targets bound (commit 78ac5caf)

**F1 — gyre_list_mrs no-arg fallback.** `validate_tool_scope`'s `gyre_list_mrs` arm returned `Ok(())` when `repository_id` was absent on the false assumption that the handler rejects the missing field; `handle_list_mrs` treats it as optional and fell back to the shared unfiltered `merge_requests.list()` store. A worker token scoped to a single repo calling `tools/call gyre_list_mrs {}` silently received every MR across all workspaces/tenants. Now: the gate requires a single repo scope for repo-tier tokens (`check_repo_scope(None, read)`) and rejects workspace-tier tokens with a clear "pass repository_id" error (they carry no repo to clamp to); `handle_list_mrs` takes the caller and clamps the fallback listing to the caller's signed `repo_id` claim via `list_by_repo` — never the unfiltered store (unscoped ABAC-governed callers keep the pre-existing full listing). The with-arg path is unchanged (already denied for foreign repos).

**F2 — worker-tool target binding.** The worker-tool arm (`gyre_agent_heartbeat | gyre_agent_complete | gyre_record_activity`) only verified the caller's own token carries a repo-write right; `gyre_agent_heartbeat` had no identity or target check anywhere, so a worker scoped to r-1 could heartbeat any agent in any other repo/workspace/tenant, updating the foreign agent's `last_seen` and defeating cross-tenant stale-agent reaping. Now the arm additionally binds non-self `agent_id` targets via `check_agent_target_scope`: the target's real repo is resolved from state (repo orchestrators carry `repo_id` directly, workers via their worktree, current task's repo as final fallback) and checked against the caller's repo scope with `check_repo_target_by_id`; unresolvable targets fail closed. Self targets pass (identity is in scope by construction; `gyre_agent_complete` keeps its self-only dispatch check).

### Prior rounds (unchanged)

`AgentJwtClaims` (auth.rs) carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, and `scope: Vec<String>`, minted from real hierarchy context in spawn.rs / orchestrator.rs / stale_agents.rs; `with_hierarchy_from_claims` mirrors all hierarchy fields (including tenant) from signature-verified claims on every JWT-resolved path. `validate_tool_scope` runs before dispatch on every `tools/call` for scoped tokens (worker/spawn/workspace/task-tool tiers as described below); mismatch returns a `PERMISSION_DENIED` JSON-RPC error naming the target. Non-scope-list callers keep the ABAC/RBAC path. Residual (unchanged): worker mint passes persona/attestation_level as None (no persona input on spawn; attestation recorded post-spawn).

### Test evidence (this round, head 78ac5caf)

- New discriminating tests (mcp.rs):
  - `mcp_scope_worker_list_mrs_no_repo_clamped_to_own_repo` — MRs seeded in r-1 (own), r-2 (sibling repo, same workspace), r-x (other workspace); worker of r-1 calling `gyre_list_mrs {}` sees ONLY the r-1 MR (kills the F1 leak: fails if the unfiltered fallback returns), explicit own repo still passes.
  - `mcp_scope_workspace_orchestrator_list_mrs_requires_repo` — workspace-tier token with `{}` is denied with PERMISSION_DENIED naming repository_id; explicit in-workspace repo passes.
  - `mcp_scope_worker_cannot_heartbeat_foreign_agent` — worker of r-1 self-heartbeat records; heartbeating a foreign agent (r-x, ws-2) is denied pre-dispatch naming the target repo, and the foreign agent's `last_heartbeat` stays untouched; `gyre_record_activity` with the same foreign agent_id is denied too (kills F2).
- `cargo test -p gyre-server --lib` at head 78ac5caf: **1317 passed / 0 failed** (includes the 3 new tests above); `mcp::` module subset: 89/89. Logs: /tmp/stage/review-evidence/lib-test-full.log, mcp-tests.log, focused-tests.log.
- Static gates OK at head: check-arch, check-mcp-write-tools, check-inert-enforcement, check-abac-exempt-handlers, check-abac-route-registry, check-fabricated-scope-defaults, check-forged-scope-fields, check-scope-literal-defaults (both default and `crates` invocations), check-relative-path-defaults, check-lossy-secret-conversion, check-forwarded-header-trust, check-in-memory-state-stores, check-unbounded-external-http, check-byte-slice-truncation, check-dead-message-kinds, check-migration-versions, check-task-commit-attribution; `cargo check -p gyre-server` warning-free (source warnings only from build.rs web asset steps). The two scope-literal-defaults lib.rs exemption entries were re-pointed for pre-existing line drift (502→503, 546→547; same sites, entry count unchanged).
- Known pre-existing flake, untouched by this task: `api::scim::tests::scim_delete_user` under full-suite parallelism (process-global `GYRE_SCIM_TOKEN` env mutation, scim.rs:451/574); passes in isolation.

Full workspace suite, all-target Clippy, and exact-head GitHub CI remain with verification/publication.
