# Review — task-195 (Manifest-driven gate selection for MRs referencing a spec)

Spec: `specs/system/spec-registry.md` §16 Agent Gates Integration (+ Agent Approver / Gate Schema).
Assignment: base `3af80bd21`, candidate `6c8b3e09`.
Verdict: **approved** — no blocking findings. Compile/test execution deferred to the verifier (see below).

## Environment restriction (not a code defect)

This review sandbox has no crates.io access (`curl https://index.crates.io/config.json`
returns an empty body; cargo falls back through retries and fails to fetch `anyhow`),
no vendored registry, and an empty `CARGO_TARGET_DIR` — so no Rust compilation or test
execution was possible here. Evidence: `/tmp/stage/review-evidence/cargo-check-gyre-server.txt`.
Per the assignment contract ("Large gyre-server builds/test compilation must run in the
independent verifier"), the compile/test probes are recorded below for the verifier.
In their place, this review verified every API/signature the new code calls against its
definition (all match) and ran all shell-based mechanical checks.

## Probes run

- `bash scripts/check-arch.sh` → **pass** (hexagonal boundaries intact).
- `bash scripts/check-fabricated-scope-defaults.sh` → **pass** (exemption count stays 1, no growth).
- `bash scripts/check-mem-port-contracts.sh` → **pass**.
- `bash scripts/check-inert-enforcement.sh` → **pass**.
- `bash scripts/check-byte-slice-truncation.sh` → **pass**.
- `bash scripts/check-relative-path-defaults.sh` → **pass**.
- `bash scripts/check-lossy-secret-conversion.sh` → **pass**.
- `bash scripts/check-scope-literal-defaults.sh` → **pass**.
- `bash scripts/check-forwarded-header-trust.sh` → **pass**.
- `bash scripts/check-fail-open-ref-resolution.sh` → **pass**.
- `bash scripts/check-unbounded-external-http.sh` → **pass**.
- Full diff inspection `3af80bd21..6c8b3e09` (gate_executor.rs, api/spawn.rs, mcp.rs,
  ralph_loop.rs, signal_chain.rs, docs, exemptions, coverage matrix).
- Signature verification of every dependency the new code calls: `AgentRepository::find_by_name`/
  `list_by_workspace`, `ReviewRepository::{submit_review,list_reviews}`, `MessageRepository::list_unacked`,
  `QualityGateRepository::list_by_repo_id`, `GateResultRepository::list_by_mr_id`,
  `ComputeTargetRepository::{get_by_id,get_default_for_tenant}`, `WorkspaceRepository::find_by_id`,
  `AgentSigningKey::mint`, `emit_event`, `revoke_agent_token`, `attest_agent_with_container`,
  `derive_attestation_level`, `read_manifest`/`parse_manifest`, `Agent::new`, `Repository::new`,
  `MergeRequest::new`, `Review::new`, `register_spawned_agent`, `ComputeTargetConfig`,
  `gyre_ports::ComputeTarget::{spawn_process,kill_process,is_alive}` — all exist with matching shapes.
  `tempfile` present in gyre-server deps; workspace edition 2021 (`std::env::set_var` in tests is safe).

## Verifier must execute

1. `SKIP_WEB_BUILD=1 cargo build --all` (or `cargo check -p gyre-server --tests` minimum).
2. `cargo test -p gyre-server --lib gate_executor` — the 20+ manifest-gate tests, notably
   `manifest_gates_resolved_from_spec_ref`, `trigger_gates_runs_manifest_gates_for_spec_ref_mr`,
   `trigger_gates_manifest_gate_wins_over_repo_gate_for_same_persona`,
   `trigger_gates_no_spec_ref_runs_repo_gates_only`,
   `trigger_gates_repo_without_manifest_runs_repo_gates_only`,
   `trigger_gates_spawns_gate_agents_for_manifest_gates`,
   `trigger_gates_launches_gate_agent_runtime_with_token`,
   `gate_agent_runtime_process_killed_on_teardown`,
   `persona_gate_fails_without_qualifying_review` (kills any regression to the auto-pass stub),
   `persona_gate_fails_when_attestation_below_minimum`, `persona_gate_fails_on_stack_hash_mismatch`.
3. `cargo test -p gyre-server --lib spawn` and `--lib ralph_loop` (upstream spawn/re-spawn tests
   re-compiled against the shared `dispatch_agent_launch`).
4. Integration suites: `cargo test -p gyre-server --test api_integration`, `--test e2e_ralph_loop`,
   `--test git_integration`, `--test auth_integration`.

## Verified against the acceptance criteria

- **Per-spec gate selection is real.** `manifest_gates_for_mr` (pure, unit-tested) resolves the
  manifest entry matching the MR's `spec_ref` path (normalizing `specs/` prefixes and bare
  no-`@sha` paths) and returns its `gates:` entries as `ManifestGateSpec {persona,
  min_attestation_level (default 1), stack_hash, spec_path}`; empty for no manifest / no
  spec_ref / no match / no gates. `resolve_manifest_gates_for_mr` reuses task-193's
  `read_manifest` (no second reader) at repo HEAD — a justified deviation from the plan's
  preferred spec-SHA source, documented in code: the `spec_ref` SHA is the spec file's *blob*
  SHA (ledger `current_sha`), not a treeish, so it cannot address `specs/manifest.yaml`.
