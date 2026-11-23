---
title: "Entropy management: golden-principles corpus plus an authorized fix-up pipeline converting patrol findings into fix-up tasks and fix-up MRs"
spec_ref: "Spec gap spec-8f4edcf188b12e2a350050e2 / fixup-mr-pipeline / fd38a2fca0f8"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Per specs/development/agent-experience.md §Entropy Management / Garbage Collection: "Encode golden principles in the repo and run recurring cleanup agents that scan for deviations and open fix-up MRs" and "Human taste is captured once, then enforced continuously on every line." At base 87377fe1 the meta-spec mechanism is real (REQUIRED meta-specs injected into every prompt and recorded in provenance; daily meta_spec_conformance_sweep compares the active-set SHA against provenance and creates drift-review tasks) but the corpus is minimal — seed_builtin_meta_specs (lib.rs:2726) seeds exactly one Principle (conventional-commits) — and no fix-up MR path exists (grep fixup/fix-up across crates/ returns zero matches; the only automatic MR creation is create_mr_for_completion tied to task completion; the patrols create notifications/messages only). Deliver: (1) Golden-principles corpus: extend the builtin seed set with additional Principle meta-specs encoding golden principles as they are already normatively stated in canonical specs — at minimum the hexagonal boundary invariant (specs/development/architecture.md: gyre-domain depends only on ports + common) and the real-work/no-fakery rule (specs/GOAL.md) — each seeded REQUIRED following the existing conventional-commits pattern, with prompt text derived from the canonical spec wording (never invented), so prompt_assembly injects them into every agent prompt and provenance records them; the existing conformance sweep then continuously enforces the active set. (2) Fix-up pipeline: a production conversion function (e.g. ensure_fixup_task) that takes a patrol/sweep finding and idempotently creates a durable fix-up Task scoped to the finding's real workspace/repo — mirroring run_conformance_sweep's dedup (skip when an open task with the same title and scope exists), skipping and logging (never fabricating a "default" scope) when the finding's workspace cannot be resolved (check-fabricated-scope-defaults / check-scope-literal-defaults invariants) — labeled (e.g. "fixup", "auto-created") so the existing scheduler spawns a cleanup agent whose completion flows through the existing create_mr_for_completion: the fix-up MR path. (3) Producers, with authorization pinned per controller review feedback fd6e133d1621c46338a657b0:unauthorized-cross-tenant-fixup-write-path, correction option (b): the conversion call sites in the manual patrol HTTP handlers must establish caller identity and enforce per-handler authorization. patrol_spec_links (api/specs.rs:2137) currently declares only State and Json extractors — no caller identity at all — while the route sits in scripts/abac-route-registry-exemptions.txt (line 29) and so runs with NO middleware ABAC evaluation: add the auth extractor and, before creating any fix-up task, resolve the finding's scope (workspace, and repo via the spec ledger where the finding carries a spec path) and require the caller's tenant/workspace to contain it, returning Forbidden on mismatch; findings whose scope cannot be resolved are skipped and logged. This per-handler containment is additive to task-292's middleware-ABAC work on the same route (when task-292 lands, its resolver entry and exemption shrink provide middleware policy evaluation and the per-handler containment remains valid defense in depth); do not remove or add exemption-file entries here and do not add duplicate resolver entries. patrol_spec_lifecycle (already covered by middleware ABAC via the resolver entry at abac_middleware.rs:458) gains the conversion with fix-up creation scoped to the finding's resolved workspace — never a scope outside the finding's real one. System-principal producers (the doc-gardening job delivered by the doc-garden task, and the conformance sweep's existing drift-review tasks) create fix-up work as the system principal and need no caller containment. Coordination: task-292's scheduled spec-links patrol job is its own frozen contract (notification fan-out only) — do not modify it here; task-292 is not a dependency because the per-handler containment makes this task's wiring safe to land in either order.

## Acceptance Criteria

- [ ] Precondition on this criterion per the review feedback: the caller is authorized (within the finding's tenant/workspace scope) or is the system principal. Such a caller POSTing /api/v1/patrol/spec-links against state containing an error-severity finding gets exactly one durable fix-up task created for it — scoped to the finding's workspace/repo and labeled fixup — and a repeated invocation creates no duplicate (dedup by open-task title + scope); the fix-up task is spawnable by the existing scheduler and its completion produces a fix-up MR via create_mr_for_completion.
- [ ] A principal authenticated in a DIFFERENT tenant POSTing /api/v1/patrol/spec-links against the same state causes zero fix-up task creation (task row count unchanged before/after) and receives Forbidden.
- [ ] Findings with no resolvable workspace are skipped with a log line naming the finding; no task is created under a fabricated "default" scope.
- [ ] A fresh server boot seeds more than one REQUIRED Principle meta-spec (hexagonal boundary and no-fakery alongside conventional-commits), and an assembled agent prompt or its provenance attestation records all of them.
- [ ] scripts/check-abac-exempt-handlers.sh, check-abac-route-registry.sh, check-fabricated-scope-defaults.sh, check-scope-literal-defaults.sh, and check-arch.sh pass; the ABAC exemption file is byte-identical to its committed baseline after this task (no additions, no removals — task-292 owns the shrink).

## Verification intent

Cross-tenant arm (the exact review-feedback probe): seed state with an error-severity stale_link in tenant A, authenticate a principal in tenant B, and POST /api/v1/patrol/spec-links comparing task row counts before/after — on current code there is no fix-up path at all (grep fixup in crates/ is empty; the handler's only side effect is notification fan-out), so the fix-up half of the probe cannot even execute; after this task the count must be unchanged and the response Forbidden, proving the per-handler containment gates the new durable, compute-spawning write. Corpus arm: inspect the seeded meta-specs on a fresh boot and the REQUIRED principles injected into an assembled prompt — on current code exactly one Principle (conventional-commits) exists, so an assertion over the corpus fails; after the task the hexagonal-boundary and no-fakery principles must appear in both the seed and the injected prompt/provenance. Dedup arm: invoke the patrol twice against the same error finding and count fix-up tasks — the second invocation must not create another row, proving idempotent conversion rather than unbounded task creation.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/agent-experience.md`. Source: `87377fe145ebddf27ef975fba7829fb10f4bb136`; plan: `06fdb4a49c8ed342ae5f76200503e9e22b0b9b4ae513eb26ddd930d0be5ad861`.

[
  {
    "id": "fd6e133d1621c46338a657b0",
    "decision": "gap",
    "requirement_quote": "- Encode **golden principles** in the repo and run recurring cleanup agents that scan for deviations and open fix-up MRs.\n- Technical debt is paid continuously in small increments, not in painful bursts.\n- Human taste is captured once, then enforced continuously on every line.",
    "detail": "The principle-encoding and continuous-enforcement halves are partially real; the recurring-cleanup-agents-opening-fix-up-MRs half is not. Real: the meta-spec system encodes principles machine-actionably — seed_builtin_meta_specs seeds Principle/Standard/Persona meta-specs, prompt_assembly injects all REQUIRED meta-specs into every agent prompt ordered persona → principle → standard → process and records them in provenance attestations, and the daily meta_spec_conformance_sweep job (registered in jobs.rs) compares the active set SHA against provenance and auto-creates drift-review tasks on drift. Missing: (1) only one Principle meta-spec is seeded ('conventional-commits') — the mechanism exists but the golden-principles corpus is minimal. (2) The spec patrols (spec_patrol.rs run_patrol with five real spec-graph checks; spec_lifecycle_patrol.rs with three more) run only via manually-invoked HTTP endpoints — their sole production callers are the patrol_spec_links/patrol_spec_lifecycle handlers; jobs.rs registers no patrol job, so no recurring cleanup scan over spec-graph deviations exists (the repo's own task-292 records this as not-started). (3) No fix-up MR path: grep for 'fixup'/'fix-up' across crates/ returns zero matches; the only automatic MR creation is create_mr_for_completion, tied to agent completion of an existing task — no code path opens a fix-up MR from a patrol or sweep finding (all MergeRequest::new uses in reconciliation.rs are test fixtures). A drift task can eventually flow task → scheduler-spawned agent → completion MR, but that is task-driven indirection, not cleanup agents opening fix-up MRs, and the patrol path creates only notifications/messages, never tasks or MRs.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "\"conventional-commits\",",
        "detail": "seed_builtin_meta_specs seeds meta-specs including this single MetaSpecKind::Principle entry ('All commits must follow Conventional Commits specification.') plus one Standard and personas. The machine-actionable encoding of golden principles exists as a mechanism, but the seeded Principle corpus is one entry — human taste is captured only for commit style."
      },
      {
        "file": "crates/gyre-server/src/prompt_assembly.rs",
        "quote": "//! The assembled set is also the attestation source: `meta_specs_used` records",
        "detail": "Continuous per-line enforcement is real: every agent prompt is assembled from all REQUIRED meta-specs (kind-ordered persona → principle → standard → process) at their pinned approved versions, and the injected set is recorded in provenance attestations — 'captured once, enforced continuously' is realized for whatever principles are registered."
      },
      {
        "file": "crates/gyre-server/src/reconciliation.rs",
        "quote": "task.labels = vec![DRIFT_REVIEW_LABEL.to_string(), \"auto-created\".to_string()];",
        "detail": "The daily meta_spec_conformance_sweep (registered in jobs.rs as a recurring job) detects drift between the active meta-spec set and provenance-recorded SHAs and auto-creates drift-review tasks — recurring deviation detection with durable task creation is real production behavior, the strongest satisfied fragment of this section."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "crate::spec_patrol::run_patrol(&state, now, threshold).await;",
        "detail": "The spec-graph patrol's only production callers are the manually-invoked HTTP handlers patrol_spec_links and patrol_spec_lifecycle (grep for run_patrol/run_and_escalate across crates/gyre-server/src shows only these handlers and tests). jobs.rs registers many recurring jobs (retention_cleanup, staleness checks, conformance sweep, pr_hygiene_check) but no patrol job — the patrol never runs on a schedule, so there is no recurring cleanup scan over spec-graph deviations."
      },
      {
        "file": "specs/tasks/task-292.md",
        "quote": "the patrol exists only as a manually invoked HTTP endpoint",
        "detail": "The repo's own task ledger records this exact gap as progress: not-started, requiring a scheduled spec-links patrol job, ABAC gating of the endpoint, and persona patrol inputs — confirming the recurring-cleanup integration is a known open gap, not an implemented behavior."
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "/// Create the completion MR for an agent (agent-runtime.md §1 Phase 6 step 1).",
        "detail": "The only automatic MR creation path is agent-completion-driven (shared by POST /agents/:id/complete and the MCP gyre_agent_complete tool). Grep for 'fixup'/'fix-up' across crates/ returns zero matches, and all MergeRequest::new uses in reconciliation.rs are inside test fixtures — no code path opens a fix-up MR from a patrol or conformance-sweep finding."
      }
    ]
  }
]
