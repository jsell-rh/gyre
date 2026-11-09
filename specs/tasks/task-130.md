---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["78ac5cafce92ae65451b5ed3ee95ba9181d8265d", "894f7175715c63960f7b25dda9d2188b744c6ec0", "371bed863d11d347ac239d99921ea2ef7bed1b37"]
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

Implemented `platform-model.md` §1 Token Scoping end-to-end. This round is the verification repair of the three findings from verification run `e5b03f0c` (`bash /tmp/stage/checks.sh`, exit 1, candidate `3dea1d2c` against base `fefde2946`): rustfmt on changed lines, one clippy lint, and re-pointed inert-enforcement exemption entries the verifier forbids. All repaired without behavior change; the §1 implementation itself is unchanged from the previously reviewed rounds summarized below.

### This round: verification repair (commit 371bed863)

**rustfmt.** The five task-touched files (orchestrator.rs, spawn.rs, specs.rs, auth.rs, mcp.rs) had changed lines needing formatting (multi-line constructs rustfmt collapses, e.g. short `vec![...]`/`mint(...)` calls). `cargo fmt --all` reflowed exactly those five files; `cargo fmt --all --check` now clean.

**clippy.** `mcp.rs:7388 needless_borrows_for_generic_args` — dropped the `&` on a `format!` argument in a test worktree fixture. Verified with `cargo clippy -p gyre-server --all-targets --all-features -W clippy::all`: **0 warnings on changed lines** vs base `fefde2946` (check-clippy-diff semantics); 625 pre-existing warnings elsewhere, all ignored by the changed-lines gate.

**inert-enforcement exemptions.** The verifier forbids new/changed entries in `scripts/inert-enforcement-exemptions.txt`; the prior round had re-pointed the two task-099 F2 `validate_persona` entries (187/283 → 212/308) for merge line drift. But the underlying violations are **already fixed for real in this branch**: `validate_persona` returns `bool` and both spawn cores bind and branch on the finding (no statement-position discard). Per the file's own rule ("When you fix one, DELETE its exemption line"), both entries — and the task-099 comment block that referenced them — were **deleted**, not re-pointed. The file is now a strict subset of base: deletions only, zero added lines, empty exemption set; `check-inert-enforcement.sh crates` passes on real enforcement (no exemptions needed).

**scope-literal line drift.** rustfmt shifted the seven pre-existing auth.rs extractor sites; re-pointed the exemption entries (689/730/773/817/850/882/1338 → 686/727/770/814/847/879/1335 — same seven pre-existing sites, entry count frozen at 24) so the `crates` invocation of check-scope-literal-defaults stays green. Note: that check's no-arg invocation iterates zero files; the `crates` invocation is the real gate and is what was verified.

### Implementation (unchanged this round)

`AgentJwtClaims` (auth.rs) carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, `jti`, and `scope: Vec<String>`, minted from real hierarchy context in spawn.rs / orchestrator.rs / stale_agents.rs; `with_hierarchy_from_claims` mirrors all hierarchy fields (including tenant) from signature-verified claims on every JWT-resolved path. `validate_tool_scope` runs before dispatch on every `tools/call` for scoped tokens (worker/spawn/workspace/task-tool tiers); mismatch returns a `PERMISSION_DENIED` JSON-RPC error naming the target; `gyre_list_mrs {}` clamps to the caller's signed repo via `list_by_repo` (never the unfiltered store); worker-tool targets (`gyre_agent_heartbeat`/`gyre_record_activity`) are bound to the caller's repo scope via `check_agent_target_scope`, unresolvable targets fail closed. Orchestrator spawn cores enforce tenant containment (`spawn_*_orchestrator_core` compares the loaded entity's tenant against the caller's). Residual (unchanged): worker mint passes persona/attestation_level as None (no persona input on spawn; attestation recorded post-spawn).

### Prior repair rounds (retained)

- **Rebase onto `fefde2946` (merge `149111bd2`)**: kept §1 `scope: Vec<String>` AND base's `jti` claim (all three construction sites mint both); task-103 tenant-claim schema-default merged cleanly; base's new tests (jti uniqueness, task-098 secrets-resource, task-099 exempt-handler sites) updated to the §1 claims shape (`agent_auth` mints repo/workspace-tier scope lists and mirrors `with_hierarchy_from_claims`).
- **Rebase onto `c5deca801` (merge `afb1d86bd`)**: base's spec-approve agent detection (`claims["scope"] == "agent"`) replaced with the §1 array-based detection; `register_e2e_agent` updated to the 17-arg `mint_with_workload` hierarchy signature with attestation from the fixture.
- **Review 92601fb6 (commit `78ac5caf`)**: F1 `gyre_list_mrs` no-arg fallback clamped to the caller's signed repo; F2 worker-tool target binding (see Implementation above).

### Test evidence (this round, head 371bed863)

- `cargo test -p gyre-server --lib`: **1499 passed / 0 failed**. Log: /tmp/stage/review-evidence/lib-test-full.log.
- Integration suites: auth_integration 21/21, m18_oidc_integration 8/8, e2e_ralph_loop 1/1, api_integration 66/66. Log: /tmp/stage/review-evidence/integration-tests.log.
- `cargo clippy -p gyre-server --all-targets --all-features -W clippy::all`: 0 warnings on changed lines vs base (625 pre-existing elsewhere, ignored by the changed-lines gate). Log: /tmp/stage/review-evidence/clippy-changed-lines-analysis.txt (raw JSON: clippy-gyre-server-alltargets.json).
- `cargo fmt --all --check`: clean. All static gates green including the `crates` invocations (check-inert-enforcement, check-scope-literal-defaults, check-relative-path-defaults, check-byte-slice-truncation, check-abac-exempt-handlers, check-dead-parameters, check-ignored-tool-tests, check-unwritten-store-fields, check-warn-continue-creation) plus the fixed-path gates. Log: /tmp/stage/review-evidence/static-gates-fmt.log; exemption-file diffs vs base: inert-enforcement-exemptions-diff-vs-base.txt (deletions only), scope-literal-exemptions-diff-vs-base.txt.
- Known pre-existing flake, untouched by this task: `api::scim::tests::scim_delete_user` under full-suite parallelism (process-global `GYRE_SCIM_TOKEN` env mutation, scim.rs:451/574); passes in isolation.

Full workspace suite, all-target Clippy on every crate, and exact-head GitHub CI remain with verification/publication.
