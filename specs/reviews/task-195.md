# Review — task-195: Manifest-driven gate selection for MRs referencing a spec

**Candidate:** `e80fcfd62205de9668208e04a978b98f976b46ea`
**Base:** `1e61f6637fba1a1fe8b851eaac13691d1230f665`
**Verdict:** NOT APPROVED (2 blocking findings)
**Date:** 2026-10-30 (review round on the merged tree including repair `d11ffee85`)

## Scope reviewed

Diff base→candidate touches `gate_executor.rs` (manifest gate selection,
persona-constrained review path, gate agent spawn/teardown), `api/spawn.rs`
(`launch_agent_workload`/`dispatch_agent_launch` extraction + persona field),
`mcp.rs`, `ralph_loop.rs`, `signal_chain.rs`, docs, and the
fabricated-scope-defaults exemption file (legitimate entry removal — the
flagged GateFailure ws_id fallback is genuinely fixed at gate_executor.rs:748).
`web/dist` untouched (no generated-output concern).

## Probes run (evidence: /tmp/stage/review-evidence/)

- `bash scripts/check-arch.sh` — PASS
- `bash scripts/check-fabricated-scope-defaults.sh` — PASS (exemption removal verified legitimate)
- `check-dead-message-kinds`, `check-inert-enforcement`, `check-scope-literal-defaults`, `check-mem-port-contracts` — PASS
- `/tmp/stage/bin/cargo check -p gyre-server --tests` — PASS (cold, 8m05s; lib + all test targets type-check)
- `rustfmt --check` on all changed `.rs` files — PASS
- Test execution deferred to the verifier (wrapper blocks gyre-server codegen in
  the review sandbox). Required: `cargo test -p gyre-server --lib gate_executor`,
  `--lib spawn`, `--lib ralph_loop`, the integration suites, `cargo build --all`.

## What is genuinely implemented (verified in source)

- **Selection:** `manifest_gates_for_mr` is a pure, path-normalizing resolver
  (strips `./`, `/`, `specs/`; handles bare no-`@sha` spec_refs); dedupes
  repeated personas within an entry; empty for no manifest / no spec_ref / no
  matching entry / no gates. Unit tests assert the resolved gate set directly.
- **Union + dedup:** repo-level gates keep running; on persona overlap the
  manifest gate wins (documented deviation from an earlier review round) so the
  attestation constraints survive and the persona still runs exactly once.
  Asserted by `trigger_gates_manifest_gate_wins_over_repo_gate_for_same_persona`.
- **Merge-blocking:** manifest gates reuse the `GateResult` flow; deterministic
  gate ids `manifest-gate:<repo>:<spec>:<persona>` are unknown to the
  quality-gates store so `check_gates_for_mr` defaults them to `required=true`
  (gate_executor.rs:2276-2282). Failure blocking is asserted.
- **Fail-closed persona path:** `run_persona_review_gate` never auto-passes; it
  polls for a qualifying review and fails at timeout naming the unmet
  constraints. Attestation-level/stack-hash checks and latest-verdict-wins
  review semantics are well tested (including stub-regression killers).
- **HEAD sourcing** is correctly justified: the `spec_ref` SHA is the spec
  blob SHA, not a treeish.
- **Gate agent spawn:** real Agent record, persona in `agent_personas`,
  token in `agent_tokens`, ReviewRequest dispatched, runtime launched through
  the shared compute-target dispatch (token via `GYRE_AUTH_TOKEN`, workload
  attestation recorded — asserted by an env-dump launch-contract test).

## Blocking findings

### F1 — Persona/attestation constraints enforced against a caller-controlled identity (missing enforcement)

The gate agent's token is minted with `AgentSigningKey::mint` (scope
`"agent"`, gate_executor.rs:458-461) rather than the existing
`mint_scoped(..., REVIEW_SUBMIT_SCOPE)` the command-based gate path uses.
`submit_review` (api/merge_requests.rs:1047-1061) binds `reviewer_agent_id` to
the verified token subject only for `review:submit`-scoped JWTs; for ordinary
agent tokens the review is recorded under the **request-body** reviewer id, and
the builtin Agent-role ABAC policy grants every spawned agent write access to
the reviews route. `evaluate_persona_reviews`/`check_reviewer_attestation`
then evaluate persona and `min_attestation_level`/`stack_hash` against that
unauthenticated identity.

Consequence: any authenticated agent can satisfy a manifest gate by posting a
review naming a qualifying agent id (e.g. the forge-spawned gate agent itself,
whose persona and workload attestation the server recorded, id discoverable via
the agents listing). This defeats the spec's stated purpose for `stack_hash`
("prevents a weakened persona from producing approvals that satisfy policies
written for the original persona") and does not mirror task-193's validation,
which derives the approver identity from verified JWT claims
(`approve_spec`, api/specs.rs:560-596). Fix direction: mint the gate agent
token with `REVIEW_SUBMIT_SCOPE` (extending the reviewer route allow-list so
the runtime can still poll its ReviewRequest) so the reviewer identity is
bound to the token subject.

### F2 — Teardown kills via LocalTarget, ignoring the backend that owns the handle

`teardown_gate_agent` (gate_executor.rs:526-560) calls
`LocalTarget::kill_process` on the registry handle directly instead of routing
through `state.spawned_backends` — the invariant the admin kill path
(api/admin.rs:296-315) implements and spawn.rs enforces with a dedicated test.
Container/SshDocker/K8s handles carry `pid: None`, so the kill is a silent
no-op there (orphaned container/Pod after the gate resolves); SshBare handles
carry a **remote** pid, so the local `kill -TERM` can hit an unrelated local
process. The `spawned_backends` entry is also leaked. The shipped
docs/agent-protocol.md claims the runtime process is killed on teardown —
untrue for remote backends; the teardown test only covers a LocalTarget
process.

## Notes for the verifier

- Run the deferred test probes listed above; the gate_executor manifest-gate
  tests are strong (selection, dedup, fail-closed, attestation enforcement,
  launch contract) and are expected to pass — the findings above are
  source-level enforcement gaps the tests do not cover (identity binding and
  remote-backend teardown), not expected test failures.
- The working-tree modification to `specs/tasks/task-195.md` (frontmatter reset
  to `not-started`) was present on entry to this review round (controller
  staging); this review did not modify it.
