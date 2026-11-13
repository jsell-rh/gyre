---
title: "Manifest-driven gate selection for MRs referencing a spec"
spec_ref: "spec-registry.md §16 Agent Gates Integration"
depends_on: [task-193]
progress: complete
coverage_sections:
  - "spec-registry.md §16"
commits: ["58f28dfdd06aa5bc6372c737db94297b0d1fed51", "159b7408a12cef7990486878ffa33bf07c880c61", "fac8540c419d5a08a99c9601234ef1154274cad8", "42d995b061382af73d162321b960fede5527ba6b", "d11ffee85d3ccff5158300d113dbf82241aaf7c8", "a3e0a875ab383c2630868139e30837835b4d1a69", "4494a058f44b4c14f26ce39839ff0b952d3939d6", "a65ae04d808bdfb43bfc726a7e591fa7fad93589", "35b4dc9914d8ec050e2e8d6ede0e03e86206d6f9", "bb9dfd83ce2e7725ae4784ee0a0003d5e827c33d", "3c2c5462c1bb3cea490cc1033b0bc00d9b319a67"]
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

All behavior lives in `crates/gyre-server/src/gate_executor.rs` (initial `bb9dfd83c`, review/verification/rebase/merge repairs through `159b7408a` and merge rounds `18d286423`/`e3db445a`, verification repair `58f28dfd`), with supporting changes in `api/spawn.rs` (validated bare-persona spawn), `api/reviews.rs` (reviewer identity binding), `spec_registry.rs` (manifest parsing from task-193), and the web review flows. It reuses task-193's `read_manifest` and the existing `GateResult`/`check_gates_for_mr` flow — no parallel results table.

### What actually runs

