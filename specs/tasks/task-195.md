---
title: "Manifest-driven gate selection for MRs referencing a spec"
spec_ref: "spec-registry.md §16 Agent Gates Integration"
depends_on: [task-193]
progress: ready-for-review
coverage_sections:
  - "spec-registry.md §16"
commits: ["42d995b061382af73d162321b960fede5527ba6b", "d11ffee85d3ccff5158300d113dbf82241aaf7c8", "a3e0a875ab383c2630868139e30837835b4d1a69", "4494a058f44b4c14f26ce39839ff0b952d3939d6", "a65ae04d808bdfb43bfc726a7e591fa7fad93589", "35b4dc9914d8ec050e2e8d6ede0e03e86206d6f9", "bb9dfd83ce2e7725ae4784ee0a0003d5e827c33d"]
---

## Spec Excerpt

From `specs/system/spec-registry.md` §16 (Integration → Agent Gates):

> The `gates:` field in the manifest feeds the gate chain:
> - MR references `system/identity-security.md` -> manifest says gates = [security, accountability]
> - Forge spawns security gate agent and accountability gate agent for this MR
> - Different specs can require different gate agents
>
> The spec approval ledger in agent-gates.md is now unified with the forge ledger described here. One table, one source of truth.

Manifest schema (§Agent Approver / Gate Schema): each gate entry carries `persona`, `min_attestation_level` (default 1), and optional `stack_hash`.

## Problem

Hollow. `trigger_gates_for_mr()` (`crates/gyre-server/src/gate_executor.rs:23-60`) sources gates **only** from repo-level configuration: `state.quality_gates.list_by_repo_id(repo_id)`. The manifest's per-spec `gates:` (`GateConfig` on `SpecEntry`, `crates/gyre-server/src/spec_registry.rs:107,181-186`) are parsed but **never consumed anywhere**. An MR carrying `spec_ref = "specs/system/identity-security.md@<sha>"` does not get the `[security, accountability]` gate agents its manifest entry declares — per-spec gate selection is unimplemented.

`trigger_gates_for_mr` is invoked from `crates/gyre-server/src/api/merge_queue.rs:77` with the MR's `repository_id`. The MR's `spec_ref` (`MergeRequest.spec_ref`, `crates/gyre-domain/src/merge_request.rs:77`) has the form `"path@sha"`.

## Implementation Plan

1. In `trigger_gates_for_mr` (or a helper it calls), load the MR (`state.merge_requests.find_by_id`) to read `spec_ref`. If `spec_ref` is `Some("path@sha")`, split off the path and SHA.

2. Read the manifest for the MR's repo at that spec SHA (or repo HEAD if you determine HEAD is the correct policy source — match the approach task-193 uses for reading policy; prefer the spec SHA embedded in `spec_ref` so gates reflect the exact reviewed version): `crate::spec_registry::read_manifest(repo_path, sha)` (helper from task-193). Resolve `repo_path` from `repo_id` via `state.repos`.

3. Find the manifest `SpecEntry` matching the spec_ref path (normalize `specs/`-prefix consistently). For each `GateConfig` in `entry.gates`, materialize a gate to run for this MR:
   - Map each manifest gate `{persona, min_attestation_level, stack_hash}` onto the existing gate-execution machinery. Reuse the `QualityGate` / `GateResult` flow already in `gate_executor.rs` (create a `Pending` `GateResult`, then run an agent-review/validation gate for the named persona). Do NOT invent a parallel results table — the merge-blocking check `check_gates_for_mr` (gate_executor.rs:837) must see these results.
   - The gate must actually enforce the persona: spawn/run the agent gate for that persona (reuse `run_agent_review_gate` / the persona-driven path), not the auto-pass stub, when a persona is configured. Carry `min_attestation_level` / `stack_hash` into the gate so the reviewing agent's attestation can be constrained (mirror how task-193 validates agent approvals; if the current gate machinery has no attestation constraint plumbing, wire the minimum needed to record and enforce it — do not silently drop the constraint).

4. **Union, don't replace:** keep running repo-level `quality_gates` gates as today, and additionally run the manifest per-spec gates. Deduplicate if a repo-level gate and a manifest gate target the same persona to avoid double-spawning.

5. If the MR has no `spec_ref`, or the repo has no manifest, or the entry declares no `gates`, behavior is unchanged (repo-level gates only).

## Acceptance Criteria

