---
title: "Manifest-driven gate selection for MRs referencing a spec"
spec_ref: "spec-registry.md §16 Agent Gates Integration"
depends_on: [task-193]
progress: ready-for-review
coverage_sections:
  - "spec-registry.md §16"
commits: ["fbfba8ca5b7ba41726637e93a53ffbb98f78bb0b", "a3e0a875ab383c2630868139e30837835b4d1a69", "4494a058f44b4c14f26ce39839ff0b952d3939d6", "a65ae04d808bdfb43bfc726a7e591fa7fad93589", "35b4dc9914d8ec050e2e8d6ede0e03e86206d6f9", "bb9dfd83ce2e7725ae4784ee0a0003d5e827c33d"]
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

All behavior lives in `crates/gyre-server/src/gate_executor.rs` (initial commit `bb9dfd83c`, repairs `35b4dc991`, `a65ae04d8`, `4494a058f`, rebase repairs `0c137722c` and `fbfba8ca5`), reusing task-193's `read_manifest` and the existing `GateResult`/`check_gates_for_mr` flow — no parallel results table.

### What actually runs

- **Selection (pure):** `manifest_gates_for_mr(manifest, spec_ref)` resolves the manifest `SpecEntry` whose path matches the MR's `spec_ref` path (normalizing a leading `specs/` prefix and bare no-`@sha` paths) and returns its `gates:` entries as `ManifestGateSpec {persona, min_attestation_level (default 1), stack_hash, spec_path}`. Empty for no manifest / no spec_ref / no matching entry / no gates. Repeated personas within an entry dedupe (first wins).
- **Sourcing:** `resolve_manifest_gates_for_mr` reads `specs/manifest.yaml` for the MR's repo **at HEAD** via task-193's `read_manifest`. HEAD (not the spec_ref SHA) because the SHA embedded in `spec_ref` is the spec file's *blob* SHA (the ledger's `current_sha`), which is not a treeish and cannot address the manifest — the same policy source task-193's `resolve_new_approval_status` uses. This deviation from the plan's preferred option is deliberate and documented in the code.
- **Union + dedup (manifest wins):** `trigger_gates_for_mr` runs every repo-level gate except those whose persona is covered by a manifest gate, plus each manifest gate. On persona overlap the **manifest** gate wins (review R2): it carries the `min_attestation_level`/`stack_hash` constraints the manifest declares, which the repo-level gate cannot enforce. The persona still runs exactly once.
- **Forge-spawned gate agents that execute (spec §16):** for each manifest gate, `trigger_gates_for_mr` spawns a real gate agent at enqueue time: an `Agent` record (`gate-<persona>-<mr-id>`, workspace-scoped, Active), the persona recorded in the `agent_personas` KV namespace, a scoped EdDSA JWT registered in `agent_tokens`, and a `ReviewRequest` message dispatched carrying `{mr_id, repo_id, spec_path, spec_ref, persona, min_attestation_level, stack_hash}`. The agent's runtime process is launched through the platform's compute-target machinery (see the rebase-repair note below for the post-merge structure) with the scoped token in `GYRE_AUTH_TOKEN` plus review context (`GYRE_PERSONA`, `GYRE_MR_ID`, `GYRE_GATE_ID`, `GYRE_DIFF_URL`, `GYRE_SPEC_REF`, `GYRE_MIN_ATTESTATION_LEVEL`, `GYRE_STACK_HASH`). The launch records the gate agent's **workload attestation** (the namespace `check_reviewer_attestation` derives levels from), so its own reviews derive at level ≥ 1. Best-effort like every spawn: if no runtime command is available the gate still runs — it polls for a qualifying review and fails closed at its timeout; it never auto-passes. Spawn is idempotent while the gate runs; teardown kills the runtime process via its process-registry handle, revokes the token with an audit event, and stops the record (kept for attribution).
- **Production persona writer:** `SpawnAgentRequest` gains an optional `persona` field (bare name, the manifest `gates:` form), also plumbed through the `gyre_spawn_worker` MCP tool; `spawn_agent_core` records it in `agent_personas` at spawn time and injects it as `GYRE_PERSONA` in the container env.
- **Materialization:** each manifest gate becomes an `AgentReview` `QualityGate` with a deterministic id `manifest-gate:<repo>:<spec_path>:<persona>` (carrying upstream's `validation_type: None` after the rebase), deliberately NOT persisted to the quality-gates store (per-MR selection, not repo config). GateResults for those ids flow through `check_gates_for_mr`, where unknown gate ids default to `required = true`, so manifest gate failures block the merge.
- **Real persona path (never the stub):** `run_agent_review_gate` routes manifest-constrained gates to `run_persona_review_gate` when no command is configured. The gate passes only on an `Approved` review from an agent whose persona matches, whose workload attestation satisfies `min_attestation_level` (via the same `derive_attestation_level` task-193 uses) and, when pinned, an exact `stack_hash` match. Verdict evaluation is latest-per-reviewer, approval-wins; with no qualifying approval, a persona agent's final `ChangesRequested` fails fast; otherwise the gate polls up to its timeout, then FAILS closed naming the missing constraints. Since the rebase onto `3af80bd21`, gates with no command AND no constraints also fail closed (upstream task-134 semantics; the legacy auto-pass stub is gone entirely — the merge deleted the now-dead stub function rather than keep it behind the constrained path).

### Rebase repair (`fbfba8ca5`, onto assignment base `3af80bd21`)

The branch merged the assignment base, which carries task-134's gate-executor rewrite, task-193's spec-registry manifest reader, and the Ralph-loop launch refactor (`AgentLaunchCtx`/`launch_agent_process`, `SpawnAgentResponse.process_launched`, `create_mr_for_completion`, `ralph_loop.rs`). Conflicts resolved in `api/spawn.rs`, `gate_executor.rs`, and `scripts/fabricated-scope-defaults-exemptions.txt`:

- `gate_executor.rs`: both sides had diverged on `run_agent_review_gate`'s no-command behavior. Merged semantics: command → spawned reviewer process (upstream); no command + manifest constraints → `run_persona_review_gate` (task-195); no command + no constraints → fail closed (upstream task-134; supersedes the older auto-pass stub, whose function was removed as dead code). Added upstream's `QualityGate.validation_type` field to `materialize_manifest_gate` and the manifest-gate test initializers; upstream's no-command test call sites gained the `constraints: None` argument.
- `api/spawn.rs`: the two sides had extracted overlapping but incompatible launch helpers (upstream's ctx-based `launch_agent_process`, used by `spawn_agent_core` and `ralph_loop.rs`; task-195's env-passing `launch_agent_workload`, used by `gate_executor.rs`). Both are kept, now delegating to one shared four-backend dispatch `dispatch_agent_launch` (container / SshDocker / SshBare / kubernetes / local auto-detect + `register_spawned_agent`), which carries upstream's improvements (per-arm `launched` tracking, explicit-`GYRE_AGENT_COMMAND`-wins-over-container-auto-detect) and task-195's M19.5-A SSH agent-name guard (returns `Err`; `launch_agent_process` maps a rejection to not-launched since `spawn_agent_core` validates pre-write). `launch_agent_workload` keeps its contract for gate agents: caller-built env, workload attestation recorded, container id returned. `SpawnAgentRequest.persona` is carried through a new `AgentLaunchCtx.persona` field into `GYRE_PERSONA` (`ralph_loop.rs` re-spawns pass `None`; the recorded persona is unchanged by re-spawning).
- `signal_chain.rs`: worker-spawn `SpawnAgentRequest` initializer gained `persona: None`.
- Exemptions file: both sides fixed the same `gate_executor.rs` ws_id fallback (frozen count stays 1); this branch's history note kept.

### Test evidence

- Earlier rounds (full logs referenced in their commits): 46 gate_executor tests pass (touched crate lib: 1506 passed, 0 failed; api_integration 66, e2e_ralph_loop 21, git_integration 1, auth_integration 16 — all pass), mutation-checked selection/launch tests included.
- This round (rebase repair onto `3af80bd21`): server test builds exceed this sandbox's 4 GiB budget, so per the assignment contract the verifier must run them. Type checking covers the full merged surface: `SKIP_WEB_BUILD=1 cargo check -p gyre-server` and `cargo check -p gyre-server --tests` pass (lib + lib-test). `bash scripts/check-arch.sh` passes, and the mechanical checks touched by the merge pass: fabricated-scope-defaults (count 1), mem-port-contracts, inert-enforcement, byte-slice-truncation, relative-path-defaults, lossy-secret-conversion.
- **Verifier must execute:** `cargo test -p gyre-server --lib gate_executor` (46 manifest-gate tests, incl. `trigger_gates_launches_gate_agent_runtime_with_token`, which exercises the re-merged `launch_agent_workload`/`dispatch_agent_launch` path, and `agent_review_no_command_fails_closed`, which pins the merged no-command fail-closed semantics), `cargo test -p gyre-server --lib spawn` and `--lib ralph_loop` (upstream spawn/re-spawn tests re-compiled against the shared dispatch), plus the integration suites listed above and `cargo build --all`.
