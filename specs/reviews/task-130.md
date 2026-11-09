# Review — task-130 (Token Scoping: hierarchy claims + MCP scope validation)

Spec: `platform-model.md` §1 Token Scoping.
Commits under review (frontmatter): `894f7175` (recovered implementation), `78ac5caf` (review 92601fb6 repair: list_mrs clamp + worker-tool target binding), `371bed863` (verification repair: rustfmt, clippy borrow, exemption deletions).
Base: `fefde2946` — candidate head: `e9a35e7e`.
Verdict: **approved**.

## What was reviewed

Full diff base..candidate (20 files, +2146/−172): auth.rs (claims + mint + extractor mirroring), mcp.rs (validate_tool_scope + handler clamps), spawn.rs / orchestrator.rs / stale_agents.rs (mint from real hierarchy context, fail-closed), specs.rs (array-based agent detection), test suites, exemption-file hygiene.

## Independent evidence (logs under /tmp/stage/review-evidence/)

- Static gates, all OK: check-arch, check-abac-route-registry, check-abac-exempt-handlers (107 handlers), check-migration-versions, check-dead-message-kinds, check-byte-slice-truncation, check-relative-path-defaults, check-fabricated-scope-defaults, check-lossy-secret-conversion, check-mem-port-contracts, check-mcp-write-tools, check-unbounded-external-http, check-fail-open-ref-resolution, check-migration-sql-portability, check-in-memory-state-stores, check-forged-scope-fields, check-task-commit-attribution, check-inert-enforcement (`crates`), check-scope-literal-defaults (`crates`). `cargo fmt -p gyre-server --check` clean.
- `cargo test -p gyre-server --lib`: **1499 passed / 0 failed** (test-lib-full.log).
- Integration on the exact head: auth_integration 21/21, m18_oidc_integration 8/8, api_integration 66/66, e2e_ralph_loop 1/1.
- **Mutation probe** (mutation-validate-tool-scope.log): with `validate_tool_scope` neutralized, 4 of 10 `mcp_scope_*` tests FAIL (worker foreign-agent heartbeat, worker spawn, ws-orch list_mrs fallback, other-workspace rejection) — the gate is load-bearing, not decorative. The tests that still pass under mutation are covered by handler-level clamping (`gyre_list_mrs` fallback → `list_by_repo` of the signed repo) and the pre-existing TASK-216 worktree check — defense in depth, not weakness.
- **Self-maintenance probe** (probe-ws-orch-self-maintenance.log, temporary test reverted after run): a workspace orchestrator calling `gyre_agent_heartbeat` / `gyre_agent_complete` / `gyre_record_activity` on *itself* is denied (`token carries no single repo scope for agent maintenance`). Assessed as spec-conformant, not a defect: platform-model.md §4's tool table scopes `agent.heartbeat` / `activity.record` / `agent.complete` to **repo**, and §1 mandates per-call scope validation — a workspace-tier token (workspace:read|spawn only) being denied a repo-scoped tool is exactly the specced semantics, fail-closed. §3's workspace-orchestrator duties (read all repos, cross-repo tasks, MR dependencies, communicate with repo orchestrators, spawn/restart them) all remain permitted — verified `gyre_list_mrs` with explicit in-workspace `repository_id` passes and `gyre_spawn_repo_orchestrator` gates to workspace tier + tenant containment. Liveness/teardown via REST (`PUT /api/v1/agents/:id/heartbeat`, `POST /api/v1/agents/:id/complete`) is unaffected. Note for future rounds: the `mint_orchestrator` docstring's "task_id … required on all agent JWTs for `gyre_agent_complete` compatibility" is now stale for the workspace tier — comment-only.

## Verified working (no findings)

- `AgentJwtClaims` carries `tenant_id`, `workspace_id`, `repo_id` (None on workspace orchestrators), `persona`, `attestation_level`, `jti`, and `scope: Vec<String>`; all three construction sites (worker `mint`/`mint_with_workload`, `mint_orchestrator`) mint real hierarchy context resolved from live stores (repo → workspace → tenant). Worker scope `repo:{name}:write`; repo-orchestrator `write+spawn`; workspace-orchestrator `workspace:{id}:read+spawn` — asserted in orchestrator.rs unit tests.
- Fail-closed everywhere probed: unresolvable workspace fails the spawn **before** any agent record or token exists (spawn.rs 404; orchestrator spawn errors; stale-restart skips re-mint and logs) — the legacy UUID-token fallback that would bypass the scope gate is gone; unresolvable repo/task/agent targets in scope checks return PERMISSION_DENIED rather than allow; `check_agent_target_scope` resolves the target's real repo from `agent.repo_id`/worktree/current task and fails closed when unresolvable.
- `with_hierarchy_from_claims` mirrors tenant/workspace/repo/scope from **signature-verified** claims on every JWT-resolved path (extractor, `authenticate_token`, Keycloak, federated-JWKS); the federated path verifies against trusted-issuer JWKS before claims are trusted, so no unverified claim reaches the principal.
- Tenant containment added for real to both orchestrator spawn cores (target workspace's real tenant vs. caller's claims-derived tenant, Forbidden on mismatch; dangling workspace reference → NotFound, no JWT minted) — cross-tenant probe test passes; the task-093 F2/F3 abac-exempt-handlers exemption entries were **deleted** (fix-and-delete), matching the real fixes.
- task-099 F2 closed for real: `validate_persona` returns `bool`, both spawn cores bind and branch on it; inert-enforcement exemption entries deleted (file is a strict subset of base).
- `gyre_list_mrs` no-arg fallback clamped to the caller's signed repo (never the unfiltered cross-tenant store); workspace-tier tokens must address a repository explicitly; `gyre_list_tasks` filtered by `task_visible_to_caller`; `gyre_search` clamped to the caller's workspace claim; `gyre_create_task` absent targets default to the caller's own claims.
- `handle_update_task` cannot reassign repo/workspace scope fields — no scope escape via task update.
- `is_agent_jwt` array-based redefinition keeps Keycloak/API-key/global/legacy-UUID callers outside the token-scope gate (ABAC/RBAC still governs them, per pre-existing behavior); a Keycloak token carrying an array `scope` would be treated as scoped and *denied* where claims are absent — fail-closed.
- Exemption hygiene: inert-enforcement deletions only; scope-literal re-points point at real pre-existing violation lines in files unchanged vs. base (count frozen); fabricated-scope +2 comment lines only; abac-exempt-handlers deletions only.
- Disclosed residual is accurate: worker mint passes `persona`/`attestation_level` as None (no persona input on spawn; attestation recorded post-spawn).

## Deferred to verification

- Workspace-wide `cargo test --all` and all-target Clippy on every crate (wrapper rejects workspace-wide invocations here; changed surface is confined to gyre-server, whose lib + 4 integration suites pass on the exact head).
- Exact-head GitHub CI.
