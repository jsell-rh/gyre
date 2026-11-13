---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["25fc7ab37182951b5f63a09498e00f03f966ba35", "d500a34a88be3b0e0a1c967fc82b98d58b34f12b", "371bed863d11d347ac239d99921ea2ef7bed1b37", "78ac5cafce92ae65451b5ed3ee95ba9181d8265d", "894f7175715c63960f7b25dda9d2188b744c6ec0", "19b92e072f610b4c7b7e374ca85b9761a2f11148"]
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

`platform-model.md` §1 Token Scoping is implemented end-to-end in production code on this branch (head `19b92e072`, rebase repair onto `349a32ac6`).

### Implementation

`AgentJwtClaims` (`gyre-server/src/auth.rs`) carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, `jti`, `scope: Vec<String>`, and workload claims (`wl_image_hash`), minted with full hierarchy context via `mint_with_workload` from `spawn.rs` / `orchestrator.rs` / `stale_agents.rs` / `signal_chain.rs` / `ralph_loop.rs`: worker agents get `["repo:{repo_name}:write"]`, workspace orchestrators get `["workspace:{ws}:read", "workspace:{ws}:spawn"]`, repo orchestrators get `["repo:{name}:write", "repo:{name}:spawn"]`. `with_hierarchy_from_claims` mirrors all hierarchy fields (including tenant) from signature-verified claims into `AuthenticatedAgent` on every JWT-resolved path. `validate_tool_scope` in `mcp.rs` runs before dispatch on every `tools/call` for scoped tokens: repo-scoped tools require a matching `repo:{name}` scope, workspace-scoped tools a matching `workspace:{ws}` scope, `gyre_list_mrs {}` clamps to the caller's signed repo via `list_by_repo`, worker-tool targets are bound to the caller's repo scope via `check_agent_target_scope`, and every mismatch returns a `PERMISSION_DENIED` JSON-RPC error naming the target (no silent allow; unresolvable targets fail closed). Orchestrator spawn cores enforce tenant containment; persona resolution is fail-closed.

### This round: rebase repair onto `349a32ac6` (merge `19b92e072`)

Resolved the four content conflicts (`spawn.rs`, `auth.rs`, `mcp.rs`, `stale_agents.rs`) keep-both, and repaired the silent auto-merge breakage the same way the prior round did for the gate-agent path:

- **`mint_worker_scoped`** (upstream's identity-security.md worker mint) arrived in the pre-§1 shape — `scope` as a plain string, no `tenant_id` claim — a compile break and a fail-open containment hole. It now delegates to `mint_with_workload` with `tenant_id` + `repo_name`, so every worker token carries the hierarchy claims and the `repo:{name}:write` scope list.
- **Worker spawn mint** now populates `attestation_level` at mint time from the resolved compute target (identity-security.md: 1=raw subprocess, 2=SSH, 3=container; no target = level 1, never an absent claim) and keeps the fail-closed mint error (upstream's UUID-token fallback would have minted a scope-less token that bypasses `validate_tool_scope`).
- **`mint_orchestrator`** keeps both sides: upstream's `persona: &str` param (replacing HEAD's `persona := orchestrator_type` stand-in) and §1's `tenant_id`/`repo_name` params.
- **`relaunch_agent_session`** (upstream's ralph-loop re-spawn) resolves the tenant fail-closed from the repo's workspace and mints with the agent's bound persona slug + compute-target attestation level — the re-spawn path no longer passes persona/attestation as `None`.
- **`search_scope.rs`**: upstream's `is_agent_jwt` read `scope == "agent"` as a string — under §1 the claim is an array, so every §1 token silently fell out of the agent-JWT path. Now recognizes the array form; workers search their signed workspace/repo claims with the agent record still required (unknown callers fail closed).
- **`gyre_search`** keeps §1 workspace clamping for scoped tokens on top of upstream's §Query Language facet parser; a scoped caller passing a foreign `workspace_id` is rejected with `PERMISSION_DENIED` by `validate_tool_scope` (upstream's silently-narrowed "No results" test expectation updated to assert the rejection).
- **`signal_chain.rs`**: upstream's `ensure_repo_orchestrator_for_failure` resolves the workspace tenant fail-closed for the §1 spawn core; `scheduler_system_auth` carries the §1 `AuthenticatedAgent` fields.
- Upstream test mints (`abac_middleware`, `mcp`, `auth`, `search_wiring`, `task134_review_probe`, `specs_assist`) updated to the §1 signatures with real seeded tenant/workspace rows; `search_wiring`'s worker test now asserts the §1 repo-claim scope (worker sees only its repo's doc), and its ghost-agent fail-closed assertion is preserved.

