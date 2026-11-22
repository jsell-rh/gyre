---
title: "§11 observability completion: MetaSpecDriftDetected on the merge-gate path plus workspace/tenant/repo dashboard surfaces (drift indicators, reconciliation progress, per-spec provenance, commit badges)"
spec_ref: "Spec gap spec-556a0b49e9f2803bf0e2ff3d / meta-spec-observability-surfaces / b288723d2f73"
depends_on: [task-328, task-329, task-330]
progress: not-started
commits: []
---

## Required behavior

Per §11. Events and metrics are largely real; two areas are missing. (1) Event completion — the merge-gate drift path emits MetaSpecDriftDetected: record_meta_spec_drift_warning (merge_processor.rs:802, called at :4730 and :11110) and the strict-mode block path emit the same event/message kind the conformance sweep already emits, so the event fires for BOTH 'MR or conformance sweep' triggers per the §11 table (today an MR found drifted records only a [WARN] GateResult and no event is emitted). (2) Dashboard surfaces, served by real data: (a) workspace detail view — the workspace surface's Agent Rules section gains current/drifted version indicators per governing entry (governing version vs registry current — drifted badge when they differ), a reconciliation progress bar (per active wave: done/total tasks grouped by reconciliation_id), and drift count per repo (from the sweep's per-repo drift state); (b) tenant admin view — the tenant administration surface gains the rollout/reconciliation view: rollout plan status, per-workspace reconciliation progress, and total drift count across the tenant's workspaces; (c) repo detail view — per-spec meta-spec provenance (the governing versions recorded for each spec's most recent provenance record) and drift badges on commits (the commit's attested governing versions vs current). Backend support: drift/reconciliation state is served through real endpoints (workspace drift-status / reconciliation-progress and repo-level provenance views), each registered with an ABAC RouteResourceMapping entry (AGENTS.md route-registry invariant — unregistered routes get no policy evaluation); all numbers derive from the sweep's drift state, wave tasks, and provenance records — nothing fabricated. Metrics (gyre_meta_spec_drift_total, gyre_reconciliation_tasks_total, gyre_reconciliation_duration_seconds) already exist and must keep recording from these paths.

## Acceptance Criteria

- [ ] Driving the merge queue with a drifted MR (attested governing version superseded) records a MetaSpecDriftDetected event/message in addition to the [WARN] GateResult — the same kind the sweep emits.
- [ ] A workspace whose sweep flagged repo R drifted renders a drifted indicator on the affected governing entry and a per-repo drift count ≥ 1 for R; entries at current versions show current indicators; the reconciliation progress bar reflects done/total wave tasks grouped by reconciliation_id.
- [ ] The tenant admin view shows per-workspace reconciliation progress and a total drift count across the tenant's workspaces; the repo detail view shows per-spec governing-version provenance and drift badges on commits whose attested versions are superseded.
- [ ] Every new endpoint is present in the ABAC RouteResourceMapping registry (scripts/check-abac-route-registry.sh passes with no new exemptions).

## Verification intent

Arm 1 (event): enqueue a drifted MR through the merge queue and assert a MetaSpecDriftDetected event is recorded — at the base record_meta_spec_drift_warning's body persists only a GateResult (meta-spec-drift, Passed, [WARN]) and emits no event; only the sweep path emits the kind, so the MR-triggered half of the §11 event table never fires and the probe fails. Arm 2 (dashboard): with a sweep-flagged drifted repo, assert the workspace view renders the drifted indicator and per-repo drift count and the tenant admin view shows reconciliation progress and total drift — at the base the Agent Rules section renders only version numbers (rule-version spans) with no drift state, TenantSettings contains no reconciliation/drift/rollout content at all (its tabs are Users/Compute Targets/Budget/Audit/Health/Jobs), and no repo-detail per-spec provenance or commit drift badge exists anywhere under web/src — the component-level assertions fail.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/meta-spec-reconciliation.md`. Source: `50575cf65e6491279049052901a65c68a4d2993d`; plan: `c2ceefa3b1c58d079ded6a7ee7cd9311539758965f2e1d0edd37b7ffdd4c9071`.

[
  {
    "id": "4ec94a54317e88be7eb5b60b",
    "decision": "gap",
    "requirement_quote": "- **Workspace detail view:** meta-spec set with version indicators (current/drifted), reconciliation progress bar, drift count per repo.",
    "detail": "Events and metrics are largely real; the dashboard is largely missing. Events: MetaSpecChanged (emit_meta_spec_changed on registry content update), MetaSpecSetUpdated (workspace and tenant variants), ReconciliationStarted (custom Event-tier kind on wave creation), ReconciliationCompleted (MessageKind::ReconciliationCompleted with notification fan-out and duration metric), MetaSpecDriftResolved (on last terminal reconciliation/drift task) all exist; MetaSpecDriftDetected is emitted by the conformance sweep but NOT by the merge-gate path — an MR found drifted records only a [WARN] gate result (record_meta_spec_drift_warning saves a GateResult; no event), while the spec's table says the event fires when 'MR or conformance sweep found code under superseded meta-spec'. Metrics: all three specced Prometheus metrics exist (gyre_meta_spec_drift_total{workspace,repo}, gyre_reconciliation_tasks_total{workspace,status}, gyre_reconciliation_duration_seconds{workspace}) and are recorded in production paths. Dashboard: the workspace home shows the effective meta-spec set grouped by kind with version numbers and a 'rules reconciling' banner — but no current/drifted indicators, no reconciliation progress bar, no drift count per repo; the tenant admin view (TenantSettings) has no rollout-plan status, per-workspace reconciliation progress, or total drift count (its tabs are Users/Compute Targets/Budget/Audit/Health/Jobs); the repo detail view has no per-spec meta-spec provenance display and no drift badges on commits (grep across web components finds no such surface).",
    "evidence": [
      {
        "file": "crates/gyre-server/src/metrics.rs",
        "quote": "\"gyre_meta_spec_drift_total\",",
        "detail": "All three specced metrics are registered on the Prometheus registry (gyre_meta_spec_drift_total{workspace,repo}, gyre_reconciliation_tasks_total{workspace,status}, gyre_reconciliation_duration_seconds{workspace}) and recorded from production paths (record_drift_metrics / record_reconciliation_metrics / wave completion)."
      },
      {
        "file": "web/src/components/WorkspaceHome.svelte",
        "quote": "<span class=\"rule-version\">v{ms.version}</span>",
        "detail": "The workspace home's Agent Rules section renders the effective meta-spec set grouped by kind with version numbers and a reconcile-status banner — the partial realization of the workspace detail view; it carries no current/drifted indicator, no reconciliation progress bar, and no per-repo drift count (inspected the full section template)."
      },
      {
        "file": "web/src/components/TenantSettings.svelte",
        "quote": "Tabs: Users | Compute Targets | Budget | Audit | Health | Jobs",
        "detail": "The tenant administration surface has no meta-spec rollout content at all — no rollout plan status, per-workspace reconciliation progress, or total drift count (inspected the component; no reconciliation/drift/rollout references exist in it)."
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "quote": "async fn record_meta_spec_drift_warning(",
        "detail": "The merge-gate drift path only persists a GateResult (meta-spec-drift, Passed, [WARN] output) — the inspected function body performs no event emission, so MetaSpecDriftDetected never fires for MR-found drift, only for sweep-found drift (reconciliation.rs emits the custom meta_spec_drift_detected kind), contrary to the event table's 'MR or conformance sweep' trigger."
      }
    ]
  }
]
