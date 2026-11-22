# Review — task-297 (Gate all implementation work on approved specs)

Spec: `specs/system/agent-runtime.md` §1 The Signal Chain ("There is no other
trigger. The `SpecApproved` event is the universal starting signal.").
Candidate under review: `38fdee9f` (branch
`pipeline/task-297/9e592fd03db744fabbfd46698cdae8b6-1`, base `7e746233`;
product commit `569ea138` plus review/CI-repair fixes `6c7ce409`,
`52397d16`, `ae87cf5b`, `611bc5ff`, `bb42b05c`).
Verdict: **approved** — no findings. All three gates are real, fail-closed,
and placed before the effect they guard; the approval-side behavior is
untouched; the tests are behavior-killing, not self-confirming.

## Probes

- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 (15m28s).
  Server test execution is blocked by the sandbox build wrapper and deferred
  to the verifier (required probes listed in
  `/tmp/stage/review-evidence/task-297-review.md`).
- All mechanical repository checks pass on the candidate (arch, ABAC route
  registry + exempt handlers, api-auth, inert enforcement, MCP write tools,
  mem port contracts, fabricated/lossy/scope-literal checks, byte-slice,
  relative-path, forwarded-header, in-memory state stores, unbounded HTTP,
  forged scope fields, conditional test guards, rustfmt-diff, task-commit
  attribution on the committed tree). `check-assertionless-tests.sh` exits 2
  identically at the base commit (environmental awk failure) — not a
  candidate regression.
- CI evidence (PR head `01752c5d`, nextest run 45014043762): 4378 passed /
  1 failed / 6 skipped; every task-297 AC test existed at that head and
  passed. The single failure (`push_records_agent_commit_provenance`) was a
  test-harness storage-isolation regression (the branch's two
  `start_server_with_state` helpers lacked the in-memory opt-in after
  main's task-308 durable default landed) — root cause verified against
  `01752c5d` sources; fixed by `bb42b05c` (test files only, no assertion
  changed). The candidate head differs from `bb42b05c` only in
  `specs/tasks/task-297.md`.

## Verified working (no findings)

- **Gate 1 — task creation.** `create_task` (api/tasks.rs) and MCP
  `gyre_create_task` (mcp.rs) reject `task_type=implementation` unless
  `gates::verify_implementation_spec_ref` resolves the pinned `path@sha`,
  BEFORE any task row is written: 400 `InvalidInput` (HTTP) /
  `SPEC_APPROVAL_REQUIRED` (MCP), both naming the single-trigger
  requirement. The gate is fail-closed on BOTH the durable per-SHA approval
  row (the merge-gate `verify_spec_ref` query) AND the registry entry
  (Approved + `current_sha` == pinned SHA — exactly the condition under
  which `approve_spec` emits `SpecApproved`, so partial multi-mode
  approvals correctly fail). Delegation/Coordination/untyped tasks pass
  untouched. Bypass sweep clean: `update_task` cannot set
  task_type/spec_ref; compose/budget/admin/orchestrator creation paths
  produce untyped or Coordination tasks only; POST /api/v1/tasks is the
  only REST task-create route; `gyre_decompose_spec` pins sub-task
  spec_refs and refuses unpinnable parents.
- **Gate 2 — scheduler (defense in depth).** The Phase-4 scan verifies the
  spec_ref BEFORE claiming: on failure the task stays Backlog with the
  observable `spec-approval-missing` label (fresh-row re-read, added only
  if still Backlog/Implementation, cleared on a later successful claim) and
  a logged reason — no agent record, no InProgress transition. An approval
  invalidated by a later push (registry moved to Pending/new SHA + rows
  revoked) stops the spawn.
- **Gate 3 — direct spawn.** POST /api/v1/agents/spawn returns 403 naming
  the requirement for human/agent principals spawning a worker bound to an
  Implementation task; interrogation (`agent_type`) and the genuine system
  principal (`agent_id=="system" && user_id.is_none()`, unforgeable) remain.
  The scheduler calls `spawn_agent_core` directly and performs its own
  spec verification; the core's task-claim boundary keeps a direct kill
  test.
- **Approval side preserved.** No diff in the approve/revoke handlers,
  spec_registry sync, or post-receive revocation. The coordination leg now
  pins the approved source spec_ref at creation so its review sub-task
  passes the Phase-4 gate instead of stalling Backlog forever (with tests).
- **Test quality.** The AC tests drive the real router/MCP dispatch and
  assert 4xx/tool-error + zero rows written, then the identical request
  succeeding after a REAL `POST /api/v1/specs/:path/approve`; the scheduler
  test inserts the unapproved twin directly into storage and asserts zero
  agent records while the approved twin spawns exactly one worker; the
  spawn test uses a verified human Keycloak JWT (403) and an agent
  principal interrogation spawn (201). Fixture seeding replicates exactly
  the durable state the approve endpoint writes. No assertion weakened;
  the changed own-task respawn expectation (CREATED→403) reflects the new
  specced behavior, with the core boundary still probed directly.

Non-blocking observations (recorded in
`/tmp/stage/review-evidence/task-297-review.md`):

- MCP `gyre_spawn_worker` (repo-orchestrator tier, system-minted downstream
  of SpecApproved) still reaches `spawn_agent_core` without the direct-spawn
  403 — within the task contract (gate 3 names the REST endpoint; Phase-4
  remains defense in depth), suggested as future hardening.
- The working tree carries an unstaged executor-stage revert of
  `specs/tasks/task-297.md` frontmatter (progress → not-started, commits →
  []); it makes `check-task-commit-attribution.sh` fail in the working
  tree while the committed candidate passes. The controller should publish
  the committed frontmatter.

Verifier must run (server test codegen exceeds this sandbox): the focused
nextest probes listed in the evidence file (signal_chain, mcp gate tests,
spawn gate tests, api_integration AC test, git_integration CI-repair pair,
auth/m18/e2e harnesses), then the full workspace suite and Clippy.
