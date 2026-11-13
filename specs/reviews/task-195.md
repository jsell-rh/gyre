---
task: task-195
candidate: 11e69bc6abfcba3f9b4b994de920d70dd10bdb2d
base: 4dc0eb9bbd0a2cddad39f478b7bc70167fa455ed
verdict: approved
date: 2026-10-05
reviewer: independent review (merge round)
---

# Review — task-195 (Manifest-driven gate selection for MRs referencing a spec)

Spec: `specs/system/spec-registry.md` §16 Agent Gates Integration + §Agent
Approver / Gate Schema (`persona`, `min_attestation_level` default 1,
optional `stack_hash`).

Round context: rebase-repair round. The previous published head `218d2660`
was DIRTY against main `4dc0eb9b` (task-139 forge enforcement policies). The
candidate merges main into the retained implementation; the candidate differs
from merge head `157e4057` only in `specs/tasks/task-195.md` process
bookkeeping — the reviewed code is identical to what the implementation agent
probed.

Verdict: **approved** (no blocking findings). Full probe evidence:
`/tmp/stage/review-evidence/task-195-review-round.md`.

## What was verified

**Selection (spec §16, "manifest feeds the gate chain").**
`manifest_gates_for_mr(manifest, spec_ref)` (gate_executor.rs:101) is the
required pure helper: normalizes `specs/`-prefixed and bare paths, splits
`path@sha` (rsplit on `@`, tolerating bare no-SHA refs spawn.rs records),
matches the manifest entry by path, and returns `ManifestGateSpec`s carrying
`persona`, `min_attestation_level` (default 1), `stack_hash`, `spec_path`;
intra-entry repeated personas dedupe (first wins). Unit tests assert directly
on the resolved gate set (both personas, defaults, constraints, empty
results for no-manifest/no-spec_ref/unknown-path/no-gates, dedupe).

**Manifest source.** `resolve_manifest_gates_for_mr` reuses task-193's
`crate::spec_registry::read_manifest` at HEAD — no second reader. The
HEAD-not-spec-SHA determination is sound and verified: the SHA in
`spec_ref` is the ledger's blob SHA (`git rev-parse <commit>:<path>`,
spec_registry.rs:1100), not a treeish, and task-193's approval resolver
reads policy from HEAD the same way. The task plan explicitly permits
matching task-193's policy source.

**Union + dedup.** `trigger_gates_for_mr` (gate_executor.rs:621) runs every
repo-level gate except those whose persona (via `persona_name_from_path`)
is covered by a manifest gate, plus each manifest gate — the persona runs
exactly once and the manifest gate wins on overlap (it carries the
attestation constraints; the repo-level duplicate would enforce none).
No-spec_ref and no-manifest paths run exactly the repo-level gates
(`trigger_gates_no_spec_ref_runs_repo_gates_only`,
`trigger_gates_repo_without_manifest_runs_repo_gates_only`).

**Real persona path, never the stub.** Manifest gates materialize as
`AgentReview` QualityGates with `command: None` + `GateConstraints`;
`run_agent_review_gate` routes them to `run_persona_review_gate`, which
polls `evaluate_persona_reviews` (latest-verdict-per-reviewer,
approval-wins) and passes only on an `Approved` review from a
persona-matching agent whose real workload attestation
(`workload_attestations` KV → `derive_attestation_level`) satisfies
`min_attestation_level` and, when pinned, an exact `stack_hash` match;
otherwise it fails closed at the gate timeout naming the missing
constraints. `persona_gate_fails_without_qualifying_review` kills any
regression to an auto-pass stub. Repo-level no-command AgentReview gates
keep the base's fail-closed behavior (verified against base source).

**Merge-blocking.** Manifest gates are plain `GateResult` rows — no parallel
results table; unknown gate ids default `required = true` in
`check_gates_for_mr`, and merge_processor consults that check. Covered by
`trigger_gates_runs_manifest_gates_for_spec_ref_mr` (holds the MR while
running) and `trigger_gates_failed_manifest_gate_blocks_merge`.

