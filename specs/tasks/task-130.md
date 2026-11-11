---
title: "Implement token scoping: add hierarchy claims to AgentJwtClaims and validate in MCP"
spec_ref: "platform-model.md §1 Token Scoping"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Token Scoping"
commits: ["d500a34a88be3b0e0a1c967fc82b98d58b34f12b", "371bed863d11d347ac239d99921ea2ef7bed1b37", "78ac5cafce92ae65451b5ed3ee95ba9181d8265d", "894f7175715c63960f7b25dda9d2188b744c6ec0"]
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

## Agent Instructions

Read `specs/system/platform-model.md` §1 Token Scoping for the full spec. Current JWT implementation: `gyre-server/src/auth.rs` (AgentJwtClaims at line ~46, mint_with_workload at ~176). Token validation: `auth.rs` ~216. MCP server: `gyre-server/src/mcp.rs`. Agent spawn: `gyre-server/src/api/spawn.rs`. The `scope` field is currently a `String` hardcoded to `"agent"` — it needs to become `Vec<String>`. This is a breaking change to the JWT format, so update ALL token creation paths (search for `AgentJwtClaims` usage). Check migration numbering: `ls crates/gyre-adapters/migrations/ | tail -5` — currently at 000049.

## Shipped

`platform-model.md` §1 Token Scoping is implemented end-to-end. This round is the rebase repair of the merge conflicts from rebasing the branch onto base `65f71e657` (task-085 HSI Trust Suggestions); the §1 implementation is unchanged in substance, and every conflict was resolved to keep BOTH sides' behavior.

### This round: rebase onto 65f71e657 (merge `f90151146`)

**auth.rs (4 conflict sites).** Base's `bearer_token` field (task-107 Fulcio signing boundary — the raw bearer token must survive to the CA flow) collided with task-130's §1 hierarchy fields at all four `AuthenticatedAgent` construction sites (agent-token middleware path, agent-token extractor path, Keycloak user path, federated JWT path). Merged: BOTH — every site sets `workspace_id`/`repo_id`/`scope` AND `bearer_token`; the §1 `with_hierarchy_from_claims` mirroring after each site is preserved.

**Auto-merge fallout in test code (commit `d500a34a8`).** Base's new task-107 tests were authored against the pre-§1 shapes: `api/jj.rs`'s Fulcio-fallback test called the 5-arg `.mint()` (updated to the 9-arg §1 form with the fixture's real repo context — tenant "default", workspace "test-ws", the actual repo_id, repo name "test-repo"), and `commit_signatures.rs`'s two attribution-test `AuthenticatedAgent` literals were missing the hierarchy fields (updated to `None`/`Vec::new()` — user/system principals are ABAC-gated, not token-scoped, the same convention as impersonation.rs).

### Implementation (unchanged in substance)

`AgentJwtClaims` (auth.rs) carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, `jti`, `scope: Vec<String>`, and `wl_image_hash`/other workload claims, minted from real hierarchy context in spawn.rs / orchestrator.rs / stale_agents.rs; `with_hierarchy_from_claims` mirrors all hierarchy fields (including tenant) from signature-verified claims on every JWT-resolved path. `validate_tool_scope` runs before dispatch on every `tools/call` for scoped tokens (worker/spawn/workspace/task-tool tiers); mismatch returns a `PERMISSION_DENIED` JSON-RPC error naming the target; `gyre_list_mrs {}` clamps to the caller's signed repo via `list_by_repo`; worker-tool targets (`gyre_agent_heartbeat`/`gyre_record_activity`) are bound to the caller's repo scope via `check_agent_target_scope`, unresolvable targets fail closed. Orchestrator spawn cores enforce tenant containment; persona resolution is fail-closed (`resolve_orchestrator_persona`). Residual (unchanged): worker mint passes persona/attestation_level as None (no persona input on spawn; attestation recorded post-spawn).

### Test evidence (this round, head `d500a34a8`)

`SKIP_WEB_BUILD=1 cargo check -p gyre-server` and `cargo check -p gyre-server --tests` pass (the latter caught and fixed the 3 test-shape errors above); `rustfmt --check` on all touched files passes; `scripts/check-arch.sh` passes. gyre-server test binaries were not executed in this sandbox (4 GiB budget rule); the independent verifier must run `cargo test -p gyre-server --lib` — §1 slices: the ten `mcp_scope_*` tests in mcp.rs, `agent_jwt_tenant_mirrored_from_verified_claims`, `jwt_tenant_id_extracted_from_claim`, `mint_with_workload_roundtrips_wl_image_hash_claim`, `minted_jwt_carries_unique_jti`, plus the jj.rs Fulcio-fallback and commit_signatures.rs attribution tests updated this round. Full evidence and exact test names: `/tmp/stage/review-evidence/task-130-rebase-65f71e657.md`.

### Prior repair rounds (retained, condensed)

- **Verification repair (`371bed863`)**: rustfmt, one clippy borrow lint, inert-enforcement exemptions deleted.
- **Rebase onto `fefde2946` (`149111bd2`)**: kept §1 `scope: Vec<String>` AND base's `jti`; task-103 tenant-claim schema-default; base's new tests updated to §1 claims shape.
- **Rebase onto `c5deca801` (`afb1d86bd`)**: §1 array-based spec-approve detection; `register_e2e_agent` to 17-arg `mint_with_workload`.
- **Review `92601fb6` (`78ac5caf`)**: F1 `gyre_list_mrs` no-arg fallback clamped to signed repo; F2 worker-tool target binding.
- **Rebase onto `774737638` (`f6ea08e94`)**: task-177 rendering + task-140 fail-closed persona resolution + task-099 tenant fixes.
- **Rebase onto `b5a872087` (`f69bd657a`)**: task-108 impersonation merged with §1 hierarchy mirroring at all four `AuthenticatedAgent` sites.
- **Rebase onto `dbf9edaea` (`b326f2711`)**: task-117 compute target model — `wl_image_hash` and §1 hierarchy args both flow into the single `mint_with_workload` call; signal_chain.rs callers pass the target entity's real tenant, fail-closed.
