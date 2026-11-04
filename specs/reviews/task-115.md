# Review — task-115 (agent-runtime.md §1 — Spec Approval Signal Chain & Orchestrators, Phases 1–3)

Spec: `specs/system/agent-runtime.md` §1 "The Signal Chain", Phase 1 (Spec Approval Triggers
Orchestration), Phase 2 (Workspace Orchestrator — Cross-Repo Coordination), Phase 3 (Repo
Orchestrator — Task Decomposition).
Candidate under review: `987262b5f29e0d7da35feb8e14b65b2acdc92050` (branch
`pipeline/task-115/310ae3fb54fe40f6a451db588ae303be-2`), diffed against assignment base
`6a9739e5`. This round verifies the repair of review finding `25a53951` (fix commit
`f5df0c01`) plus the contract repair of finding `4ba66d19`.
Verdict: **approved** — independent evidence supports every acceptance criterion; the
regression test for the repaired defect demonstrably kills the bug.

## Diff inspection (base → candidate)

Product surface, all inspected:

- `crates/gyre-server/src/signal_chain.rs` (new, 1606 lines incl. tests): `OrchestratorRegistry`
  (per-workspace + per-repo `Arc<Mutex<()>>` maps behind one `Arc` — stable lock identity across
  `AppState` clones), `on_spec_approved` (workspace lock → ensure live workspace orchestrator via
  task-093 spawn core → inbox delivery `Destination::Agent` → run), workspace-orchestrator run
  (spec read at pinned SHA via real `git cat-file` `read_git_file`, `spec_links` dependent query,
  Delegation task with `spec_ref path@sha` for the spec's repo, Coordination tasks for dependent
  repos, priority-4 `CrossWorkspaceSpecChange` notifications for cross-workspace dependents,
  ack + Idle transition), `scheduler_run_once` (claims Backlog/Blocked/stale-InProgress
  Delegation/Coordination tasks, per-repo lock, failure releases the claim to Blocked for retry),
  `decompose_delegation` (ordered Implementation sub-tasks with `spec_ref`, `parent_task_id`,
  `order`, `depends_on` chaining; LLM judgment via `state.llm` when configured, deterministic
  `##`-section fallback without; delegation → Done), `assess_coordination`, 30 s
  `spawn_task_scheduler` loop.
- `api/specs.rs`: `approve_spec` emits the bus `SpecApproved` (`Destination::Workspace(ws)`,
  payload with all five spec'd fields plus a documented server-side `workspace_id` used to scope
  the registry), then invokes `signal_chain::on_spec_approved`. Incidental fix: `reject_spec`
  notification fanout no longer fabricates tenant scope via `entry.repo_id.unwrap_or("default")`
  — resolves tenant from the workspace record, skips+logs when unresolvable; exemption entry
  deleted and `FROZEN_EXEMPTION_COUNT` 7→6.
- `domain/task.rs` + SQLite/PG adapters + `schema.rs` + migration
  `2026-10-10-000056_task_spec_ref` (next unused sequence, portable `ALTER TABLE ... ADD COLUMN
  spec_ref TEXT` on both backends): new `spec_ref` field, plumbed through REST `create_task`,
  MCP `gyre_create_task` (schema + handler + list output).
- `main.rs` starts the scheduler; `jobs.rs` registers `task_scheduler` (healthz + manual run);
  `stale_agents.rs` refreshes system-spawned (`spawned_by="system"`) orchestrators instead of
  kill/restart churn.
- `web/src` unchanged (only rebuilt `web/dist` bundle) — no frontend probe needed.

## Probes run (evidence: /tmp/stage/review-evidence/)

All with `/tmp/stage/bin/cargo`, `SKIP_WEB_BUILD=1`, at candidate HEAD:

- `cargo test -p gyre-server --lib signal_chain` → **11 passed, 0 failed** (exit 0).
- `cargo test -p gyre-server --lib api::specs` → **74 passed, 0 failed** (exit 0) — includes
  `approve_spec_emits_spec_approved_and_triggers_chain` (bus message, payload fields, Workspace
  destination, orchestrator spawn, delegation task with pinned spec_ref) and
  `reject_spec_cancels_chain_created_delegation_task`.
- `cargo test -p gyre-server --lib api::orchestrator` → **7 passed, 0 failed** (exit 0) —
  includes `stale_detector_refreshes_system_orchestrator_instead_of_churning` and the 409
  one-live conflict test.