- **Selection (pure):** `manifest_gates_for_mr(manifest, spec_ref)` resolves the manifest `SpecEntry` whose path matches the MR's `spec_ref` path (normalizing a leading `specs/` prefix and bare no-`@sha` paths) and returns its `gates:` entries as `ManifestGateSpec {persona, min_attestation_level (default 1), stack_hash, spec_path}`. Empty for no manifest / no spec_ref / no matching entry / no gates. Repeated personas within an entry dedupe (first wins).
- **Sourcing:** `resolve_manifest_gates_for_mr` reads `specs/manifest.yaml` for the MR's repo **at HEAD** via task-193's `read_manifest`. HEAD (not the spec_ref SHA) because the SHA embedded in `spec_ref` is the spec file's *blob* SHA (the ledger's `current_sha`), which is not a treeish and cannot address the manifest — the same policy source task-193's `resolve_new_approval_status` uses.
- **Union + dedup (manifest wins):** `trigger_gates_for_mr` runs every repo-level gate except those whose persona is covered by a manifest gate, plus each manifest gate. On persona overlap the **manifest** gate wins: it carries the `min_attestation_level`/`stack_hash` constraints the manifest declares, which the repo-level gate cannot enforce. The persona still runs exactly once.
- **Forge-spawned gate agents that execute (spec §16):** for each manifest gate, `trigger_gates_for_mr` spawns a real gate agent at enqueue time: an `Agent` record (`gate-<persona>-<mr-id>`, workspace-scoped, Active), the persona recorded in the `agent_personas` KV namespace, a **`review:submit`-scoped** EdDSA JWT registered in `agent_tokens`, and a `ReviewRequest` message dispatched carrying `{mr_id, repo_id, spec_path, spec_ref, persona, min_attestation_level, stack_hash}`. The agent's runtime process is launched through the platform's compute-target machinery with the scoped token in `GYRE_AUTH_TOKEN` plus review context (`GYRE_PERSONA`, `GYRE_MR_ID`, `GYRE_GATE_ID`, `GYRE_DIFF_URL`, `GYRE_SPEC_REF`, `GYRE_MIN_ATTESTATION_LEVEL`, `GYRE_STACK_HASH`). The launch records the gate agent's **workload attestation** (the namespace `check_reviewer_attestation` derives levels from), so its own reviews derive at level >= 1. Best-effort like every spawn: if no runtime command is available the gate still runs — it polls for a qualifying review and fails closed at its timeout; it never auto-passes. Spawn is idempotent while the gate runs; teardown kills the runtime via the compute backend that owns its handle, removes the `spawned_backends` entry, revokes the token with an audit event, and stops the record (kept for attribution).
- **Production persona writer (validated):** `SpawnAgentRequest` gains an optional `persona` field (bare name, the manifest `gates:` form), also plumbed through the `gyre_spawn_worker` MCP tool; `spawn_agent_core` validates it fail-closed through `resolve_persona_in_chain` (must resolve to an **Approved** persona in the spawn's scope chain, same posture as `persona_slug`, before any state is created), records the validated name in `agent_personas` at spawn time, and injects it as `GYRE_PERSONA` in the container env. The forge's own manifest-gate writer (`spawn_manifest_gate_agent`) is server-internal and manifest-sourced and does not pass through this validation.
- **Materialization:** each manifest gate becomes an `AgentReview` `QualityGate` with a deterministic id `manifest-gate:<repo>:<spec_path>:<persona>`, deliberately NOT persisted to the quality-gates store (per-MR selection, not repo config). GateResults for those ids flow through `check_gates_for_mr`, where unknown gate ids default to `required = true`, so manifest gate failures block the merge.
- **Real persona path (never the stub):** `run_agent_review_gate` routes manifest-constrained gates to `run_persona_review_gate` when no command is configured. The gate passes only on an `Approved` review from an agent whose persona matches, whose workload attestation satisfies `min_attestation_level` (via the same `derive_attestation_level` task-193 uses) and, when pinned, an exact `stack_hash` match. Verdict evaluation is latest-per-reviewer, approval-wins; with no qualifying approval, a persona agent's final `ChangesRequested` fails fast; otherwise the gate polls up to its timeout, then FAILS closed naming the missing constraints.
- **Reviewer identity (review repairs `42d995b06`/`fac8540c4`):** the gate agent's token is `review:submit`-scoped (no unscoped fallback; mint failure fails the spawn closed) and `submit_review` binds the reviewer to the verified token subject for every agent-token principal; human principals may use a body `reviewer_agent_id` only when it equals their own verified `user_id`, with Admin principals allowed to record on behalf. The dashboard's human review flows submit the caller's own identity (`selfReviewerId()`).

### This round's repair (`58f28dfd`)

Verification `860d84daca` (`cargo test --all`) failed on `gate_executor::tests::trigger_gates_spawns_gate_agents_for_manifest_gates` ("ReviewRequest must be dispatched to the security gate agent"). Root cause was a **fixture gap, not a production defect**: `setup_manifest_mr` never created the `ws-manifest-gates` Workspace record, and `emit_event` resolves a server-originated message's tenant from the workspace record, skipping persistence when it is missing (the anti-forged-scope invariant — a fabricated tenant would re-target delivery). The gate agent's ReviewRequest was therefore never stored, so `list_unacked` found nothing. In production every MR's workspace exists (repos are created inside workspaces; workspace deletion is reference-guarded). The fixture now creates the workspace exactly as the `dep_staleness`/`signal_chain` fixtures do; no production code changed, and the launch path resolves the same local compute-target fallback (the fixture workspace declares no compute target).

### Test evidence

Probes run in this sandbox (build wrapper `/tmp/stage/bin/cargo`) at head `58f28dfd` on branch `pipeline/task-195/3ca175d0de704dbc9f203ca243b5f9de-1`: `cargo check -p gyre-server --tests` exit 0; `cargo fmt -p gyre-server -- --check` exit 0; `bash scripts/check-arch.sh`, `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`, and `check-in-memory-state-stores.sh` exit 0; `bash scripts/check-task-commit-attribution.sh` exit 0 with this round's frontmatter. The wrapper blocks gyre-server test builds in this sandbox (4 GiB budget) — recorded in `/tmp/stage/review-evidence/` for the verifier, which must execute: `cargo test -p gyre-server --lib gate_executor` (covers the `manifest_gates` selection tests, the `trigger_gates_*` integration tests including the repaired `trigger_gates_spawns_gate_agents_for_manifest_gates`, `gate_agent_token_is_scoped_reviewer_jwt`, `gate_agent_teardown_routes_kill_through_registered_backend`, `trigger_gates_launches_gate_agent_runtime_with_token`, and the merge-blocking scopes), plus the `submit_review` / `spawn` / `task134_review_probe` scopes in the same crate, and the full `cargo test --all` suite that failed this round.
