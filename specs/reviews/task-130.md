# Review — task-130 (Implement token scoping: hierarchy claims on AgentJwtClaims + MCP scope validation)

Spec: `specs/system/platform-model.md` §1 Token Scoping.
Candidate under review: `cbcd0c3e349bfee5336d344cdbbd25424441faa5` (branch
`pipeline/task-130/b6460d5eecc441c581346781ad081a43-1`, assigned base
`349a32ac6f5557786f82d17c13f8815de600e837`; the branch head is the result of
several rebase-repair rounds onto advancing bases, all preserved in history).
Verdict: **approved**.

## Probe evidence (saved under /tmp/stage/review-evidence)

- `cargo-check-gyre-server.txt` — `SKIP_WEB_BUILD=1 cargo check -p gyre-server
  --lib --tests` at the candidate: **exit 0, zero warnings** (only the
  SKIP_WEB_BUILD build-script notice). Server test binaries were NOT executed
  here per the 4 GiB budget rule; the tests the verifier must run are listed
  at the bottom.
- `mechanical-checks.txt` — all 18 repo invariant checks pass at the candidate:
  check-arch, check-abac-route-registry, check-abac-exempt-handlers,
  check-mcp-write-tools, check-migration-versions, check-dead-message-kinds,
  check-mem-port-contracts, check-fabricated-scope-defaults,
  check-scope-literal-defaults, check-lossy-secret-conversion,
  check-inert-enforcement, check-fail-open-ref-resolution,
  check-byte-slice-truncation, check-relative-path-defaults,
  check-forwarded-header-trust, check-in-memory-state-stores,
  check-unbounded-external-http, check-forged-scope-fields (all exit 0).
  Note: the scope-literal-defaults exemption file's line-number shifts
  (merge_request.rs 163→166 etc.) are corrections matching unchanged source —
  the base's entries were stale; no entries were added and FROZEN_EXEMPTION_COUNT
  (12) is unchanged.
- `fmt-check.txt` — `cargo fmt -p gyre-server -- --check`: clean.
- `clippy-lib.txt` — `cargo clippy -p gyre-server --lib`: exit 0; the 399
  warnings are pre-existing crate-wide style lints; the only hits in touched
  files (mcp.rs:2008/3190, auth.rs:229) are in pre-existing code regions, none
  in the new §1 code (the new mint functions carry `#[allow(too_many_arguments)]`).

## Verified against the task contract

- **Claims shape**: `AgentJwtClaims` (auth.rs) now carries `tenant_id`
  (String), `workspace_id`/`repo_id` (repo_id Option, None on workspace
  orchestrators), `persona`, `attestation_level: Option<u32>`, and
  `scope: Vec<String>`. `AgentJwtClaims` is constructed only inside auth.rs
  mint functions (grep: 4 sites — mint_with_workload, mint_orchestrator,
  mint_scoped, one test); no other mint path exists.
- **Minting from spawn context**: `spawn_agent_core` resolves repo → workspace
  → tenant and fails closed when the workspace row is dangling (404 naming
  both ids, before any agent record is created) and on mint error (the legacy
  UUID-token fallback is gone — it would have minted a scope-less token that
  bypasses the MCP gate); worker scope is `["repo:{name}:write"]` with
  attestation level from the resolved compute target.
  `mint_orchestrator` mints `["workspace:{id}:read","workspace:{id}:spawn"]`
  (workspace tier) / `["repo:{name}:write","repo:{name}:spawn"]` (repo tier)
  with tenant resolved from the workspace row (fail-closed).
  `stale_agents.rs` (replacement re-mint), `ralph_loop.rs` (re-spawn),
  `signal_chain.rs` (system-originated orchestrator spawns — real tenant of
  the target workspace, never a fabricated identity) and `gate_executor.rs`
  (gate-agent tokens carry the MR workspace's tenant + persona slug,
  fail-closed) all populate the hierarchy claims the same way.
- **AuthenticatedAgent**: `workspace_id`/`repo_id`/`scope` fields mirrored from
  signature-verified claims by `with_hierarchy_from_claims` on every
  JWT-resolved path (agent-token extractor path, authenticate_token,
  validate_jwt, validate_federated_jwt); `tenant_id` is mirrored from the
  claim when present (with a dedicated regression test:
  `agent_jwt_tenant_mirrored_from_verified_claims`).
- **MCP validates scope on every tool call**: `validate_tool_scope` runs in
  `mcp_handler` before the single tools/call dispatch match. Workers are
  gated to their single repo scope (create_mr/list_mrs/heartbeat/complete/
  record_activity — the latter's caller-chosen agent_id targets are bound to
  the caller's repo via `check_agent_target_scope` resolving the target's real
  repo from state, unresolvable fails closed); spawn tools require the spawn
  right (worker write-only tokens rejected); workspace tools gated to
  `workspace:{id}` scopes; task tools resolve the target task's real
  repo/workspace from state; `gyre_list_mrs` with no repository_id is clamped
  to the caller's signed repo (never the unfiltered cross-tenant store);
  `gyre_search`/`gyre_list_tasks` clamp scoped callers to their own
  workspace/repo. Mismatches return a `PERMISSION_DENIED` JSON-RPC error
  naming the target — no silent allow. Tools where validate_tool_scope defers
  to the handler (`gyre_spawn_worker`, `gyre_spawn_repo_orchestrator`) are
  genuinely tier-enforced there (`repo_orchestrator_scope` /
  `workspace_orchestrator_scope` + same-workspace repo check +
  `spawn_*_orchestrator_core` tenant containment, which now also compares the
  claims-derived caller tenant — regression-tested by
  `mcp_spawn_repo_orchestrator_denies_tenant_mismatch`).
