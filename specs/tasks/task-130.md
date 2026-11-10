---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["371bed863d11d347ac239d99921ea2ef7bed1b37", "78ac5cafce92ae65451b5ed3ee95ba9181d8265d", "894f7175715c63960f7b25dda9d2188b744c6ec0", "b326f2711"]
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

Implemented `platform-model.md` §1 Token Scoping end-to-end. This round is the rebase repair of the merge conflicts from rebasing the branch onto base `dbf9edaea` (task-117 compute target model). The §1 implementation is unchanged in substance; every conflict was resolved to keep BOTH sides' enforcement.

### This round: rebase onto dbf9edaea (merge `b326f2711`)

**spawn.rs worker-mint region (2 conflict sites).** Base (task-117) added pre-mint `wl_image_hash` resolution (docker `image inspect` / SSH remote digest, best-effort, Kubernetes defers to the post-spawn attestation record); task-130 had made the same region the §1 hierarchy mint (fail-closed `ws` binding, tenant/workspace/repo claims, mint failure fails the spawn — no UUID-token downgrade). Merged: BOTH — the §1 hierarchy args AND `wl_image_hash` flow into the single `mint_with_workload` call (the auto-merged auth.rs signature carries task-130's hierarchy params plus base's `wl_image_hash` param); HEAD's fail-closed `.map_err(...)?` error handling kept over base's UUID-token fallback.

**spawn.rs tests (2 conflict sites).** Base rewrote the env-delivery test infrastructure to a stub-`kubectl` Pod-manifest capture (`pod_env_map`, `do_spawn_with_target`) and added `kubernetes_pod_working_dir_is_workspace_not_local_worktree`; base's ghost-workspace test still expected the task-097 skip-secret-resolution behavior (spawn SUCCEEDS, secrets skipped). Task-130 had converted that test to `spawn_unresolvable_workspace_fails_closed` (404, no agent, no container). Merged: base's new k8s workingDir test kept verbatim; the ghost-workspace test stays the §1 fail-closed variant — the merged production code fails closed on a missing workspace row (it is the tenant-scope source for the minted JWT), so base's expects-"active" variant would contradict shipped behavior. Also dropped the post-conflict common lines that only made sense against base's `env` binding (git had aligned HEAD's closing `);` with base's).

**signal_chain.rs (auto-merge fallout).** Base's new file called `spawn_*_orchestrator_core` with the pre-§1 4-arg signature (task-130 added `caller_tenant`). The signal chain is a server-originated (system) spawn with no authenticated caller; fixed by passing the target entity's REAL tenant — `ensure_workspace_orchestrator` loads the workspace row, `run_repo_orchestrator` loads the repo's workspace row — failing closed on dangling references, mirroring the HTTP handlers' documented system-principal bypass. Never a fabricated identity.

**auth.rs / orchestrator.rs tests (auto-merge fallout).** Base's `mint_with_workload_roundtrips_wl_image_hash_claim` minted with the pre-§1 11-arg signature and 5-arg `.mint()`; updated to the 17-arg hierarchy signature (assertions unchanged — the test proves wl_* claim roundtrip, not hierarchy). Base's `stale_detector_refreshes_system_orchestrator_instead_of_churning` called the workspace-orchestrator core without `caller_tenant`; updated to the seeded ws-1 tenant (`t1`). One clippy lint from my own edit (`useless_conversion`) fixed.

### Implementation (unchanged in substance)

`AgentJwtClaims` (auth.rs) carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, `jti`, `scope: Vec<String>`, and base's `wl_image_hash`, minted from real hierarchy context in spawn.rs / orchestrator.rs / stale_agents.rs; `with_hierarchy_from_claims` mirrors all hierarchy fields (including tenant) from signature-verified claims on every JWT-resolved path. `validate_tool_scope` runs before dispatch on every `tools/call` for scoped tokens (worker/spawn/workspace/task-tool tiers); mismatch returns a `PERMISSION_DENIED` JSON-RPC error naming the target; `gyre_list_mrs {}` clamps to the caller's signed repo via `list_by_repo` (never the unfiltered store); worker-tool targets (`gyre_agent_heartbeat`/`gyre_record_activity`) are bound to the caller's repo scope via `check_agent_target_scope`, unresolvable targets fail closed. Orchestrator spawn cores enforce tenant containment (`spawn_*_orchestrator_core` compares the loaded entity's tenant against the caller's); persona resolution is fail-closed (`resolve_orchestrator_persona`). Residual (unchanged): worker mint passes persona/attestation_level as None (no persona input on spawn; attestation recorded post-spawn).

