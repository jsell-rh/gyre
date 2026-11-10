# Review — task-150 (platform-model.md §4.3 — MCP Prompts and persona injection at spawn)

Spec: `specs/system/platform-model.md` §4.3 "MCP Prompts (Injected at Agent Startup)": four
system prompts (`system://persona`, `system://protocol`, `system://context`,
`system://constraints`), injected into the agent's system prompt at spawn time.
Candidate: `1bedd7b05b6232c747d1cb4ebab535fae746ad7b` (base `18adceb684926b31138c6d05be1403b1621d3b02`;
all intermediate merged bases `402142b1`/`b5a87208`/`bd49c78` are ancestors of the base, so the
base→candidate delta is purely task-150 work: `api/prompts.rs` +476, `mcp.rs` +777,
`api/spawn.rs` +485, `api/orchestrator.rs` +140, docs, coverage row 23).
Verdict: **approved**.

## Independent evidence (logs under /tmp/stage/review-evidence/)

- `cargo test -p gyre-server --lib -- mcp::tests:: api::spawn::tests:: api::prompts::
  api::orchestrator::tests::` → **170 passed, 0 failed** (`task150-review-focused-tests.log`),
  including all 20 task-150 tests: `mcp_prompts_list_returns_four_system_prompts`,
  `mcp_initialize_advertises_prompts_capability`, `mcp_prompts_get_unknown_and_missing_name_errors`,
  `mcp_prompts_get_persona_resolves_scope_chain` (tenant fallback + repo override),
  `mcp_prompts_get_persona_uses_bound_persona_and_errors_without`,
  `mcp_prompts_get_persona_rejects_unapproved_and_unknown`,
  `mcp_prompts_get_context_includes_task_spec_and_worktree`,
  `mcp_prompts_get_context_rejects_cross_scope_task_id`,
  `mcp_prompts_get_constraints_include_budget_and_scope`,
  `mcp_prompts_get_protocol_returns_norms`, `mcp_prompts_get_scoped_prompts_require_agent_identity`,
  `spawn_injects_all_four_system_prompts_with_persona`,
  `spawn_without_persona_injects_prompts_but_no_persona`, `spawn_persona_slug_fails_closed`
  (400 on unknown slug and on Pending persona; no orphaned agent rows),
  `orchestrator_launch_injects_system_prompts`, plus the task-116 delivery guards
  (`spawn_delivers_meta_spec_prompt_to_agent_env`, `spawn_omits_meta_spec_set_sha_without_required_meta_specs`).
- **Mutation probes (all killed, then restored; tree verified clean at the candidate):**
  1. Disable the workspace containment check in `context_prompt_for_caller`
     (`if false && t.workspace_id != scope.workspace_id`) →
     `mcp_prompts_get_context_rejects_cross_scope_task_id` FAILS (the repo check provides
     defense-in-depth and catches the cross-tenant victim task) — `mutation1.log`.
  2. Rename the injected `GYRE_PROMPT_PERSONA` env var in `spawn_agent_core` →
     `spawn_injects_all_four_system_prompts_with_persona` FAILS (exact-equality persona
     assertion) — `mutation2.log`.
  3. Disable the Approved-only gate in `resolve_persona_in_chain` (`&& false`) → BOTH
     `mcp_prompts_get_persona_rejects_unapproved_and_unknown` AND
     `spawn_persona_slug_fails_closed` FAIL — `mutation3.log`.
  The tests prove the production behavior, not mirrors of it.
- Invariant scripts at the candidate (`invariant-scripts-summary.txt`): check-arch,
  check-mcp-write-tools, check-abac-exempt-handlers, check-abac-route-registry,
  check-migration-versions, check-byte-slice-truncation, check-relative-path-defaults,
  check-dead-message-kinds, check-mem-port-contracts, check-fail-open-ref-resolution,
  check-fabricated-scope-defaults, check-scope-literal-defaults, check-lossy-secret-conversion,
  check-inert-enforcement, check-forged-scope-fields, check-forwarded-header-trust,
  check-in-memory-state-stores, check-unbounded-external-http, check-task-commit-attribution,
  check-placeholder-stubs, check-conditional-test-guards → all exit 0.
  check-self-confirming-tests / check-mirrored-logic-tests / check-assertionless-tests exit 2,
  **confirmed identical at the base `18adceb` in a temp worktree** (pre-existing, not this task).
- `cargo fmt --check -p gyre-server` → clean.

## Verified working (no findings)

