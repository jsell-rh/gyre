---
title: "Manifest-driven gate selection for MRs referencing a spec"
spec_ref: "spec-registry.md §16 Agent Gates Integration"
depends_on: [task-193]
progress: ready-for-review
coverage_sections:
  - "spec-registry.md §16"
commits: ["bb9dfd83ce2e7725ae4784ee0a0003d5e827c33d"]
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

All behavior lives in `crates/gyre-server/src/gate_executor.rs` (commit `bb9dfd83c`), reusing task-193's `read_manifest` and the existing `GateResult`/`check_gates_for_mr` flow — no parallel results table.

### What actually runs

- **Selection (pure):** `manifest_gates_for_mr(manifest, spec_ref)` resolves the manifest `SpecEntry` whose path matches the MR's `spec_ref` path (normalizing a leading `specs/` prefix and bare no-`@sha` paths, which `spawn.rs` records for ledger-missing specs) and returns its `gates:` entries as `ManifestGateSpec {persona, min_attestation_level (default 1), stack_hash, spec_path}`. Empty for no manifest / no spec_ref / no matching entry / no gates. Repeated personas within an entry dedupe (first wins).
- **Sourcing:** `resolve_manifest_gates_for_mr` reads `specs/manifest.yaml` for the MR's repo **at HEAD** via task-193's `read_manifest`. HEAD (not the spec_ref SHA) because the SHA embedded in `spec_ref` is the spec file's *blob* SHA (the ledger's `current_sha`, set by `sync_spec_ledger` via `git rev-parse <commit>:<path>`), which is not a treeish and cannot address the manifest — the same policy source task-193's `resolve_new_approval_status` uses. This deviation from the plan's preferred option is deliberate and documented in the code.
- **Union + dedup:** `trigger_gates_for_mr` still runs every repo-level gate exactly as before, plus each manifest gate — except a manifest gate whose persona is already covered by a repo-level gate (persona name from the repo gate's `personas/<name>.md` path), which is skipped so the persona never double-runs. The repo-level gate wins because the task plan says repo-level gates keep running "as today".
- **Materialization:** each manifest gate becomes an `AgentReview` `QualityGate` with a deterministic id `manifest-gate:<repo>:<spec_path>:<persona>`, deliberately NOT persisted to the quality-gates store — manifest gates are a per-MR selection; persisting them would make every other MR in the repo run them. GateResults for those ids flow through `check_gates_for_mr`, where unknown gate ids default to `required = true`, so manifest gate failures block the merge (mem + SQLite both handle the string id; `find_by_id` returns None).
- **Real persona path (not the stub):** `run_agent_review_gate` routes manifest-constrained gates to a new `run_persona_review_gate` when no command is configured (the legacy auto-pass stub only remains for repo-level gates without commands, unchanged). The gate passes **only** when the MR has an `Approved` review from an agent whose persona (KV `agent_personas`, the same source `build_agent_context` reads) matches the gate persona, whose workload attestation satisfies `min_attestation_level` (via the same `derive_attestation_level` task-193 uses, never invented) and, when pinned, an exact `stack_hash` fingerprint match — the §9 agent-approval validity rules applied to reviews. A persona agent's `ChangesRequested` fails fast; otherwise the gate waits (polling every 2s) up to the gate timeout for a qualifying review, then FAILS with a message naming the missing constraints. Pass/fail participates in merge-blocking through the ordinary GateResult status.

### Test evidence

39 gate_executor tests pass (touched crate lib: 1387 passed, 0 failed); `cargo build --all` and `scripts/check-arch.sh` pass (log: `/tmp/stage/review-evidence/task-195-probes.txt` in the assignment sandbox).

- Pure-helper tests: `manifest_gates_resolved_from_spec_ref` (both personas + defaults + carried constraints — fails if manifest sourcing is removed), `manifest_gates_match_bare_path_spec_ref_without_sha`, `manifest_gates_empty_for_missing_inputs`, `manifest_gates_dedupe_repeated_persona`.
- Persona-path tests: passes with a qualifying persona review (level-3 workload, pinned stack hash); **fails with no review** (kills regression to the auto-pass stub); fails on attestation below minimum; fails on stack-hash mismatch; ignores reviews from other personas; fails fast on ChangesRequested.
- Trigger tests (real temp git repos with a manifest at HEAD): spec_ref MR gets GateResult rows for both manifest personas and `check_gates_for_mr` holds the MR (`Ok(false)`) while they run; a failed manifest-gate result blocks the merge (`Err` naming the manifest gate); repo-level gate + manifest gate on the same persona run exactly once (repo gate wins); MR without spec_ref and repo without manifest run exactly the repo-level gates.
