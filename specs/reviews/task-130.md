# Review — task-130: Token Scoping (platform-model.md §1)

- **Candidate:** `8382260d52a32b2b241d7d4899daaa9aabb150fd` (branch `pipeline/task-130/36391d2089aa402fb99fa13279432a3f-2`)
- **Base:** `c5deca801a331e049f35b181f42d898df24e7e2b` (merge `afb1d86bd` rebase repair reviewed as part of the base→candidate diff)
- **Reviewer stage:** independent review (Gyre pipeline)
- **Verdict:** **approved** — see /tmp/stage/verdict.json

## What was reviewed

Full diff base→candidate (3288 lines across 20 files): auth.rs claims/extractor,
mcp.rs `validate_tool_scope` + handler clamps, spawn.rs fail-closed mint,
orchestrator.rs tenant containment + hierarchy resolution, stale_agents.rs
re-mint, specs.rs agent-detection fix (rebase semantic break 1), m18/auth
integration fixtures, and the four exemption-file conflict resolutions from
the rebase onto `c5deca801`.

## Acceptance criteria — evidence

| Criterion | Evidence |
|---|---|
| `AgentJwtClaims` carries tenant_id/workspace_id/repo_id/persona/attestation_level | auth.rs struct (diff hunk); m18 `token_info_for_jwt_agent_token` asserts claims live over HTTP (8/8 pass) |
| `scope: Vec<String>` fine-grained | auth.rs; mints emit `repo:{name}:write`, `workspace:{id}:read\|spawn`, `repo:{name}:write\|spawn` |
| Minting populates hierarchy from spawn context | spawn.rs resolves workspace row (fails closed 404 on dangling ref, no UUID fallback), orchestrator.rs resolves tenant from workspace + repo name, stale_agents.rs re-mint resolves the same |
| Worker tokens repo-scoped | `mint()` → `["repo:{name}:write"]`; orchestrator.rs tests assert tier scope lists |
| Workspace orchestrator tokens workspace-scoped | `mint_orchestrator` workspace tier; `agent_jwt_tenant_mirrored_from_verified_claims` asserts scope list end-to-end through the extractor |
| MCP validates scope on every tool call | `validate_tool_scope` invoked before the dispatch match for every `tools/call` (mcp.rs:3782) |
| Scope mismatch → clear error | PERMISSION_DENIED JSON-RPC errors naming the target repo/workspace/task/agent |
| `AuthenticatedAgent` exposes workspace_id/repo_id | `with_hierarchy_from_claims` on all four JWT-resolved paths (extractor, authenticate_token, validate_jwt, federated) |
| Existing tests updated | all touched suites pass (logs below) |

## Probes run (evidence: /tmp/stage/review-evidence/)

- **Discrimination probe 1** — in a throwaway worktree at the candidate HEAD, disabled the `validate_tool_scope` pre-dispatch gate: 4/10 `mcp_scope_*` tests FAIL (notably `workspace_orchestrator_other_workspace_rejected` gets a successful `[]` listing of a foreign workspace — the gate is the sole enforcement there). Tests are not self-confirming.
- **Discrimination probe 2** — restored the gate, disabled only the `handle_list_mrs` fallback clamp: `mcp_scope_worker_list_mrs_no_repo_clamped_to_own_repo` FAILS with sibling-repo and cross-workspace MRs leaking. The F1 fix is genuinely discriminated.
- **Focused tests at HEAD** (focused-tests-rerun.log): mcp:: 91/91, mcp_scope 10/10, orchestrator 18/18, auth:: 44/44, api::spawn 36/36, api::specs 79/79, auth_integration 21/21, m18_oidc_integration 8/8, e2e_ralph_loop 1/1.
- **Static gates at HEAD** (static-gates-rerun.log): all 18 mechanical checks green (arch, abac route registry + exempt handlers, fabricated-scope, scope-literal, relative-path, forged-scope, forwarded-header, lossy-secret, in-memory-state, unbounded-http, byte-slice, dead-message-kinds, migration versions + portability, mcp-write-tools, inert-enforcement, task-commit-attribution, mem-port-contracts).
- **Rebase semantic breaks verified fixed:** specs.rs agent detection is array-based and the 4 base-side e2e scenarios pass; `register_e2e_agent` uses the 17-arg mint (api::specs 79/79).

## Non-blocking observations (recorded, no action required for approval)

1. **Stale inert-enforcement exemption entries.** `scripts/inert-enforcement-exemptions.txt` keeps `orchestrator.rs:212`/`:308` entries describing the task-099 `validate_persona` void-call sites, but the fix landed in this merge tree (calls now bound at `:248`/`:388`); the entries are unused dead weight. check-inert-enforcement passes with or without them. The repo's own convention elsewhere in this diff is fix-and-delete; a future violation landing on exactly those lines would be silently exempted. Hygiene only.
2. **`is_agent_jwt` heuristic breadth.** "non-empty `scope` array ⇒ Gyre agent JWT" (mcp.rs, specs.rs) would also classify an IdP user token carrying an array-typed `scope` claim as an agent. Stock OIDC `scope` is a string, so this is not reachable with standard Keycloak mappers; noted as a theoretical false-positive path.
3. **Documented residual (unchanged from prior rounds).** Worker mint passes persona/attestation_level as None (no persona input on spawn; attestation recorded post-spawn). The claims and mint parameters exist per the task plan; no production caller populates them for workers yet.
4. **Old-format agent JWTs** (pre-upgrade `scope: "agent"` string) fail `AgentJwtClaims` deserialization and degrade to legacy unscoped agent tokens until TTL expiry — the task declared the JWT format change breaking by design, and the degraded class matches the pre-existing legacy-UUID-token behavior.

Full workspace suite, all-target Clippy, and exact-head CI remain with verification/publication.
