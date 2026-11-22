---
title: "Conformance sweep convergence: unbounded provenance comparison, per-spec drift detection and task creation, and rollout-policy-governed work on the drift path"
spec_ref: "Spec gap spec-556a0b49e9f2803bf0e2ff3d / conformance-sweep-convergence / b288723d2f73"
depends_on: [task-302, task-326, task-328]
progress: not-started
commits: []
---

## Required behavior

Per §10 and the §6 controller clause 'When there's drift, the controller creates reconciliation work according to the workspace's rollout policy.' The sweep job exists (meta_spec_conformance_sweep, daily, configurable via GYRE_META_SPEC_SWEEP_INTERVAL_SECS) but: (a) the provenance comparison is windowed to 30 days (SWEEP_PROVENANCE_WINDOW_SECS = 30*86400 at reconciliation.rs:119, feeding find_by_repo(repo, since, now) at :1729) so drift older than the window is permanently invisible, defeating the section's convergence purpose ('a workspace binding was updated while agents were offline'); (b) it compares per repo (any mismatched attestation flags the whole repo) and creates one workspace-level drift-review task rather than iterating per spec implemented in each repo; (c) the sweep-created task is hardcoded TaskPriority::Medium and created immediately — the sweep body (reconciliation.rs:1697-1860) never calls get_workspace_rollout_policy, so drift-caught work bypasses the §7 rollout policy that only the trigger path honors. Deliverables: (1) Unbounded comparison — remove the 30-day window: the sweep examines ALL provenance records per repo (every attestation regardless of age), still database queries only with no agent spawns. (2) Per-spec comparison — iterate per (repo, spec): group provenance by spec identity; a spec is drifted when its MOST RECENT provenance record's governing meta-spec versions differ from the current effective governing collection (desired state); replace the whole-repo flag and single workspace-level drift-review task with per-spec drift state. (3) Per-spec reconciliation work — for each drifted spec, ensure a reconciliation task exists (create if missing, deduped per repo+spec against open reconciliation tasks), carrying the per-spec task identity from reconciliation-candidate-scoping (spec_ref/meta_spec_ref/reconciliation_id). (4) Rollout-policy conformance — work created by the continuous comparison goes through the same policy dispatch as trigger-path waves: strategy handling (Immediate all-at-once, Rolling batches via the kv-backed pending state, Background left inert in Backlog without a spawning task_type), priority from the policy's ReconciliationPriority mapping (no hardcoded Medium), auto_merge handling identical — the sweep path reads get_workspace_rollout_policy. COORDINATION: active task-302 migrates the sweep's desired-vs-actual basis to the registry model (required versions / meta_specs_used tuples); the window, per-spec, and policy requirements are model-independent and must survive that migration. When a repo-level override governs a repo (repo-scope-set-cascade-and-injection), the desired state for that repo's specs is the repo-effective collection.

## Acceptance Criteria

- [ ] A repo whose newest attestation for spec S carries a superseded governing version with created_at 40 days in the past is flagged drifted by the sweep and gets a reconciliation task for S — the 30-day window no longer hides it.
- [ ] A repo with spec S1 drifted and spec S2 current yields drift state for S1 only and a task scoped to S1's spec_ref — not a whole-repo flag or a workspace-level review task.
- [ ] A workspace with rollout strategy Background: sweep-created drift work sits in Backlog without a spawning task_type and with the policy-mapped priority; with strategy Rolling and max_concurrent_agents 1, drift work for two specs is batched; the sweep path provably reads the workspace's rollout policy (priority comes from ReconciliationPriority, never a hardcoded constant).
- [ ] The sweep still spawns no agents and emits MetaSpecDriftDetected / records drift metrics for the per-spec findings.

## Verification intent

Three arms, all failing at the base. (1) Stale drift: seed a repo whose newest attestation for spec S records a superseded governing version with created_at 40 days ago (fixture clock); run the sweep and assert S is flagged and a task ensured — at the base the query is bounded (let since = now.saturating_sub(SWEEP_PROVENANCE_WINDOW_SECS) feeding find_by_repo(repo, since, now), reconciliation.rs:1729), so the record is invisible and no work is created. (2) Per-spec granularity: with S1 drifted and S2 current in one repo, assert drift state exists for S1 only — at the base any mismatched attestation flags the entire repo and one workspace-level drift-review task is created regardless of which spec drifted. (3) Policy: with the workspace's strategy set to Background, assert sweep-created work is inert in Backlog — at the base the sweep creates its task immediately with task.priority = TaskPriority::Medium hardcoded in the sweep body, which never calls get_workspace_rollout_policy, so policy-less work is the observable defect.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/meta-spec-reconciliation.md`. Source: `50575cf65e6491279049052901a65c68a4d2993d`; plan: `c2ceefa3b1c58d079ded6a7ee7cd9311539758965f2e1d0edd37b7ffdd4c9071`.

