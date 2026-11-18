# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Independent review agent
**Round:** R-current (candidate 59a46cd1fd1ce93738a47c59e77c00cd8ec128c8, base f70cfac6f829847f994daf0cb96631503b1a1561)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Status:** rejected — one blocking code defect (dead REST containment guard)

---

## Verdict

**Not approved.** The core §5 preview path is a real, well-built implementation,
but the REST-surface preview-agent containment added by the repair rounds is
dead code, and the tests that claim to prove it cannot pass when actually run.

## Blocking finding — `caller_is_preview_agent` never fires (dead guard)

`crates/gyre-server/src/api/meta_specs.rs:1159-1171` requires the JWT `scope`
claim to equal the *string* `"agent"`:

```rust
let agent_jwt = auth.jwt_claims.as_ref()
    .and_then(|c| c.get("scope"))
    .and_then(|s| s.as_str())
    == Some("agent");
agent_jwt && is_preview_agent(state, &auth.agent_id).await
```

Gyre-minted agent JWTs serialize `scope` as a **non-empty array**
(`AgentJwtClaims.scope: Vec<String>` — auth.rs:241; `mint_with_workload` sets
`vec!["repo:{repo_name}:write"]` — auth.rs:508; `jwt_claims` is populated via
`serde_json::to_value(&AgentJwtClaims)` — auth.rs:1424/1750), so `as_str()`
returns `None` for every agent JWT and the function always returns `false`.
The repo itself documents this exact failure class at
`api/specs.rs:3565-3572` ("Gyre agent JWTs carry scope as a NON-EMPTY ARRAY …
Deriving caller_is_agent via `scope.as_str() == Some(\"agent\")` never fired"),
and the working discriminators (`with_hierarchy_from_claims` auth.rs:1078-1087,
`is_agent_jwt` mcp.rs:696-703) all read the claim as an array.

Consequences:

- All 21 REST guards keyed on `caller_is_preview_agent` are dead: agents.rs:144
  (register a fresh non-preview agent identity — launders past every
  caller-keyed guard), agents.rs:294 (status → Idle, defeating
  killed-on-completion), spawn.rs:2924/3311/3477 (complete/fail/stop),
  merge_requests.rs:275/916 (MR create + merge-queue enqueue),
  tasks.rs:196/311/371, merge_queue.rs:69/168, compose.rs:69/229,
  specs.rs:456 (spec approve), specs_assist.rs:910, orchestrator.rs:904/1153,
  agent_tracking.rs:106.
- A preview agent's live JWT authenticates as `UserRole::Agent`
  (auth.rs:1466) and the builtin `agent-scoped-access` policy
  (abac_middleware.rs:1628-1653, priority 700) allows the Agent role
  read/write/complete/approve on `*` — so these escapes are real in production,
  not theoretical.
- The MCP dispatch guard (mcp.rs:4969-4990) works — it uses the array-based
  `is_agent_jwt` — and the target-keyed `is_preview_agent` guard at
  spawn.rs:2947 works. Only the REST caller-keyed class is dead.
- The Shipped claim "MCP and REST surfaces refuse preview-agent mutation
  escape routes" is false for REST; commits 7cad1261, 2f10e8d1 and c1a709ba
  did not achieve what they claim.

### Test consequence

`preview_agent_rest_ceremony_is_refused` (POST /merge-requests leg),
`preview_agent_rest_task_ceremony_is_refused`,
`preview_agent_control_routes_are_refused`,
`preview_agent_rest_status_routes_are_refused` and
`preview_agent_rest_identity_provenance_and_repo_orchestrator_are_refused`
drive the preview agent's minted JWT through the production router and assert
403 on legs where the dead guard is the only refusal layer. They must fail
when executed (the requests return 201/200). They were never executed in this
pipeline environment — task-206.md:144 records that only
`cargo check -p gyre-server --all-targets` ran (type-check only), and the
review sandbox's build wrapper blocks gyre-server test compilation
(`/tmp/stage/review-evidence/test-wrapper-blocked.log`). The tests themselves
are well designed — they discriminate the guard — the production guard is the
defect, and the "tests green" claim is unsupported.

### Required fix

Use the array-based discriminator in `caller_is_preview_agent` (mirror
mcp.rs `is_agent_jwt`, or `!auth.scope.is_empty()` after
`with_hierarchy_from_claims`). The five tests above must then pass.

## What is sound (verified this round)

- **Core §5 path is real**: `POST /api/v1/meta-specs/preview` validates every
  target up front (repo exists → 404, ABAC + tenant containment → 403, spec
  path on default branch), provisions per target a real Active agent row, a
  `preview/{preview_id}/{slug}` branch pinned to the default-branch tip, a
  real git worktree, and a short-lived JWT (separate
  `GYRE_PREVIEW_JWT_TTL_SECS`); 202 response matches the spec shape exactly;
  rollback compensates provision/persist failures (slot, branch, worktree,
  token, rows). Draft travels via `GYRE_META_SPEC_DRAFT_KIND/CONTENT` +
  `GYRE_TARGET_SPEC_PATH` and `agent-runner.mjs` builds the preview prompt
  with the draft as governing context, withholding task-mutating tools.
- **Completion semantics**: every compute-target monitor exit routes through
  `on_agent_process_exit` → `finish_preview_agent` — token revoked, worktree
  removed, budget slot released, row Stopped (never Idle); branch survives
  for `GET .../preview/{id}` status + diff (merge-base plumbing, MR-diff
  reuse). `preview_agent_finish_teardown_and_diff` exercises this end-to-end
  with a real script-driven agent process, a real commit and a real diff
  assertion.
- **Cleanup**: `DELETE` (kill via recorded spawn backends, worktree, branch,
  open-MR belt-and-braces, token, slot, records; 404 unknown) and the
  `meta_spec_preview_gc` job (TTL boundary test at/under TTL, orphan pass).
  Budget: workspace budget by default (`check_spawn_budget_for`, grouped
  per workspace for multi-target requests), separate
  `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` cap when set.
- **Clean cutover**: workspace-scoped preview routes and the hollow
  structural-impact implementation are gone; ABAC registry entries exist for
  the new global routes; docs (api-reference.md, server-config.md) updated.
- **Probes run this round** (evidence in `/tmp/stage/review-evidence/`):
  `cargo check -p gyre-server` exit 0; `bash scripts/check-arch.sh`,
  `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`,
  `check-mcp-write-tools.sh`, `check-migration-versions.sh`,
  `check-in-memory-state-stores.sh`, `check-dead-message-kinds.sh`,
  `check-byte-slice-truncation.sh`, `check-forged-scope-fields.sh` all exit 0;
  `python3 -m unittest scripts.test_dev_checks` 10/10 OK (including both
  overlay data-follows-merge tests); `cd web && npm ci && npm test` —
  2572 passed, 41 skipped, 127 files. Web preview-loop tests
  (MetaSpecs.test.js) assert the spec-conformant POST body, per-agent
  progress, diff rendering and iterate/clear flows.

## Verifier instructions (beyond the standard `cargo test --all`)

- `cargo test -p gyre-server preview_agent_` — prediction with the current
  candidate: the five REST-ceremony tests listed above FAIL at their first
  caller-keyed-guard-only 403 assertion; the core §5 tests pass. After the
  guard fix, all must pass.
- The known phase-one `check-abac-exempt-handlers` failure naming the removed
  workspace-scoped preview routes is the documented overlay artifact
  (durable finding 100139746): the ROOT overlay restores base f70cfac6's
  frozen exemption data over the merged tree. It is not a candidate defect;
  phase two on the merged tree's own scripts is green (re-verified this
  round, see dev-checks.log). The in-branch overlay repair d28e4a32 was
  reviewed: it only re-restores `scripts/*-exemptions.txt` files present in
  both trees from the candidate after the base restore; frozen count
  baselines live in the check scripts (logic, still base-restored) and the
  growth gate forbids additions, so merged data can only be a subset of base
  data — semantically correct and not a weakening. ROOT copies are unaffected
  by candidate edits, so it cannot mask anything in the current round.