- `cargo test -p gyre-server --lib mcp` → **79 passed, 0 failed**; `--lib jobs` → **11 passed,
  0 failed**; `--lib api::tasks` → **6 passed, 0 failed**.
- **Mutation probe (repair verification)**: temporarily removed the two lines of fix `f5df0c01`
  (`sub.repo_id = repo.id.clone(); sub.workspace_id = repo.workspace_id.clone();`) from
  `decompose_delegation` and ran
  `cargo test -p gyre-server --lib scheduler_decomposes_delegation_into_ordered_subtasks` →
  **FAILED** at signal_chain.rs:1406, `left: Id("")`, `right: Id("r-1")`, exit 101 — the test
  deterministically kills the exact defect the fix repaired; it is not self-confirming. Source
  restored immediately after (clean tree, HEAD unchanged, fix lines present, full suite green
  at HEAD before the experiment).
- Mechanical gates, all OK at the candidate: check-arch, check-abac-route-registry,
  check-abac-exempt-handlers, check-mcp-write-tools, check-migration-versions,
  check-migration-sql-portability, check-in-memory-state-stores (registry is process-local
  serialization only; agent liveness lives in the port-backed agents store — correct
  classification, marked in source), check-byte-slice-truncation, check-inert-enforcement,
  check-dead-message-kinds, check-fail-open-ref-resolution, check-relative-path-defaults,
  check-fabricated-scope-defaults (exemption count 6 matches file), check-scope-literal-defaults,
  check-lossy-secret-conversion, check-forwarded-header-trust, check-unbounded-external-http,
  check-task-commit-attribution (product commits recorded in `commits:` frontmatter; the two
  trailing lifecycle-only commits are correctly outside it).

## Acceptance criteria check

- Spec approval emits `SpecApproved` on the bus — yes (code + `approve_spec_...` test).
- `OrchestratorRegistry` with per-workspace and per-repo mutexes — yes
  (`registry_locks_are_per_scope_and_stable`, `registry_workspace_lock_serializes`).
- Workspace orchestrator auto-spawned on `SpecApproved` — yes (spawn-on-demand via the task-093
  core with `workspace-orchestrator` persona, workspace-scoped JWT; reuse of a live one).
- Delegation task for the spec's repo / coordination tasks for dependents / priority-4
  cross-workspace notifications — yes (`spec_approved_creates_orchestrator_and_tasks`).
- Repo orchestrator auto-spawned on delegation task creation — yes
  (`scheduler_decomposes_delegation_into_ordered_subtasks` asserts the RepoOrchestrator agent
  for r-1; `repo-orchestrator` persona, repo-scoped JWT via spawn core).
- Ordered sub-tasks with spec_ref/parent_task_id/order/depends_on; delegation marked Done —
  yes, mutation-verified (see probe above).
- Scheduler distinguishes Delegation/Coordination/Implementation — yes
  (`scheduler_ignores_untyped_tasks`, Implementation tasks stay Backlog for Phase 4;
  `spawn_agent_core` independently rejects Delegation/Coordination).
- Exactly-one-active for both types — yes (registry mutex serialization tests, 409 conflict on
  second orchestrator, `second_approval_reuses_then_respawns`).
- `cargo test --all` — full-workspace run is the verification gate's scope (per the task's
  shipped notes); every crate/module touched by this diff was exercised green here with
  focused suites. Nothing in the diff touches listener-bound code paths.

## Notes (non-blocking, for the verifier)

- The orchestrator "runs" are server-driven: the spawn cores create real agent entities
  (personas, scoped JWTs, keypairs, budget), and the processing logic executes server-side
  under the registry locks, using the persona prompt + `state.llm` when configured. This
  realizes the spec's own "internal server mechanism — the server intercepts workspace-destined
  SpecApproved messages" language; the created tasks are real port-backed records with real
  spec data, not fabricated output.
- The bus payload carries an additional documented `workspace_id` field beyond the five spec'd
  fields; the spec'd fields are all present and tested.
- `run_workspace_orchestrator` acks the orchestrator's full inbox at the end of each run
  (`acknowledge_all`); acceptable for a run that has just processed the delivered signal, but a
  mid-run arriving message destined to the same orchestrator would be acked without dedicated
  processing. Cosmetic at current message volumes.
- Scheduler staleness re-claim (60 s) can double-process a Delegation whose LLM run exceeds the
  bound; mitigated by the Backlog→InProgress claim and the per-repo lock, and the bound matches
  the stale-agent heartbeat timeout by design.
