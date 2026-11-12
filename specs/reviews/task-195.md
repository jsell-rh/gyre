# Review — task-195 (Manifest-driven gate selection for MRs referencing a spec)

Spec: `specs/system/spec-registry.md` §16 Agent Gates Integration + §Agent
Approver / Gate Schema. Task: `specs/tasks/task-195.md`.

Candidate: `d736d5af2dce92e3ea0bfc9cfc653cc413e288ca` (base
`e402e1ed620c0407906528548621734d776d43a6`; cumulative diff
`git diff e402e1ed d736d5af`, +3307/−434 across gate_executor.rs,
api/spawn.rs, api/merge_requests.rs, abac_middleware.rs, mcp.rs, docs, web
dashboard sources, coverage matrix). This is the fourth review round of this
branch; prior rounds' findings (unscoped gate-agent token, LocalTarget-only
teardown, unvalidated bare `persona` spawn field, human-principal reviewer
forgery) are all repaired in this candidate — verification below.

Verdict: **approved** (see /tmp/stage/verdict.json). Evidence:
`/tmp/stage/review-evidence/task-195-review.md`.

## Acceptance criteria vs implementation

- **Per-spec gate selection is real.** `manifest_gates_for_mr`
  (gate_executor.rs:101) is a pure `(Option<&SpecManifest>, spec_ref) ->
  Vec<ManifestGateSpec>` helper: splits `path@sha` (rsplit on `@`), strips
  `./`, `/`, `specs/` prefixes for manifest matching, matches the manifest
  entry by normalized path, maps each `GateConfig` to
  `{persona, min_attestation_level (default 1), stack_hash, spec_path}`,
  dedupes repeated personas in one entry (first declaration wins). Unit
  tests assert the resolved gate set directly
  (`manifest_gates_resolved_from_spec_ref`,
  `manifest_gates_match_bare_path_spec_ref_without_sha`,
  `manifest_gates_empty_for_missing_inputs`,
  `manifest_gates_dedupe_repeated_persona`).
- **Union, not replace; no double-run.** `trigger_gates_for_mr` keeps
  repo-level `quality_gates` and adds manifest gates; when a repo-level
  gate and a manifest gate share a persona the manifest gate wins (it
  carries the attestation constraints) and the repo-level duplicate is
  skipped. Covered by
  `trigger_gates_manifest_gate_wins_over_repo_gate_for_same_persona`
  (asserts exactly one security-persona result, and that it is the
  manifest gate).
- **No regression.** No spec_ref / no manifest / no gates → repo-level
  gates only (`trigger_gates_no_spec_ref_runs_repo_gates_only`,
  `trigger_gates_repo_without_manifest_runs_repo_gates_only`).
- **Merge-blocking via the existing flow.** Manifest gates materialize as
  `QualityGate`s with deterministic ids
  (`manifest-gate:<repo>:<spec_path>:<persona>`, `required: true`) and
  pending `GateResult` rows in the one results table; `check_gates_for_mr`
  sees them (unknown gate ids default to `required = true`); a failed
  manifest gate returns `Err` and blocks the merge
  (`trigger_gates_failed_manifest_gate_blocks_merge`). No parallel
  results path.
- **Real persona path, not the auto-pass stub.** Manifest AgentReview
  gates run `run_persona_review_gate`: poll `evaluate_persona_reviews`
  (persona from the `agent_personas` KV, latest-verdict-per-reviewer,
  approval-wins across reviewers, final ChangesRequested from the persona
  fails) + `check_reviewer_attestation`
  (`derive_attestation_level(workload_attestations) >= min`, exact
  `stack_hash` when pinned). No qualifying review within the gate timeout
  → **Failed** (fail closed), never auto-pass.