- An MR whose `spec_ref` points at a spec whose manifest entry declares `gates: [security, accountability]` results in gate executions for **both** personas (visible as `GateResult` rows for the MR), in addition to any repo-level gates.
- An MR with no `spec_ref` (or a repo with no manifest) runs exactly the repo-level gates it runs today — no regression, no spurious gates.
- Manifest gates and repo-level gates targeting the same persona do not double-run.
- A configured manifest gate with a persona runs the real persona-driven gate path (not the always-pass stub) and its pass/fail participates in `check_gates_for_mr` merge-blocking.
- A test (in `gate_executor.rs` tests) that FAILS if manifest-gate sourcing is removed: with a fixture manifest declaring per-spec gates and an MR referencing that spec, `trigger_gates_for_mr` (or the extracted selection helper) yields gate executions for the manifest personas. Prefer extracting the "which gates apply to this MR?" resolution into a pure helper `(Option<&SpecManifest>, mr) -> Vec<GateSpec>` and asserting on it directly.
- `cargo build --all`, touched-crate tests, and `bash scripts/check-arch.sh` pass.

## Agent Instructions

- Reuse `crate::spec_registry::read_manifest` from task-193; do not add another manifest reader.
- Reuse the existing `GateResult`/`check_gates_for_mr` flow so merge-blocking sees manifest gates — do NOT build a separate gate path that the merge processor can't observe.
- Do NOT satisfy the manifest gate with the auto-pass stub when a persona is configured; run the real persona gate.
- Extract gate selection into a pure, unit-testable helper; assert on the resolved gate set, not merely that "some gate ran".
- Run only touched-crate tests plus `scripts/check-arch.sh`; skip formatters and the full suite.

## Shipped

All behavior lives in `crates/gyre-server/src/gate_executor.rs` (initial commit `bb9dfd83c`, repairs `35b4dc991`, `a65ae04d8`, `4494a058f`, rebase repair `a3e0a875a`, verification repair `d11ffee85`, review repair `42d995b06`), reusing task-193's `read_manifest` and the existing `GateResult`/`check_gates_for_mr` flow — no parallel results table.

### What actually runs

- **Selection (pure):** `manifest_gates_for_mr(manifest, spec_ref)` resolves the manifest `SpecEntry` whose path matches the MR's `spec_ref` path (normalizing a leading `specs/` prefix and bare no-`@sha` paths) and returns its `gates:` entries as `ManifestGateSpec {persona, min_attestation_level (default 1), stack_hash, spec_path}`. Empty for no manifest / no spec_ref / no matching entry / no gates. Repeated personas within an entry dedupe (first wins).
- **Sourcing:** `resolve_manifest_gates_for_mr` reads `specs/manifest.yaml` for the MR's repo **at HEAD** via task-193's `read_manifest`. HEAD (not the spec_ref SHA) because the SHA embedded in `spec_ref` is the spec file's *blob* SHA (the ledger's `current_sha`), which is not a treeish and cannot address the manifest — the same policy source task-193's `resolve_new_approval_status` uses.
- **Union + dedup (manifest wins):** `trigger_gates_for_mr` runs every repo-level gate except those whose persona is covered by a manifest gate, plus each manifest gate. On persona overlap the **manifest** gate wins: it carries the `min_attestation_level`/`stack_hash` constraints the manifest declares, which the repo-level gate cannot enforce. The persona still runs exactly once.
- **Forge-spawned gate agents that execute (spec §16):** for each manifest gate, `trigger_gates_for_mr` spawns a real gate agent at enqueue time: an `Agent` record (`gate-<persona>-<mr-id>`, workspace-scoped, Active), the persona recorded in the `agent_personas` KV namespace, a **`review:submit`-scoped** EdDSA JWT (see repair below) registered in `agent_tokens`, and a `ReviewRequest` message dispatched carrying `{mr_id, repo_id, spec_path, spec_ref, persona, min_attestation_level, stack_hash}`. The agent's runtime process is launched through the platform's compute-target machinery (`launch_agent_workload` → shared `dispatch_agent_launch`) with the scoped token in `GYRE_AUTH_TOKEN` plus review context (`GYRE_PERSONA`, `GYRE_MR_ID`, `GYRE_GATE_ID`, `GYRE_DIFF_URL`, `GYRE_SPEC_REF`, `GYRE_MIN_ATTESTATION_LEVEL`, `GYRE_STACK_HASH`). The launch records the gate agent's **workload attestation** (the namespace `check_reviewer_attestation` derives levels from), so its own reviews derive at level ≥ 1. Best-effort like every spawn: if no runtime command is available the gate still runs — it polls for a qualifying review and fails closed at its timeout; it never auto-passes. Spawn is idempotent while the gate runs; teardown kills the runtime via the compute backend that owns its handle (below), removes the `spawned_backends` entry, revokes the token with an audit event, and stops the record (kept for attribution).
- **Production persona writer:** `SpawnAgentRequest` gains an optional `persona` field (bare name, the manifest `gates:` form), also plumbed through the `gyre_spawn_worker` MCP tool; `spawn_agent_core` records it in `agent_personas` at spawn time and injects it as `GYRE_PERSONA` in the container env.
- **Materialization:** each manifest gate becomes an `AgentReview` `QualityGate` with a deterministic id `manifest-gate:<repo>:<spec_path>:<persona>`, deliberately NOT persisted to the quality-gates store (per-MR selection, not repo config). GateResults for those ids flow through `check_gates_for_mr`, where unknown gate ids default to `required = true`, so manifest gate failures block the merge.
- **Real persona path (never the stub):** `run_agent_review_gate` routes manifest-constrained gates to `run_persona_review_gate` when no command is configured. The gate passes only on an `Approved` review from an agent whose persona matches, whose workload attestation satisfies `min_attestation_level` (via the same `derive_attestation_level` task-193 uses) and, when pinned, an exact `stack_hash` match. Verdict evaluation is latest-per-reviewer, approval-wins; with no qualifying approval, a persona agent's final `ChangesRequested` fails fast; otherwise the gate polls up to its timeout, then FAILS closed naming the missing constraints.

