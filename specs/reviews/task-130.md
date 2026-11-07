# Review — task-130 (Implement token scoping: hierarchy claims on AgentJwtClaims + MCP validation)

Spec: `specs/system/platform-model.md` §1 Token Scoping. Cumulative diff under review:
`git diff 88bd81bf..106be8dd` (task commits `894f7175`, `78ac5caf`; `745eb3ad`/`106be8dd` are
docs/process-only). Working tree at candidate `106be8dd`, verified pristine before and after
every probe.

Verdict: **complete** (round 3 — repairs both findings of review 92601fb6d780421cbda4ca4b576d31ac,
candidate 15798cf5).

## Round 3 probes (evidence: /tmp/stage/review-evidence/probes.md + logs)

- **New tests pass at head:** `mcp_scope_worker_list_mrs_no_repo_clamped_to_own_repo`,
  `mcp_scope_workspace_orchestrator_list_mrs_requires_repo`,
  `mcp_scope_worker_cannot_heartbeat_foreign_agent` — 1/1 each via the built test binary.
- **Mutation probe F1:** `handle_list_mrs`'s clamped fallback (`list_by_repo(own repo_id claim)`)
  temporarily replaced with the pre-fix unfiltered `merge_requests.list()` → the clamp test
  FAILS. The test genuinely discriminates the repaired behavior.
- **Mutation probe F2:** the `check_agent_target_scope(...)` call in the worker-tool arm of
  `validate_tool_scope` temporarily removed → the foreign-heartbeat test FAILS (and asserts the
  foreign agent's `last_heartbeat` stays untouched, so it catches the write, not just the error
  shape). Genuinely discriminating.
- **Suites:** `cargo test -p gyre-server --lib` → 1317 passed / 0 failed (matches the shipped
  claim; the known pre-existing scim env-mutation flake did not trigger). `mcp::` subset 89/89,
  `--test m18_oidc_integration` 8/8, `--test auth_integration` 21/21.
- **Static gates:** all 17 mechanical checks pass at head (arch, mcp-write-tools,
  inert-enforcement, abac-exempt-handlers, abac-route-registry, fabricated-scope-defaults,
  forged-scope-fields, scope-literal-defaults, relative-path-defaults, lossy-secret-conversion,
  forwarded-header-trust, in-memory-state-stores, unbounded-external-http,
  byte-slice-truncation, dead-message-kinds, migration-versions, task-commit-attribution).
  The exemption-file edits in the diff are line-drift re-pointings of pre-existing entries
  (counts unchanged) plus deletion of two abac-exempt-handlers entries whose sites
  (`spawn_*_orchestrator_core`) now genuinely enforce tenant containment in-handler
  (orchestrator.rs: `workspace.tenant_id != caller_tenant` → Forbidden, both cores) — verified
  in source and covered by `workspace_orchestrator_spawn_rejects_cross_tenant` /
  `repo_orchestrator_spawn_rejects_cross_tenant`.

## Verified against the task contract (no findings)

- **F1 repair (gyre_list_mrs no-arg fallback).** `validate_tool_scope`'s `gyre_list_mrs` arm now
  requires a single repo scope for repo-tier tokens and rejects workspace-tier tokens with a
  PERMISSION_DENIED error naming `repository_id`; `handle_list_mrs` clamps the optional-target
  fallback to the caller's signed `repo_id` claim via `list_by_repo` (never the shared
  unfiltered store), and errors for a scoped caller with no repo claim. Unscoped
  ABAC-governed callers keep the full listing. The with-arg path was already correct.
- **F2 repair (worker-tool target binding).** The worker-tool arm binds non-self `agent_id`
  targets via `check_agent_target_scope`: target repo resolved from real state
  (agent.repo_id → worktree → current task's repo), checked against the caller's signed repo
  via `check_repo_target_by_id`; unresolvable targets and store errors fail closed. Self
  targets pass; `gyre_agent_complete` keeps its separate self-only dispatch check. Missing or
  non-string `agent_id` never reaches a write (handler requires the field).
- **Claims/minting (re-verified this round, unchanged):** `AgentJwtClaims` carries
  `tenant_id`, `workspace_id`, `repo_id` (Option), `persona`, `attestation_level`,
  `scope: Vec<String>`; `mint_with_workload`/`mint` populate worker scope
  `["repo:{name}:write"]` from real spawn context (repo → workspace → tenant; missing
  workspace row or mint failure fails the spawn closed — the legacy unscoped UUID fallback is
  gone, proven by `spawn_unresolvable_workspace_fails_closed`); `mint_orchestrator` mints
  workspace tier `["workspace:{id}:read", "workspace:{id}:spawn"]` and repo tier
  `["repo:{name}:write", "repo:{name}:spawn"]`; `stale_agents.rs` re-mints replacement
  orchestrator tokens with the same real-hierarchy resolution (unresolvable → skip + warn,
  never a fabricated tenant).
- **Extractor mirroring:** `with_hierarchy_from_claims` runs on every JWT-resolved path
  (header extractor agent-token branch, `authenticate_token`, federation), mirroring
  tenant/workspace/repo/scope from signature-verified claims. Agent JWTs must be present in
  `agent_tokens` (revocation) and are signature+issuer validated.
- **"Every tool call" coverage:** `validate_tool_scope` runs before dispatch on every
  `tools/call` in `mcp_handler` (the sole MCP transport, POST /mcp). Tools without an explicit
  arm are either clamped in-handler (`gyre_list_tasks` filters by `task_visible_to_caller`),
  workspace-contained by construction (messaging same-workspace constraint),
  caller-bound (conversation upload), or aggregate (analytics) — documented per-tool in the
  coverage row. Tier mismatches on spawn tools are enforced in-handler
  (`repo_orchestrator_scope`/`workspace_orchestrator_scope` require the matching
  orchestrator_type claim).

## Non-blocking observations (for future tasks, not this contract)

- `resources/read` (JSON-RPC method, not a tool call) with `agents://*` still lists all agents
  across tenants to any authenticated caller, including repo-scoped worker JWTs. Pre-existing
  at the base commit, untouched by this diff, and outside this task's plan (which scopes MCP
  validation to `tools/call`); same leak class as F1, worth its own task.
- Worker mint passes `persona`/`attestation_level` as None — disclosed residual; the spawn
  request genuinely has no persona input and attestation is recorded post-spawn. Claims exist
  and `mint_with_workload` accepts them.