[
  {
    "id": "0f67c6dbc5bb328a54047724",
    "decision": "gap",
    "requirement_quote": "When there's drift, the controller creates reconciliation work according to the workspace's rollout policy.",
    "detail": "The controller's comparison half is real: run_conformance_sweep (§10's background job, daily by default) continuously compares each workspace's desired state (effective set SHA) against the meta_spec_set_sha recorded in provenance attestations, and on drift it ensures a drift-review task exists and notifies members. The gap is the rollout-policy clause: work created by the continuous comparison does not follow the workspace's rollout policy. The sweep's drift-review task is created directly with a hardcoded TaskPriority::Medium, created immediately regardless of strategy (no Immediate/Rolling/Background dispatch, no max_concurrent_agents batching, no auto_merge handling) — the inspected sweep body (reconciliation.rs 1697-1860) never calls get_workspace_rollout_policy. Only the set-update/approval-triggered waves (run_reconciliation) apply the policy. So a workspace whose drift is caught by the continuous controller rather than a binding change gets policy-less work: the specced 'creates reconciliation work according to the workspace's rollout policy' holds for the trigger path but not for the drift-detection path.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/reconciliation.rs",
        "quote": "/// Run one conformance sweep pass across all workspaces (§10).",
        "detail": "The continuous comparator: for each workspace it compares the effective set SHA (desired) against provenance-recorded SHAs (actual) and creates drift work — the controller's compare-and-create loop exists as a real background job."
      },
      {
        "file": "crates/gyre-server/src/reconciliation.rs",
        "quote": "task.priority = TaskPriority::Medium;",
        "detail": "The sweep-created drift-review task is hardcoded to Medium priority and created immediately in the sweep body (inspected 1697-1860): no get_workspace_rollout_policy call, no RolloutStrategy dispatch, no batching — drift-caught work bypasses the §7 rollout policy the controller is required to honor."
      }
    ]
  },
  {
    "id": "8dc04f90bec1c77e6b42f252",
    "decision": "gap",
    "requirement_quote": "A background job (like speculative merging) continuously checks for meta-spec drift across all repos:",
    "detail": "The sweep exists and does the core job: a registered, enabled background job (meta_spec_conformance_sweep, daily by default, interval configurable via GYRE_META_SPEC_SWEEP_INTERVAL_SECS) runs run_conformance_sweep across all workspaces, compares each workspace's effective set SHA against provenance-recorded set SHAs, creates a drift-review task if missing (deduped per workspace+set-sha), emits MetaSpecDriftDetected, records drift metrics, and notifies workspace members and meta-spec owners — database queries only, no agent spawns. Two concrete shortfalls against the specced sweep: (1) the comparison is windowed — only attestations within the last 30 days (SWEEP_PROVENANCE_WINDOW_SECS = 30*86400) are examined, so code whose provenance is older than 30 days is never compared and its drift is never detected; the section's stated purpose ('ensures the system converges even after disruptions', catching 'a workspace binding was updated while agents were offline') fails for drift older than the window; (2) the pseudocode iterates per spec implemented in each repo, while the implementation compares per repo (any attestation with a mismatched set SHA flags the whole repo) and creates one workspace-level drift-review task rather than per-spec reconciliation tasks. The sweep-created work also bypasses the rollout policy (assessed under §6 The Reconciliation Controller).",
    "evidence": [
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "quote": "name: \"meta_spec_conformance_sweep\".to_string(),",
        "detail": "The conformance sweep is a registered, enabled background job with configurable interval (GYRE_META_SPEC_SWEEP_INTERVAL_SECS, default SWEEP_INTERVAL_SECS = 86400) bound to the real sweep — the 'runs every N hours, configurable' requirement is met."
      },
      {
        "file": "crates/gyre-server/src/reconciliation.rs",
        "quote": "const SWEEP_PROVENANCE_WINDOW_SECS: u64 = 30 * 86_400;",
        "detail": "The sweep's provenance query is bounded to a 30-day window: chain_attestations.find_by_repo(repo, since, now) with since = now - 30 days. Provenance older than 30 days is never compared, so drift in code produced before the window is permanently invisible to the sweep — contradicting the section's convergence purpose for exactly the disruption cases it names."
      },
      {
        "file": "crates/gyre-server/src/reconciliation.rs",
        "quote": "let since = now.saturating_sub(SWEEP_PROVENANCE_WINDOW_SECS);",
        "detail": "The windowing call site inside run_conformance_sweep — the only provenance input to the desired-vs-actual comparison; no unbounded or per-spec provenance walk exists on the sweep path."
      }
    ]
  }
]
