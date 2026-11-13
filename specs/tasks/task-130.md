---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["f3beafe789e921436efa8603d7c6412ebaf84502", "318f1fd0322c68304ae134434f74e57514449d82", "33468526f46ca5eb4dcf4afbd473d9960495ee0e", "25fc7ab37182951b5f63a09498e00f03f966ba35", "d500a34a88be3b0e0a1c967fc82b98d58b34f12b", "371bed863d11d347ac239d99921ea2ef7bed1b37", "78ac5cafce92ae65451b5ed3ee95ba9181d8265d", "894f7175715c63960f7b25dda9d2188b744c6ec0"]
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

`platform-model.md` §1 Token Scoping is implemented end-to-end in production code on this branch. This round repairs the three test failures from the verification run (`cargo test --all`, round `4199d965`: 3 failed / 2001 passed).

### §1 Token Scoping implementation (unchanged this round)

`AgentJwtClaims` (`gyre-server/src/auth.rs`) carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, `jti`, `scope: Vec<String>`, and workload claims (`wl_image_hash`), minted with full hierarchy context via `mint_with_workload` from `spawn.rs` / `orchestrator.rs` / `stale_agents.rs` / `signal_chain.rs` / `ralph_loop.rs`: worker agents get `["repo:{repo_name}:write"]`, workspace orchestrators get `["workspace:{ws}:read", "workspace:{ws}:spawn"]`, repo orchestrators get `["repo:{name}:write", "repo:{name}:spawn"]`. `AuthenticatedAgent` mirrors all hierarchy fields from signature-verified claims. `validate_tool_scope` in `mcp.rs` runs before dispatch on every `tools/call` for scoped tokens and rejects every mismatch with a `PERMISSION_DENIED` JSON-RPC error naming the target (no silent allow; unresolvable targets fail closed). See prior shipped notes in git history for the full rebase-repair and scope-literal-cleanup ledger.

### This round: verification-failure repair (`f3beafe7`)

All three failures were test-fixture defects against the (correct) fail-closed §1 mint, not production bugs:

- **`api::orchestrator::tests::workspace_orchestrator_spawn_and_scoped_jwt`**: asserted the `persona` claim equals `"workspace_orchestrator"` — a copy-paste from the `orchestrator_type` serde name asserted two lines above. The persona claim truthfully carries the persona slug `"workspace-orchestrator"` (the slug `resolve_orchestrator_persona` resolves and bootstrap step 5 registers); minting the underscore form would fabricate a slug matching no persona. Assertion corrected to the real slug.
- **`gate_executor::tests::agent_review_context_resolves_blob_pinned_spec_ref`**: the fixture seeded the `ws-test` repo/MR/persona rows but never the workspace row, so the §1 gate-agent mint's fail-closed MR → workspace → tenant resolution correctly errored "workspace ws-test not found". Fixed by seeding the workspace via the existing `ensure_workspace_row` helper (the same pattern `seed_full_review_fixture` already uses).
- **`gate_executor::tests::validation_token_ttl_covers_configured_gate_timeout`**: invoked `run_agent_validation_gate` with an unseeded `make_mr_id()`; the §1 mint now requires the MR to exist before spawning the validator ("merge request … not found"). Switched to `seed_mr_with_repo(&state)`, exactly like its passing sibling `agent_validation_token_carries_validation_scope_not_review`.

### Test evidence

This sandbox follows the 4 GiB budget rule (no gyre-server test binaries here). At head `f3beafe7`: `cargo check -p gyre-server --lib` exit 0; `cargo fmt --check -p gyre-server` clean; evidence saved under `/tmp/stage/review-evidence/` (repair diff, cargo-check exit, head SHAs).

The independent verifier must run: `cargo test --all` (the prior failing round's command), in particular `cargo test -p gyre-server --lib` slices `api::orchestrator::tests::workspace_orchestrator_spawn_and_scoped_jwt`, `gate_executor::tests::agent_review_context_resolves_blob_pinned_spec_ref`, and `gate_executor::tests::validation_token_ttl_covers_configured_gate_timeout`, plus the §1 slices from prior rounds (mcp/auth/abac §1 tests, `--test search_wiring`, `--test task134_review_probe`), `cargo test -p gyre-adapters --lib`, and `cargo test -p gyre-domain`.
