# Review — task-115 (Spec approval signal chain & orchestrators)

Spec: `specs/system/agent-runtime.md` §1 Phases 1–3 (The Model, Agent
Lifecycle, Signal Chain, Phase 1 Spec Approval, Phase 2 Workspace
Orchestrator, Phase 3 Repo Orchestrator).
Candidate: `009d294669cb9fb410f85bbe6d705c367543957b` (base
`dbf9edaeae5c8a5441bd9ff50c9dff3cec99172a`, task-117 compute targets;
rebase merge `7fdac40e2`).
Verdict: **approved** — no findings.

## Round 24 (2026-11-10) — re-review after rebase onto task-117 base

### Scope

Diff from base is **docs-only** (`specs/tasks/task-115.md` contract
restore, `specs/coverage/system/agent-runtime.md` rows 3–6 → implemented);
all 13 frontmatter commits are ancestors of the base (the previously
approved signal chain, published to mainline through earlier bases). The
production code under review is therefore the tree at the candidate, which
I verified directly against the spec rather than trusting the prior
approval.

### Probes (evidence under /tmp/stage/review-evidence/)

- `cargo test -p gyre-server --lib signal_chain` (fresh debug build,
  10m53s cold compile via /tmp/stage/bin/cargo) → **19 passed, 0 failed**,
  exit 0 (`task-115-signal-chain-rerun.txt`; includes the 2 MCP
  signal-chain-field tests matched by the filter).
- `cargo test -p gyre-server --lib -- approve_spec_emits_spec_approved_and_triggers_chain
  reject_spec_cancels_chain_created_delegation` → **2 passed, 0 failed**,
  exit 0 (`task-115-approve-chain-e2e.txt`) — real HTTP-router-driven
  end-to-end: POST approve → bus `SpecApproved` (all five payload fields,
  `Destination::Workspace`) → orchestrator spawned → delegation task with
  `spec_ref: path@sha`; plus rejection-cancellation regression.
- Mechanical invariants → exit 0 each: `check-arch`,
  `check-in-memory-state-stores` (OrchestratorRegistry is locks-only, no
  durable bare-map state), `check-dead-message-kinds` (SpecApproved has a
  real emitter), `check-task-commit-attribution`,
  `check-unbounded-external-http` (LLM path latency-bounded by
  `LLM_CALL_TIMEOUT` = 120 s), `check-fail-open-ref-resolution`
  (`task-115-<script>.txt` per script).
- Full workspace `cargo test --all` remains verification-gate scope per
  review constraints; the focused suites above are the meaningful probe
  for this delta.

### Acceptance criteria — verified against the current tree

