---
title: "Reconciliation candidates scoped by provenance with per-spec task identity (spec_ref/meta_spec_ref/reconciliation_id) and reconcile/{id}/{spec} branches"
spec_ref: "Spec gap spec-556a0b49e9f2803bf0e2ff3d / reconciliation-candidate-scoping / b288723d2f73"
depends_on: [task-327]
progress: not-started
commits: []
---

## Required behavior

Per §6 Flow and the 'What This Does NOT Do' anti-requirement 'Only specs with provenance records under the old meta-spec version are candidates. Unaffected repos are skipped.' Today run_reconciliation iterates EVERY repo in the workspace for each changed meta-spec (for repo in &repos over list_by_workspace, dedup only against open tasks) and the created task's spec_path is the changed META-SPEC's own name (reconciliation.rs:591, spec_path: ms.name.clone()). Deliverables: (1) Provenance-scoped candidates — the controller identifies candidates via a provenance query (the query designed in blast-radius-provenance): only implementation specs with provenance records produced under the superseded version of the changed meta-spec become candidates; repos with no such provenance are skipped entirely (no task created for them). (2) Per-spec task shape — one task per affected (repo, spec): title 'Reconcile: {spec_path} under {meta_spec}@{new_sha}'; labels ['reconciliation', 'auto-created'] preserved; the task carries spec_ref = '{spec_path}@{current_sha}' (the AFFECTED implementation spec at its current pinned SHA, resolved from the spec ledger/attestation — not the meta-spec's name), meta_spec_ref = '{meta_spec_path}@{new_sha}', and a reconciliation_id field (Task struct + persistence extension, real fields not just labels) grouping every task in the wave; existing wave labels may remain for backward compatibility. (3) Reconciliation branches — agents spawned for reconciliation tasks work on branches named reconcile/{reconciliation-id}/{spec-slug} (wired through the spawn/worktree provisioning path; today agents get generic per-agent branches and the only 'reconcile/' string in the tree is a test fixture, reconciliation.rs:3407). (4) Preserve the satisfied machinery — §7 strategy dispatch (Immediate all-at-once, Rolling batches with kv-backed restart-safe pending state advanced on terminal status, Background tasks inert in Backlog), ReconciliationPriority mapping, auto_merge enqueue at agent completion, dedup (now per repo+spec identity, still only non-terminal tasks suppress), §8 tenant waves (one wave per non-overriding workspace, override/unreadable-set skips, force semantics), the no-op-MR validity contract in the task description, and the §6 trigger gating on Approved entries. COORDINATION: task-302 changes the binding/trigger basis (required flags + spec-level bindings instead of path@sha set diffs); the candidate-scoping, per-spec identity, and branch requirements are model-independent and must survive that migration.

## Acceptance Criteria

- [ ] Workspace with repo R1 (spec S has provenance under the superseded version) and repo R2 (no provenance at all): after the binding update to the new version, tasks exist for (R1, S) — carrying spec_ref 'S@{current sha}', meta_spec_ref '{meta-spec}@{new sha}', and a shared reconciliation_id — and NO task exists for R2.
- [ ] The agent spawned for the (R1, S) task works on a branch named reconcile/{reconciliation-id}/{spec-slug}.
- [ ] Rolling strategy still batches: with max_concurrent_agents = 1 and two affected specs, the first task is created at trigger and the second only after the first reaches terminal state (kv-backed, restart-safe).
- [ ] The task description retains the re-implement directive including no-op validity ('no-op outcome (code already conforms) is valid').
- [ ] A workspace whose only repos lack provenance under the superseded version produces zero tasks (unaffected repos skipped) while a provenance-bearing workspace in the same tenant wave produces its per-spec tasks.

## Verification intent

Set up R1 (spec S with provenance under the old version) and R2 (no provenance), trigger the reconciliation wave, then assert: a task exists for (R1, S) with spec_ref/meta_spec_ref/reconciliation_id fields, its agent's branch matches reconcile/{id}/{s}, and R2 has no task. At the base every assertion fails: the controller creates a task for EVERY repo — R2 included — whose spec_path is the changed meta-spec's own registry name (reconciliation.rs:591, spec_path: ms.name.clone()), the Task struct has no meta_spec_ref or reconciliation_id fields (waves are grouped only by labels), and spawned agents get standard per-agent branches (no reconcile/ prefix exists in production code; the only occurrence is a test fixture at reconciliation.rs:3407) — the base behavior violates the NOT-DO anti-requirement by requiring re-implementation work from repos with no code under the superseded version.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/meta-spec-reconciliation.md`. Source: `50575cf65e6491279049052901a65c68a4d2993d`; plan: `c2ceefa3b1c58d079ded6a7ee7cd9311539758965f2e1d0edd37b7ffdd4c9071`.

[
  {
    "id": "649ed86870daac6de2032d54",
    "decision": "gap",
    "requirement_quote": "   -> Agents work on reconciliation branches: reconcile/{reconciliation-id}/{spec}",
    "detail": "The flow's spine is implemented (registry records new SHA + approval reset on content edit; set update / approval triggers the controller; tasks are created per repo with reconciliation/auto-created labels plus wave-id and meta-spec@version provenance labels grouping the wave; delegation-typed tasks spawn agents through the normal loop under the new rules; MRs open through normal Ralph completion and merge through the normal merge queue; new provenance records the new set, resolving drift). Three concrete divergences from the specced flow: (1) tasks are created per (repo, changed meta-spec) for EVERY repo in the workspace — not 'per affected spec' identified 'via provenance query' (step 3's blast-radius-via-provenance is absent; run_reconciliation iterates all workspace repos); (2) the created task's spec_path is the changed META-SPEC's name, whereas the specced task carries spec_ref of the affected implementation spec plus meta_spec_ref and a reconciliation_id field — the affected implementation spec is never identified per task; (3) agents work on normal per-agent branches, not reconcile/{reconciliation-id}/{spec} branches (no reconcile/ branch prefix exists in production code; only a test fixture string).",
    "evidence": [
      {
        "file": "crates/gyre-server/src/reconciliation.rs",
        "quote": "spec_path: ms.name.clone(),",
        "detail": "The candidate's spec_path is the changed meta-spec's registry name — the created task's spec_path points at the governing meta-spec, not at an affected implementation spec; the specced task fields spec_ref ('same spec, new rules') and meta_spec_ref are not both carried, and per-spec provenance scoping is replaced by whole-repo candidates."
      },
      {
        "file": "crates/gyre-server/src/reconciliation.rs",
        "quote": "for repo in &repos {",
        "detail": "Candidate generation iterates every repo in the workspace (repos = list_by_workspace) with dedup only against existing open tasks — no provenance query filters to repos/specs with code under the superseded version; step 3's 'Affected repos and specs identified via provenance query' is not implemented on this path."
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "let jj_workspace_name = format!(\"agent-{}\", agent.id);",
        "detail": "Spawned agents (including reconciliation-task agents) get standard per-agent workspaces/branches; no reconcile/{reconciliation-id}/{spec} branch naming exists in production code (the only 'reconcile/' occurrence in the tree is a test fixture string in reconciliation.rs tests) — reconciliation work rides normal agent branches."
      }
    ]
  },
  {
    "id": "a854e91085ee72f68e6ca854",
    "decision": "gap",
    "requirement_quote": "- **Does not require re-implementation of everything.** Only specs with provenance records under the old meta-spec version are candidates. Unaffected repos are skipped. Agents may produce no-op MRs for code that already conforms.",
    "detail": "Four of the five anti-requirements hold: meta-spec sets are never auto-updated on content change (bindings change only via the explicit set PUT or tenant rollout — no code path writes meta_spec_sets on registry update); spec iteration remains the spec lifecycle's job; the default rollout policy is Normal priority interleaved with feature work (reconciliation does not block feature work); deployment/production concerns are out of scope here. The violated anti-requirement is the quoted one: candidates are NOT limited to specs with provenance records under the old version, and unaffected repos are NOT skipped. run_reconciliation treats every repo in the workspace as a reconciliation candidate for each changed meta-spec (its doc comment states 'every repo in the workspace is a reconciliation candidate... the workspace binding governs all repos'), with dedup only against existing open tasks — no provenance query filters unaffected repos, so reconciliation task waves do require re-implementation work for repos with no code under the superseded version. (The no-op-MR clause itself is honored — the task contract allows no-op outcomes.)",
    "evidence": [
      {
        "file": "crates/gyre-server/src/reconciliation.rs",
        "quote": "for repo in &repos {",
        "detail": "Candidate generation loops over ALL repos of the workspace (list_by_workspace) for each changed meta-spec; there is no provenance-based filter that would skip a repo with no code produced under the superseded version — the anti-requirement 'Unaffected repos are skipped' is not implemented on the reconciliation path."
      },
      {
        "file": "crates/gyre-server/src/reconciliation.rs",
        "quote": "/// (the workspace binding governs all repos in the workspace — §2), and",
        "detail": "The controller's own doc comment codifies the divergence: 'For each, every repo in the workspace is a reconciliation candidate (the workspace binding governs all repos in the workspace — §2), and §6 step 2 requires a task per repo' — an explicit design choice that contradicts this section's 'Only specs with provenance records under the old meta-spec version are candidates. Unaffected repos are skipped.'"
      }
    ]
  }
]