**Forge-spawned gate agents (spec §16 literal requirement).**
`spawn_manifest_gate_agent` creates a real Agent record, writes the persona
to `agent_personas` (the namespace the gate reads), mints a
`review:submit`-scoped EdDSA JWT (fails closed on mint/registration
failure — no unscoped fallback), dispatches a ReviewRequest, and launches
the runtime through `api::spawn::launch_agent_workload` — the platform's
single compute-target dispatch, legitimately extracted from
`spawn_agent_core` (refactor verified content-preserving) — with
`GYRE_AUTH_TOKEN` and the review-context env, recording a workload
attestation so the agent's own reviews can satisfy level ≥ 1. Teardown
kills the runtime through the owning backend (`spawned_backends` routing —
container/SshDocker/Kubernetes handles never go to a local pid kill),
removes both registry entries, revokes the token with an audit event, and
Stops the record (kept for attribution). Re-enqueue reuses a still-Active
gate agent instead of double-spawning.

**Reviewer identity binding (supporting repair).** `submit_review` binds
agent-token principals to their verified token subject (scope-independent),
and human principals to their own verified `user_id` (Admin may record on
behalf; the comparison never uses the user-controlled display name). This
closes the forgery hole against the persona/attestation constraints; tests
cover the agent-forge, human-forge (403 + nothing persisted), and
admin-on-behalf cases. The web review flows were updated to resolve the
caller's own id (`selfReviewerId`) instead of the legacy pseudo-ids.

**Merge-conflict resolution (this round's focus).** The single source
conflict, `abac_middleware.rs`, was resolved as a true union:
`AGENT_MESSAGES_ROUTE` and `MR_SPEC_ROUTE` are each defined exactly once,
both registered in the resolver registry, both on the reviewer allow-list;
the base→candidate diff on the file is purely additive. task-139's
additions are intact on the merged tree (MR pinned-spec endpoint, test
`agent_review_context_resolves_blob_pinned_spec_ref`,
`get_mr_spec_serves_pinned_sha_not_head`). The rest of the base→candidate
diff contains no deletions of main-side behavior (spawn.rs deletions are
the `dispatch_agent_launch` extraction). The inbox-poll route enforces
per-handler authorization (own-inbox only for non-admin callers).

**Probes run in this sandbox** (evidence logs under
`/tmp/stage/review-evidence/`): `cargo check -p gyre-server --tests`
exit 0; `cargo check -p gyre-server` exit 0; `cargo check -p gyre-cli
--tests` exit 0; `cargo test -p gyre-domain -p gyre-ports` 499+8 passed;
`cargo test -p gyre-adapters --lib` 491 passed; `check-arch.sh` exit 0;
the full `scripts/check-*.sh` sweep run at BOTH the candidate and the base
(worktree at 4dc0eb9b) — every failing checker fails identically at base
(diffs are line-number shifts of pre-existing findings only; the awk-based
checkers hit a mawk-vs-gawk incompatibility in this sandbox, identical at
base); `check-task-commit-attribution.sh` exit 0 (all 11 product commits
recorded in the task frontmatter); `npm ci` + focused vitest
(Inbox/DetailPanel) 106 tests passed.

**Deferred to the verifier** (the sandbox wrapper blocks gyre-server test
builds at the 4 GiB budget — exit 64, recorded): `cargo test -p gyre-server
--lib gate_executor`, `cargo test -p gyre-server --test
task134_review_probe`, the `submit_review`/`spawn` test scopes,
`cargo test --all`, `cargo build --all`. Test-code compilation is proven by
`cargo check -p gyre-server --tests` exit 0; the test bodies were reviewed
statically and are substantive (they assert on resolved gate sets,
GateResult rows, spawned agent records, token claims, and kill routing —
they fail if manifest-gate sourcing, the persona path, the reviewer
binding, or the backend-routed teardown is removed).

## Non-blocking observations (recorded for completeness)

- `resolve_manifest_gates_for_mr` returns empty (repo gates only) on a
  transient MR/repo store error — the same fail-open posture as the
  pre-existing `unwrap_or_default` repo-gate listing in the same function;
  the merge processor's independent ledger-approval check still blocks the
  merge without spec approval. Hardening, not a defect.
- On persona overlap a repo-level command-driven gate is skipped in favor
  of the manifest persona gate. This is the task's dedup requirement
  implemented with the documented manifest-wins rationale, covered by a
  dedicated test; conscious decision, not an oversight.
- A server restart while a manifest gate is Pending/Running orphans the
  result row (the MR stays unmergeable until re-enqueue) — identical
  pre-existing semantics for repo-level gates; not a new failure class.
- Generated `web/dist/` state is the deterministic verifier's domain; the
  committed bundle was left at main's assets per the merge-round handoff.