- **No fail-open string reads of the array claim**: every production reader of
  the JWT `scope` claim now uses the array form or the extractor-derived
  `auth.scope` (abac_middleware reviewer/validator gate, submit_review
  identity binding, git_receive_pack gate-agent push block, specs.rs agent
  detection, mcp.rs, search_scope.rs). search_scope.rs's legacy string
  tolerance is harmless: pre-§1 tokens (scope:"agent" string) now FAIL JWT
  deserialization against `Vec<String>` and cannot authenticate at all — the
  breaking change is fail-closed, as the task specified.
- **Read-tier default-allow is documented, not hidden**: analytics/messaging/
  conversation-upload fall through `_ => Ok(())` with per-tool justification
  recorded in the coverage row; messaging enforces same-workspace
  destinations from the agent record (verified in source), so no cross-scope
  leak on that surface.
- **Tests are real, not self-confirming**: the `mcp_scope_*` suite drives the
  full router (`build_router`) with genuinely minted+registered EdDSA JWTs
  through the real MCP transport helper. Kill-proof analysis (source-level,
  since test binaries are deferred to the verifier):
  `mcp_scope_worker_cannot_spawn_workers` and
  `mcp_scope_workspace_orchestrator_list_mrs_requires_repo` assert a JSON-RPC
  `error` object, which only the pre-dispatch `validate_tool_scope` gate
  produces on those tools (the handler denials are tool_result errors) — they
  fail if the gate is disabled. `mcp_scope_worker_list_mrs_no_repo_clamped_to_own_repo`
  seeds MRs in three scopes and asserts sibling/foreign MRs absent — fails if
  the clamp is removed. `mcp_scope_worker_cannot_heartbeat_foreign_agent`
  additionally asserts the foreign agent's `last_heartbeat` was NOT written —
  a real state side-effect check. Negative/positive pairs exist for each
  tier (own-repo allowed, own-workspace allowed, global token unaffected).
- **Coverage matrix** (`specs/coverage/system/platform-model.md` row 7) is
  updated to `implemented` with accurate evidence and an explicitly disclosed
  residual: first-spawn worker mints pass `persona: None` because the spawn
  request carries no persona input (persona reaches the claim on orchestrator,
  gate-agent, and worker re-spawn mints). This is an input-surface limitation,
  not a hollow claim — the field exists and is populated wherever a persona is
  known at mint time.

## Non-blocking observations (recorded for completeness)

- Read-only platform tools (analytics, messaging, conversation upload) are
  permitted within tier for scoped callers rather than enumerated per-scope;
  justified in the coverage row; messaging is workspace-bounded by its own
  handler. No cross-tenant/cross-repo write or read leak found.
- `validate_jwt` (Keycloak user path) mirrors hierarchy claims if a deployment
  ever embeds them, overriding the user's stored tenant binding with a
  trusted-IdP-signed claim. Trusted-IdP claims are authoritative by design;
  noting it for the record.
- `check_repo_scope` parses `repo:{name}:{right}` with rsplit_once — repo
  names containing ':' parse consistently on both mint and check sides
  (symmetric), so no mismatch is possible.

## Verifier must run (server test binaries, deferred per 4 GiB rule)

`cargo test -p gyre-server --lib` — §1 slices:
`mcp_scope_worker_cannot_create_mr_in_other_repo`,
`mcp_scope_worker_create_mr_own_repo_passes_scope_gate`,
`mcp_scope_worker_list_mrs_no_repo_clamped_to_own_repo`,
`mcp_scope_workspace_orchestrator_list_mrs_requires_repo`,
`mcp_scope_worker_cannot_heartbeat_foreign_agent`,
`mcp_scope_worker_cannot_spawn_workers`,
`mcp_scope_workspace_orchestrator_cannot_write_repo`,
`mcp_scope_workspace_orchestrator_own_workspace_allowed`,
`mcp_scope_workspace_orchestrator_other_workspace_rejected`,
`mcp_scope_global_token_unaffected`,
`mcp_spawn_repo_orchestrator_denies_tenant_mismatch`,
`agent_jwt_tenant_mirrored_from_verified_claims`,
`mint_with_workload_roundtrips_wl_image_hash_claim`,
`minted_jwt_carries_unique_jti`, plus the merged upstream §1-token tests
(`mcp_search_is_access_scoped_to_the_agent_token`,
`agent_jwt_claims_reach_live_evaluation`,
`agent_jwt_wl_stack_hash_normalized`,
`agent_jwt_persona_and_attestation_level_reach_live_evaluation`).
Then `cargo test -p gyre-server --test search_wiring --test
task134_review_probe --test m18_oidc_integration --test auth_integration`,
and the gate_executor slices (`agent_validation_token_carries_validation_scope_not_review`,
`gate_token_revoked_after_validation_completes`, `agent_validation_exit_zero_passes`,
`e2e_review_submission_is_abac_allowed_with_seeded_policies`). Finally
`cargo test --all` for the task's AC10.