- **Forge spawns the gate agents.** `spawn_manifest_gate_agent` creates a
  real workspace-scoped Agent (`gate-<persona>-<mr-id>`, spawned_by
  "server"), records the persona in `agent_personas`, mints a scoped
  `review:submit` EdDSA JWT via `mint_scoped` (fail-closed on mint or
  registration failure), dispatches a `ReviewRequest` message carrying
  the constraints, and launches the runtime through
  `api::spawn::launch_agent_workload` (compute-target machinery, token in
  `GYRE_AUTH_TOKEN`, workload attestation recorded at launch so the
  agent's own reviews derive at level ≥ 1). Teardown on resolution routes
  the kill through the backend that owns the handle (`spawned_backends`),
  removes the map entry, revokes the token with audit, and sets Stopped.
  Re-enqueue reuses the still-Active gate agent (no double-spawn).

## Prior-round findings — repaired and verified in this candidate

- **R1-F1 unscoped gate-agent token:** now `mint_scoped(...,
  REVIEW_SUBMIT_SCOPE)`; an unscoped fallback is explicitly refused
  (agent Stopped, gate polls and fails at timeout).
- **R1-F2 LocalTarget-only teardown:** kill now goes through
  `ComputeTarget::kill_process(recorded_backend, &handle)` with
  LocalTarget only as the no-backend fallback; `spawned_backends` entry
  removed; test
  `gate_agent_teardown_routes_kill_through_registered_backend`.
- **R2-F1 unvalidated caller-supplied `persona`:**
  `spawn_agent_core` validates the bare name fail-closed through
  `resolve_persona_in_chain` (Approved persona required in the
  repo/workspace/tenant chain — the same posture as `persona_slug`)
  BEFORE any state is created; only the validated value reaches
  `agent_personas` / `GYRE_PERSONA`; MCP `gyre_spawn_worker`'s `persona`
  param flows through the same validated core. Tests:
  `spawn_bare_persona_fails_closed` (unknown + Pending personas 400,
  namespace stays empty, no orphaned agent rows),
  `spawn_bare_persona_approved_is_recorded`.
- **R2-F2 human-principal reviewer forgery:** `submit_review` binds human
  callers (user JWT / API key / impersonation) to their own verified
  `auth.user_id` — deliberately NOT `auth.agent_id`, which API-key/JWT
  auth fills from the user-controlled display name (auth.rs:1175); a body
  reviewer naming a different agent requires an Admin principal (who can
  merge directly anyway); the server's system token keeps the body
  reviewer for internal flows. Tests:
  `submit_review_human_non_admin_cannot_forge_agent_reviewer` (403 + no
  persisted review; own-identity submission 201),
  `submit_review_admin_can_record_on_behalf_of_agent`. Dashboard flows
  now submit the caller's own identity (`selfReviewerId()` in api.js,
  used by Inbox.svelte and DetailPanel.svelte). The stale task-134
  lookalike probe was rewritten to assert the new binding (lookalike
  token's review stored under its own verified subject) plus exact-scope
  route-allow-list discrimination via GET /api/v1/repos.

The base merge (6cd6b2f94, task-121 workspace membership) does not touch
gate_executor.rs / spawn.rs / merge_requests.rs relative to the previously
reviewed tree — no fix was dropped in conflict resolution.

## Probes (this review; logs in /tmp/stage/review-evidence)

- `/tmp/stage/bin/cargo check -p gyre-server --tests`: exit 0 (cold run
  10m11s; "Checking gyre-server v0.1.0" confirmed via a
  touch-and-recheck probe — 1m55s, exit 0, lib.rs content untouched).
- `bash scripts/check-arch.sh`, `check-abac-route-registry.sh`,
  `check-abac-exempt-handlers.sh`, `check-mcp-write-tools.sh`,
  `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`,
  `check-inert-enforcement.sh`, `check-forged-scope-fields.sh`,
  `check-in-memory-state-stores.sh`, `check-unbounded-external-http.sh`,
  `check-byte-slice-truncation.sh`, `check-lossy-secret-conversion.sh`,
  `check-dead-message-kinds.sh`, `check-relative-path-defaults.sh`,
  `check-migration-sql-portability.sh`, `check-migration-versions.sh`,
  `check-mem-port-contracts.sh`, `check-fail-open-ref-resolution.sh`,
  `check-forwarded-header-trust.sh`: all exit 0. Note the
  fabricated-scope-defaults exemption file SHRANK (2→1): the
  gate_executor gate-event ws_id fallback was actually fixed in this
  branch (unresolvable MR → skip + log, matching the
  emit_reconciliation_completed pattern) — a removal, not growth.
- `cd web && npx vitest run` on the touched suites (Inbox, DetailPanel,
  api): 126 tests pass.
- Server test binaries are verifier-owned under the build wrapper's
  4 GiB budget. Required verifier commands recorded in
  `/tmp/stage/review-evidence/task-195-review.md`:
  `cargo test -p gyre-server --lib`,
  `cargo test -p gyre-server --test task134_review_probe`,
  `cargo build --all`, `cd web && npm test`.

## Non-blocking observations

- `resolve_manifest_gates_for_mr` reads `specs/manifest.yaml` at repo HEAD
  rather than the spec_ref SHA. The in-code justification is sound (the
  spec_ref SHA is a blob SHA, not a treeish, so it cannot address the
  manifest; task-193's approval resolver uses HEAD as the policy source
  for the same reason), and the task plan explicitly allowed determining
  HEAD as the policy source.
- Residual platform trust model, unchanged by this task and identical to
  the posture the prior round prescribed and to task-193's agent-approval
  validation: a spawn-capable caller can spawn a worker bound to an
  *Approved* persona, and that worker's review can satisfy a manifest
  gate whose min_attestation_level is at or below its derived attestation
  level. The persona's human approval is the trust gate; gates demanding
  level 3 or a pinned stack_hash additionally require container/image or
  exact-fingerprint attestations. Recorded as an observation for any
  future hardening task, not a defect of this one.
- Generated `web/dist/` is verifier-owned; the authored frontend sources
  changed by this task (reviewer identity submission) were reviewed and
  tested here.

## Conclusion

Every acceptance criterion is met by production code on the real
gate-execution path (GateResult/check_gates_for_mr flow, real persona
evaluation against agent_personas + workload_attestations, real
forge-spawned gate agents with scoped tokens and compute-target launches),
the tests fail if the behavior is removed, all prior review findings are
repaired with covering tests, and every mechanical invariant check passes.
Approved.
