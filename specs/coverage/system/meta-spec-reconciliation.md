# Coverage: Meta-Spec Reconciliation

**Spec:** [`system/meta-spec-reconciliation.md`](../../system/meta-spec-reconciliation.md)
**Last audited:** 2026-04-13 (full audit — verified against meta_spec.rs, meta_spec_set.rs, meta_specs.rs, attestation.rs, constraint_check.rs, message.rs, notification.rs)
**Coverage:** 5/11 (7 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | n/a | - | Problem statement — no implementable requirement. |
| 2 | Core Insight | 2 | n/a | - | Design rationale — no implementable requirement. |
| 3 | Definitions | 2 | n/a | - | Terminology definitions — no implementable requirement. |
| 4 | Design | 2 | n/a | - | Section heading only — no implementable requirement. |
| 5 | 1. Meta-Specs as First-Class Versioned Artifacts | 3 | implemented | - | MetaSpec struct with version: u32, MetaSpecVersion history archive. 4 kinds: meta:persona, meta:principle, meta:standard, meta:process. Approval lifecycle (Pending/Approved/Rejected). Content hashing. Full CRUD API. SQLite + Postgres adapters. Migration 000032. |
| 6 | 2. Meta-Spec Sets: Workspace-Level Binding | 3 | implemented | - | MetaSpecSet binds personas (named map), principles, standards, process specs (ordered lists) to workspaces. Path@SHA pinning. MetaSpecSetRepository port. GET/PUT /workspaces/:id/meta-specs/set (Admin only). SQLite + Postgres adapters. Migration 000018. |
| 7 | 3. Extended Provenance | 3 | implemented | - | MetaSpecUsed struct captures id, kind, content_hash, version, required, scope. Stored in MergeAttestation.meta_specs_used. meta_spec_set_sha in InputContent and AgentContext tracks active set hash at commit time. Full provenance chain. |
| 8 | 4. Blast Radius Computation | 3 | implemented | - | GET /api/v1/meta-specs/{path}/blast-radius endpoint. Scans all workspace meta-spec sets for references to the spec path. Returns BlastRadiusResponse with affected_repos and affected_workspaces lists. |
| 9 | 5. Preview Mode: The Fast Iteration Loop | 3 | implemented | - | POST /workspaces/:id/meta-specs/preview creates preview. GET /workspaces/:id/meta-specs/preview/:id retrieves results. Computes structural impact and blast radius. Stores preview records in kv_store. Returns preview_id with completion state. |
| 10 | 6. Reconciliation: The Slow Rollout | 3 | task-assigned | task-156 | Partial — ReconciliationCompleted MessageKind exists. emit_reconciliation_completed() emits Event-tier message + MetaSpecDrift notification. Missing: autonomous reconciliation controller that detects drift, creates reconciliation tasks per repo, and manages the reconciliation lifecycle. |
| 11 | 7. Rollout Policy | 3 | task-assigned | task-157 | Not implemented — no RolloutPolicy struct, no strategy types (Immediate/Rolling/Manual), no per-workspace rollout configuration. Depends on task-156 reconciliation controller. |
| 12 | 8. Tenant-Scope Meta-Spec Changes | 3 | task-assigned | task-158 | Not implemented — MetaSpecScope only has Global and Workspace variants, no Tenant scope. No tenant-level meta-spec set binding or cross-workspace propagation mechanism. |
| 13 | 9. Merge Gate Behavior | 3 | task-assigned | task-159 | Partial — meta_spec_set_sha constraint evaluator exists (constraint_check.rs) but not wired into merge gate results. No [WARN] meta-spec-drift warning in merge queue. No require_current_meta_spec_set workspace setting. |
| 14 | 10. Conformance Sweeps (Steady State) | 3 | task-assigned | task-156 | Not implemented — no background job that periodically sweeps repos to verify code conforms to active meta-spec set. Infrastructure ready (job framework, notification types) but sweep logic missing. |
| 15 | 11. Observability | 3 | task-assigned | task-156 | Partial — ReconciliationCompleted event and MetaSpecDrift notification exist. Missing: specific domain events (MetaSpecChanged, MetaSpecSetUpdated, ReconciliationStarted, DriftDetected, DriftResolved), Prometheus metrics (gyre_meta_spec_drift_total, gyre_reconciliation_tasks_total). |
| 16 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
| 17 | Kubernetes Parallels | 2 | n/a | - | Analogy/rationale — no implementable requirement. |
| 18 | What This Does NOT Do | 2 | n/a | - | Anti-requirements — no implementable requirement. |
