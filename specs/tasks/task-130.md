---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["371bed863d11d347ac239d99921ea2ef7bed1b37", "78ac5cafce92ae65451b5ed3ee95ba9181d8265d", "894f7175715c63960f7b25dda9d2188b744c6ec0"]
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

Implemented `platform-model.md` §1 Token Scoping end-to-end. This round is the rebase repair of the merge conflicts from rebasing the branch onto base `774737638` (task-177 rendering + task-140 persona resolution + task-099 system-principal/API-key tenant fixes). The §1 implementation is unchanged in substance; every conflict was resolved to keep BOTH sides' enforcement.

### This round: rebase onto 774737638 (merge f6ea08e94)

**auth.rs.** Base moved system/agent-token construction to `system_principal_tenant()`, API-key auth to the user's stored tenant binding (fail-closed when absent), and added `is_system_principal()` (user_id discriminator, not a spoofable name match). Task-130 added the §1 hierarchy fields (`workspace_id`/`repo_id`/`scope`) to the same construction sites. Merged: every site uses base's tenant resolution AND carries the §1 fields (`None`/empty for non-JWT paths); the impl block keeps both `with_hierarchy_from_claims` (mirrors all §1 fields from signature-verified claims) and `is_system_principal`.

**orchestrator.rs.** Base replaced the soft `validate_persona` with fail-closed `resolve_orchestrator_persona` (scope chain repo→workspace→tenant, unapproved → Conflict, missing → NotFound) and added `LaunchOutcome` to the spawn tail. Task-130 moved tenant containment into the spawn cores (`caller_tenant` param) so the MCP spawn tools (no middleware ABAC) are contained too. Merged: base's persona resolution + `LaunchOutcome` returns + task-130's core-level containment; base's 4-arg test calls updated to pass the seeded tenant. The REST handlers combine base's `is_system_principal` bypass with the core containment (for the bypassed system principal they pass the target workspace's tenant so the core comparison matches base's semantics for the bootstrap principal).

**stale_agents.rs.** Base's `match &token` with task-130's token-registration comment.

**exemption files.** Base fixed the task-099 scope-literal sites for real (`system_principal_tenant()` helper, user-bound tenant, workspace-tenant-resolving emit paths, `bootstrap_workspace()` in domain agent.rs), so its side of `scope-literal-defaults-exemptions.txt` has 9 fewer entries and no `agent.rs` entry — took base's deletions (12 entries, frozen count unchanged); re-pointed `domain/merge_request.rs` line drift (119 → 124) to the merged tree. `inert-enforcement-exemptions.txt` and `abac-exempt-handlers-exemptions.txt`: no entries on either side; comment blocks combined. Zero new exemptions anywhere.

**spawn.rs (auto-merge fallout).** Base changed the tenant resolution to `Option<String>`; the secret-accessed audit log inside the `Some` arm now logs the matched `tenant` rather than the `Option`.

### Implementation (unchanged in substance)

`AgentJwtClaims` (auth.rs) carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, `jti`, and `scope: Vec<String>`, minted from real hierarchy context in spawn.rs / orchestrator.rs / stale_agents.rs; `with_hierarchy_from_claims` mirrors all hierarchy fields (including tenant) from signature-verified claims on every JWT-resolved path. `validate_tool_scope` runs before dispatch on every `tools/call` for scoped tokens (worker/spawn/workspace/task-tool tiers); mismatch returns a `PERMISSION_DENIED` JSON-RPC error naming the target; `gyre_list_mrs {}` clamps to the caller's signed repo via `list_by_repo` (never the unfiltered store); worker-tool targets (`gyre_agent_heartbeat`/`gyre_record_activity`) are bound to the caller's repo scope via `check_agent_target_scope`, unresolvable targets fail closed. Orchestrator spawn cores enforce tenant containment (`spawn_*_orchestrator_core` compares the loaded entity's tenant against the caller's); persona resolution is fail-closed (`resolve_orchestrator_persona`). Residual (unchanged): worker mint passes persona/attestation_level as None (no persona input on spawn; attestation recorded post-spawn).

### Prior repair rounds (retained)

- **Verification repair (commit `371bed863`)**: rustfmt on task-touched files, one clippy borrow lint, inert-enforcement exemptions deleted (fix-and-delete contract) rather than re-pointed.
- **Rebase onto `fefde2946` (merge `149111bd2`)**: kept §1 `scope: Vec<String>` AND base's `jti` claim (all three construction sites mint both); task-103 tenant-claim schema-default merged cleanly; base's new tests (jti uniqueness, task-098 secrets-resource, task-099 exempt-handler sites) updated to the §1 claims shape (`agent_auth` mints repo/workspace-tier scope lists and mirrors `with_hierarchy_from_claims`).
- **Rebase onto `c5deca801` (merge `afb1d86bd`)**: base's spec-approve agent detection replaced with the §1 array-based detection; `register_e2e_agent` updated to the 17-arg `mint_with_workload` hierarchy signature with attestation from the fixture.
- **Review 92601fb6 (commit `78ac5caf`)**: F1 `gyre_list_mrs` no-arg fallback clamped to the caller's signed repo; F2 worker-tool target binding (see Implementation above).

### Test evidence (this round, head f6ea08e94)

- `cargo test -p gyre-server --lib`: **1541 passed / 0 failed** (includes the task-130 mcp_scope_* suite, the orchestrator containment/persona/LaunchOutcome tests from both sides, and base's is_system_principal spoof-shape test updated to the §1 claims shape).
- Integration suites: auth_integration 21/21, m18_oidc_integration 8/8, e2e_ralph_loop 1/1, api_integration 66/66. Log: /tmp/stage/review-evidence/integration-tests.log.
- `cargo check --all --all-targets`: clean (whole workspace).
- `cargo clippy -p gyre-server --all-targets --all-features`: **0 warnings on changed lines vs base 774737638** (596 pre-existing elsewhere, ignored by the changed-lines gate). Log: /tmp/stage/review-evidence/clippy-changed-lines-analysis.txt (raw JSON: clippy-gyre-server-alltargets.json).
- `cargo fmt --all --check`: clean. All static gates green (check-arch, check-abac-route-registry, check-mcp-write-tools, check-migration-versions, check-dead-message-kinds, check-byte-slice-truncation, check-relative-path-defaults, check-fail-open-ref-resolution, check-task-commit-attribution, check-mem-port-contracts, check-lossy-secret-conversion, check-inert-enforcement [crates], check-scope-literal-defaults [crates], check-fabricated-scope-defaults [crates], check-forwarded-header-trust, check-in-memory-state-stores, check-unbounded-external-http, check-migration-sql-portability, check-forged-scope-fields, check-abac-exempt-handlers, check-dead-parameters, check-ignored-tool-tests, check-unwritten-store-fields, check-warn-continue-creation). Log: /tmp/stage/review-evidence/static-gates.log.
- `check-mcp-wrapper-parity.sh` fails on pre-existing sites (verified exit 1 on base `774737638` in a clean worktree — not a regression of this merge).

Full workspace suite, all-target Clippy on every crate, and exact-head GitHub CI remain with verification/publication.
