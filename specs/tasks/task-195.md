---
title: "Manifest-driven gate selection for MRs referencing a spec"
spec_ref: "spec-registry.md §16 Agent Gates Integration"
depends_on: [task-193]
progress: ready-for-review
coverage_sections:
  - "spec-registry.md §16"
commits: ["a65ae04d808bdfb43bfc726a7e591fa7fad93589", "35b4dc9914d8ec050e2e8d6ede0e03e86206d6f9", "bb9dfd83ce2e7725ae4784ee0a0003d5e827c33d", "4494a058f44b4c14f26ce39839ff0b952d3939d6"]
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

All behavior lives in `crates/gyre-server/src/gate_executor.rs` (initial commit `bb9dfd83c`, repairs `35b4dc991`, `a65ae04d8`, `4494a058f`), reusing task-193's `read_manifest` and the existing `GateResult`/`check_gates_for_mr` flow — no parallel results table.

### What actually runs

- **Selection (pure):** `manifest_gates_for_mr(manifest, spec_ref)` resolves the manifest `SpecEntry` whose path matches the MR's `spec_ref` path (normalizing a leading `specs/` prefix and bare no-`@sha` paths, which `spawn.rs` records for ledger-missing specs) and returns its `gates:` entries as `ManifestGateSpec {persona, min_attestation_level (default 1), stack_hash, spec_path}`. Empty for no manifest / no spec_ref / no matching entry / no gates. Repeated personas within an entry dedupe (first wins).
- **Sourcing:** `resolve_manifest_gates_for_mr` reads `specs/manifest.yaml` for the MR's repo **at HEAD** via task-193's `read_manifest`. HEAD (not the spec_ref SHA) because the SHA embedded in `spec_ref` is the spec file's *blob* SHA (the ledger's `current_sha`, set by `sync_spec_ledger` via `git rev-parse <commit>:<path>`), which is not a treeish and cannot address the manifest — the same policy source task-193's `resolve_new_approval_status` uses. This deviation from the plan's preferred option is deliberate and documented in the code.
- **Union + dedup (manifest wins):** `trigger_gates_for_mr` runs every repo-level gate except those whose persona (name from the repo gate's `personas/<name>.md` path) is covered by a manifest gate, plus each manifest gate. On persona overlap the **manifest** gate wins (review R2): it carries the `min_attestation_level`/`stack_hash` constraints the manifest declares for the spec, which the repo-level gate cannot enforce — the old repo-gate-wins ordering let a no-command repo-level AgentReview gate auto-pass and merge without them. The manifest gate is not a downgrade of a working repo-level command gate: it spawns a real, executing gate agent runtime (see below). The persona still runs exactly once either way.
- **Forge-spawned gate agents that execute (spec §16; review rounds R3 + `94b30b1d`):** for each manifest gate, `trigger_gates_for_mr` spawns a real gate agent at enqueue time: an `Agent` record (`gate-<persona>-<mr-id>`, workspace-scoped, `spawned_by: "server"`, Active), the persona recorded in the `agent_personas` KV namespace, a scoped EdDSA JWT registered in `agent_tokens`, and a `ReviewRequest` message dispatched to the agent carrying `{mr_id, repo_id, spec_path, spec_ref, persona, min_attestation_level, stack_hash}`.

  The agent's **runtime process is then launched through the platform's compute-target machinery** — the same execution path every spawned agent uses. The launch block was extracted from `spawn_agent_core` into `api::spawn::launch_agent_workload` (compute-target dispatch container/ssh/local with the same resolution priority workspace assignment → tenant default → local; process-registry handle; exit monitor; workload attestation) so the task-driven spawn path and the gate-agent spawn path share one execution path — `spawn_agent_core` behavior is unchanged. `launch_gate_agent_runtime` (gate_executor) calls it with the gate agent's scoped token in `GYRE_AUTH_TOKEN` (the credential delivery channel every launched runtime uses) plus its review context (`GYRE_SERVER_URL`, `GYRE_AGENT_ID`, `GYRE_PERSONA`, `GYRE_MR_ID`, `GYRE_REPO_ID`, `GYRE_GATE_ID`, `GYRE_DIFF_URL`, `GYRE_SPEC_REF`, `GYRE_MIN_ATTESTATION_LEVEL`, `GYRE_STACK_HASH`) and the repo checkout as work dir. The launch records the gate agent's **workload attestation** (the `workload_attestations` namespace `check_reviewer_attestation` derives levels from), so its own reviews derive at level ≥ 1 and can satisfy the manifest-schema default `min_attestation_level` of 1 — higher levels follow the normal mechanisms (stack attestation → 2, container id + image → 3). The runtime command is server-controlled only (compute-target config → `GYRE_AGENT_COMMAND` → `/gyre/entrypoint.sh`), same as task-driven spawns. Best-effort, like every spawn: if no runtime command is available the gate still runs — it polls for a qualifying review and fails closed at its timeout; it never auto-passes.

  Spawn is idempotent while the gate runs (an Active agent with the deterministic name is reused on re-enqueue); a resolved (terminal) agent is replaced under a suffixed name. When the gate resolves — pass, rejection, or timeout — the agent is torn down: its runtime process is killed via its process-registry handle (the admin kill path's pattern), token revoked with an `AgentTokenRevoked` audit event (`agent_audit::revoke_agent_token`), and status `Stopped`, record kept for attribution (agent-gates.md §Gate Agent Lifecycle).
- **Production persona writer (review R3b):** `SpawnAgentRequest` gains an optional `persona` field (bare name, the manifest `gates:` form), also plumbed through the `gyre_spawn_worker` MCP tool; `spawn_agent_core` records it in `agent_personas` at spawn time and injects it as `GYRE_PERSONA` in the container env. The namespace `build_agent_context` reads — and the persona gate matches reviewers against — now has production writers (agent spawn + gate agent spawn), where previously only tests wrote it.
- **Materialization:** each manifest gate becomes an `AgentReview` `QualityGate` with a deterministic id `manifest-gate:<repo>:<spec_path>:<persona>`, deliberately NOT persisted to the quality-gates store — manifest gates are a per-MR selection; persisting them would make every other MR in the repo run them. GateResults for those ids flow through `check_gates_for_mr`, where unknown gate ids default to `required = true`, so manifest gate failures block the merge (mem + SQLite both handle the string id; `find_by_id` returns None). The gate is materialized before the agent spawn so the launched runtime receives its deterministic `GYRE_GATE_ID`.
- **Real persona path (not the stub):** `run_agent_review_gate` routes manifest-constrained gates to `run_persona_review_gate` when no command is configured (the legacy auto-pass stub only remains for repo-level gates without commands, unchanged). The gate passes **only** on an `Approved` review from an agent whose persona (KV `agent_personas`) matches the gate persona, whose workload attestation satisfies `min_attestation_level` (via the same `derive_attestation_level` task-193 uses, never invented) and, when pinned, an exact `stack_hash` fingerprint match — the §9 agent-approval validity rules applied to reviews. Verdict evaluation is **latest-per-reviewer, approval-wins** (review R1): each reviewer's chronologically latest review is the one that counts (the normal request-changes → fix → approve cycle is not vetoed by the reviewer's own stale ChangesRequested; conversely an approval withdrawn by a later ChangesRequested does not pass), and a qualifying approval wins over another persona reviewer's final ChangesRequested, matching `check_review_verdict`/`is_approved`. With no qualifying approval, a persona agent's final `ChangesRequested` fails fast; otherwise the gate waits (polling every 2s) up to the gate timeout, then FAILS closed with a message naming the missing constraints. Pass/fail participates in merge-blocking through the ordinary GateResult status.

### Test evidence

46 gate_executor tests pass (touched crate lib: 1506 passed, 0 failed; api_integration 66, e2e_ralph_loop 21, git_integration 1, auth_integration 16 — all pass); `cargo check -p gyre-server`, `scripts/check-arch.sh`, and all mechanical checks pass; clippy warning count identical to the assignment base (482, same set); rustfmt clean. Log: `/tmp/stage/review-evidence/task-195-repair.txt`.

- Pure-helper tests: `manifest_gates_resolved_from_spec_ref` (both personas + defaults + carried constraints — fails if manifest sourcing is removed), `manifest_gates_match_bare_path_spec_ref_without_sha`, `manifest_gates_empty_for_missing_inputs`, `manifest_gates_dedupe_repeated_persona`.
- Persona-path tests: passes with a qualifying persona review (level-3 workload, pinned stack hash); **fails with no review** (kills regression to the auto-pass stub); fails on attestation below minimum; fails on stack-hash mismatch; ignores reviews from other personas; fails fast on a final ChangesRequested; **later approval supersedes the same reviewer's stale ChangesRequested** (mutation-checked: fails against first-review-wins ordering); **ChangesRequested after approval fails** (latest verdict wins both directions).
- Gate agent tests: `trigger_gates_spawns_gate_agents_for_manifest_gates` (real Agent per persona, Active, workspace-scoped, persona in `agent_personas`, scoped token in `agent_tokens`, `ReviewRequest` dispatched with mr_id/persona/min_attestation_level); `trigger_gates_launches_gate_agent_runtime_with_token` (**the launch contract**: a stand-in runtime command launched via the compute-target machinery dumps its environment — asserted to contain `GYRE_AUTH_TOKEN` equal to the registered scoped token plus `GYRE_PERSONA`/`GYRE_MR_ID`/`GYRE_GATE_ID`/`GYRE_MIN_ATTESTATION_LEVEL`; the workload attestation is recorded with the launched PID and derives at level ≥ 1. Mutation-checked: fails when the `launch_gate_agent_runtime` call is removed); `trigger_gates_reuses_running_gate_agent_on_reenqueue` (re-enqueue does not double-spawn); `gate_agent_runtime_process_killed_on_teardown` (a real long-running process registered as the agent's runtime handle is killed on teardown, handle removed, record kept Stopped); `gate_agent_torn_down_after_gate_resolves` (token revoked, status Stopped, record kept).
- Trigger tests (real temp git repos with a manifest at HEAD): spec_ref MR gets GateResult rows for both manifest personas and `check_gates_for_mr` holds the MR (`Ok(false)`) while they run; a failed manifest-gate result blocks the merge (`Err` naming the manifest gate); repo-level gate + manifest gate on the same persona run exactly once **with the manifest gate winning**; MR without spec_ref and repo without manifest run exactly the repo-level gates.

### Repair rounds

- **Verification findings (commit `35b4dc991`):** rustfmt on the task-195 hunks; the GateFailure emission's fabricated `Id::new("default")` workspace fallback fixed at the site (skip + log, the `emit_reconciliation_completed` pattern) and the pinned exemption removed (`FROZEN_EXEMPTION_COUNT` 3→2).
- **Review findings, attempt `ba000fc94a9943d2b056349f53759d91` (commit `a65ae04d8`):** R1 verdict ordering (latest-per-reviewer, approval-wins); R2 manifest-wins dedup; R3 production reachability — gate agent records with persona/token/ReviewRequest, the `agent_personas` production writer on the agent-spawn path (`SpawnAgentRequest.persona`, `gyre_spawn_worker` param).
- **Review findings, attempt `94b30b1d2e1d4e228ca5e4c72ca36603` (commit `4494a058f`):** the gate agent now **executes** — its runtime process is launched through the platform's compute-target machinery (`api::spawn::launch_agent_workload`, extracted from `spawn_agent_core` so both spawn paths share one execution path), the scoped token is delivered to the launched runtime via `GYRE_AUTH_TOKEN`, and a **workload attestation is recorded at launch** so the agent's own reviews derive at level ≥ 1 (the prior spawn was internally inconsistent: level 0 could never satisfy even the default `min_attestation_level` of 1). Teardown kills the runtime process via its process-registry handle. The manifest-wins dedup is retained: with the pass path now real, it no longer trades a working repo-level command gate for a deterministic blocker — the manifest gate's runtime actually reviews, under the manifest's attestation constraints.