Known residual: first-spawn worker mint still passes `persona` as `None` (the spawn request takes no persona input); persona reaches the claim on orchestrator mints, gate-agent mints, and worker re-spawns.

### Test evidence

This sandbox follows the 4 GiB budget rule (no gyre-server test-binary execution here). Type checks pass at head `19b92e072`: `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` clean; `cargo clippy -p gyre-server --lib --tests` reports only pre-existing warnings outside the edited regions; `cargo fmt -p gyre-server -- --check` clean. Mechanical checks all pass at head: `check-arch.sh`, `check-scope-literal-defaults.sh`, `check-fabricated-scope-defaults.sh`, `check-inert-enforcement.sh`, `check-fail-open-ref-resolution.sh`, `check-lossy-secret-conversion.sh`, `check-forged-scope-fields.sh`, `check-byte-slice-truncation.sh`, `check-relative-path-defaults.sh`, `check-mcp-write-tools.sh`, `check-migration-versions.sh`, `check-dead-message-kinds.sh`, `check-mem-port-contracts.sh`, `check-abac-route-registry.sh`, `check-unbounded-external-http.sh`, `check-forwarded-header-trust.sh`, `check-in-memory-state-stores.sh`.

The independent verifier must run `cargo test -p gyre-server --lib` — §1 slices: `mcp_scope_worker_cannot_create_mr_in_other_repo`, `mcp_scope_worker_create_mr_own_repo_passes_scope_gate`, `mcp_scope_worker_list_mrs_no_repo_clamped_to_own_repo`, `mcp_scope_workspace_orchestrator_list_mrs_requires_repo`, `mcp_scope_worker_cannot_heartbeat_foreign_agent`, `mcp_scope_worker_cannot_spawn_workers`, `mcp_scope_workspace_orchestrator_cannot_write_repo`, `mcp_scope_workspace_orchestrator_own_workspace_allowed`, `mcp_scope_workspace_orchestrator_other_workspace_rejected`, `mcp_scope_global_token_unaffected` (mcp.rs); `agent_jwt_tenant_mirrored_from_verified_claims`, `jwt_tenant_id_extracted_from_claim`, `mint_with_workload_roundtrips_wl_image_hash_claim`, `minted_jwt_carries_unique_jti` (auth.rs); this round's merged upstream tests now exercising §1 tokens: `mcp_search_is_access_scoped_to_the_agent_token` (mcp.rs — includes the new PERMISSION_DENIED widening-rejection assertion), `agent_jwt_claims_reach_live_evaluation`, `agent_jwt_wl_stack_hash_normalized`, `agent_jwt_persona_and_attestation_level_reach_live_evaluation` (abac_middleware.rs — persona/attestation claims now minted through the §1 worker path); plus `cargo test -p gyre-server --test search_wiring` (`search_is_access_scoped_to_the_authenticated_caller` — worker repo-claim scope + ghost fail-closed) and `cargo test -p gyre-server --test task134_review_probe` (all five probes plus the spec-probe gate tokens minted with resolved tenant). The gate-agent slices (`agent_validation_token_carries_validation_scope_not_review`, `gate_token_revoked_after_validation_completes`, `agent_validation_exit_zero_passes`, `agent_validation_bad_command_fails`, `e2e_review_submission_is_abac_allowed_with_seeded_policies` in gate_executor.rs) are unchanged this round but exercise the merged mint path.

### Prior repair rounds (condensed)

- **Rebase onto `cfeefbda9` (`25fc7ab37`, merge `e415cdd5e`)**: gate-agent token path brought to the §1 shape (`mint_scoped` takes tenant + persona; capability as scope list; capability readers compare against extractor-derived `auth.scope`); validation-gate tests seed real MR/workspace rows.
- **Verification repair (`371bed863`)**: rustfmt, one clippy borrow lint, inert-enforcement exemptions deleted.
- **Review `92601fb6` (`78ac5cafc`)**: `gyre_list_mrs` no-arg fallback clamped to signed repo; worker-tool target binding.
- **Rebase onto `65f71e657` (merge `f90151146`, tests `d500a34a8`)**: task-107 Fulcio `bearer_token` vs §1 hierarchy-field conflicts at all four `AuthenticatedAgent` sites; base's task-107 tests updated to the §1 claims shape.
- **Earlier rebases** (`149111bd2`, `afb1d86bd`, `f6ea08e94`, `f69bd657a`, `b326f2711`): kept §1 `scope: Vec<String>` alongside base's `jti`, task-103 tenant-claim schema-default, task-140 fail-closed persona resolution, task-108 impersonation hierarchy mirroring, task-117 compute-target `wl_image_hash` — every conflict resolved to keep both sides' behavior.
