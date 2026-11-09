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

Implemented `platform-model.md` §1 Token Scoping end-to-end. This round is the rebase repair onto base `fefde2946` (merge commit `149111bd2`, parents `8382260d5` + `fefde2946`): the candidate's scope-format change met base's newer JWT-ID (`jti`) claim and task-103 tenant-claim schema-default fix mid-tree, producing two conflicts plus three semantic breaks the auto-merge could not see. Earlier rounds (review 92601fb6: list_mrs fallback leak, worker-tool target binding; rebase onto `c5deca801` merge `afb1d86bd`: spec-approve agent detection, e2e mint signature) remain fixed as described below.

### This round: rebase onto fefde2946 (merge 149111bd2)

**Conflicts.** `auth.rs`: kept the §1 `scope: Vec<String>` fine-grained list AND adopted base's `jti: String` claim (JWT ID recorded in `AgentTokenRevoked` audit events, observability.md); all three `AgentJwtClaims` construction sites now mint both. `fabricated-scope-defaults-exemptions.txt`: base's task-103 repair removed the `auth.rs` JWT claims.tenant_id entry (the use-site `unwrap_or("default")` fallback is gone in the merged tree — `tenant_id` is now the claim's serde schema default via `default_tenant_claim` on `JwtClaims`, verified present in the merged auth.rs), so the entry stays removed; kept the git_http.rs FIXED note. Re-pointed, never added: nine scope-literal entries for merge line drift in auth.rs (645/686/729/773/806/838/1291 → 689/730/773/817/850/882/1338; same seven pre-existing extractor sites) and lib.rs (515/559 → 520/564); 22 entries, frozen count unchanged.

**Semantic break 1 — old-signature mint call.** Base's new `minted_jwt_carries_unique_jti` test called the pre-task 5-arg `mint`; updated to the 9-arg hierarchy signature (tenant/workspace/repo fixtures) — jti uniqueness, extractability, and claims-match assertions preserved.

**Semantic break 2 — old-shape test auth helpers.** Base's new secrets-resource tests (task-098) built `AuthenticatedAgent` with the pre-task shape (no workspace_id/repo_id/scope fields, string `"scope": "agent"` claims — the old JWT format this task replaced). Under the merged tree they would not compile, and under the §1 array-based `is_agent_jwt` the string-scope claims would not resolve an agent identity at all (`agent_secret_scope` would return None and the secrets tests would fail). Updated the `agent_auth` helper to mint §1-format claims (repo tier: `scope: ["repo:{repo}:write"]` + repo_id; workspace tier: `workspace:{id}:read/spawn`) and mirror the hierarchy fields exactly as `with_hierarchy_from_claims` derives them; the user-token test got `workspace_id/repo_id/scope` fields (None/empty — its no-agent-identity error path is unchanged).

**Semantic break 3 — exemptions vs merged tree.** The `crates` invocation of check-scope-literal-defaults initially failed on line drift (8 findings); re-pointed as above after confirming each site is a pre-existing extractor/message constructor, not new code.

### Prior round: rebase onto c5deca801 (merge afb1d86bd)

**Exemption-file conflicts.** Each side had fixed sites the other still exempted; every resolution was verified against the merged tree, not picked by side: kept base's deletion of the task-087 `traces.rs:150` entry (span-payload tenant containment is present in the merged traces.rs, with its cross-tenant test) and of the `specs.rs`/`git_http.rs` fabricated-scope entries (both fixes present in the merged tree); kept ours' deletion of the task-093 `orchestrator.rs` entries (`spawn_*_orchestrator_core` tenant containment from this task is present); kept base's deletion of the `relative-path-defaults` `spawn.rs`/`gyre-cli main.rs` entries (both sites gone in the merged tree).

**Semantic breaks.** Base's spec-approve handler (migration 000057 feature) detected Gyre agent JWTs via `claims["scope"] == "agent"` — a string compare that never matches this task's `Vec<String>` scope format; replaced with the §1 array-based detection (non-empty scope list, identical to `is_agent_jwt` in mcp.rs; Keycloak user JWTs and legacy UUID tokens keep the human path). Base's `register_e2e_agent` test helper called the pre-task 11-arg `mint_with_workload`; updated to the 17-arg hierarchy signature with tenant/workspace/repo from the seeded e2e fixture and `attestation_level: Some(level)` matching the helper's registered attestation.

### Prior round: list_mrs fallback clamped, worker-tool targets bound (commit 78ac5caf)

**F1 — gyre_list_mrs no-arg fallback.** `validate_tool_scope`'s `gyre_list_mrs` arm returned `Ok(())` when `repository_id` was absent on the false assumption that the handler rejects the missing field; `handle_list_mrs` treats it as optional and fell back to the shared unfiltered `merge_requests.list()` store. A worker token scoped to a single repo calling `tools/call gyre_list_mrs {}` silently received every MR across all workspaces/tenants. Now: the gate requires a single repo scope for repo-tier tokens (`check_repo_scope(None, read)`) and rejects workspace-tier tokens with a clear "pass repository_id" error (they carry no repo to clamp to); `handle_list_mrs` takes the caller and clamps the fallback listing to the caller's signed `repo_id` claim via `list_by_repo` — never the unfiltered store (unscoped ABAC-governed callers keep the pre-existing full listing). The with-arg path is unchanged (already denied for foreign repos).

**F2 — worker-tool target binding.** The worker-tool arm (`gyre_agent_heartbeat | gyre_agent_complete | gyre_record_activity`) only verified the caller's own token carries a repo-write right; `gyre_agent_heartbeat` had no identity or target check anywhere, so a worker scoped to r-1 could heartbeat any agent in any other repo/workspace/tenant, updating the foreign agent's `last_seen` and defeating cross-tenant stale-agent reaping. Now the arm additionally binds non-self `agent_id` targets via `check_agent_target_scope`: the target's real repo is resolved from state (repo orchestrators carry `repo_id` directly, workers via their worktree, current task's repo as final fallback) and checked against the caller's repo scope with `check_repo_target_by_id`; unresolvable targets fail closed. Self targets pass (identity is in scope by construction; `gyre_agent_complete` keeps its self-only dispatch check).

