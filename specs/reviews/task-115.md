# Review — task-115 (Implement spec approval signal chain & orchestrators)

Spec: `specs/system/agent-runtime.md` §1 Phases 1–3 (Signal Chain, Spec Approval Triggers
Orchestration, Workspace Orchestrator, Repo Orchestrator — Task Decomposition).
Candidate under review: `b45facc603d4c037c7cd67476c123c3d084918ee` on
`pipeline/task-115/5fff7ac45c2142a8a9bdf7180c359a16-1`, base `72ad2a128` (candidate is a
descendant; last three commits are docs-only on `specs/tasks/task-115.md`).
Verdict: **approved** (no findings).

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-server --lib signal_chain` → **19 passed, 0 failed** (registry lock
  semantics, full chain spawn→delegation/coordination→decomposition, crash recovery,
  failure-release-and-retry, concurrent-cycle serialization, targeted ack, empty-SHA guards).
- `cargo test -p gyre-server --lib -- approve_spec_emits_spec_approved_and_triggers_chain
  reject_spec_cancels_chain_created_delegation_task` → **2 passed, 0 failed** (real router
  oneshot through the production approve/reject handlers).
- `cargo test -p gyre-adapters --lib create_persists_workspace_scope` → **1 passed**;
  `cargo test -p gyre-adapters --lib` full → **371 passed, 0 failed** (12 ignored,
  external-service gated).
- `cargo test -p gyre-server --lib` full → **1423 passed, 0 failed** (includes `ws::` —
  listener permitted in this sandbox).
- `cargo test -p gyre-domain -p gyre-common` → **all passed**; `cargo check -p gyre-cli` clean.
- Mechanical checks all OK: arch, in-memory-state-stores, unbounded-external-http,
  migration-versions, migration-sql-portability, fabricated-scope-defaults,
  scope-literal-defaults, mcp-write-tools, abac-route-registry, dead-message-kinds,
  byte-slice-truncation, relative-path-defaults, fail-open-ref-resolution,
  lossy-secret-conversion, inert-enforcement, forwarded-header-trust, forged-scope-fields,
  mem-port-contracts, task-commit-attribution.
- **Temporary probe (restored; tree clean)**: cross-workspace dependency path — the only
  signal-chain path with no committed test — exercised via a scratch test seeded with a
  two-workspace `spec_links` dependency. Result: coordination task lands in the dependent
  repo's own workspace (r-3/ws-2) and a **priority-4** `CrossWorkspaceSpecChange`
  notification is created for the dependent workspace's Admin (`default_priority()` → 4).
  Probe passed; file restored, `git status` clean.

## Verified against acceptance criteria

- **Phase 1 emission**: `approve_spec` emits `SpecApproved` on the bus (Workspace
  destination, spec'd payload fields + `workspace_id` for registry scoping), correctly
  gated on the ledger transition INTO Approved (base's mode-based resolution preserved
  through the rebase onto `72ad2a128`); partial/redundant approvals do not trigger the
  chain. Bus emission is real (signed, persisted, Directed/Event tiers).
- **OrchestratorRegistry**: per-workspace and per-repo tokio mutexes behind an `Arc`
  (stable lock identity across state clones), plus a whole-cycle scheduler run lock.
  Process-local serialization only — durable liveness stays in the port-backed agents
  store (accepted by check-in-memory-state-stores; no orphaned state on restart).
- **Spawn-on-demand**: both orchestrators are real `Agent` entities spawned through the
  task-093 cores (scoped EdDSA JWT, keypair bootstrap, budget check, DB one-live check);
  the chain adds the in-process mutexes the spec calls for.
- **Phase 2 fanout**: delegation task for the spec's repo with `spec_ref: path@sha`
  (pinned blob; bare `spec_path` preserved so rejection cancellation via
  `list_by_spec_path` reaches it — regression-tested), coordination tasks for dependent
  repos from a real `spec_links` query, p4 notifications for cross-workspace dependents
  (probe-verified). Spec content is really read (git object at the approved SHA) and the
  summary is consumed into the delegation description.
- **Phase 3**: 30 s scheduler (job-registry recorded, admin-triggerable) claims
  Backlog/Blocked/stale-InProgress Delegation/Coordination tasks only — Implementation
  tasks stay Backlog for Phase 4 (tested). Decomposition produces real Implementation
  sub-tasks with `spec_ref`, `parent_task_id`, `order`, and `depends_on` chaining; empty/
  unresolvable SHA hard-fails (claim released to Blocked, no sub-tasks) — no fail-open.
  LLM judgment is real when `GYRE_VERTEX_PROJECT` is configured (`predict_json` → actual
  Vertex AI HTTP, latency-bounded at 120 s plus adapter client timeouts — the exemption
  list was emptied by actually bounding the clients, not by adding entries); the no-LLM
  fallback is deterministic from real spec content and fabricates no model output.
- **Failure handling**: run failure releases the claim to Blocked and retries (tested via
  budget-cap injection); crash recovery re-claims stale InProgress claims exactly once;
  targeted ack leaves Escalations for their real consumers; stale-agent detector refreshes
  system orchestrators instead of churning spawn/kill/restart (regression-tested), and the
  Dead transition now releases the budget slot.

## Notes (non-blocking)

- The orchestrators' run logic is server-driven (documented stance: "equivalent effect" to
  the spec's "via MCP tools" wording — task creation via the task store, git reads direct).
  Phase 3 judgment is LLM-driven when configured; Phase 2's fanout is deterministic. The
  spawned agent entities, personas, and JWTs are real; no fabricated LLM output anywhere.
- The repo orchestrator stays Active after decomposition (documented: live for future
  coordination tasks, one per repo) and therefore holds one budget slot per repo — a
  design consequence of exactly-one-active, not a leak.
- Coverage rows for agent-runtime.md §1 Phases 1–3 remain `task-assigned` pending the
  auditor stage; nothing in this candidate contradicts them.
- `cargo test --all` (workspace-wide) is the verification gate's to run; the two touched
  test crates plus domain/common/cli all pass here.
