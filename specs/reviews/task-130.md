# Review — task-130 (Implement token scoping: hierarchy claims on AgentJwtClaims + MCP scope validation)

Spec: `specs/system/platform-model.md` §1 "Token Scoping".
Candidate: `0bbb90079ff86182b995fa2c382c071c932abb41` (base `d9c419a09466379eeae83b6abc0cf50286e70553`).
Task-labeled product commits (all recorded in the candidate's `specs/tasks/task-130.md` frontmatter):
`894f7175`, `78ac5caf`, `371bed86`, `d500a34a`, `25fc7ab3`, `33468526`, `318f1fd0`.
Verdict: **approved** — independent evidence supports the task contract; no blocking findings.
Server test execution is deferred to the independent verifier per the 4 GiB budget rule (list below).

## Acceptance-criteria verification (source inspection at the candidate)

- **Claims struct** (`gyre-server/src/auth.rs:67-125`): `AgentJwtClaims` carries `tenant_id: String`
  (`#[serde(default)]` for legacy-token tolerance), `workspace_id`/`repo_id: Option<String>`
  (repo_id None on workspace orchestrators), `persona: Option<String>`,
  `attestation_level: Option<u32>`, and `scope: Vec<String>` (fine-grained entries).
  The old hardcoded `scope: "agent"` string is gone (grep: zero non-test remnants;
  `search_scope.rs` accepts the legacy string form only for pre-§1 tokens).
- **Minting from spawn context**: `mint_with_workload`/`mint_worker_scoped` mint
  `["repo:{repo_name}:write"]` with tenant/workspace/repo resolved from real state —
  `spawn.rs` resolves repo→workspace→tenant and FAILS CLOSED (no legacy UUID fallback) when
  the workspace row or mint fails; `orchestrator.rs` resolves workspace→tenant (unresolvable
  workspace = spawn failure) and mints workspace tier `["workspace:{id}:read",
  "workspace:{id}:spawn"]` / repo tier `["repo:{name}:write","repo:{name}:spawn"]`;
  `ralph_loop.rs` (re-spawn), `stale_agents.rs` (replacement), `signal_chain.rs`
  (system-originated spawns), and `gate_executor.rs` (gate agents via `mint_scoped`,
  tenant resolved fail-closed from the gated MR's workspace) all carry the same hierarchy.
  All plain `mint()` call sites are inside test modules (verified by grep + context read).
- **`AuthenticatedAgent`** (`auth.rs:731-757`): `workspace_id`, `repo_id`, `scope` mirrored
  from signature-verified claims by `with_hierarchy_from_claims` on every JWT-resolved path
  (agent-token path validates EdDSA signature + expiry AND requires the token to be present
  in `agent_tokens`; revoked/unknown "ey" tokens never fall through to API-key/Keycloak).
- **MCP validates scope on every tool call** (`mcp.rs:4442`): `validate_tool_scope` runs
  before dispatch on every `tools/call`, unconditionally. Scoped callers with an empty
  derived scope fail closed. Repo-tier tools check the target against the signed `repo_id`
  claim (ids vs scope-entry names reconciled via the same-mint invariant, documented);
  workspace-tier tools against `workspace:{id}` scopes; task-addressing tools resolve the
  target task's real repo/workspace from `state.tasks`; agent-addressing worker tools
  (heartbeat/complete/record_activity) bind caller-chosen `agent_id` targets to the
  caller's repo via `check_agent_target_scope` (unresolvable targets fail closed);
  `gyre_list_mrs` fallback is clamped to the caller's own repo (never the unfiltered
  store); `gyre_search`/`gyre_list_tasks` clamp to the caller's workspace/tier
  (`task_visible_to_caller`); `gyre_spawn_worker` requires the `spawn` right (a worker's
  write-only token is rejected pre-dispatch). All rejections are `PERMISSION_DENIED`
  JSON-RPC errors naming the target — no silent allow. Catch-all tools
  (analytics/messaging/conversation upload) are caller-bound in their handlers (verified:
  `handle_message_send` same-workspace constraint, `handle_message_ack`/
  `handle_conversation_upload` use `auth.agent_id`, never args).
- **Fail-open repairs from the rebase** (`25fc7ab3`): `git_http` push denial, the ABAC
  scoped-token allow-list, and `submit_review`'s reviewer binding previously read the raw
  JWT `scope` claim with `as_str()` — an array claim silently failed open. All three now
  compare exact-match against the extractor-derived `auth.scope` list.
- **Search scope** (`search_scope.rs`): `is_agent_jwt` recognizes the §1 array form (the
  old `scope == "agent"` string check silently missed every §1 token); worker claims bound
  search to the signed workspace/repo claims; unknown agents fail closed.

## Test quality (mcp.rs `mod tests`, `mcp_scope_*` suite)

Tests mint REAL Ed25519 JWTs via the production `mint`/`mint_orchestrator` helpers with
hierarchy resolved from seeded state, register them in `agent_tokens`, and drive the real
router (`build_router(...).oneshot(...)` with `Authorization: Bearer`) — no
`AuthenticatedAgent` stub injection. Strong cases uniquely attributable to
`validate_tool_scope` (they fail if the gate is removed): `mcp_scope_worker_cannot_spawn_workers`
(the handler's own tier denial says "requires a repo-orchestrator token", not
PERMISSION_DENIED), `mcp_scope_worker_cannot_heartbeat_foreign_agent` (asserts the foreign
agent's `last_heartbeat` was NOT touched — no other gate covers this),
`mcp_scope_worker_list_mrs_no_repo_clamped_to_own_repo` (asserts sibling-repo and
cross-workspace MR titles do not leak), `mcp_scope_workspace_orchestrator_list_mrs_requires_repo`,
`mcp_scope_workspace_orchestrator_other_workspace_rejected`. Integration tests updated to
assert the §1 claims shape end-to-end (`m18_oidc_integration.rs::token_info_for_jwt_agent_token`
asserts scope array `repo:{name}:write`, `repo_id`, `workspace_id`, non-empty `tenant_id`
from a real spawn; `auth_integration.rs`, `search_wiring.rs`, `task134_review_probe.rs`,
jj/merge_deps fixtures updated). Residual honestly documented in the coverage row:
first-spawn worker mints pass `persona: None` (spawn request takes no persona input);
persona reaches the claim on orchestrator/gate-agent mints and worker re-spawns.

## Verification-repair round (exemption gate)

Prior verification round `6ff2aa5c` failed only on "new verification exemptions forbidden"
(re-pointed `scope-literal-defaults-exemptions.txt` entries). Repair verified independently:
the exemption file is STRICTLY SUBTRACTIVE vs base (12→6 entries; all 6 remaining lines
byte-identical to base; zero added/re-pointed lines — verified by `git show` diff of both
revisions), `FROZEN_EXEMPTION_COUNT` lowered 12→6, and `check-scope-literal-defaults.sh`
passes on explicit crate paths. Underlying adapter sites fixed for real: SQLite/Postgres
`merge_request.rs`/`repository.rs` `create()` rows now stamp the storage handle's resolved
`tenant_id` instead of the `"default"` literal.

## Probes run (evidence in /tmp/stage/review-evidence)

- `bash scripts/check-scope-literal-defaults.sh crates/gyre-domain/src crates/gyre-adapters/src crates/gyre-server/src` → OK (exit 0)
- `bash scripts/check-arch.sh` → OK (hexagonal boundary intact)
- `bash scripts/check-mem-port-contracts.sh` → OK (touched adapters enforce port contracts)
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib` → clean, exit 0 (6m33s)
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → clean, exit 0 (test code compiles; only the SKIP_WEB_BUILD note)
- `cargo test -p gyre-domain` → 8 passed, 0 failed
- `cargo test -p gyre-adapters --lib` → 512 passed, 0 failed, 6 ignored (256s; includes the touched merge_request/repository slices)
- `bash scripts/check-task-commit-attribution.sh` against the candidate's committed
  `specs/tasks/task-130.md` → OK (the working-tree copy was reset to `not-started`/`commits: []`
  by the stage controller; the check fails on that reset copy — controller artifact, not a
  candidate defect; working tree restored byte-identically after the probe)

## Verifier must execute (gyre-server test binaries deferred per 4 GiB budget rule)

- `cargo test -p gyre-server --lib` — at minimum the §1 slices: `mcp_scope_worker_cannot_create_mr_in_other_repo`,
  `mcp_scope_worker_create_mr_own_repo_passes_scope_gate`, `mcp_scope_worker_list_mrs_no_repo_clamped_to_own_repo`,
  `mcp_scope_workspace_orchestrator_list_mrs_requires_repo`, `mcp_scope_worker_cannot_heartbeat_foreign_agent`,
  `mcp_scope_worker_cannot_spawn_workers`, `mcp_scope_workspace_orchestrator_cannot_write_repo`,
  `mcp_scope_workspace_orchestrator_own_workspace_allowed`, `mcp_scope_workspace_orchestrator_other_workspace_rejected`,
  `mcp_scope_global_token_unaffected`, `mcp_search_is_access_scoped_to_the_agent_token`,
  `is_agent_jwt_detects_gyre_jwt`, auth.rs §1 claim tests (`mint_with_workload_roundtrips_wl_image_hash_claim` et al.)
- `cargo test -p gyre-server --test m18_oidc_integration` (spawn → tokeninfo §1 claims assertions)
- `cargo test -p gyre-server --test auth_integration`
- `cargo test -p gyre-server --test search_wiring`
- `cargo test -p gyre-server --test task134_review_probe`
- `cargo test --all` (the task's final acceptance criterion)

## Non-blocking observations

1. **Domain constructor "fix" is behavior-identical to base** (`33468526`/`318f1fd0`):
   `Task::new`/`MergeRequest::new` now bind `workspace_id` via a `bootstrap_workspace()`
   helper that returns `Id::new("default")` — the same value the exempted literal held in
   base. This replicates the `Agent::new` helper pattern that already exists in base
   `d9c419a0` (accepted in the task-099 repair round) and is justified by
   `hierarchy-enforcement.md` §1 "Bootstrap Behavior"; production callers with a real
   workspace assign it explicitly (spawn paths, REST/MCP create_mr). No regression, no new
   fabrication, no scope leak — but the shipped note's "fixed for real" wording overstates
   what changed at these two construction sites (the adapter tenant-stamping fixes ARE real).
2. `mcp_scope_workspace_orchestrator_cannot_write_repo` could also pass via the TASK-216
   worktree check if `validate_tool_scope` were removed; unique attribution for the
   create_mr gate comes from the worker cross-repo test (scope denial names the target
   repo, TASK-216 denial names the worktree) and the rest of the suite.
3. `web/dist/` in this checkpoint may match the upstream bundle — generated output owned
   by the verifier; not reviewed, not blocking.