### Review repair (this round, `42d995b06`)

Repair of the two findings from review `e08eef14` against candidate `e80fcfd62`:

- **Scoped reviewer identity (finding 1):** `spawn_manifest_gate_agent` now mints the gate agent's token with `mint_scoped(..., REVIEW_SUBMIT_SCOPE)` exactly like the command-based gate path — no `AgentSigningKey::mint`, no opaque-UUID fallback (a mint or registration failure fails the spawn closed; the gate then polls and fails at its timeout). The ABAC `REVIEWER_ALLOWED_ROUTES` gains the agent inbox poll route (`/api/v1/agents/:id/messages`, via a shared `AGENT_MESSAGES_ROUTE` const so the pattern stays defined once) so the launched runtime can still receive its ReviewRequest over REST; the `/mcp` `gyre_message_poll` surface was already outside the allow-list. Additionally, `submit_review` now binds the reviewer to the verified token subject for **every** agent-token principal (`user_id` none, not system) — closing the stated reproduction where an unrelated agent B POSTed a review with a forged `reviewer_agent_id` of a persona-holding, attested agent A and unblocked the merge. Human callers (user JWT / API key / impersonation, e.g. the dashboard's `human-reviewer`) and the system token keep the request-body reviewer.
- **Backend-routed teardown (finding 2):** `teardown_gate_agent` no longer kills via `LocalTarget` directly. It removes the `process_registry` handle AND the `spawned_backends` entry (no map leak) and routes the kill through the backend that owns the handle — container ids to `docker rm --force`, Pods to `kubectl delete pod`, remote pids to the remote host — with `LocalTarget` only as the unknown-backend fallback, the same invariant the admin kill path implements and `spawn.rs` enforces with a dedicated test. Container/SshDocker/Kubernetes-launched gate agents are no longer orphaned, and SshBare remote pids can no longer SIGTERM an unrelated local process.

### Base merge (this round)

Merged assignment base `b4b6bae13` (task-150 MCP prompts + `persona_slug` spawn binding) into the branch (`ce0c4d3f5`). The conflicts were the two coexisting persona features: the candidate's bare-name `persona` request field (recorded in `agent_personas`, injected as `GYRE_PERSONA`) vs. the base's `persona_slug` binding (resolved persona row, `GYRE_PROMPT_PERSONA` prompt injection). Resolved as a union — both request fields kept, the `AgentLaunchCtx` bare-name field renamed `persona_name` to avoid the type clash with the base's `persona: Option<gyre_domain::Persona>`, and the re-spawn path passes both (`persona_name: None` + the resolved bound persona). No behavior of either feature was dropped.

### Test evidence

Probes run in this sandbox (build wrapper `/tmp/stage/bin/cargo`): `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` exit 0; `bash scripts/check-arch.sh` and `bash scripts/check-abac-route-registry.sh` exit 0; rustfmt clean on all touched files. The wrapper blocks gyre-server test builds in this sandbox (4 GiB budget) — recorded in `/tmp/stage/review-evidence/task-195-repair.md` for the verifier: `gate_agent_token_is_scoped_reviewer_jwt` (fails if the mint reverts to an unscoped token), `gate_agent_teardown_routes_kill_through_registered_backend` (fails if teardown kills via `LocalTarget` directly — a real local process registered with a Container backend must survive, and both registry entries must be removed), `submit_review_binds_agent_token_caller_to_own_identity` (fails if the forged-reviewer hole reopens), plus the full `gate_agent` / `manifest` / `submit_review` / `spawn` regression scopes.
