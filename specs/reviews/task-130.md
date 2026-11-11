# Review — task-130 (Token scoping: hierarchy claims on AgentJwtClaims + MCP scope validation)

Spec: `specs/system/platform-model.md` §1 Token Scoping.
Base `65f71e657`, candidate `bb2338a2a` (branch pipeline/task-130/a807294eeb3a44ffaa2195d3adf3340f-1).
Verdict: **approved** — the §1 contract is implemented in real production code with discriminating tests; no blocking findings. Evidence under `/tmp/stage/review-evidence/`.

## Round 1 (this review)

Probes (sandbox follows the 4 GiB rule — gyre-server test binaries deferred to the verifier):

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → exit 0.
- `cargo test -p gyre-common -p gyre-domain -p gyre-ports` → 595 passed, 0 failed.
- All 21 mechanical invariant scripts → exit 0, with two investigated exceptions:
  - `check-task-commit-attribution.sh` exits 0 against the candidate's committed
    tree; the transient exit 1 came from the executor's working-tree contract
    restore of `specs/tasks/task-130.md` (empty `commits:`), not the candidate.
  - `check-mcp-wrapper-parity.sh` reports 35 violations — byte-identical set to
    base `65f71e657` (temp-worktree comparison; line numbers only drifted).
    Pre-existing legacy debt, not introduced here.
- Exemption-file diff audit: the four touched `scripts/*-exemptions.txt` changes
  are comment rewordings, entry deletions, and line-number corrections — no new
  entries, no raised frozen counts.

Verified working:

- **Claims shape is real.** `AgentJwtClaims` (auth.rs) carries `tenant_id`,
  `workspace_id`, `repo_id: Option`, `persona: Option`, `attestation_level:
  Option<u32>`, and `scope: Vec<String>`. `mint`/`mint_with_workload` take the
  hierarchy parameters; `mint_orchestrator` mints workspace tier
  `["workspace:{ws}:read","workspace:{ws}:spawn"]` and repo tier
  `["repo:{name}:write","repo:{name}:spawn"]`; workers get
  `["repo:{name}:write"]`. Old string-scope tokens fail closed at
  `validate()` (Vec deserialization error → 401), so the breaking change cannot
  strand legacy tokens in a bypass state.
- **Minting resolves real hierarchy, fail-closed.** `spawn_agent_core` now
  requires the repo's workspace row to exist (404 naming workspace+repo before
  any agent record is created) and fails the spawn on mint error — the old
  UUID-token fallback that would have bypassed the MCP gate is gone.
  `spawn_orchestrator` (REST), `stale_agents::restart_orchestrator`, and the
  signal-chain spawns all resolve tenant from the workspace row and repo name
  from the repos store; unresolvable → error/skip+log, never a fabricated
  "default" identity.
- **`AuthenticatedAgent` exposes `workspace_id`/`repo_id`/`scope`**, derived
  only from signature-verified claims via `with_hierarchy_from_claims` on every
  JWT-resolved path (agent-token extractor + `authenticate_token`, Keycloak,
  federated). Tenant mirroring from verified claims keeps the
  tenant-containment handlers (`spawn_*_orchestrator_core`) comparing real
  identities; the cross-tenant denial test (`mcp_spawn_repo_orchestrator_denies_tenant_mismatch`)
  pins this.
- **MCP validates scope on every tool call.** `validate_tool_scope` runs
  pre-dispatch in `mcp_handler` for `tools/call`; the SSE path only streams.
  Repo-addressed writes match the signed `repo_id` claim plus the
  `repo:{name}:{right}` scope entries (spawn right required for spawn tools —
  a worker's write-only token cannot spawn); task-addressed tools resolve the
  target task's real repo/workspace from state; `gyre_search`/`gyre_list_tasks`/
  `gyre_list_mrs` fallbacks are clamped to the caller's claims instead of the
  unfiltered store; heartbeat/record_activity/complete agent-id targets are
  bound to the caller's repo via worktree/task resolution (unresolvable fails
  closed). Non-scope-list callers (global token, API key, Keycloak, legacy UUID)
  keep the pre-existing ABAC/RBAC treatment, and a claims-array-with-empty-
  extraction fails closed. Every rejection is a `PERMISSION_DENIED` JSON-RPC
  error naming the target — no silent allow found.
- **Tenant containment added where the MCP spawn path needed it.**
  `spawn_workspace_orchestrator_core`/`spawn_repo_orchestrator_core` now compare
  the target workspace's real tenant against the caller's tenant (Forbidden on
  mismatch), covering the MCP tools that run without middleware ABAC; the REST
  handlers and system-originated signal-chain spawns route real tenant
  identities through the same check.
- **Tests are discriminating, not self-confirming.** The spawn-worker denial,
  other-workspace listing denial, foreign-agent heartbeat, and clamped list_mrs
  tests each assert a JSON-RPC `error` shape and/or absent state changes that
  the pre-gate handlers would have produced as successful tool results — they
  fail if `validate_tool_scope` is removed (analysis in
  `/tmp/stage/review-evidence/README.md`). Integration tests
  (m18_oidc_integration, auth_integration) were updated to the §1 claims shape
  and assert `repo:{name}:write` scope on really spawned tokens.

Non-blocking observations (recorded, no repair required):

- Worker mint passes `persona`/`attestation_level` as `None` — the spawn request
  carries no persona input and attestation is recorded post-spawn. The claims
  exist, `mint_with_workload` accepts them, and the coverage row documents the
  residual honestly; the acceptance criterion (claims present on
  `AgentJwtClaims`) is met.
- Workspace-tier tokens are denied `gyre_agent_heartbeat` even for themselves
  (the branch requires a repo scope). Fail-closed and deliberate; the canonical
  liveness path (`PUT /api/v1/agents/:id/heartbeat`) is unaffected and
  system-spawned orchestrators are refreshed server-side by the stale detector.
- `validate_tool_scope`'s `_ => Ok(())` catch-all covers messaging/analytics/
  conversation tools; messaging enforces same-workspace routing in-handler and
  the read-surface justification is documented in the coverage row.

Verifier must run (4 GiB rule): `cargo test --all`, focused slices listed in
`/tmp/stage/review-evidence/README.md`.
