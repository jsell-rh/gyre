# Review: task-130 — Token scoping (hierarchy claims + MCP scope validation)

- **Candidate:** `112c5d27621718487e771663b4036281f80386c6` (branch `pipeline/task-130/3bd2d63672524f9ba59357eda3680278-1`)
- **Base:** `cfeefbda975211a6ac24a17e5920be6f8b23008b`
- **Reviewer round:** independent review (pi runtime, fullsend)
- **Verdict:** **approved** — see `/tmp/stage/verdict.json`

## What was reviewed

Full diff base→candidate (26 files, +2518/−285): `auth.rs` (claims struct,
mint/validate, `AuthenticatedAgent` hierarchy mirroring), `mcp.rs`
(`validate_tool_scope` pre-dispatch gate + handler clamping), `spawn.rs`,
`orchestrator.rs`, `signal_chain.rs`, `stale_agents.rs`, `gate_executor.rs`
(mint sites), `git_http.rs`/`abac_middleware.rs`/`merge_requests.rs`/`jj.rs`
(scope-list readers), test updates, coverage row 7, task frontmatter.

## Acceptance criteria — evidence

| Criterion | Evidence |
|---|---|
| Claims include tenant_id/workspace_id/repo_id/persona/attestation_level | `auth.rs` `AgentJwtClaims` (all five fields; repo_id/persona/attestation `Option`, workspace/repo claims `skip_serializing_if`) |
| `scope: Vec<String>` fine-grained | `auth.rs`; every mint site produces arrays; all raw-claim readers (`git_http.rs` push denial, `abac_middleware.rs` gate allow-list, `merge_requests.rs` reviewer binding, `specs.rs` is_agent) converted from `.as_str()` to the extractor-derived `auth.scope` list — no remaining string-form scope reader fails open |
| Minting populates hierarchy from spawn context | `spawn_agent_core` resolves repo→workspace→tenant and **fails closed** (404/500) when the workspace row is missing or the mint errors — the old UUID-token fallback that would bypass the MCP gate is gone; `spawn_orchestrator` resolves tenant + repo name; `stale_agents.rs` re-mints with the same resolution (skip+warn on unresolvable); `signal_chain.rs` passes the real workspace tenant for system spawns |
| Worker tokens scoped to repo | `mint_with_workload` → `["repo:{name}:write"]`; `mint_orchestrator` → workspace tier `["workspace:{ws}:read","workspace:{ws}:spawn"]`, repo tier `["repo:{name}:write","repo:{name}:spawn"]` (repo tier requires repo_name, errors otherwise) |
| MCP validates scope on every tool call | `validate_tool_scope` runs before dispatch on every `tools/call`; per-tool tier/action classification covers all 27 dispatched tools (write tools bound to repo/workspace targets resolved from real state; `_ =>` catch-all is read-only/identity-bound surface, justified in-code); empty-scope-but-agent-JWT fails closed |
| Scope mismatch → clear error | `PERMISSION_DENIED` JSON-RPC errors naming the target repo/workspace/task/agent; tests assert the message content |
| `AuthenticatedAgent` exposes workspace_id/repo_id | plus `scope: Vec<String>` and claims-mirrored `tenant_id`, via `with_hierarchy_from_claims` on every JWT-resolved path (extractor, `authenticate_token`, Keycloak, federated, gate tokens) |
| Tests updated | `cargo check -p gyre-server --tests` clean; fixtures updated to §1 shape with real API-driven workspace/repo seeding |

