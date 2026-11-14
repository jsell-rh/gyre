# Review — task-132 (Implement ABAC policy management UI)

Spec: `specs/system/abac-policy-engine.md` §UI.
Base `5984177c` → candidate `00280157` (product commits `11b0a02f` UI, `5ba1bdf3` rustfmt repair; process/merge commits in between).
Verdict: **approved** — every acceptance criterion holds on independent evidence; no blocking findings.

## Probes run (checkout at candidate `00280157`)

- `cd web && npm ci` → clean install of locked versions.
- `npx vitest run src/__tests__/{abac,PolicyManager,TenantSettings,WorkspaceSettings,RepoSettings}.test.js` → **226/226 passed** (evidence: `vitest-focused.log`).
- `npm test` (full web suite) → **115 files, 2351 passed, 41 skipped** — matches the shipped notes exactly (`vitest-full.log`).
- Mutation probe 1: `detectPolicyConflicts` neutered to `return []` → **9 tests failed** across `abac.test.js` + `PolicyManager.test.js` (conflict-detector tests are load-bearing). Source restored, tree clean.
- Mutation probe 2: scope query params omitted from PolicyManager's `load()` → **4 tests failed** across `PolicyManager.test.js` + `WorkspaceSettings.test.js` (URL-pattern and integration tests are load-bearing). Source restored, tree clean.
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → **clean** in 10m24s (compiles the new server test; web assets built via build.rs).
- `rustfmt --check --edition 2021 crates/gyre-server/src/api/policies.rs` → exit 0.
- `scripts/check-arch.sh`, `check-abac-route-registry.sh`, `check-mem-port-contracts.sh` → all exit 0.
- Greps: no `abac-policies` / workspace-prefixed policy URL anywhere in `web/src`; no leftover references to the removed TenantSettings inline policy form (`loadPolicies`/`policyForm`/`policyDecisions` all gone).

## Verified

- **All seven §UI surfaces are real production code** in `web/src/components/abac/`: PolicyManager (scope-filtered CRUD table + view tabs), PolicyEditor (visual condition builder: add/remove rows, attribute autocomplete via `<datalist>` over the §Attributes catalog, all 8 operators, typed value serialization — `in`/`not_in` → arrays, `greater_than`/`less_than` → numbers, `exists` → null), PolicySimulator (subject/action/resource + scope chain → decision, matched policy, evaluated count, evaluation ms), PolicyDecisionLog (server-side `subject_id`/`resource_type` + client-side allow/deny and date-range filters), EffectivePermissions (`?subject=` per known resource type → resource types × sampled actions), and the client-side conflict detector in `lib/abac.js`.
- **URL pattern compliance.** Every policy call targets the GLOBAL `/api/v1/policies` endpoints with `?scope=`/`?scope_id=` query params; grep confirms zero workspace-prefixed policy URLs in `web/src`. The pre-existing `workspaceAbacPolicies` helper (base `5984177c`, used by WorkspaceSettings' preserved trust summary) also uses the global endpoint with `?scope=Workspace&scope_id=`.
- **The server-side `parse_scope` case-insensitivity fix is a genuine bug fix, not scope creep.** At base, `workspaceAbacPolicies` sent the spec's documented capitalized `?scope=Workspace` while `parse_scope` matched only lowercase — the pre-existing workspace policies view 400'd. The fix (`s.to_lowercase()`) makes the documented §UI query form work; `list_policies_scope_query_is_case_insensitive` is a real killing test (creates a policy, asserts the capitalized filter lists it, lowercase still works, other scope_id excluded). `cargo check -p gyre-server --tests` confirms it compiles; **verifier must execute `cargo test -p gyre-server policies::`** (in-sandbox test-binary linking exceeds the 4 GiB budget; recorded below).
- **Conflict detector mirrors the real engine.** `scopesOverlap` matches `policy_engine::scope_applies` semantics (null scope_id at a level applies broadly; cross-scope broad-unscoped overlaps narrower), and `resolveWinner` mirrors the engine's composition order exactly (immutable Deny first → priority desc → scope specificity repo>workspace>tenant → Deny over Allow), verified against `crates/gyre-server/src/policy_engine.rs` `policy_sort_key`/`evaluate`. The approximations (scoped-broader vs narrower not flagged — client cannot see entity ownership) are documented in-code and conservative.
- **CRUD guards mirror the server's**: edit/delete disabled for `built_in`/`immutable`/`trust:`/`builtin:` prefixed policies, matching the server-side refusals in `api/policies.rs`.
- **Integration at all three scope levels** with real assertions: TenantSettings Policies tab (admin tenant prefilled from `me()`), WorkspaceSettings (scope locked, `?scope=Workspace&scope_id=ws-1`), RepoSettings (scope locked, `?scope=Repo&scope_id=repo-1`) — each asserts the exact global-endpoint query params on the mocked `api.policies`.
- **Tests are not inflated.** Both mutation probes produced multiple failures; component tests assert exact request payloads (`{scope: 'Workspace', scope_id: 'ws-1'}`, condition serialization shapes, PUT vs POST routing); the conflict tests include killing cases for every non-overlap dimension (same effect, disjoint actions, disjoint resource types, different scope_ids, disabled, scoped-tenant vs other scope).
- **Coverage matrix flip is honest**: row 17 (UI) → implemented with accurate notes; task frontmatter records both product commits (`11b0a02f`, `5ba1bdf3`), both present on the branch.

## Non-blocking observations (recorded, not defects)

- Svelte compiler warnings in the new components (`state_referenced_locally` on `defaultScope`/`defaultWorkspaceId`/`defaultSubjectId` initial captures) are the intentional initialize-form-from-props pattern, identical to pre-existing `RepoSettings.svelte` at base. Unused-CSS warnings in TenantSettings (`.create-form`, `.form-row`, …) are leftovers from the removed inline form — warnings only, same class as pre-existing warnings elsewhere; vite build succeeds.
- `web/dist/` was not regenerated by the agent — per pipeline policy this is verifier-owned; the deterministic verifier rebuilds and binds publication to the tested tree. Pure generated-output observation, not a source defect.

## Verifier must run (deferred from sandbox budget)

- `cargo test -p gyre-server policies::` — covers `list_policies_scope_query_is_case_insensitive` plus all task-128/129 policy API tests.
- `cargo test --all` and `cd web && npm test` per the task's acceptance criteria (npm test already run here: 2351 passed).
