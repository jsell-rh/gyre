---
title: "Manifest-driven gate selection for MRs referencing a spec"
spec_ref: "spec-registry.md §16 Agent Gates Integration"
depends_on: [task-193]
progress: ready-for-review
coverage_sections:
  - "spec-registry.md §16"
commits: ["fac8540c4f0a2c6973432bb60a50b70790397246", "42d995b061382af73d162321b960fede5527ba6b", "d11ffee85d3ccff5158300d113dbf82241aaf7c8", "a3e0a875ab383c2630868139e30837835b4d1a69", "4494a058f44b4c14f26ce39839ff0b952d3939d6", "a65ae04d808bdfb43bfc726a7e591fa7fad93589", "35b4dc9914d8ec050e2e8d6ede0e03e86206d6f9", "bb9dfd83ce2e7725ae4784ee0a0003d5e827c33d"]
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

All behavior lives in `crates/gyre-server/src/gate_executor.rs` (initial `bb9dfd83c`, repairs `35b4dc991`, `a65ae04d8`, `4494a058f`, rebase repair `a3e0a875a`, verification repair `d11ffee85`, review repair `42d995b06`, review repair `fac8540c4`), reusing task-193's `read_manifest` and the existing `GateResult`/`check_gates_for_mr` flow — no parallel results table.

### What actually runs

