# Review — task-130 (Platform Model §1 — Token Scoping: hierarchy claims + MCP validation)

Spec: `specs/system/platform-model.md` §1 "Token Scoping".
Candidate under review: `afd5610c993aa6c8b0e8398bc5a0001a1002b595` (base `63aa1486`, merged pipeline lineage; task commits recorded in `specs/tasks/task-130.md` frontmatter — attribution gate OK).
Verdict: **needs-revision** (2 findings, both empirically proven by probe).

## Round 1

Checks run (main checkout at the candidate, `CARGO_TARGET_DIR` under `/tmp/gyre-target` via `/tmp/stage/bin/cargo`):

- `cargo test -p gyre-server --lib mcp_scope` → **7/7 pass** (worker cross-repo MR denied naming r-2; own-repo create_mr passes; worker cannot spawn; ws-orchestrator cannot repo-write; ws tools scoped to own workspace, other-workspace denied naming ws-2; global token unaffected).
- `cargo test -p gyre-server --lib` → **1220 passed, 0 failed** (evidence: `/tmp/stage/review-evidence/gyre-server-lib-tests.txt`).
- Static gates: `check-task-commit-attribution`, `check-mcp-write-tools`, `check-scope-literal-defaults`, `check-inert-enforcement`, `check-fabricated-scope-defaults` → all OK. The fabricated-scope-defaults exemption file legitimately dropped from 8 to 7 entries (spawn.rs tenant-fallback site fixed by this task's fail-closed tenant resolution; removal is the sanctioned direction).
- Temporary probe tests (restored; source + output in `/tmp/stage/review-evidence/`): two adversarial MCP calls through `build_router` with production-minted worker JWTs — see findings.
- Sandbox TCP listeners unsupported (errno 95, `/tmp/stage/capabilities.json`) — no live HTTP probe; `cargo test --all`, all-target Clippy, and exact-head CI belong to verification.

Verified working (no findings):

- **Claims**: `AgentJwtClaims` carries `tenant_id`, `workspace_id` (Option), `repo_id` (Option, None for workspace orchestrators), `persona`, `attestation_level`; `scope` is `Vec<String>` (auth.rs). Old-format JWTs (`scope: "agent"` string) fail `validate()` and get 401 — no silent bypass of the new gate via stale tokens.
- **Minting from real hierarchy**: `spawn_agent_core` resolves repo→workspace→tenant and mints `["repo:{name}:write"]`; unresolvable workspace fails closed to a UUID token with an error log (no fabricated tenant). `spawn_orchestrator` / `restart_orchestrator` resolve workspace→tenant and repo name; `mint_orchestrator` mints workspace tier `["workspace:{id}:read","workspace:{id}:spawn"]` and repo tier `["repo:{name}:write","repo:{name}:spawn"]` (repo_name required, else mint fails). Persona set to the tier slug on orchestrator mints.
- **`AuthenticatedAgent`**: `workspace_id`/`repo_id`/`scope` derived from signature-verified claims via `with_hierarchy_from_claims` on every JWT-resolved path (agent-token extractor, `authenticate_token`, Keycloak `validate_jwt`, federated JWT). Global/API-key/legacy-UUID callers stay unscoped and keep the pre-existing ABAC/RBAC path.
- **The repaired predicate is genuinely fixed**: `check_repo_scope` now enforces `right == "spawn"` for spawn-tier actions and `write || spawn` for write actions (the prior dead-`require_spawn_right` short circuit is gone); the 7 mcp_scope tests exercise both directions.
- **Defense in depth intact**: handler-level `repo_orchestrator_scope`/`workspace_orchestrator_scope` tier checks and the TASK-216 worktree check still run after the new gate.

Findings:

- [ ] **F1 (major): `gyre_update_task` scope gate is vacuous — cross-repo/cross-workspace task WRITE bypass.** `validate_tool_scope`'s cross-tier arm only inspects `repo_id`/`workspace_id` **args**, but `gyre_update_task` addresses its target by the `id` arg and never reads those; with both absent the arm returns `Ok(())`, and `handle_update_task` (mcp.rs:1316) takes no auth and performs no scope check. Probe proof at the candidate: a worker JWT minted through the production path (scope `["repo:r-x:write"]`, ws-2) retitled a task in repo r-1 / workspace ws-1 — JSON-RPC result "Updated task probe-task-1", no PERMISSION_DENIED, store read-back confirms the mutation. The tool was explicitly classified in the cross-tier arm, so the check exists but can never fire — audit-only enforcement where the spec requires rejection ("a worker agent's token is scoped to its repo"; AC "MCP server validates scope on every tool call"). Remedy: resolve the target task's repo/workspace from `state.tasks` and validate against the caller's scope; deny unknown/unscoped targets for scoped callers. Evidence: `/tmp/stage/review-evidence/probe-output.txt`, `probe-source.rs`.
- [ ] **F2 (moderate): `gyre_search` takes an explicit `workspace_id` target that the scope gate never checks — cross-workspace READ leak for scoped tokens.** The default arm of `validate_tool_scope` returns `Ok(())` for `gyre_search` on the rationale that read tools are not addressable beyond the caller's principal — but `handle_search` (mcp.rs:2103) reads `workspace_id` from args and passes it to the search port. Probe proof: worker scoped to r-1/ws-1 searched ws-2 without denial. Violates task plan step 5 ("workspace_id claim must match workspace context for workspace-scoped tools"). Same class: `gyre_list_tasks` returns all tasks with no scoping for scoped callers; graph/analytics reads are likewise unscoped — the "reads within their tier" rationale does not hold for tools that enumerate or address other scopes. Remedy: run `check_workspace_scope`/`check_repo_scope` on `gyre_search`'s workspace arg for scoped callers and scope `gyre_list_tasks` output (or amend the task/spec to justify the read surface per tool — as-is, the coverage row's "validates scope on EVERY tool call" is overstated).

Minor (not blocking, recorded for completeness):

- `AuthenticatedAgent.tenant_id` remains the literal `"default"` on agent-JWT paths even though the claims now carry a real `tenant_id` (`with_hierarchy_from_claims` does not propagate it). Pre-existing fabricated-identity debt (task-099 F1 exemptions, line-drifted only), but the new claims make the fix trivial when touched.
- Worker mints pass `persona`/`attestation_level` as `None` (spawn request has no persona input; attestation recorded post-spawn). Honestly disclosed as residual in the coverage row; claims + mint parameters exist, so AC is met to the letter.

Both findings are in the candidate's own new feature surface (the tools were classified in `validate_tool_scope`'s match), fixable without touching unrelated code. Not approved this round; re-review after F1 (required) and F2 (required for the search/list reads or an explicit spec amendment) are addressed.
