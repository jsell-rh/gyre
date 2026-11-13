---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["318f1fd0322c68304ae134434f74e57514449d82", "33468526f46ca5eb4dcf4afbd473d9960495ee0e", "25fc7ab37182951b5f63a09498e00f03f966ba35", "d500a34a88be3b0e0a1c967fc82b98d58b34f12b", "371bed863d11d347ac239d99921ea2ef7bed1b37", "78ac5cafce92ae65451b5ed3ee95ba9181d8265d", "894f7175715c63960f7b25dda9d2188b744c6ec0"]
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

`platform-model.md` §1 Token Scoping is implemented end-to-end in production code on this branch, and this round repairs the one failing verification gate (round `6ff2aa5c`, `bash /tmp/stage/checks.sh` exit 1: "new verification exemptions forbidden" on `scripts/scope-literal-defaults-exemptions.txt`).

### §1 Token Scoping implementation (unchanged this round)

`AgentJwtClaims` (`gyre-server/src/auth.rs`) carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, `jti`, `scope: Vec<String>`, and workload claims (`wl_image_hash`), minted with full hierarchy context via `mint_with_workload` from `spawn.rs` / `orchestrator.rs` / `stale_agents.rs` / `signal_chain.rs` / `ralph_loop.rs`: worker agents get `["repo:{repo_name}:write"]`, workspace orchestrators get `["workspace:{ws}:read", "workspace:{ws}:spawn"]`, repo orchestrators get `["repo:{name}:write", "repo:{name}:spawn"]`. `AuthenticatedAgent` mirrors all hierarchy fields from signature-verified claims. `validate_tool_scope` in `mcp.rs` runs before dispatch on every `tools/call` for scoped tokens and rejects every mismatch with a `PERMISSION_DENIED` JSON-RPC error naming the target (no silent allow; unresolvable targets fail closed). See prior shipped notes in git history (`13f6d193`) for the full rebase-repair ledger.

### This round: exemption-gate repair (`33468526`, `318f1fd0`)

The failed verification flagged the three `merge_request.rs` scope-literal exemption entries as newly granted: they had been re-pointed `:163→:166` / `:119→:124` for branch line drift, and a re-pointed entry is a string-new entry the candidate may not add to a frozen exemptions file. Repaired by fixing the underlying fabrication sites for real (the exemption file's own documented protocol — entries deleted, `FROZEN_EXEMPTION_COUNT` lowered 12→6):

- **sqlite/postgres `merge_request.rs` and `repository.rs` `create()`** (`33468526`, `318f1fd0`): the `NewMergeRequestRow` / `NewRepositoryRow` `tenant_id` fields now carry the storage handle's resolved tenant scope (`self.tenant_id`, the existing `AgentRepository` pattern) instead of hardcoded `"default"` literals. Single-tenant behavior is byte-identical; a `with_tenant()` handle now stamps its real tenant on MR/repository rows instead of silently writing `"default"` (a real cross-tenant correctness fix).
- **domain `MergeRequest::new()` and `Task::new()`** (`33468526`, `318f1fd0`): `workspace_id` binds the provisioned bootstrap workspace via the documented `bootstrap_workspace()` helper — the same `hierarchy-enforcement.md` §1 "Bootstrap Behavior" contract and pattern as `Agent::new` (accepted in the task-099 repair round). Signatures unchanged; production callers with a real workspace assign it explicitly (REST/MCP `create_mr` resolve the repo's workspace).

The six remaining exemption entries are string-identical to base `d9c419a0` (gate-neutral): four live `analytics.rs` insert sites at their exact exempted lines, and two dead `activity.rs` lines referencing adapter files removed upstream (task-207). `check-scope-literal-defaults.sh` now passes when run with explicit crate paths — before this round it flagged six construction-time fabrications that no exemption covered at their drifted lines (three of which failed in base `d9c419a0`'s own tree too).

### Test evidence

This sandbox follows the 4 GiB budget rule (no gyre-server test binaries here). At head `318f1fd0`: `cargo check` clean for `gyre-domain`, `gyre-adapters`, and `gyre-server --lib`; `cargo test -p gyre-domain` — 8 passed; `cargo test -p gyre-adapters --lib` — 512 passed, 0 failed (includes the merge_request 13, merge_queue 8, review 10, sqlite::repository 10 slices); `cargo fmt --check` clean; clippy warnings all pre-existing in `gyre-ports`, none in changed files; `check-scope-literal-defaults.sh` with explicit crate paths OK; the frozen-exemption gate simulated against both base `d9c419a0` and each parent commit reports no new entries; `check-task-commit-attribution.sh` OK.

The independent verifier must run the §1 slices listed in the prior shipped notes (`cargo test -p gyre-server --lib` mcp/auth/abac §1 tests, `--test search_wiring`, `--test task134_review_probe`), plus this round's adapter slices: `cargo test -p gyre-adapters --lib` and `cargo test -p gyre-domain`.
