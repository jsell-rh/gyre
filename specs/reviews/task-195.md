# Review — task-195 (Manifest-driven gate selection for MRs referencing a spec)

Spec: `specs/system/spec-registry.md` §16 Agent Gates Integration — "The `gates:` field
in the manifest feeds the gate chain: MR references `system/identity-security.md` ->
manifest says gates = [security, accountability]; Forge spawns security gate agent and
accountability gate agent for this MR."
Candidate: `6f511cae` on `pipeline/task-195/7d920e566230421a893d8105eb07ec6b-1`,
assignment base `f764bc00`. This round reviews the rebase resolution
(`cc161e22`, merging the assignment base into the branch after the DIRTY publication
state) plus the carried-forward implementation through `a57e0326`.
Verdict: **approved** — no blocking findings.

## What was verified (independently, this round)

### Selection is real and pure

`manifest_gates_for_mr(manifest, spec_ref)` (`gate_executor.rs:101`) resolves the
manifest `SpecEntry` whose normalized path matches the `spec_ref` path (leading
`specs/`/`./`/`/` stripped on both sides; `rsplit_once('@')` handles pinned SHAs and
bare paths) and returns its `gates:` entries as `ManifestGateSpec
{persona, min_attestation_level (default 1), stack_hash, spec_path}`. Empty for no
manifest / no spec_ref / no matching entry / no gates; repeated personas within an
entry dedupe (first wins). Unit tests assert the resolved gate set directly
(`manifest_gates_resolved_from_spec_ref`, `..._empty_for_missing_inputs`,
`..._dedupe_repeated_persona`).

Sourcing (`resolve_manifest_gates_for_mr`) reads `specs/manifest.yaml` via task-193's
single `read_manifest` (no second reader) at repo **HEAD** — a documented, justified
determination the task plan explicitly permits: the SHA in `spec_ref` is the spec
file's *blob* SHA (the ledger's `current_sha`), not a treeish, so it cannot address
`specs/manifest.yaml`; HEAD is also the policy source task-193's approval resolver
uses.

### Union without double-spawning

`trigger_gates_for_mr` keeps every repo-level gate except those whose persona is
covered by a manifest gate, and materializes each manifest gate as a
`manifest-gate:<repo>:<spec_path>:<persona>` `QualityGate` (AgentReview, command
`None`, not persisted to the repo gate store — correct, since manifest gates are a
per-MR selection; deterministic id keeps re-enqueues attributable). On persona
overlap the manifest gate wins with a stated fail-closed counterfactual rationale
(the repo-level alternative either blocks via the no-command fail-closed path or runs
an unconstrained reviewer); the persona runs exactly once.
`trigger_gates_manifest_gate_wins_over_repo_gate_for_same_persona` pins this.
No-regression paths are pinned by `trigger_gates_no_spec_ref_runs_repo_gates_only`
and `trigger_gates_repo_without_manifest_runs_repo_gates_only`.

### Real persona path, never the auto-pass stub; merge-blocking

`run_agent_review_gate` routes manifest gates (command `None` + constraints present)
to `run_persona_review_gate`: passes only on an `Approved` review from an agent
running the named persona (KV `agent_personas` — the same source
`build_agent_context` reads) whose workload attestation satisfies
`min_attestation_level` and, when pinned, an exact `stack_hash` match
(`check_reviewer_attestation` derives the level from the real
`workload_attestations` record). Otherwise it fails closed at the gate timeout.
Latest-verdict-per-reviewer semantics hold in both directions (stale
ChangesRequested superseded by the same reviewer's later Approved; withdrawn
approvals do not pass). Persona mismatch, sub-minimum attestation, and stack-hash
mismatch each have a dedicated failing test. GateResults are persisted through the
existing store; `check_gates_for_mr` defaults unknown gate ids to `required = true`,
so pending manifest gates hold the MR and failed ones block it
(`trigger_gates_runs_manifest_gates_for_spec_ref_mr`,
`trigger_gates_failed_manifest_gate_blocks_merge`).

### Forge-spawned gate agents (spec §16 wording)

