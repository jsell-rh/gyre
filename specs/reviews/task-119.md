# Review: task-119 — Implement budget enforcement cascade

**Candidate:** `8a7571754c2bda80c8ac9af1ad0c7e7badd686d4` (branch `pipeline/task-119/d5241de86cdf4b23a592d797de79f89e-1`)
**Base:** `d31906c5a16ab8045ab435dfad2bd7dc1ce3f616`
**Spec:** `agent-runtime.md` §4 (Budget Enforcement), §5 (Agent Prompt Structure)
**Reviewed:** full diff base→candidate (38 files, +3539/−995), incl. the rebase-merge
repairs (mcp.rs duplicate prompt handlers, docs conflicts).

## Verdict: APPROVED

## Evidence (independent probes, logs in the pipeline evidence dir)

- `cargo check -p gyre-server` — PASS (8m24s).
- `cargo check -p gyre-server --tests` — PASS (test code type-checks; 3m38s).
- `cargo test -p gyre-adapters --lib` — 465 passed / 0 failed (real SQLite:
  `list_since` window+tenant-spanning, `add_compute_minutes` accumulation,
  repo `budget_json` round-trip incl. clear).
- `cargo test -p gyre-domain -p gyre-ports` — 474 passed / 0 failed.
- `cargo fmt --check` (server/adapters/domain/ports) — PASS.
- Mechanical invariants at candidate HEAD (via temp worktree, since the
  executor resets `specs/tasks/task-119.md` in the working tree):
  arch, migration-versions (000061 is next unused, portable SQL both
  backends), dead-message-kinds (BudgetWarning/BudgetExhausted now have
  real emitters; exemptions entries removed), mem-port-contracts,
  in-memory-state-stores (grace-timer set is process-local sync, correctly
  not port-backed), task-commit-attribution (all six product commits in
  frontmatter), ABAC route registry + exempt handlers, mcp-write-tools,
  byte-slice, relative-path, forged-scope, scope-literal,
  fabricated-scope, lossy-secret, inert-enforcement, unbounded-http,
  migration portability, forwarded-header — ALL PASS.

gyre-server test binaries exceed this sandbox's 4 GiB build budget
(wrapper exit 64). Required verifier probes (not run here):

```
cargo test --all                                   # acceptance criterion
cargo test -p gyre-server --lib api::budget        # enforcement levels, cascade, rolling window
cargo test -p gyre-server --lib api::repos         # repo budget cascade validation (400s)
cargo test -p gyre-server --lib mcp                # prompt delivery tests
cargo test -p gyre-server --lib prompt_assembly    # protocol prompt + rendered_prompt pinning
cargo test -p gyre-server --lib api::spawn         # env injection + persona-prompt delivery
cargo test -p gyre-server --lib signal_chain       # scheduler repo-budget + BudgetExhausted flag
```

## Acceptance criteria assessment (from source review + adapter-test evidence)

- **Cascade repo ≤ workspace ≤ tenant** — real: workspace ≤ tenant on
  PUT budget for all four `BudgetConfig` fields (incl. new lifetime
  check); repo ≤ **effective** workspace limit (min of workspace config
  and tenant ceiling — a dimension unset on the workspace inherits the
  tenant per platform-model §1) on PUT /repos/:id, 400 + not-persisted on
  violation; `{}` clears. Migration 000061 `budget_json` round-trips in
  SQLite and PG adapters (SQLite proven by tests; PG mirrors).
- **max_agents ≤ workspace max_agents_per_repo** — pre-existing
  (task-118), retained; spawn-time enforcement with fallback confirmed in
  signal_chain.rs `effective_max_agents`.
- **Usage per repo, rolled up** — `repo:{id}` usage entries charged from
  every agent report (repo resolved from `agent.repo_id` or the current
  task), workspace + tenant counters incremented on the same report.
- **80%** — `AgentBudgetWarning` inbox notification (High per the unified
  user-management taxonomy; spec's stale "priority 7" draft wording
  amended in agent-runtime.md + HSI to match the canonical notification
  spec) + `BudgetWarning` bus event on the crossing, per-period dedup.
- **100%** — `BudgetExhausted` to every Active agent's inbox
  (`Destination::Agent` — the surface `list_unacked` serves), 60s grace
  timer (one per workspace, re-evaluates before killing), then
  `ComputeTarget::kill_process()` via the recorded spawn backend, agent
  marked `Stopped`, worktree preserved, task stays InProgress, token
  revoked, AgentKilled audit. Overshooting crossing reports correctly get
  grace (beyond-limit reserved for budget-reduced-below-usage, per spec).
- **Beyond limit** — workspace budget PUT runs enforcement synchronously
  with `BudgetChanged` cause: usage strictly > new limit → immediate
  synchronous kill.
- **Compute minutes** — charged on every terminal transition (complete
  REST + MCP, fail, stop, admin kill, stale-detector death, budget kill)
  from `spawned_at` wall clock; `BudgetUsage.compute_minutes` persisted
  (SQLite/PG/mem adapters, `add_compute_minutes` port method).
- **llm_query → workspace only** — all five user-initiated surfaces
  (briefing/ask, explorer/generate, specs/assist ×2, MCP spec_assist)
  record `llm_query`; excluded from repo charges in BOTH the incremental
  path and the hourly resync (regression tests for both).
- **Budget reset** — hourly job recomputes every entity's counters from
  per-call `BudgetCallRecord`s inside `now−24h` (rolling, not calendar);
  `list_since` is deliberately deployment-wide (tenant-scope-blind) with
  tests proving cross-tenant records survive the resync.
- **Prompt structure §5** — `GYRE_META_SPEC_PROMPT`/`SET_SHA` +
  `GYRE_PROTOCOL_PROMPT` (Ralph loop, MCP tools, 60s heartbeat,
  escalation, agent.complete) + `GYRE_ITERATION` at spawn; prompt-set
  record pins the rendered persona section; four MCP prompts serve
  persona (pinned assembly verbatim, end-to-end test asserts it contains
  the seeded meta-spec and NOT the protocol text), protocol, context
  (task/spec-ref/branch/gate-failures/inbox), constraints (budget +
  usage + lifetime + iteration ceiling + scope).

The mcp.rs merge resolution (duplicate `prompt_definitions()`/
`handle_prompt_get` from base task-150 vs candidate) follows the spec
text: agent-runtime.md §5 (present in the BASE, not added by this
candidate) states `system://persona` delivers the assembled meta-spec
prompt set, and agent-runtime.md's header supersedes platform-model §4.
Spawn-time persona binding/resolution (`GYRE_PROMPT_*`,
`persona_slug` fail-closed) is preserved; coverage row platform-model #23
was reconciled to record this.

## Non-blocking observations (for the verifier / future tasks)

1. `prompts/get` with an unknown prompt name now returns a JSON-RPC **ok**
   result with an embedded `{"error": ...}` object; the base returned
   `-32602`. MCP-conformance nit; intentional, tested, docs updated.
2. `admin_kill_agent` charges `record_compute_minutes_for_agent`
   unconditionally — killing an already-terminal (e.g. Idle) agent
   double-charges the tracking-only `compute_minutes` field.
3. The tenant-ceiling-reduction flavor of "beyond limit" has no API
   surface (no route sets the tenant budget config — pre-existing); the
   reachable path (workspace budget PUT) is enforced.
4. `enforce_agent_lifetimes` runs a full agent scan after every usage
   report — correct but O(agents) per report; fine at current scale.
5. 80% bus event fires only for agent-attributed crossings; user-query
   crossings still create the inbox notification (the spec's requirement).