### Prior rounds (unchanged)

`AgentJwtClaims` (auth.rs) carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, and `scope: Vec<String>`, minted from real hierarchy context in spawn.rs / orchestrator.rs / stale_agents.rs; `with_hierarchy_from_claims` mirrors all hierarchy fields (including tenant) from signature-verified claims on every JWT-resolved path. `validate_tool_scope` runs before dispatch on every `tools/call` for scoped tokens (worker/spawn/workspace/task-tool tiers as described below); mismatch returns a `PERMISSION_DENIED` JSON-RPC error naming the target. Non-scope-list callers keep the ABAC/RBAC path. Residual (unchanged): worker mint passes persona/attestation_level as None (no persona input on spawn; attestation recorded post-spawn).

### Test evidence (this round, head 149111bd2)

- `cargo test -p gyre-server --lib`: **1499 passed / 0 failed** (was 1445 at the prior round — base fefde2946 added the secrets-resource, jti/revocation-audit, and rollout-policy suites; all pass against the merged claims format). Log: /tmp/stage/review-evidence/lib-test-full.log.
- Integration suites touched by the merge: auth_integration 21/21, m18_oidc_integration 8/8, e2e_ralph_loop 1/1, api_integration 66/66. Log: /tmp/stage/review-evidence/integration-tests.log.
- Static gates green on the merged head (log: /tmp/stage/review-evidence/static-gates.log): check-arch, check-abac-exempt-handlers, check-abac-route-registry, check-fabricated-scope-defaults, check-scope-literal-defaults (default and `crates` invocations), check-relative-path-defaults, check-forged-scope-fields, check-forwarded-header-trust, check-lossy-secret-conversion, check-in-memory-state-stores, check-unbounded-external-http, check-byte-slice-truncation, check-dead-message-kinds, check-migration-versions, check-mcp-write-tools, check-inert-enforcement, check-task-commit-attribution, check-mem-port-contracts, check-migration-sql-portability. `cargo check -p gyre-server --lib` clean.
- Known pre-existing flake, untouched by this task: `api::scim::tests::scim_delete_user` under full-suite parallelism (process-global `GYRE_SCIM_TOKEN` env mutation, scim.rs:451/574); passes in isolation.

### Prior-round test evidence (head afb1d86bd)

- `cargo test -p gyre-server --lib`: **1445 passed / 0 failed** (was 1317 at 78ac5caf — base added the spec-approval-attestation suite; the 4 initially-failing e2e tests passed with the detection fix).
- New discriminating tests from review 92601fb6 (mcp.rs):
  - `mcp_scope_worker_list_mrs_no_repo_clamped_to_own_repo` — MRs seeded in r-1 (own), r-2 (sibling repo, same workspace), r-x (other workspace); worker of r-1 calling `gyre_list_mrs {}` sees ONLY the r-1 MR (kills the F1 leak: fails if the unfiltered fallback returns), explicit own repo still passes.
  - `mcp_scope_workspace_orchestrator_list_mrs_requires_repo` — workspace-tier token with `{}` is denied with PERMISSION_DENIED naming repository_id; explicit in-workspace repo passes.
  - `mcp_scope_worker_cannot_heartbeat_foreign_agent` — worker of r-1 self-heartbeat records; heartbeating a foreign agent (r-x, ws-2) is denied pre-dispatch naming the target repo, and the foreign agent's `last_heartbeat` stays untouched; `gyre_record_activity` with the same foreign agent_id is denied too (kills F2).
- `cargo test -p gyre-server --lib` at head 78ac5caf: **1317 passed / 0 failed** (includes the 3 new tests above); `mcp::` module subset: 89/89. Logs: /tmp/stage/review-evidence/lib-test-full.log, mcp-tests.log, focused-tests.log.
- Static gates OK at head: check-arch, check-mcp-write-tools, check-inert-enforcement, check-abac-exempt-handlers, check-abac-route-registry, check-fabricated-scope-defaults, check-forged-scope-fields, check-scope-literal-defaults (both default and `crates` invocations), check-relative-path-defaults, check-lossy-secret-conversion, check-forwarded-header-trust, check-in-memory-state-stores, check-unbounded-external-http, check-byte-slice-truncation, check-dead-message-kinds, check-migration-versions, check-task-commit-attribution; `cargo check -p gyre-server` warning-free (source warnings only from build.rs web asset steps). The two scope-literal-defaults lib.rs exemption entries were re-pointed for pre-existing line drift (502→503, 546→547; same sites, entry count unchanged).
- Known pre-existing flake, untouched by this task: `api::scim::tests::scim_delete_user` under full-suite parallelism (process-global `GYRE_SCIM_TOKEN` env mutation, scim.rs:451/574); passes in isolation.

Full workspace suite, all-target Clippy, and exact-head GitHub CI remain with verification/publication.