`spawn_manifest_gate_agent` creates a real `Agent` record (workspace-scoped,
`spawned_by: "server"`), records the persona in `agent_personas` (fail-closed on
store errors), mints a `review:submit`-scoped EdDSA JWT via `mint_scoped`
(fail-closed — no unscoped fallback), registers it in `agent_tokens`, dispatches a
`ReviewRequest` message carrying `{mr_id, repo_id, spec_path, spec_ref, persona,
min_attestation_level, stack_hash}`, and launches the runtime through the platform's
compute-target machinery (`api::spawn::launch_agent_workload` — workspace-assigned
target → tenant default → local; server-controlled command only; token in
`GYRE_AUTH_TOKEN`; review-context env `GYRE_MR_ID`/`GYRE_PERSONA`/
`GYRE_MIN_ATTESTATION_LEVEL`/`GYRE_STACK_HASH`/…; workload attestation recorded so
the agent's own reviews derive at level ≥ 1). Re-enqueue reuses a still-Active gate
agent (no double-spawn); teardown kills the runtime via the backend that owns the
handle (`spawned_backends` routing — the admin-kill invariant, with a test that fails
if teardown regresses to a LocalTarget kill), revokes the token, and sets Stopped.
`trigger_gates_launches_gate_agent_runtime_with_token` asserts the launch contract
against a real forked process (token + context in its env dump, attestation
recorded); `gate_agent_token_is_scoped_reviewer_jwt` pins the scope and subject.

### Reviewer identity binding (supporting surface)

`submit_review` binds agent-token principals (any scope, not just `review:submit`)
to their verified token subject; human principals may use a body reviewer id only
when it equals their own verified `user_id` (Admin may record on behalf). The
comparison is against `user_id`, never the user-controlled display name. Web review
flows were updated to resolve the caller's own id (`selfReviewerId`), failing closed
to the legacy pseudo-id the server rejects. The unrouted legacy
`agent_messages.rs` drain-and-clear handlers (dead code at base — verified the
router never referenced them) were deleted rather than exempted. Scoped-token ABAC
allow-list keeps the REST agent-inbox route off-limits to review tokens
(`scoped_review_token_cannot_read_agent_inbox_route`), with delivery via launch env
+ `/mcp` `gyre_message_poll` (own-inbox by token subject).

### Spawn-side persona trust

`SpawnAgentRequest.persona` (bare name, also exposed on `gyre_spawn_worker`) is
validated fail-closed against the repo/workspace/tenant Approved-persona chain
before any state is created — a spawn-capable caller cannot mint a
gate-satisfiable persona identity (`spawn_bare_persona_fails_closed` covers unknown
and Pending personas, no orphan rows, no `agent_personas` writes;
`spawn_bare_persona_approved_is_recorded` is the positive counterpart).

## Probes (this sandbox; logs in /tmp/stage/review-evidence)

- `cargo check -p gyre-server --tests` → exit 0 (cold, 10m31s).
- `bash scripts/check-arch.sh`, `check-abac-route-registry.sh`, `check-mcp-write-tools.sh`,
  `check-dead-message-kinds.sh`, `check-abac-exempt-handlers.sh`,
  `check-byte-slice-truncation.sh`, `check-fabricated-scope-defaults.sh`,
  `check-relative-path-defaults.sh`, `check-fail-open-ref-resolution.sh`,
  `check-inert-enforcement.sh`, `check-forwarded-header-trust.sh`,
  `check-in-memory-state-stores.sh`, `check-unbounded-external-http.sh`,
  `check-lossy-secret-conversion.sh`, `check-scope-literal-defaults.sh`,
  `check-forged-scope-fields.sh` → all exit 0. Exemption files: no entry added
  (fabricated-scope count lowered 2→1 by this task's fix).
- `cd web && npm ci && npm test` → exit 0 (113 files, 2304 passed / 41 skipped).
- Rebase integrity: `git diff f764bc00..6f511cae -- web/dist` empty (generated assets
  restored to base; verifier owns the rebuild); `git diff 50180a5d..cc161e22` on the
  task-195 Rust surface is empty — the merge brought in only the incoming
  task-187/main explorer surface, which the check and web suites above cover.
  `web/src/lib/api.js` is a verified union of both sides' functions.

The build wrapper blocks gyre-server test execution in this sandbox (exit 64,
recorded); the verifier must run `cargo build --all`,
`cargo test -p gyre-server --lib gate_executor`,
`cargo test -p gyre-server --test task134_review_probe`,
`cargo test -p gyre-server --lib api::spawn`,
`cargo test -p gyre-server --lib api::merge_requests`, and `cargo test --all`.
Compilation of all of that test code is proven by `cargo check --tests` above; the
assertions were reviewed for real discriminators (each persona-gate failure mode,
counterfactual tests that fail if manifest-gate sourcing is removed).

Findings: none blocking.

Non-blocking observations:

- `web/dist` intentionally matches the assignment base in this checkpoint; the
  verifier rebuilds and binds publication to the tested tree.
- Server test *execution* deferred to the verifier per the sandbox memory budget
  (infrastructure restriction, not a code defect; deferral recorded).

— Reviewer, task-195 rebase round (candidate 6f511cae)