- **Spec approval emits `SpecApproved` on the bus** — `approve_spec`
  (specs.rs ~841–887) emits only on transition INTO Approved (partial
  human_and_agent approvals don't fire), payload
  `{repo_id, spec_path, spec_sha, approved_by, approval_id}` +
  internal `workspace_id`, `Destination::Workspace` when scoped; verified
  by code reading and the router-driven e2e test.
- **`OrchestratorRegistry` per-workspace/per-repo mutexes** —
  signal_chain.rs:66–140; real tokio `Mutex` per scope behind
  `Arc<RwLock<…>>` with stable identity across `AppState` clones, plus a
  whole-cycle scheduler run lock; registry unit tests assert scope
  isolation and serialization.
- **Workspace orchestrator auto-spawned on `SpecApproved`** —
  `on_spec_approved` (signal_chain.rs:185) holds the per-workspace lock,
  `ensure_workspace_orchestrator` reuses a live orchestrator or spawns via
  the spawn core (fail-closed persona resolution, workspace compute
  target), delivers the Directed-tier persisted inbox message, then runs
  processing. Workspace-less (Broadcast) payloads are a logged no-op.
- **Delegation task for the spec's repo** — `spec_ref: path@sha` pins the
  approved blob; bare `spec_path` keeps `list_by_spec_path` cancellation
  reachable (regression-tested); repo/workspace scoped from the ledger.
- **Coordination tasks for dependent repos** — real `spec_links` query
  (target repo + path match), plus priority-4
  `CrossWorkspaceSpecChange` notifications (`default_priority() == 4`,
  gyre-common/notification.rs:151) to Admin/Owner members of dependent
  workspaces; tenant resolved from the workspace record, never fabricated.
- **Repo orchestrator auto-spawned on delegation creation** — 30 s
  scheduler loop (main.rs:124, job-registry-recorded) claims only
  Backlog/Blocked/stale-InProgress Delegation/Coordination tasks under
  the cycle lock, spawns the repo orchestrator via the spawn core
  (repo-scoped JWT), runs under the per-repo mutex with re-read-after-lock.
- **Ordered sub-task decomposition** — `decompose_delegation`: empty/absent
  SHA hard-fails (claim released to Blocked, no sub-tasks — tested twice
  over), spec blob read via real `git show sha:path`, sub-tasks carry
  spec_ref/parent_task_id/order/depends_on chaining; LLM judgment when
  configured with honest deterministic per-`##`-section fallback;
  delegation marked Done after.
- **Scheduler distinguishes task types** — only Delegation/Coordination
  claimed; Implementation and untyped push-hook tasks stay Backlog
  (tested).
- **Exactly-one-active both types** — per-scope mutexes + spawn-core DB
  one-live check; concurrent-cycle, stale-claim crash recovery, and
  failure-release-retry are all regression-tested.
- **Tests pass** — focused suites green above; workspace-wide suite is
  verification-gate scope.

### Test-quality check

No test inflation found: the new tests assert real state through
port-backed stores (agent records, task records, inbox messages) and a
real HTTP router. Data-flow uniqueness argument (in lieu of a mutation
probe, which prior round performed on the identical test and which source
edits would invalidate here): the only code path creating
`WorkspaceOrchestrator` agents reachable in
`approve_spec_emits_spec_approved_and_triggers_chain` is the approve
handler → `on_spec_approved` → `ensure_workspace_orchestrator` chain (the
test makes no orchestrator-spawn API call), so the test cannot pass with
the production hook disabled.

### Non-blocking observations (recorded, not findings)

- **Stale Shipped sentence vs final frontmatter.** The Shipped section
  (written in `4ffc616ef`) states the `commits:` frontmatter "keeps the
  full 15-commit attribution, including the two checkpoint commits
  (`336dd201`, `79b06a7a`)". The final candidate commit `009d29466`
  ("process: record task-115 branch commits", the checkpoint recording
  step) normalized the frontmatter back to the 13 task-labeled commits,
  making that sentence stale as delivered. Not a finding because: (a) the
  two dropped commits are unlabeled workspace-wide recovery snapshots
  ("checkpoint: recover interrupted pipeline source" — they carry
  task-170/229/232 surface alongside an earlier snapshot of this task's
  files), so the task-commit-attribution invariant passes (verified,
  exit 0) and no task-labeled product-surface commit is unlisted; (b) the
  authoritative scoping list (the assigned task body) matches the final
  frontmatter — controller and doc are consistent; (c) the dropped
  commits carry no signal-chain content absent from the current tree,
  which was reviewed directly; (d) the frontmatter line is
  checkpoint-recording-owned and demonstrably re-normalized each round,
  so an agent-side "fix" would not stick — the durable fix is for the
  Shipped text not to over-specify the recording-owned line. A verifier
  noticing the mismatch loses nothing: the staleness claims MORE scrutiny
  than the list provides, never less.
- Prior-round observations remain accurate on this tree: spawn-failure
  degradation asymmetry (Phase-1 signal dropped until next approval,
  bus message persists; Phase-3 retries via Blocked), Idle-orchestrator
  reuse on next signal, process-local locks paired with the DB one-live
  check, and the unfiltered `spec_links.status` in the dependents query.

### Coverage-matrix delta check

Rows 3–6 of `specs/coverage/system/agent-runtime.md` flipped
task-assigned → implemented with code line references — spot-checked
against the tree (specs.rs:842–887, signal_chain.rs:66–140/185/250/286/
568/727/767, lib.rs:458, main.rs:124); all match, and the behaviors are
genuinely present per the probes above. Coverage count 5/33 → 9/33 is
arithmetically consistent. Rows 1–2 correctly remain task-assigned
(parent headings whose requirements live in Phases 4–8, task-118).
