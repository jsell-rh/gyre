---
title: "Manifest-driven gate selection for MRs referencing a spec"
spec_ref: "spec-registry.md §16 Agent Gates Integration"
depends_on: [task-193]
progress: complete
coverage_sections:
  - "spec-registry.md §16"
commits: ["9068ca910fc7c5202502a1fdae4a52045ac00361", "a57e032679e848ccc93aa95077d5d961c7fd9958", "055a41aef2b67d752795487e944d5a71bd1a2c27", "53b89baa96361836507800e9871b496a11037fd8", "a689c9f05ce4529e1d5691b32dc1b1e2ea58534d", "3c2c5462c1bb3cea490cc1033b0bc00d9b319a67", "58f28dfdd06aa5bc6372c737db94297b0d1fed51", "159b7408a12cef7990486878ffa33bf07c880c61", "fac8540c419d5a08a99c9601234ef1154274cad8", "42d995b061382af73d162321b960fede5527ba6b", "d11ffee85d3ccff5158300d113dbf82241aaf7c8", "a3e0a875ab383c2630868139e30837835b4d1a69", "4494a058f44b4c14f26ce39839ff0b952d3939d6", "a65ae04d808bdfb43bfc726a7e591fa7fad93589", "35b4dc9914d8ec050e2e8d6ede0e03e86206d6f9", "bb9dfd83ce2e7725ae4784ee0a0003d5e827c33d"]
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

All behavior lives in `crates/gyre-server/src/gate_executor.rs` (initial `bb9dfd83c`, review/verification repairs through `a57e0326`), with supporting changes in `api/spawn.rs`, `api/reviews.rs` / `api/merge_requests.rs`, `spec_registry.rs` (task-193's `read_manifest`, reused — no second reader), and the web review flows. It reuses the existing `GateResult`/`check_gates_for_mr` flow — no parallel results table.

### What actually runs

- **Selection (pure):** `manifest_gates_for_mr(manifest, spec_ref)` resolves the manifest `SpecEntry` whose path matches the MR's `spec_ref` path (normalizing a leading `specs/` prefix and bare no-`@sha` paths) and returns its `gates:` entries as `ManifestGateSpec {persona, min_attestation_level (default 1), stack_hash, spec_path}`. Empty for no manifest / no spec_ref / no matching entry / no gates; repeated personas within an entry dedupe (first wins). Unit tests assert on the resolved gate set directly.
- **Sourcing:** `resolve_manifest_gates_for_mr` reads `specs/manifest.yaml` for the MR's repo at HEAD via task-193's `read_manifest` — HEAD because the SHA in `spec_ref` is the spec ledger's blob SHA, not a treeish, matching task-193's policy source.
- **Union + dedup (manifest wins):** `trigger_gates_for_mr` runs every repo-level gate except those whose persona is covered by a manifest gate, plus each manifest gate. On persona overlap the manifest gate wins because the manifest is the reviewed per-spec policy artifact carrying the `min_attestation_level`/`stack_hash` constraints; the repo-gate alternative either fails closed (a no-command AgentReview gate cannot spawn a reviewer and blocks the merge) or runs a command-based reviewer with no attestation constraints. The persona runs exactly once.
- **Real persona path (never the stub):** each manifest gate becomes an `AgentReview` `QualityGate` (`manifest-gate:<repo>:<spec_path>:<persona>`, not persisted to the repo gate store). `run_agent_review_gate` routes it to `run_persona_review_gate`, which passes only on an `Approved` review from a persona-matching agent whose workload attestation satisfies `min_attestation_level` and, when pinned, an exact `stack_hash` match; otherwise it fails closed naming the missing constraints. Unknown gate ids default to `required = true` in `check_gates_for_mr`, so failures block the merge.
- **Forge-spawned gate agents (spec §16):** for each manifest gate, `spawn_manifest_gate_agent` creates the `Agent` record, persona KV entry, a `review:submit`-scoped EdDSA token, and dispatches a `ReviewRequest`; the runtime launches through the compute-target machinery with the scoped token and review context env (`GYRE_PERSONA`, `GYRE_MR_ID`, `GYRE_SPEC_REF`, `GYRE_MIN_ATTESTATION_LEVEL`, `GYRE_STACK_HASH`, …), recording a workload attestation. The ReviewRequest reaches the runtime via the launch env and `/mcp gyre_message_poll` (mounted outside the ABAC-layered api router) — not via a REST inbox route (that route is denied to review-scoped tokens as least privilege). Teardown kills the runtime via the owning backend and revokes the token.
- **Reviewer identity:** `submit_review` binds the reviewer to the verified token subject for agent principals; human principals may use a body `reviewer_agent_id` only when it equals their own verified `user_id` (Admin may record on behalf).

### This round: rebase repair against current main

The published PR (head `b9cb5a5f`) conflicted with current main. Resolved by merge `74514d8e` of assignment base `349a32ac` (task-037) into this branch: the incoming `web/src` view-query changes merged cleanly, and the conflicting generated `web/dist` assets were restored exactly to the assignment base (`git diff 349a32ac -- web/dist` is empty); final integration rebuilds and publishes those assets remotely. No Rust source changed in this round — `git diff b9cb5a5f -- crates/` against the merge result is empty, so the independently approved implementation at `a57e0326` is carried forward unchanged.

### Test evidence

Probes run in this sandbox (build wrapper `/tmp/stage/bin/cargo`) at merge result `74514d8e`, recorded in `/tmp/stage/review-evidence/rebase-349a32ac-round.md`: `cargo check -p gyre-server --tests` exit 0, `check-arch.sh` exit 0, `check-abac-route-registry.sh` exit 0, `web: npm ci` + `npm test` exit 0 (112 files, 2298 passed / 41 skipped — covers both the merged task-037 view-query validator and this task's web review flows). The wrapper blocks gyre-server test builds in this sandbox (4 GiB budget) — the verifier must execute: `cargo test -p gyre-server --test task134_review_probe` (including `scoped_review_token_cannot_read_agent_inbox_route` — still 403), `cargo test -p gyre-server --lib gate_executor` (manifest_gates selection tests, `trigger_gates_runs_manifest_gates_for_spec_ref_mr`, `trigger_gates_failed_manifest_gate_blocks_merge`, `trigger_gates_manifest_gate_wins_over_repo_gate_for_same_persona`, `trigger_gates_spawns_gate_agents_for_manifest_gates`, `trigger_gates_launches_gate_agent_runtime_with_token`, `gate_agent_token_is_scoped_reviewer_jwt`, teardown and `check_gates_*` scopes), the `submit_review`/`spawn` scopes, `cargo build --all`, and `cargo test --all`.