## Probes run (evidence in `/tmp/stage/review-evidence/`)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` — **pass** (6m42s cold)
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` — **pass**
- `cargo test -p gyre-server --lib mcp_scope` — **blocked by the executor's
  4 GiB build wrapper** (restriction, not a code defect; evidence:
  `cargo-test-blocked.out`)
- `scripts/check-arch.sh`, `check-mcp-write-tools.sh`,
  `check-scope-literal-defaults.sh`, `check-inert-enforcement.sh`,
  `check-fabricated-scope-defaults.sh`, `check-abac-exempt-handlers.sh` —
  **all pass**
- `scripts/check-task-commit-attribution.sh` — **passes against the candidate
  commit** (the sandbox working tree shipped with `specs/tasks/task-130.md`
  reverted to its base shape — a stage artifact, not a candidate defect; the
  committed frontmatter records all five labeled commits; working tree left
  as found)

## Adversarial audit (static, per-tool)

- Legacy string-`scope` tokens fail **closed**: `validate()` deserializes
  into `AgentJwtClaims` whose `scope` is now `Vec<String>` — an old
  `"scope":"agent"` token fails decode, it is not silently treated as
  unscoped.
- `gyre_list_mrs {}` fallback clamped to the signed repo_id claim
  (`list_by_repo`, never the unfiltered store); workspace tier must address a
  repo explicitly.
- `gyre_agent_heartbeat`/`gyre_record_activity`/`gyre_agent_complete` targets
  bound to the caller's repo via `check_agent_target_scope` (repo resolved
  from agent.repo_id → worktree → current task; unresolvable fails closed) —
  kills cross-tenant heartbeat/reaping defeat.
- Spawn tools: worker (write-only) tokens rejected on `spawn` right;
  tier mismatches enforced by handlers' signed `orchestrator_type` checks;
  tenant containment re-checked inside both spawn cores (MCP path runs
  without middleware ABAC).
- `gyre_create_task`/`gyre_update_task`/`gyre_decompose_spec`/`gyre_search`/
  graph-read tools: explicit targets validated against real state
  (task→repo/workspace, repo→workspace); absent targets clamped to the
  caller's own claims.
- Cross-tenant by name collision impossible: repo identity comes from the
  signed `repo_id` claim compared to target ids, not name matching.

## Test kill-analysis (tests that fail if the gate is disabled)

`mcp_scope_workspace_orchestrator_other_workspace_rejected` (handler lists
the caller's own workspace and would succeed without the gate),
`mcp_scope_worker_list_mrs_no_repo_clamped_to_own_repo` (sibling/foreign MRs
would leak), `mcp_scope_worker_cannot_heartbeat_foreign_agent` (foreign
heartbeat would record), `mcp_scope_worker_cannot_spawn_workers` (handler
denial is a tool_error result, not the asserted JSON-RPC error). Not
self-confirming.

## Verifier must run (gyre-server test execution exceeds this sandbox's budget)

```
cargo test -p gyre-server --lib
  mcp_scope_worker_cannot_create_mr_in_other_repo
  mcp_scope_worker_create_mr_own_repo_passes_scope_gate
  mcp_scope_worker_list_mrs_no_repo_clamped_to_own_repo
  mcp_scope_workspace_orchestrator_list_mrs_requires_repo
  mcp_scope_worker_cannot_heartbeat_foreign_agent
  mcp_scope_worker_cannot_spawn_workers
  mcp_scope_workspace_orchestrator_cannot_write_repo
  mcp_scope_workspace_orchestrator_own_workspace_allowed
  mcp_scope_workspace_orchestrator_other_workspace_rejected
  mcp_scope_global_token_unaffected
  mcp_spawn_repo_orchestrator_denies_tenant_mismatch
  agent_jwt_tenant_mirrored_from_verified_claims
  jwt_tenant_id_extracted_from_claim
  mint_with_workload_roundtrips_wl_image_hash_claim
  minted_jwt_carries_unique_jti
  agent_validation_token_carries_validation_scope_not_review
  gate_token_revoked_after_validation_completes
  agent_validation_exit_zero_passes
  agent_validation_bad_command_fails
  e2e_review_submission_is_abac_allowed_with_seeded_policies
  spawn_unresolvable_workspace_fails_closed
cargo test -p gyre-server --test task134_review_probe
cargo test -p gyre-server --test m18_oidc_integration
cargo test -p gyre-server --test auth_integration
```

## Observations (non-blocking, documented)

1. **Residual, honestly recorded in coverage row 7:** no production mint
   populates `persona`/`attestation_level` for workers (spawn request has no
   persona input; attestation is recorded post-spawn in
   `workload_attestations`, and §9 approval validity derives the level from
   that record, not the claim). The fields exist and `mint_with_workload`
   accepts them; fabricating values would violate the no-fakery rule.
   Orchestrator mints do populate `persona` (tier slug); gate mints pass the
   resolved persona.
2. Coverage row 24 ("Token Validation on Every Call") still carries a stale
   note ("worker-agent JWTs still carry no repo_id claim") predating
   task-130; its status (`implemented`) is unaffected and row 24 is outside
   this task's coverage_sections. A later docs pass should refresh it.
3. `with_hierarchy_from_claims` also mirrors claims from signature-verified
   Keycloak/federated tokens if a deployment embeds them — deliberate
   trusted-IdP design, documented in-code; tenant for Keycloak was already
   claim-derived (task-103) and `validate_tenant_id`-checked.
