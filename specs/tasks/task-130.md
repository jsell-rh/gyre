---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["371bed863d11d347ac239d99921ea2ef7bed1b37", "78ac5cafce92ae65451b5ed3ee95ba9181d8265d", "894f7175715c63960f7b25dda9d2188b744c6ec0", "f69bd657a"]
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

Implemented `platform-model.md` §1 Token Scoping end-to-end. This round is the rebase repair of the merge conflicts from rebasing the branch onto base `b5a872087` (task-108 admin user impersonation). The §1 implementation is unchanged in substance; every conflict was resolved to keep BOTH sides' enforcement.

### This round: rebase onto b5a872087 (merge f69bd657a)

**auth.rs (4 conflict sites).** Base (task-108) added auth-success auditing (`record_auth_success` gated on `auth_already_audited`, `token_kind` distinguishing `agent_jwt`/`uuid_token`) and the `impersonation: Option<ImpersonationContext>` field to every `AuthenticatedAgent` construction; task-130 added the §1 hierarchy mirroring (`with_hierarchy_from_claims`) at the same four sites (agent-token path in the extractor, agent-token path in the secondary authenticator, Keycloak JWT path, federated JWT path). Merged: every site carries base's audit block + `impersonation` field AND task-130's mirroring of the signature-verified §1 claims onto the resolved principal.

**fabricated-scope-defaults-exemptions.txt.** Comment-only conflict: both sides describe the same two fixed legacy entries (git_http ws_id, auth.rs tenant claim) with the entry count already lowered to 2. Combined the provenance notes from both sides; entries unchanged, `FROZEN_EXEMPTION_COUNT=2` unchanged.

**impersonation.rs (auto-merge fallout).** Base's new `resolve_impersonation_auth` constructs `AuthenticatedAgent` without the §1 fields task-130 made mandatory. Added `workspace_id`/`repo_id: None` and `scope: Vec::new()` — impersonation targets a user principal, and user tokens carry no §1 hierarchy claims (gated by ABAC/RBAC, not token scope).

**auth.rs tests (auto-merge fallout).** Base's `impersonation_jwt_rejects_agent_jwt` and `auth_failure_audit_event_recorded_for_revoked_agent_jwt` mint via the old 5-arg `.mint()`; updated to the §1 hierarchy signature with tenant/workspace/repo context.

**spawn.rs tests (auto-merge fallout).** Base's five direct-drive `spawn_agent_core` unit tests (F6 legacy-path, F9 missing-branch, worktree-path cases 1a/1b, R19 bookmark-export ordering) seed a repo referencing `ws-1` without the workspace row; §1 spawn fails closed when the repo's workspace row is missing (the workspace is the tenant-scope source for the minted agent JWT), so the tests now seed `ws-1` via a new `seed_ws1_workspace` helper (tenant `default`, matching the spawner fixture principal). Test literals carry the §1 fields (`None`/empty for the spawner principal).

### Implementation (unchanged in substance)

`AgentJwtClaims` (auth.rs) carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, `jti`, and `scope: Vec<String>`, minted from real hierarchy context in spawn.rs / orchestrator.rs / stale_agents.rs; `with_hierarchy_from_claims` mirrors all hierarchy fields (including tenant) from signature-verified claims on every JWT-resolved path. `validate_tool_scope` runs before dispatch on every `tools/call` for scoped tokens (worker/spawn/workspace/task-tool tiers); mismatch returns a `PERMISSION_DENIED` JSON-RPC error naming the target; `gyre_list_mrs {}` clamps to the caller's signed repo via `list_by_repo` (never the unfiltered store); worker-tool targets (`gyre_agent_heartbeat`/`gyre_record_activity`) are bound to the caller's repo scope via `check_agent_target_scope`, unresolvable targets fail closed. Orchestrator spawn cores enforce tenant containment (`spawn_*_orchestrator_core` compares the loaded entity's tenant against the caller's); persona resolution is fail-closed (`resolve_orchestrator_persona`). Residual (unchanged): worker mint passes persona/attestation_level as None (no persona input on spawn; attestation recorded post-spawn).

