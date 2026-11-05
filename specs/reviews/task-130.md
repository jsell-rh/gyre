# Review — task-130 (Token Scoping: hierarchy claims + MCP scope validation)

Spec: `platform-model.md` §1 "Token Scoping".
Candidate under review: `86e41815` (branch `pipeline/task-130/7be3363d015b428ba9943cd25fc6094d-3`, assigned base `110180fb`).

## Round 1 (this review)

Verified working:

- `AgentJwtClaims` carries `tenant_id: String`, `workspace_id: Option<String>`, `repo_id: Option<String>`, `persona: Option<String>`, `attestation_level: Option<u32>`, and `scope: Vec<String>` (auth.rs:53-93) — matches the plan (workspace_id as `Option` rather than required `String` is a serde-tolerance deviation; every mint path populates it).
- Minting resolves hierarchy from real spawn context: `spawn_agent_core` resolves repo → workspace → tenant and mints worker `scope=["repo:{name}:write"]` (spawn.rs:435-448); `mint_orchestrator` mints workspace tier `["workspace:{id}:read","workspace:{id}:spawn"]` and repo tier `["repo:{name}:write","repo:{name}:spawn"]`; `stale_agents::restart_orchestrator` re-mints with the same resolution.
- `AuthenticatedAgent` exposes `workspace_id`/`repo_id`/`scope` derived from signature-verified claims via `with_hierarchy_from_claims` on every JWT-resolved auth path.
- MCP scope validation runs pre-dispatch on every `tools/call` (`validate_tool_scope`, mcp.rs:685, invoked at mcp.rs:3592): worker tools gated to the token's single repo scope; spawn tools require the `spawn` right (worker write-only tokens rejected); workspace tools gated to `workspace:{id}` scopes; task-addressing tools (`gyre_update_task`/`gyre_decompose_spec`) resolve the target task's real repo/workspace from `state.tasks` (the R1 vacuous-gate defect from review d87848ae is fixed); `gyre_search` validates its explicit `workspace_id` arg against the caller's workspace claim (the cross-workspace read leak is fixed); graph/spec-assist read tools go through `check_repo_read_target` with real state resolution. Scope mismatch returns a `PERMISSION_DENIED` JSON-RPC error naming the target. Claims-array-with-empty-derived-scope fails closed.
- Dead `check_repo_read_scope` deleted (review 6403a8e6 R1); `mcp_decompose_spec_creates_subtasks_in_repo_scope` now asserts the JSON-RPC denial shape and passes.

Test evidence at the exact candidate (SKIP_WEB_BUILD=1, /tmp/stage/bin/cargo):

- `cargo test -p gyre-server --lib` → 1249 passed / 0 failed (matches shipped claim).
- `cargo test -p gyre-server --lib mcp_scope` → 7 passed / 0 failed.
- `cargo test -p gyre-server` (full package, lib + all 9 integration binaries) → every suite passes except m18_oidc_integration: 5 passed / 3 failed.
- Static gates OK: check-arch, check-mcp-write-tools, check-inert-enforcement, check-fabricated-scope-defaults, check-lossy-secret-conversion, check-scope-literal-defaults, check-mem-port-contracts, check-unbounded-external-http. (check-relative-path-defaults flags a `./repos/` mention in a spawn.rs comment — byte-identical at base, pre-existing grep false positive, not candidate-caused.)
- Probe commands, exit codes, and outputs recorded under /tmp/stage/review-evidence/ (probe-summary.md).

Findings:

- [ ] **F1 (critical): candidate-introduced regression — `cargo test -p gyre-server --test m18_oidc_integration` fails 3 of 8 tests deterministically, falsifying the `cargo test --all passes` acceptance criterion; the same root cause silently downgrades repo-scoped worker spawns to legacy unscoped UUID tokens via a public-API-reachable state.** `spawn_agent_core`'s new tenant-resolution gate (spawn.rs:437-470) skips the JWT mint and issues a legacy UUID token whenever the repo's `workspace_id` has no workspace row ("JWT pre-mint skipped: workspace unresolvable"). That state is reachable from the public API — `POST /api/v1/repos` (repos.rs:139-176) accepts any `workspace_id` with no existence check — and the m18 fixtures seed repos with unseeded workspace ids (`ws-m18`, `ws-jwt-auth`, `ws-info`), so `spawn_returns_jwt_token`, `jwt_token_authenticates_api_calls`, and `token_info_for_jwt_agent_token` fail with "spawn token must be a JWT (starts with 'ey'), got: <uuid>" / `token_kind` `agent_uuid` vs `agent_jwt` (evidence: /tmp/stage/review-evidence/test-m18-at-candidate.log, full-package-test.log). At the assigned base `110180fb` the mint is unconditional (git show 110180fb:spawn.rs:429-446 — no workspace gate), so the suite passes there; the regression is introduced by this task. This is the exact defect reported in review checkpoint 4e8e1b2 (durable finding, task-130) with a stated remedy — validate workspace existence in `create_repo` and update the m18 fixtures, or at minimum update the fixtures and document the UUID-downgrade semantics — and neither remedy was applied: the candidate diff touches neither `repos.rs` nor the m18 fixture. The shipped evidence again stops at `cargo test -p gyre-server --lib`, whose fixtures always seed workspaces, so the claim cannot substitute for the failing suite. Security dimension of the same root cause: the fallback issues a legacy UUID agent token that `is_agent_jwt` classifies as unscoped, so `validate_tool_scope` returns Ok and the caller bypasses the entire §1 token-scope gate (falling back to the pre-existing ABAC/RBAC path) — a public-API-reachable path that mints agents without the scoped token the spec mandates. Fix direction: reject the spawn (fail closed) or validate workspace existence at repo creation so the dangling-ref state is unreachable, and update the m18 fixtures to create their workspaces; then `cargo test -p gyre-server` (lib + integration binaries) must pass at the candidate.

## Verdict

Not approved. F1 blocks: the acceptance criteria "cargo test --all passes" and "All existing tests updated for new claim fields" are falsified by a deterministic, candidate-introduced, previously-reported-and-unremedied regression. All other acceptance criteria verified with independent evidence.