- **`prompts/list` / `prompts/get` are real JSON-RPC handlers** (mcp.rs dispatcher arms,
  `prompt_definitions()`, `handle_prompt_get`), same response shapes as tools/resources;
  `initialize` advertises the `prompts` capability. Unknown/missing name → `-32602`.
- **`system://persona`** resolves nearest-scope-wins repo → workspace → tenant
  (`resolve_persona_in_chain`), Approved-only fail-closed; without a `slug`, the persona bound
  to the agent record (re-checking approval). An unbound agent errors honestly — no fabricated
  persona text. Spawn path (`spawn_agent_core` `persona_slug`) uses the same chain and persists
  the binding as `agent.persona_id`; rejection happens **before** the agent row is created
  (no orphan rows — asserted by test).
- **`system://context` containment** (review repair `12c5fe5e5`): a caller-supplied `task_id`
  (and the JWT-bound task) is rejected on workspace mismatch, and on repo mismatch when the
  caller is repo-scoped — mirroring `handle_spawn_worker`'s PERMISSION_DENIED checks. The
  disclosure scenario (cross-tenant victim task, "SECRET victim title") is denied with no task
  text in the error. A workspace-scoped agent may read same-workspace tasks in any repo — the
  workspace is the ABAC boundary there; consistent with the secrets:// resources' scope model.
- **`system://constraints`** reads the SAME stores and entity keys spawn enforcement reads
  (`budget_configs`/`budget_usages`, `workspace:{id}` / `tenant:global` — verified against
  `api/budget.rs`), so the prompt cannot disagree with what is enforced; includes limits,
  used/remaining, lifetime cap, and tenant/workspace/repo scope boundaries. Tenant is taken
  from the governing workspace record (`agent_secret_scope`), never fabricated.
- **Spawn injection is real dispatch, not a stub**: the spawn tests drive the real Kubernetes
  dispatch arm (`kubectl apply -f -`) with a stub `kubectl` binary at the process boundary and
  assert exact name/value pairing on `spec.containers[0].env` — `GYRE_PROMPT_PERSONA` (only
  when bound; full-equality on the persona text), `GYRE_PROMPT_PROTOCOL`, `GYRE_PROMPT_CONTEXT`
  (task title, spec ref, worktree branch), `GYRE_PROMPT_CONSTRAINTS` (workspace boundary).
  Orchestrator launches (`launch_orchestrator_process`) inject the same set from the bound
  persona; the orchestrator context honestly records no-task/no-worktree.
- **`meta_spec_set_sha`** is no longer hardcoded `None`: spawn.rs:1632-1636 returns
  `prompt_set.set_sha` (SHA-256 of the assembled meta-spec prompt set via
  `prompt_assembly::prompt_set_sha`), null when the assembled set is empty — task-116's
  approved semantics kept in the merge reconciliation, attesting exactly the set delivered as
  `GYRE_META_SPEC_SET_SHA`/`GYRE_META_SPEC_PROMPT`. Coverage row 23 (rewritten by `98f7daf73`)
  now describes these actual semantics, matching spawn.rs and `docs/api-reference.md` — the
  prior round's finding is repaired. (The literal task-plan wording "from the resolved persona's
  meta-spec set" conflicted with the merged base's already-approved task-116 semantics; the
  reconciliation is documented in the task's Shipped section and docs.)
- **Docs accurate**: `docs/agent-protocol.md` env-var table and `docs/api-reference.md`
  (spawn `persona_slug`, MCP methods list, prompts table) match the code.
- Coverage: platform-model.md has 0 `not-started` rows; row 23 flips task-assigned → implemented
  with a description that matches the code at the candidate.

## Minor (non-blocking, recorded for completeness)

- The task frontmatter `commits:` list at the candidate omits `98f7daf73` (docs-only repair of
  coverage row 23): the pipeline's process commits added it (`508a7406f`) and then removed it
  (`1bedd7b05`) to match the assignment's 4-commit list. The mechanical attribution gate passes
  (docs-only commits are outside its product-surface scope), the commit is fully described in
  the task body's Shipped section, and the repaired row text is in the tree this review verified.
- `launch_orchestrator_process` injects the bound persona's `system_prompt` without re-checking
  `approval_status` (unlike the MCP `persona_prompt` path, which re-checks). Binding is
  fail-closed Approved-only at spawn, launch follows within the same operation, and no
  production API can transition a persona out of Approved (only Pending → Approved exists), so
  there is no trigger path today. Worth aligning if persona deprecation is ever added.
- `system://context` does not include conversation history (mentioned in the task's
  implementation plan). The spec table — the contract — lists "Current task, spec refs,
  acceptance criteria, worktree info", all present (task description renders under
  "Description / Acceptance Criteria").