### Prior repair rounds (retained)

- **Verification repair (commit `371bed863`)**: rustfmt on task-touched files, one clippy borrow lint, inert-enforcement exemptions deleted (fix-and-delete contract) rather than re-pointed.
- **Rebase onto `fefde2946` (merge `149111bd2`)**: kept §1 `scope: Vec<String>` AND base's `jti` claim (all three construction sites mint both); task-103 tenant-claim schema-default merged cleanly; base's new tests (jti uniqueness, task-098 secrets-resource, task-099 exempt-handler sites) updated to the §1 claims shape (`agent_auth` mints repo/workspace-tier scope lists and mirrors `with_hierarchy_from_claims`).
- **Rebase onto `c5deca801` (merge `afb1d86bd`)**: base's spec-approve agent detection replaced with the §1 array-based detection; `register_e2e_agent` updated to the 17-arg `mint_with_workload` hierarchy signature with attestation from the fixture.
- **Review 92601fb6 (commit `78ac5caf`)**: F1 `gyre_list_mrs` no-arg fallback clamped to the caller's signed repo; F2 worker-tool target binding (see Implementation above).
- **Rebase onto `774737638` (merge `f6ea08e94`)**: task-177 rendering + task-140 fail-closed persona resolution + task-099 system-principal/API-key tenant fixes. auth.rs sites merged base's `system_principal_tenant`/user-bound API-key tenant with the §1 fields; orchestrator.rs kept base's `resolve_orchestrator_persona` + `LaunchOutcome` and task-130's core-level `caller_tenant` containment; scope-literal exemption entries took base's real fixes (9 deletions).

### Test evidence (this round, head f69bd657a)

- `cargo test -p gyre-server --lib`: **1631 passed / 0 failed** (includes the task-130 mcp_scope_* suite, base's task-108 impersonation suite, the orchestrator containment/persona tests, and the four repaired base spawn tests). Log: /tmp/stage/review-evidence/lib-tests.log.
- Integration suites: auth_integration 21/21, m18_oidc_integration 8/8, e2e_ralph_loop 1/1, api_integration 66/66. Log: /tmp/stage/review-evidence/integration-tests.log.
- `cargo check --workspace --all-targets`: clean (whole workspace).
- `python3 scripts/check-clippy-diff.py HEAD^1`: **PASS — 0 warnings on changed lines** (58 changed Rust files, 930 pre-existing elsewhere, ignored by the changed-lines gate); also verified 0 changed-line failures crate-locally (`cargo clippy -p gyre-server --all-targets --all-features`). Logs: /tmp/stage/review-evidence/clippy-changed-lines-analysis.txt (raw JSON: clippy-workspace.json, clippy-gyre-server-alltargets.json, base comparison: clippy-base.json).
- `cargo fmt --all --check`: clean. All static gates green, re-run after the merge commit (check-arch, check-abac-route-registry, check-mcp-write-tools, check-migration-versions, check-dead-message-kinds, check-byte-slice-truncation, check-relative-path-defaults, check-fail-open-ref-resolution, check-task-commit-attribution, check-mem-port-contracts, check-lossy-secret-conversion, check-inert-enforcement, check-scope-literal-defaults, check-fabricated-scope-defaults, check-forwarded-header-trust, check-in-memory-state-stores, check-unbounded-external-http, check-migration-sql-portability, check-forged-scope-fields, check-abac-exempt-handlers). Log: /tmp/stage/review-evidence/static-gates.log.
- `check-mcp-wrapper-parity.sh` was not re-run this round; it fails on pre-existing sites verified against base `774737638` in a clean worktree during the previous round (not a regression of this merge; this round's changes touch no MCP wrapper sites).

Full workspace suite, all-target Clippy on every crate, and exact-head GitHub CI remain with verification/publication.