### Prior repair rounds (retained)

- **Verification repair (commit `371bed863`)**: rustfmt on task-touched files, one clippy borrow lint, inert-enforcement exemptions deleted (fix-and-delete contract) rather than re-pointed.
- **Rebase onto `fefde2946` (merge `149111bd2`)**: kept §1 `scope: Vec<String>` AND base's `jti` claim (all three construction sites mint both); task-103 tenant-claim schema-default merged cleanly; base's new tests (jti uniqueness, task-098 secrets-resource, task-099 exempt-handler sites) updated to the §1 claims shape (`agent_auth` mints repo/workspace-tier scope lists and mirrors `with_hierarchy_from_claims`).
- **Rebase onto `c5deca801` (merge `afb1d86bd`)**: base's spec-approve agent detection replaced with the §1 array-based detection; `register_e2e_agent` updated to the 17-arg `mint_with_workload` hierarchy signature with attestation from the fixture.
- **Review 92601fb6 (commit `78ac5caf`)**: F1 `gyre_list_mrs` no-arg fallback clamped to the caller's signed repo; F2 worker-tool target binding (see Implementation above).
- **Rebase onto `774737638` (merge `f6ea08e94`)**: task-177 rendering + task-140 fail-closed persona resolution + task-099 system-principal/API-key tenant fixes. auth.rs sites merged base's `system_principal_tenant`/user-bound API-key tenant with the §1 fields; orchestrator.rs kept base's `resolve_orchestrator_persona` + `LaunchOutcome` and task-130's core-level `caller_tenant` containment; scope-literal exemption entries took base's real fixes (9 deletions).
- **Rebase onto `b5a872087` (merge `f69bd657a`)**: task-108 admin user impersonation — auth-success auditing + `impersonation` field merged with §1 hierarchy mirroring at all four `AuthenticatedAgent` sites; impersonation.rs got `scope: Vec::new()`/None hierarchy (user principals are ABAC/RBAC-gated, not token-scoped); base's impersonation/auth-failure tests and five direct-drive spawn tests updated to the §1 claims shape (`seed_ws1_workspace` helper for the now-mandatory workspace row).

### Test evidence (this round, head `b326f2711`)

- `cargo test -p gyre-server --lib`: **1674 passed / 0 failed** — includes the task-130 `mcp_scope_*` suite, the repaired `spawn_unresolvable_workspace_fails_closed`, base's new `kubernetes_pod_working_dir_is_workspace_not_local_worktree`, `mint_with_workload_roundtrips_wl_image_hash_claim`, `stale_detector_refreshes_system_orchestrator_instead_of_churning`, and the full signal_chain suite (which exercises the repaired system-driven orchestrator spawns). Log: /tmp/stage/review-evidence/lib-tests.log.
- All 10 integration suites green: api_integration 66/66, auth_integration 21/21, m18_oidc_integration 8/8, explorer_ws_integration 15/15, git_integration 17/17, graph_integration 38/38, conversation_integration 3/3, dependency_persistence 1/1, e2e_ralph_loop 1/1, search_wiring 1/1. Log: /tmp/stage/review-evidence/integration-tests.log.
- `cargo check --workspace --all-targets`: clean (whole workspace, all merge-touched crates).
- `python3 scripts/check-clippy-diff.py cc62f8d56`: **PASS — changed lines clean** (40 changed Rust files, 924 pre-existing warnings outside changes). Log: /tmp/stage/review-evidence/clippy-changed-lines.log.
- `cargo fmt --all --check`: clean. All static gates green, re-run after the merge commit (check-arch, check-abac-route-registry, check-mcp-write-tools, check-migration-versions, check-dead-message-kinds, check-byte-slice-truncation, check-relative-path-defaults, check-fail-open-ref-resolution, check-task-commit-attribution, check-mem-port-contracts, check-lossy-secret-conversion, check-inert-enforcement, check-scope-literal-defaults, check-fabricated-scope-defaults, check-forwarded-header-trust, check-in-memory-state-stores, check-unbounded-external-http, check-migration-sql-portability, check-forged-scope-fields, check-abac-exempt-handlers). Log: /tmp/stage/review-evidence/static-gates.log.
- `check-mcp-wrapper-parity.sh` was not re-run this round; it fails on pre-existing sites verified against base `774737638` in a clean worktree during a previous round (not a regression; this round's changes touch no MCP wrapper sites).

Full workspace suite, all-target Clippy on every crate, and exact-head GitHub CI remain with verification/publication.
