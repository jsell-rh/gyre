# Coverage: Spec Registry

**Spec:** [`system/spec-registry.md`](../../system/spec-registry.md)
**Last audited:** 2026-04-13 (full audit — bulk reclassification from not-started)
**Coverage:** 17/18 (1 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | Solution: Git Manifest + Forge Ledger | 2 | implemented | - | Dual-layer design: specs/manifest.yaml in git + SpecLedgerEntry in forge DB. spec_registry.rs implements full lifecycle. |
| 3 | The Manifest: `specs/manifest.yaml` | 2 | implemented | - | SpecManifest struct parsed from YAML via parse_manifest(). read_manifest_paths() reads from git commits. |
| 4 | Manifest Schema | 3 | implemented | - | ApprovalConfig, AgentApproverConfig, GateConfig fully structured. All spec-required schema fields present. |
| 5 | Agent Approver / Gate Schema | 3 | implemented | - | min_attestation_level and stack_hash fields present on AgentApproverConfig. |
| 6 | Approval Modes | 3 | implemented | - | HumanOnly, AgentOnly, HumanAndAgent enum with defaults. |
| 7 | Manifest Rules | 3 | implemented | - | check_manifest_coverage() enforces/warns on unregistered specs. Migration 000048 for enforcement. |
| 8 | The Forge Ledger | 2 | implemented | - | SpecLedgerEntry type in gyre-domain with all required fields. Persisted in DB. |
| 9 | Approval Status Resolution | 3 | implemented | - | Computed in approve_spec() based on mode + valid approvals. |
| 10 | Ledger State Machine | 3 | implemented | - | Pending → Approved → Deprecated transitions. Revoked/Rejected variants. |
| 11 | Ledger Sync on Push | 3 | implemented | - | sync_spec_ledger() handles manifest reads, SHA tracking, deprecation on every push to default branch. |
| 12 | API Surface | 3 | implemented | - | All 12+ routes: /specs, /pending, /drifted, /index, /approve, /revoke, /history, /progress, /links, etc. |
| 13 | Auto-Generated Index | 2 | implemented | - | spec_index() renders markdown grouped by directory with status indicators. |
| 14 | Integration with Existing Specs | 2 | implemented | - | Integration points verified across spec-lifecycle, agent-gates, and accountability. |
| 15 | Spec Lifecycle (spec-lifecycle.md) | 3 | implemented | - | process_spec_lifecycle() in git_http.rs uses manifest entries for task creation on spec changes. |
| 16 | Agent Gates (agent-gates.md) | 3 | implemented | - | GateConfig fields fed to gate chain. Gates enforcement in gate_executor.rs. |
| 17 | Spec-to-Code Binding (agent-gates.md) | 3 | implemented | - | approve_spec() enforces link-based gates (implements, depends_on, conflicts_with). |
| 18 | Supply Chain (supply-chain.md) | 3 | task-assigned | task-169 | No AIBOM integration with spec registry. Manifest state not included in release AIBOM artifacts. |
| 19 | Accountability Agent (personas/accountability.md) | 3 | implemented | - | spec_patrol.rs monitors orphans, stale entries, pending specs. Accountability persona integration verified. |