- **Union, not replace.** `trigger_gates_for_mr` keeps running repo-level gates and adds the
  manifest gates; on persona overlap the manifest gate wins and the repo-level duplicate is
  skipped (persona runs exactly once) — tested by
  `trigger_gates_manifest_gate_wins_over_repo_gate_for_same_persona`, which asserts exactly
  one security-persona result, the manifest gate id, and the uncovered accountability gate
  still running.
- **Merge-blocking participation is real.** Manifest gates materialize as `AgentReview`
  `QualityGate`s with deterministic ids (`manifest-gate:<repo>:<spec>:<persona>`), their
  `GateResult` rows flow through the same `gate_results` store, and `check_gates_for_mr`
  defaults unknown gate ids to `required = true` (verified in source) — pending holds the MR
  (`Ok(false)`), failed blocks with an error naming the manifest gate. Both are asserted
  (`trigger_gates_runs_manifest_gates_for_spec_ref_mr`, `trigger_gates_failed_manifest_gate_blocks_merge`).
- **Real persona path, never the stub.** `run_agent_review_gate` routes no-command gates with
  constraints to `run_persona_review_gate`, which passes only on an `Approved` review from an
  agent running the named persona whose workload attestation satisfies `min_attestation_level`
  (via the same `derive_attestation_level` task-193 uses) and, when pinned, an exact
  `stack_hash` match. Latest-verdict-per-reviewer, approval-wins; a persona agent's final
  `ChangesRequested` fails fast; otherwise it polls to its timeout and FAILS closed. The
  legacy auto-pass stub is gone entirely — the no-command/no-constraint arm fails closed
  (upstream task-134 semantics). `persona_gate_fails_without_qualifying_review` documents
  itself as the anti-stub kill test.
- **Forge actually spawns the gate agents.** `spawn_manifest_gate_agent` creates a real
  workspace-scoped `Agent` record (scope from the MR, no fabricated default), records the
  persona in `agent_personas`, registers a scoped EdDSA JWT in `agent_tokens` (same
  registration/mint shape `spawn_agent_core` uses), dispatches a `ReviewRequest` carrying the
  constraints, and launches the runtime process through the platform's compute-target
  machinery via `api::spawn::launch_agent_workload` — the shared `dispatch_agent_launch`
  refactor of the pre-existing four-backend dispatch, so there is still exactly one process
  execution path. The launch records a workload attestation (empty-stack → level 1, so the
  gate agent's own reviews can satisfy the schema-default `min_attestation_level` 1; higher
  levels require stack/container attestation as specced). `trigger_gates_launches_gate_agent_runtime_with_token`
  proves the launched runtime receives `GYRE_AUTH_TOKEN` (the actual registered token value),
  `GYRE_PERSONA`, `GYRE_MR_ID`, `GYRE_GATE_ID`, `GYRE_MIN_ATTESTATION_LEVEL`, and that the
  attestation is recorded. Teardown kills the process via its registry handle, revokes the
  token with an audit event, and stops the record (`gate_agent_runtime_process_killed_on_teardown`,
  `gate_agent_torn_down_after_gate_resolves`); re-enqueue reuses the running agent
  (`trigger_gates_reuses_running_gate_agent_on_reenqueue`).
- **No regression without spec_ref/manifest.** Both no-op cases are tested and assert exactly
  the repo-level gates (`trigger_gates_no_spec_ref_runs_repo_gates_only`,
  `trigger_gates_repo_without_manifest_runs_repo_gates_only`).
- **Test-inflation check:** the selection tests assert the resolved gate set directly
  (personas, defaults, stack_hash pin, dedup, empty cases); the trigger tests assert concrete
  `manifest-gate:` result ids, agent records, token registrations, dispatched ReviewRequest
  payloads, and env of the launched process — every one fails if manifest-gate sourcing is
  removed (the `manifest-gate:` ids are produced only by `materialize_manifest_gate`, which is
  reachable only through `resolve_manifest_gates_for_mr`).
- **Supporting surface:** `SpawnAgentRequest.persona` (also plumbed through the
  `gyre_spawn_worker` MCP tool) gives `agent_personas` a production writer at spawn time;
  `signal_chain.rs`/`ralph_loop.rs` initializers pass `None`; the GateFailure ws_id fallback
  fix (skip + log, `emit_reconciliation_completed` pattern) is genuine and the frozen
  exemption count stays at 1. Coverage §16 moved to `implemented` with an honest
  "awaiting independent review" note; §14 note updated. No ABAC route or `needs_write`
  registry changes were needed (no new routes/tools).

## Non-blocking observations (recorded, no action required)

- **Manifest-wins dedup drops a repo-level *command* gate for the same persona.** When a
  repo-level gate with a configured command targets a persona also declared in the manifest,
  the command gate is skipped in favor of the persona-review gate. This is documented in the
  code as a deliberate repair-round decision (the manifest gate carries the attestation
  constraints the repo-level gate cannot enforce, and it spawns a real gate agent). It
  satisfies the "no double-run" criterion, but operators should know a persona-overlapping
  command gate stops executing its command for spec-referencing MRs.
- **Manifest read at HEAD** (not the spec_ref SHA) — see above; matches task-193's policy
  source. A manifest changed at HEAD after the MR was opened changes the gate set; that is
  the same policy-freshness model the approval ledger uses.
- No generated-output (`web/dist/`) involvement in this task.

## Verdict

Approved. Every acceptance criterion is implemented in production code paths (not stubs),
the tests are mutation-killing by construction, and all mechanical invariants pass. The only
unverified dimension — compilation and test execution — is blocked by this sandbox's network
restriction and is recorded above for the verifier, per the assignment contract.
