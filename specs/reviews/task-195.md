# Review — task-195 (Manifest-driven gate selection for MRs referencing a spec)

Spec: `specs/system/spec-registry.md` §16 Agent Gates Integration — the manifest `gates:` field feeds the gate chain: an MR referencing a spec gets the gate agents its manifest entry declares, in addition to repo-level gates, deduplicated per persona, with `min_attestation_level` / `stack_hash` enforced.
Comparison base: `cd58a1662c0e17850ffb345bcec98a55453e901c` → candidate `3657cd00e4bd57d33dbcae6a3d8d87f7afeaeebc`.
This is review round 4 on this candidate chain. Delta since the last-reviewed candidate (`28ddc3ae`): repair commit `a57e0326` (comment-only), merge `b6e0c940` (assignment base `cd58a166` / task-202), process commit `3657cd00` (task file bookkeeping only).

## Verdict

**Approved.** All three prior blocking findings are repaired and verified; the base merge is resolved correctly; the runtime behavior — already verified across rounds 1–3 — is unchanged by this round's delta except for the corrected comments. Evidence: `/tmp/stage/review-evidence/review-3657cd00.md`.

## Prior findings — repair verification

- **Round 3 (false fail-closed counterfactual):** `a57e0326` rewrites both the `trigger_gates_for_mr` dedup comment and the `trigger_gates_manifest_gate_wins_over_repo_gate_for_same_persona` test comment. Both now state the true rationale: manifest-wins because the manifest is the reviewed per-spec policy artifact carrying the attestation constraints; the repo-gate-wins alternative fails closed (a no-command AgentReview gate cannot spawn a reviewer and blocks the merge) or runs an unconstrained reviewer. I independently confirmed the factual basis: `run_agent_review_gate`'s no-command arm returns `GateStatus::Failed` in this tree. Comment-only; no behavior change.
- **Round 2 (stale allow-list comment, false handler-identity rationale):** verified fixed in the current tree — `REVIEWER_ALLOWED_ROUTES` doc comment (abac_middleware.rs:66-76) states the true delivery channels (launch env + `/mcp gyre_message_poll`) and the least-privilege rationale for keeping the inbox route off the allow-list; `gate_executor.rs:471-474`, `docs/agent-protocol.md:157`, and `task134_review_probe.rs:151-154` correctly state the routed handler (`messages::poll_messages`) enforces own-inbox. No remaining "no caller-identity check / drain-and-clear" claims.
- **Round 1 (inbox route widening):** `AGENTS_MESSAGES_ROUTE` is absent from `REVIEWER_ALLOWED_ROUTES`; the regression test `scoped_review_token_cannot_read_agent_inbox_route` remains.

## Base merge `b6e0c940` (task-202)

Only conflict was `api/spawn.rs`, resolved correctly: task-202's search indexing and `AuditCtx.search` field were ported into task-195's `dispatch_agent_launch` refactor (also `monitor_spawned_agent_exit`, `complete_agent`). In `merge_requests.rs`, task-195's inline `create_mr` search indexing was correctly superseded by task-202's centralized `search_indexer::mr_document`; indexing added on `transition_mr_status` and `submit_review`; task-195's reviewer identity binding is untouched. `gate_executor.rs`, `task134_review_probe.rs`, `abac_middleware.rs`, `docs/agent-protocol.md`, and `web/` are byte-identical to `28ddc3ae` except the `a57e0326` comment repair. Workload-attestation recording in `spawn.rs` is intact. The `fabricated-scope-defaults-exemptions.txt` change is comment-only history consolidation; the live entry count is unchanged (1 pre-existing entry) and `check-fabricated-scope-defaults.sh` passes.

## Behavior (re-confirmed at source level; unchanged since rounds 1–3)

- `manifest_gates_for_mr` (pure selection over `(Option<&SpecManifest>, spec_ref)`) with direct unit tests: matching entry, `specs/`-prefix and bare-path normalization, empty for no-manifest / no-spec_ref / unknown path / no-gates entry, per-entry persona dedup (first wins).
- `trigger_gates_runs_manifest_gates_for_spec_ref_mr` asserts two manifest-gate `GateResult` rows (security + accountability) plus `check_gates_for_mr == Ok(false)` while pending — this test fails if manifest-gate sourcing is removed.
- `trigger_gates_failed_manifest_gate_blocks_merge`: a failed manifest gate blocks the merge through the same `check_gates_for_mr` path the merge processor uses (unknown gate ids default to `required = true`).
- `trigger_gates_manifest_gate_wins_over_repo_gate_for_same_persona`: persona overlap deduplicates; the persona runs exactly once.
- `trigger_gates_no_spec_ref_runs_repo_gates_only` / `trigger_gates_repo_without_manifest_runs_repo_gates_only`: no regression, no spurious gates.
- Real persona path, never the auto-pass stub: `run_persona_review_gate` → `evaluate_persona_reviews` → `check_reviewer_attestation` enforces persona match, `min_attestation_level`, and pinned `stack_hash`; timeout fails closed. `spawn_manifest_gate_agent` creates a real `Agent`, records the persona, mints a `review:submit`-scoped EdDSA token (mint failure fails closed), launches the runtime via the compute-target machinery with a workload attestation, and tears it down on resolution. `submit_review` binds reviewers to verified token identities for agent principals and to the caller's own `user_id` for humans.

## Probes run (this sandbox)

- `/tmp/stage/bin/cargo check -p gyre-server --tests` — exit 0 (8m14s).
- `bash scripts/check-arch.sh` — pass.
- `bash scripts/check-abac-route-registry.sh` — pass.
- `bash scripts/check-fabricated-scope-defaults.sh` — pass.

The build wrapper rejects gyre-server test builds in this sandbox (4 GiB budget) — not a code defect. The verifier must run: `cargo test -p gyre-server --lib gate_executor`, `cargo test -p gyre-server --test task134_review_probe`, `cargo test --all`, and `cd web && npm ci && npm test`.
