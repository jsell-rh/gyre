# Coverage: Spec Registry

**Spec:** [`system/spec-registry.md`](../../system/spec-registry.md)
**Last audited:** 2026-09-29 (full audit — §9/§15/§16 hollow → not-started; §5/§6/§10/§14/§19 partial; §2/§3/§17 verified)
**Coverage:** 14/18 (1 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | Solution: Git Manifest + Forge Ledger | 2 | verified | - | Dual-layer confirmed: specs/manifest.yaml (parse_manifest) as policy source + SpecLedgerEntry runtime state persisted via SpecLedgerRepository (SQLite + Postgres). |
| 3 | The Manifest: `specs/manifest.yaml` | 2 | verified | - | SpecManifest parsed via parse_manifest(); read_manifest_paths() reads the manifest from a git commit via git show. |
| 4 | Manifest Schema | 3 | verified | - | ApprovalConfig, GateConfig, SpecEntry carry all schema fields (path/title/owner/approval.mode/human_approvers/agent_approvers/gates/auto_create_tasks/auto_invalidate_on_change/superseded_by). |
| 5 | Agent Approver / Gate Schema | 3 | implemented | - | Partial — min_attestation_level & stack_hash fields parsed on AgentApproverConfig/GateConfig, but the specced enforcement (forge verifies matching stack_hash OIDC claim; attestation_level >= min) is never applied during spec approval. Consider splitting enforcement into a task. |
| 6 | Approval Modes | 3 | implemented | - | ApprovalMode enum (HumanOnly/AgentOnly/HumanAndAgent) with HumanAndAgent default exists; per-mode "what's required" semantics depend on §9 resolution, which is hollow. |
| 7 | Manifest Rules | 3 | verified | - | check_manifest_coverage() wired into push handler (git_http.rs:396) — rejects push + undoes ref updates for unregistered specs/. |
| 8 | The Forge Ledger | 2 | verified | - | SpecLedgerRepository port with real SQLite + Postgres adapters (spec_registry/spec_approvals tables). DB-persisted, not in-memory. |
| 9 | Approval Status Resolution | 3 | not-started | - | Hollow — approve_spec (specs.rs:672-678) sets Approved on ANY single valid approval for current SHA (comment: "For simplicity"). No mode-based resolution (human_and_agent never requires both human+agent), no attestation_level/stack_hash validity checks per §9. |
| 10 | Ledger State Machine | 3 | implemented | - | Partial — Pending/Deprecated transitions and SHA-based staleness in sync_spec_ledger are real; the "all required approvals → Approved" edge is the hollow §9 logic. |
| 11 | Ledger Sync on Push | 3 | verified | - | sync_spec_ledger() reads manifest at HEAD, computes blob SHA, resets Pending on change, marks Deprecated on manifest removal, warns on unregistered files. |
| 12 | API Surface | 3 | verified | - | All routes registered in api/mod.rs (list/pending/drifted/index/graph/:path/approve/revoke/history/links/progress). |
| 13 | Auto-Generated Index | 2 | verified | - | spec_index() renders markdown grouped by directory with per-spec status + short SHA. |
| 14 | Integration with Existing Specs | 2 | implemented | - | Integration points verified across spec-lifecycle, agent-gates, and accountability. |
| 15 | Spec Lifecycle (spec-lifecycle.md) | 3 | not-started | - | Hollow — process_spec_lifecycle() (git_http.rs:1361) classifies changed spec files by PATH via classify_spec_change/parse_spec_changes; it never reads the manifest. auto_create_tasks:false entries still spawn tasks and per-spec priority/gates are ignored. Spec requires manifest-driven, superseding path-prefix matching. |
| 16 | Agent Gates (agent-gates.md) | 3 | not-started | - | Hollow — manifest gates (GateConfig on SpecEntry) are parsed but never consumed anywhere. gate_executor.rs sources gates from repo-level quality_gates.list_by_repo_id, not the MR's spec manifest entry. Per-spec manifest gate selection unimplemented. |
| 17 | Spec-to-Code Binding (agent-gates.md) | 3 | verified | - | merge_processor blocks merge via verify_spec_ref() (ledger active-approval check) when spec_policy.require_approved_spec; approve_spec enforces implements/depends_on/conflicts_with link gates. |
| 18 | Supply Chain (supply-chain.md) | 3 | task-assigned | task-169 | No AIBOM integration with spec registry. Manifest state not included in release AIBOM artifacts. |
| 19 | Accountability Agent (personas/accountability.md) | 3 | implemented | - | Partial — spec_patrol.rs implements spec-links.md graph checks (stale links, orphaned supersessions, unresolved conflicts, dangling implementations, deep chains). spec-registry orphan/stale-entry detection exists at push time (find_unregistered_specs / sync warn), but the pending>1-cycle and modified-without-task patrol checks are not evidenced. |