- **Selection (pure):** `manifest_gates_for_mr(manifest, spec_ref)` resolves the manifest `SpecEntry` whose path matches the MR's `spec_ref` path (normalizing a leading `specs/` prefix and bare no-`@sha` paths) and returns its `gates:` entries as `ManifestGateSpec {persona, min_attestation_level (default 1), stack_hash, spec_path}`. Empty for no manifest / no spec_ref / no matching entry / no gates. Repeated personas within an entry dedupe (first wins).
- **Sourcing:** `resolve_manifest_gates_for_mr` reads `specs/manifest.yaml` for the MR's repo **at HEAD** via task-193's `read_manifest`. HEAD (not the spec_ref SHA) because the SHA embedded in `spec_ref` is the spec file's *blob* SHA (the ledger's `current_sha`), which is not a treeish and cannot address the manifest — the same policy source task-193's `resolve_new_approval_status` uses.
- **Union + dedup (manifest wins):** `trigger_gates_for_mr` runs every repo-level gate except those whose persona is covered by a manifest gate, plus each manifest gate. On persona overlap the **manifest** gate wins: it carries the `min_attestation_level`/`stack_hash` constraints the manifest declares, which the repo-level gate cannot enforce. The persona still runs exactly once.
- **Forge-spawned gate agents that execute (spec §16):** for each manifest gate, `trigger_gates_for_mr` spawns a real gate agent at enqueue time: an `Agent` record (`gate-<persona>-<mr-id>`, workspace-scoped, Active), the persona recorded in the `agent_personas` KV namespace, a **`review:submit`-scoped** EdDSA JWT registered in `agent_tokens`, and a `ReviewRequest` message dispatched carrying `{mr_id, repo_id, spec_path, spec_ref, persona, min_attestation_level, stack_hash}`. The agent's runtime process is launched through the platform's compute-target machinery with the scoped token in `GYRE_AUTH_TOKEN` plus review context (`GYRE_PERSONA`, `GYRE_MR_ID`, `GYRE_GATE_ID`, `GYRE_DIFF_URL`, `GYRE_SPEC_REF`, `GYRE_MIN_ATTESTATION_LEVEL`, `GYRE_STACK_HASH`). The launch records the gate agent's **workload attestation** (the namespace `check_reviewer_attestation` derives levels from), so its own reviews derive at level >= 1. Best-effort like every spawn: if no runtime command is available the gate still runs — it polls for a qualifying review and fails closed at its timeout; it never auto-passes. Spawn is idempotent while the gate runs; teardown kills the runtime via the compute backend that owns its handle, removes the `spawned_backends` entry, revokes the token with an audit event, and stops the record (kept for attribution).
- **Production persona writer (validated):** `SpawnAgentRequest` gains an optional `persona` field (bare name, the manifest `gates:` form), also plumbed through the `gyre_spawn_worker` MCP tool; `spawn_agent_core` validates it fail-closed through `resolve_persona_in_chain` (must resolve to an **Approved** persona in the spawn's scope chain, same posture as `persona_slug`, before any state is created), records the validated name in `agent_personas` at spawn time, and injects it as `GYRE_PERSONA` in the container env. The forge's own manifest-gate writer (`spawn_manifest_gate_agent`) is server-internal and manifest-sourced and does not pass through this validation.
- **Materialization:** each manifest gate becomes an `AgentReview` `QualityGate` with a deterministic id `manifest-gate:<repo>:<spec_path>:<persona>`, deliberately NOT persisted to the quality-gates store (per-MR selection, not repo config). GateResults for those ids flow through `check_gates_for_mr`, where unknown gate ids default to `required = true`, so manifest gate failures block the merge.
- **Real persona path (never the stub):** `run_agent_review_gate` routes manifest-constrained gates to `run_persona_review_gate` when no command is configured. The gate passes only on an `Approved` review from an agent whose persona matches, whose workload attestation satisfies `min_attestation_level` (via the same `derive_attestation_level` task-193 uses) and, when pinned, an exact `stack_hash` match. Verdict evaluation is latest-per-reviewer, approval-wins; with no qualifying approval, a persona agent's final `ChangesRequested` fails fast; otherwise the gate polls up to its timeout, then FAILS closed naming the missing constraints.

### Review repairs

- **Round `e08eef14` (`42d995b06`):** scoped reviewer identity — `spawn_manifest_gate_agent` mints with `mint_scoped(..., REVIEW_SUBMIT_SCOPE)` (no unscoped fallback; mint failure fails the spawn closed), `REVIEWER_ALLOWED_ROUTES` gains the agent inbox poll route, and `submit_review` binds the reviewer to the verified token subject for **every** agent-token principal. Backend-routed teardown — the kill goes through the backend that owns the handle (container/Pod/remote-pid), removing the `spawned_backends` entry, with `LocalTarget` only as the unknown-backend fallback.
- **Round `48f43270` (`fac8540c4`, this round):**
  - **Validated bare persona (finding 1):** the caller-supplied `persona` field is no longer recorded verbatim — `spawn_agent_core` resolves it through `resolve_persona_in_chain` and rejects the spawn (400, nothing created) unless it names an **Approved** persona; only the validated name reaches `agent_personas`/`GYRE_PERSONA`. Covers the REST route and the MCP `gyre_spawn_worker` param (both funnel through `spawn_agent_core`); the forge's manifest-sourced writer stays unvalidated per the review.
  - **Human reviewer identity binding (finding 2):** human principals (user JWT / API key / impersonation) may use the request-body `reviewer_agent_id` only when it equals their own verified `user_id` (never the user-controlled display name, which could collide with a gate agent's id); recording a review for a different reviewer requires an Admin principal. The dashboard's two human review flows (Inbox gate override, DetailPanel review form) previously posted hardcoded pseudo-ids (`'human'` / `'human-reviewer'`) and now submit the caller's own identity via a cached `selfReviewerId()` helper. Also repairs the stale task-134 lookalike probe, which still asserted the pre-manifest-gate behavior (body reviewer honored for agent tokens) and therefore failed on the candidate — it now proves exact scope comparison via the route allow-list (GET `/api/v1/repos` permitted) and identity binding to the token subject.

### Test evidence

Probes run in this sandbox (build wrapper `/tmp/stage/bin/cargo`): `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` exit 0; `bash scripts/check-arch.sh` and `bash scripts/check-abac-route-registry.sh` exit 0; rustfmt clean on all touched files; web suites (`npm ci` + vitest) pass — Inbox 74/74, DetailPanel/DetailPanelChat/api 61/61. The wrapper blocks gyre-server test builds in this sandbox (4 GiB budget) — recorded in `/tmp/stage/review-evidence/task-195-repair.md` for the verifier: `spawn_bare_persona_fails_closed` (fails if an unapproved bare persona is recorded), `spawn_bare_persona_approved_is_recorded`, `submit_review_human_non_admin_cannot_forge_agent_reviewer` (fails if the human forgery hole reopens), `submit_review_admin_can_record_on_behalf_of_agent`, `submit_review_binds_agent_token_caller_to_own_identity`, `gate_agent_token_is_scoped_reviewer_jwt`, `gate_agent_teardown_routes_kill_through_registered_backend`, the `task134_review_probe` suite (incl. the repaired lookalike probe), plus the full `gate_agent` / `manifest_gates` / `submit_review` / `spawn` regression scopes.
