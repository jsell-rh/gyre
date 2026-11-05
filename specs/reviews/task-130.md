# Review — task-130 (platform-model.md §1 Token Scoping)

Spec: `specs/system/platform-model.md` §1 Token Scoping.
Candidate under review: `0c6d8be9` (source commits `a2f36db7`, `bb640340`; both listed in the task's `commits:` frontmatter). Base: `d81b0fe4`.
Verdict: **approved** (round 7 of this task's review chain; all durable findings from prior rounds independently re-verified as fixed at this candidate).

## Independent evidence (exact candidate, `/tmp/stage/bin/cargo`, `SKIP_WEB_BUILD=1`)

- `cargo test -p gyre-server --lib` → **1267 passed / 0 failed** (reproduced twice).
- `cargo test -p gyre-server --tests` → lib 1267/0, api_integration 66, auth_integration 21, dependency 3, conversation 1, e2e 1, **m18_oidc_integration 8/8**, git 16 (+1 ignored), graph 37, explorer_ws 8, search 1 — all green. gyre-server has no reverse dependencies in the workspace (`grep gyre-server crates/*/Cargo.toml`), and the diff touches only gyre-server, scripts, and specs, so this covers the full Rust blast radius of "cargo test --all passes" for this change.
- Discriminating tests run individually and passing: `agent_jwt_tenant_mirrored_from_verified_claims`, `mcp_spawn_repo_orchestrator_happy_and_wrong_workspace`, `mcp_spawn_repo_orchestrator_denies_tenant_mismatch`, `spawn_unresolvable_workspace_fails_closed`, `mcp_decompose_spec_creates_subtasks_in_repo_scope`.
- Probe (temporary tests added to `mcp.rs` mod tests, worktree restored to the candidate afterwards; source and output under `/tmp/stage/review-evidence/probe-source.diff`, `probe-output.txt`): re-ran the exact prior-bypass scenarios from earlier durable findings —
  - worker scoped `repo:r-1:write` calling `gyre_update_task {"id": <task in r-x/ws-2>}` → `PERMISSION_DENIED` JSON-RPC error, store read-back shows the title unchanged;
  - same-worker update of a task in `r-2` (same workspace, other repo) → `PERMISSION_DENIED`;
  - own-repo update → no scope denial, write persists (positive control);
  - `gyre_search {"q", "workspace_id": "ws-2"}` from a ws-1-scoped worker → `PERMISSION_DENIED`.
  All four pass — the cross-repo WRITE bypass and cross-workspace READ leak from the earlier rounds are closed.
- All enforced invariant gate scripts exit 0 at the candidate (re-run this round; log: `/tmp/stage/review-evidence/gates-candidate.log`), including `check-abac-exempt-handlers` (the prior line-drift regression is resolved by landing real tenant containment in `spawn_*_orchestrator_core` and deleting the task-093 exemption entries per that file's fix-and-delete contract) and `check-task-commit-attribution`.
- `check-mcp-wrapper-parity` exits 1 with 35 violations at the candidate — identical count and exit at base `d81b0fe4` (verified in a clean worktree); pre-existing, not candidate-caused, and not in the AGENTS.md enforced set. `check-crypto-verify` exits 2 (awk syntax error in the script) at both base and candidate — pre-existing script/environment issue.
- `cargo check -p gyre-server` → no compiler warnings (the prior dead `check_repo_read_scope` is removed).

## Verified implementation

- `AgentJwtClaims` carries `tenant_id` (String, `#[serde(default)]` for legacy-token tolerance), `workspace_id`/`repo_id` (Option; `repo_id: None` on workspace orchestrators), `persona`, `attestation_level`, and `scope: Vec<String>` — all spec-required claims present.
- Minting is real crypto (EdDSA via `jsonwebtoken` encode + `decode` with issuer validation) and populates hierarchy from real state: `spawn_agent_core` resolves repo→workspace→tenant; workers get `["repo:{name}:write"]`; `mint_orchestrator` gives workspace tier `["workspace:{id}:read","workspace:{id}:spawn"]` and repo tier `["repo:{name}:write","repo:{name}:spawn"]`; `stale_agents.rs` re-mints replacement orchestrator tokens with the same hierarchy resolution.
- `spawn_agent_core` fails closed (404 naming workspace+repo) when the repo's workspace row is missing, and fails the spawn on mint error — the legacy UUID downgrade that bypassed the scope gate is gone; the m18 fixtures were updated to create their workspaces (8/8).
- `with_hierarchy_from_claims` mirrors `tenant_id`/`workspace_id`/`repo_id`/`scope` from signature-verified claims on all four JWT-resolved paths (per-agent-token extractor, WebSocket `authenticate_token`, Keycloak, federated); the Keycloak tenant claim still passes `validate_tenant_id`. This repairs the prior finding where tenant-containment handlers consumed the extractor's fabricated `"default"` tenant: the workspace-orchestrator happy path passes for the right reason and cross-tenant denial (`mcp_spawn_repo_orchestrator_denies_tenant_mismatch`) keeps firing.
- `validate_tool_scope` runs before dispatch on every `tools/call` for scoped (Gyre-minted) tokens and fails closed on a claims/scope inconsistency; task-addressing tools (`gyre_update_task`, `gyre_decompose_spec`) resolve the target task's repo/workspace from `state.tasks` (the prior vacuous-args gate is fixed); `gyre_search` validates its `workspace_id` arg; `gyre_list_tasks` filters results through `task_visible_to_caller` for scoped callers; graph/spec-assist read tools resolve the target repo's workspace from state; `gyre_create_task` clamps absent targets to the caller's claims. Non-scope-list callers (global token, API key, Keycloak user JWT, legacy UUID) keep the pre-existing ABAC/RBAC path.
- Coverage: `specs/coverage/system/platform-model.md` §7 Token Scoping updated task-assigned → implemented with evidence; SUMMARY counts updated consistently.

## Residuals (disclosed in the coverage row; not acceptance-criteria violations)

- Worker mint passes `persona`/`attestation_level` as `None`: the spawn request carries no persona input and attestation is recorded post-spawn (workload_attestations KV). The claims, the `mint_with_workload` parameters, and the acceptance criterion ("AgentJwtClaims includes … persona, attestation_level") are satisfied; orchestrator mint populates `persona`. Flagged for a future task if worker persona input is added.
