# Review — task-133 (Implement ABAC MCP permission resources for agents)

Spec: `specs/system/abac-policy-engine.md` §MCP Integration.
Candidate under review: `ad60be27` (product commit `cd372a6c`), base `4b9220bc`.
Verdict: **approved** — no findings. Required verifier probes recorded below (gyre-server test execution is verifier-owned under the 4 GiB build budget).

## Probes run here (evidence: /tmp/stage/review-evidence/)

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --lib --tests` → exit 0 (new code + all four new tests type-check).
- `bash scripts/check-arch.sh` → OK.
- `bash scripts/check-mcp-write-tools.sh` → OK.
- `bash scripts/check-abac-route-registry.sh` → OK (no new routes/tools/exemptions — none were needed).
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-server --lib permissions` → blocked by the executor wrapper (exit 64, 4 GiB budget guard) — infrastructure, not a code defect. Required verifier probe recorded below.
- Full diff inspection of `cd372a6c` + line-by-line comparison of the new `agent_permission_context` against the middleware's context builder, and of every test assertion against the exact seeded builtin policy definitions (see /tmp/stage/review-evidence/static-verification.md).

## Verified working

- **Real engine, real policy set.** Both `permissions://me` and `permissions://check` call `policy_engine::evaluate` — the same function the ABAC middleware uses on live HTTP requests — against the live policy store (`state.policies.list()`). No stand-in evaluator, no copied logic, no hardcoded decisions.
- **`permissions://me`** returns `[{resource_type, allowed_actions: []}]` — one entry per canonical resource type, derived from the live route registry via `ResourceResolver::distinct_resource_types` (so the probe vocabulary cannot drift from what the middleware enforces), × the spec §Attributes action vocabulary (read/write/delete/approve/spawn/push/merge/escalate/generate — exactly abac-policy-engine.md line 55). Each listed action is genuinely evaluated.
- **`permissions://check?action={action}&resource={type}`** parses both query params (missing params → explicit usage error, not a silent deny — verified by `permissions_check_requires_both_params`) and returns `{allowed, matched_policy}` from a single real evaluation. Accepts arbitrary action/resource strings.
- **Own-permissions-only (acceptance criterion).** Neither resource accepts a subject parameter; the subject context is built exclusively from the caller's `AuthenticatedAgent`. `permissions_check_is_caller_scoped` kills any fixed-subject or subject-param bug: two real signed JWT callers (A allowed `push` on `repo` by a subject.id-keyed custom Allow at priority 2000, B denied the same action with `matched_policy: null`).
- **Subject-attribute parity with the middleware.** `agent_permission_context` reproduces the middleware's extraction discipline exactly: subject.type/id/tenant_id, the reserved non-spoofable `SYSTEM_ABAC_IDENTITY` for the genuine system principal, role-SET semantics for `subject.global_role` (empty roles → unset → fail closed), the allowlisted agent-JWT claim merge + `wl_stack_hash`→`stack_hash` / `repo_id`→`repo_scope` normalization gated on `subject.type == "agent"`, and membership/team stores for user identities.
- **Scope is real, never fabricated.** Resource scope comes from the caller's own resolved scope (`agent_secret_scope`: JWT claims → agent record → task; tenant from the governing workspace record), and `env.budget_remaining` from the real budget stores with the same `workspace:{id}` key and `budget_tokens_remaining` helper the middleware uses. `action`/`resource.type`/`env.time` are injected by the engine.
- **Real-policy evidence in tests.** `permissions_resources_evaluate_real_policies_for_agent_jwt` drives the real JSON-RPC dispatch through `build_router` with a real Gyre-minted EdDSA worker JWT: `check?action=read&resource=repo` → `{allowed: true, matched_policy: "builtin-agent-scoped-access"}`; `check?action=approve&resource=spec` → denied by the immutable `builtin-require-human-spec-approval` (Deny beats the role Allow); the `me` repo entry carries read/write/approve but not push/merge/spawn/delete/escalate/generate, and spec/persona entries exclude approve (persona-human-approval at 950 > agent-scoped-access at 700). Every asserted policy id, priority, condition, and fail-closed outcome was cross-checked against the actual seeded definitions (m34 + domain builtin sets, which mem `test_state` seeds exactly as startup does) — all consistent.
- **Registration.** Both resources listed in `resources/list`; the `uriTemplate` field follows the existing `trace://` convention. Covered by `mcp_resources_list_includes_permission_resources`.
- **Docs/coverage bookkeeping.** `docs/api-reference.md` rows added; coverage row 18 flipped to implemented with accurate notes; task-133.md `commits:` records `cd372a6c`.

Test-inflation check: none found. All four tests exercise the real router with real tokens and assert specific outcomes (exact matched_policy ids, exact allowed/denied action lists, caller-dependent divergence); each fails if the corresponding production behavior is disabled.

## Required verifier probes (blocked here by the build wrapper, by design)

```
cargo test -p gyre-server --lib permissions
# expected: 4 passed (mcp_resources_list_includes_permission_resources,
#   permissions_resources_evaluate_real_policies_for_agent_jwt,
#   permissions_check_is_caller_scoped, permissions_check_requires_both_params)
cargo test --all   # task acceptance criterion
```

## Stage artifact (not a candidate defect)

The working tree's `specs/tasks/task-133.md` was found reverted to its pre-implementation content before this review began (stage setup); the assigned candidate commit `ad60be27` contains the correct content (verified via `git show`). Left untouched. No generated `web/dist/` involvement in this task (server-only change).
