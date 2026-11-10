# Review — task-130 (Token Scoping: hierarchy claims on AgentJwtClaims, MCP scope validation)

Spec: `platform-model.md` §1 "Token Scoping" (agent OIDC tokens encode tenant/workspace/repo/persona/attestation/scope; worker tokens scoped to their repo, workspace-orchestrator tokens to their workspace; the MCP server validates scope on every tool call).

Candidate under review: `9679882b6e6400ba58c5844dd066227f3ec60685` (branch `pipeline/task-130/699596e585f94e2390c7e54bf9996bcc-19`), diffed from assigned base `dbf9edaeae5c8a5441bd9ff50c9dff3cec99172a`. The branch carries the §1 implementation plus several rebase-repair merges onto advancing bases (task-117 compute targets, task-108 impersonation, task-177/140/099/103/098 rounds); every conflict site inspected resolves to keep BOTH sides' enforcement.

Verdict: **approved** (no findings).

## Independent evidence (all probes run at candidate `9679882b6`, logs under /tmp/stage/review-evidence)

- `cargo test -p gyre-server --lib mcp_scope` → 10/10 passed (lib-tests rerun: full `--lib` 1674 passed / 0 failed).
- **Mutation probe** (review-evidence/mutation-probe.log): with the `validate_tool_scope` dispatch call neutralized, 4 of the 10 `mcp_scope_*` tests FAIL (`worker_cannot_heartbeat_foreign_agent`, `worker_cannot_spawn_workers`, `ws_orchestrator_list_mrs_requires_repo`, `ws_orchestrator_other_workspace_rejected`) — the tests genuinely pin the gate, not mirrored logic. The 6 that still pass were individually traced to independent second enforcement layers in production code (TASK-216 worktree-repo check on `gyre_create_mr`, handler-level `list_by_repo` clamp on `gyre_list_mrs` fallback, `task_visible_to_caller` filter in `handle_list_tasks`, caller-identity binding in the messaging tools) — defense in depth, not test fakery. Mutation restored; `git status` clean at candidate SHA afterwards.
- `cargo test -p gyre-server --test auth_integration` → 21/21; `--test m18_oidc_integration` → 8/8 (the two integration files this branch touched).
- Static gates re-run: check-arch, check-scope-literal-defaults, check-fabricated-scope-defaults, check-inert-enforcement, check-abac-exempt-handlers, check-mcp-write-tools, check-migration-versions, check-task-commit-attribution — all PASS. The exemption-file diffs in this branch only DELETE entries or update comments (fix-and-delete contract honored; no growth, no new exemptions).

## Verified against the acceptance criteria

1. **Claims struct** (auth.rs:53-113): `tenant_id: String` (serde-default for legacy-token tolerance), `workspace_id: Option<String>`, `repo_id: Option<String>` (None on workspace orchestrators), `persona: Option<String>`, `attestation_level: Option<u32>`, plus base's `wl_image_hash`. ✔
2. **`scope: Vec<String>`** with fine-grained entries: `mint_with_workload` → `["repo:{name}:write"]`; `mint_orchestrator` → workspace tier `["workspace:{id}:read", "workspace:{id}:spawn"]`, repo tier `["repo:{name}:write", "repo:{name}:spawn"]`. ✔
3. **Minting from real hierarchy context**: `spawn.rs` loads repo → workspace row (missing row fails the spawn 404 — the tenant-scope source, no UUID-token downgrade; proven by `spawn_unresolvable_workspace_fails_closed`) and mints tenant/workspace/repo claims plus base's `wl_image_hash`; `orchestrator.rs` and `stale_agents.rs` resolve workspace→tenant and repo name from live stores (stale restart skips re-mint + logs on unresolvable workspace — never a fabricated "default"); `signal_chain.rs` system spawns pass the target entity's REAL tenant, failing closed on dangling references. ✔
4. **Worker tokens scoped to their repo; workspace orchestrator tokens to their workspace** — mint sites above; `mcp_scope_worker_cannot_spawn_workers` and tier tests prove enforcement. ✔
5. **MCP validates scope on every tool call**: `validate_tool_scope` is invoked in the `tools/call` dispatch (after the RBAC write-role gate, before any handler) for every tool; scoped callers with an empty derived scope fail closed; repo-tier targets are checked against the signed `repo_id` claim (scope entries name repos by name, tools address by id — identity from the same mint), workspace-tier against `workspace:{id}` scopes; task-addressing tools resolve the target task's real repo/workspace from state; `gyre_create_task` explicit targets are contained; unresolvable lookups fail closed with `PERMISSION_DENIED` naming the target. Catch-all tools (`gyre_list_tasks`, analytics, messaging, conversation upload) were individually inspected: `handle_list_tasks` filters via `task_visible_to_caller` for scoped callers; messaging tools derive agent identity from `auth.agent_id` (never args); `conversation_upload` is caller/agent-bound. ✔
6. **Scope mismatch returns a clear error**: `PERMISSION_DENIED` JSON-RPC error (code -32603) naming the target repo/workspace/agent/task in every rejection path. ✔
7. **`AuthenticatedAgent` exposes `workspace_id`/`repo_id`** (plus `scope`, `tenant_id`) mirrored from signature-verified claims via `with_hierarchy_from_claims` on all JWT-resolved extractor paths (agent JWT, Keycloak JWT, federated JWT, impersonation). ✔
8. **Tests updated and meaningful** — see the mutation probe above. ✔

## Notes (not findings)

- **Documented residual**: worker mints pass `persona`/`attestation_level` as `None` (the spawn request has no persona input; attestation is recorded post-spawn). The claims exist, `mint_with_workload` accepts both parameters, and the coverage row records the residual honestly. The acceptance criteria require the fields and mint parameters, which are present; prior review rounds (7be3363d, 92601fb6) accepted this boundary.
- The orchestrator JWT's `persona` claim carries the orchestrator tier slug while the agent row's `persona_id` is resolved fail-closed from `state.personas` (`resolve_orchestrator_persona`) — the persona binding is real and tested; the claim is a tier label. Cosmetic at most.
- Coverage row 24 ("Token Validation on Every Call") contains a pre-base historical note stating worker JWTs carry no `repo_id` claim — stale relative to this branch, but row 24 is outside this task's `coverage_sections` and the diff does not touch it; leaving it to the row's owner/spec-fidelity auditor.

— Reviewer, independent review round at candidate `9679882b6`
