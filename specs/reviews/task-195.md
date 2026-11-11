# Review — task-195 (Manifest-driven gate selection for MRs referencing a spec)

Candidate: `e875634bfa1c2dfb7e317f1c7e412f5d74e751d1` (base `65f71e657`; task commits
`bb9dfd83c`, `35b4dc991`, `a65ae04d8`, `4494a058f`, rebase repair `0c137722c` +
`a3e0a875a`). Spec: `specs/system/spec-registry.md` §16 Agent Gates Integration.
Verdict: **approved** (server test execution deferred to the independent verifier
per the 4 GiB build-wrapper contract — commands recorded in
/tmp/stage/review-evidence/review-summary.md).

## What this round independently verified

- **Diff review (base..candidate, every hunk):** selection, sourcing, union/dedup,
  agent spawn, launch, teardown, persona-constrained review evaluation, tests,
  docs, coverage row 16, and the rebase repair of `api/spawn.rs`,
  `docs/api-reference.md`, and the fabricated-scope exemptions file.
- **Selection is real and pure:** `manifest_gates_for_mr(manifest, spec_ref)`
  normalizes `specs/` prefixes and bare no-`@sha` paths (a form spawn.rs:1966
  actually writes), defaults `min_attestation_level` to 1, carries
  `stack_hash`/`spec_path`, dedupes repeated personas. Empty for no manifest /
  no spec_ref / no entry / no gates — the no-regression criteria have dedicated
  tests (`trigger_gates_no_spec_ref_runs_repo_gates_only`,
  `trigger_gates_repo_without_manifest_runs_repo_gates_only`).
- **HEAD as the manifest policy source is correct and consistent:** the SHA in
  `spec_ref` is the spec file's blob SHA (ledger `current_sha`, `git rev-parse
  <commit>:<path>`), not a treeish; task-193's `resolve_new_approval_status`
  reads the manifest at HEAD for the same reason. The task plan explicitly
  allows matching task-193's approach. Single manifest reader reused
  (`spec_registry::read_manifest`).
- **Union + dedup:** repo-level gates keep running except when a manifest gate
  covers the same persona — then the manifest gate wins (it is the only one of
  the pair that can enforce the attestation constraints) and the persona runs
  exactly once; tested (`trigger_gates_manifest_gate_wins_over_repo_gate_for_same_persona`).
- **Real persona path, not the stub:** manifest gates are materialized with
  `command: None` + constraints, routing to `run_persona_review_gate`, which
  passes only on an Approved from a persona-matching reviewer whose derived
  workload-attestation level ≥ `min_attestation_level` and, when pinned, an
  exact `stack_hash` match — using task-193's `derive_attestation_level`, no
  invented levels. It fails closed on timeout; `persona_gate_fails_without_qualifying_review`
  kills any regression to the auto-pass stub. Verdict ordering is
  latest-per-reviewer, approval-wins in both directions (two tests).
- **Merge-blocking participation:** manifest gate ids are unknown to the
  quality-gates store, and `check_gates_for_mr` defaults unknown ids to
  `required = true` (verified at gate_executor.rs:1953) — pending manifest gates
  hold the MR (`Ok(false)`), a failed one blocks with `Err` naming the gate
  (both tested). No parallel results table.
- **Forge-spawned gate agents that execute (§16):** real `Agent` record
  (Active, workspace-scoped, `spawned_by: "server"`), persona recorded in
  `agent_personas` (now with a production writer at spawn time —
  `SpawnAgentRequest.persona`, also plumbed through `gyre_spawn_worker`),
  scoped JWT registered in `agent_tokens` (verified to be the real
  authentication channel in auth.rs), `ReviewRequest` dispatched with the
  constraints, and the runtime launched through the platform's compute-target
  machinery (`api::spawn::launch_agent_workload`, extracted from
  `spawn_agent_core`). Teardown kills the runtime via the process-registry
  handle, revokes the token with an audit event (verified
  `revoke_agent_token`), stops the record; re-enqueue reuses the Active agent.
- **Rebase repair audited:** `git diff 9d0c6995..HEAD --
  crates/gyre-server/src/gate_executor.rs` is 38 lines — only the upstream
  GatePassed notification; the task-195 logic is byte-identical to the
  pre-rebase candidate that a prior round runtime-verified green (46/46
  gate_executor, 1506/1506 lib, integration suites) including a mutation probe.
  The rebuilt `launch_agent_workload` arms match the upstream base's
  `spawn_agent_core` arms line-for-line (read side-by-side), with the SSH
  container-mode name guard kept in-arm to cover the gate-agent path.

## Probes (logs in /tmp/stage/review-evidence/)

- `cargo check -p gyre-server` → pass (7m28s); `cargo check -p gyre-server
  --tests` → pass (covers gate_executor + spawn test code).
- `bash scripts/check-arch.sh` → pass. Mechanical checks all pass:
  fabricated-scope-defaults (exemption count correctly lowered 2→1), mcp-write-tools,
  scope-literal-defaults, dead-message-kinds, inert-enforcement,
  in-memory-state-stores, forged-scope-fields, unbounded-external-http,
  forwarded-header-trust, task-commit-attribution (against the committed
  candidate; the working-tree failure is the review harness's uncommitted revert
  of the task file to its pre-assignment baseline, not a defect).
- **Verifier must run (wrapper blocks gyre-server test builds at 4 GiB):**
  `cargo test -p gyre-server --lib gate_executor`; `cargo test -p gyre-server
  --lib spawn`; `cargo test -p gyre-server --lib`; `--test api_integration`;
  `--test auth_integration`; `--test e2e_ralph_loop`; `--test git_integration`.

## Non-blocking observations (recorded, not findings)

- A manifest gate with `min_attestation_level ≥ 2` is satisfiable only by
  container-launched runtimes (level 3) or agents whose stack was attested at
  launch; a locally-launched gate agent derives level 1 and the gate fails
  closed. This is the platform's pre-existing attestation model (identical
  shape to task-193's agent-approval validation) — honest enforcement, never
  fail-open.
- Teardown kills via `LocalTarget::kill_process` regardless of launch target;
  for container/SSH handles the pid-based kill is a no-op, mirroring the
  pre-existing admin kill path. Token revocation remains the containment.
- `std::env::set_var("GYRE_AGENT_COMMAND", ...)` in the launch-contract test is
  process-global (latent flake source if a future test asserts on the default
  command); benign today.
- Repo-level gates skipped on persona overlap lose any custom command they
  carried; the manifest gate still runs the persona through a real agent with
  the manifest's constraints. Documented trade-off from review round R2.
