---
title: "One table, one source of truth: unify spec_approvals and spec_approval_events into a single approval store"
spec_ref: "Spec gap spec-c08e139362b99830187a32b4 / approval-ledger-unification / a458cd7cdf30"
depends_on: [task-333]
progress: not-started
commits: []
---

## Required behavior

"The spec approval ledger in agent-gates.md is now unified with the forge ledger described here. One table, one source of truth." Today approvals live in two durable stores with separate ports — spec_approvals (SpecApprovalRepository; the per-(path,sha,approver) state machine the merge gate's verify_spec_ref enforces, migration 000058 shape) and spec_approval_events (SpecApprovalEventRepository; the §9 resolver's history with approver_type/persona/attestation_level/stack_hash) — and approve_spec writes both (specs.rs:652 and :687), while constraint_check.rs:3427 and aibom_spec_registry.rs:613 write only the event store, so the §9 resolution and the merge-gate check read different tables and can diverge. Unify into a single durable store that is the one source of truth for spec approvals: one consolidated port (replacing the SpecApprovalRepository/SpecApprovalEventRepository duality), one SQL table carrying the union of fields (approver_type, persona, attestation_level, stack_hash, signature, approved/revoked/rejected timestamps, reasons, approver identity), a migration at the next unused sequence number (000071 at planning time; re-check with `ls crates/gyre-adapters/migrations/ | sort | tail` at implementation time) written in portable SQL that runs on BOTH SQLite and PostgreSQL (check-migration-sql-portability.sh), data migration of existing rows from both tables, and adapters for SQLite, PostgreSQL, and mem with every port failure constraint enforced in each (check-mem-port-contracts.sh), wired via the store! pattern. Rework every reader onto the unified store: the §9 resolution path (resolve_approval_status callers), the merge-gate checks (verify_spec_ref, verify_signed_spec_ref, list_spec_approvals), the approve/reject handlers, users.rs judgments (list_by_approver, preserving the HSI §12 canonical-user-id attribution key), aibom.rs, constraint_check.rs, aibom_spec_registry.rs. All §9 validity semantics — including the owner-default human-approver set from this plan's approver-owner-default task (dependency: the unified store must carry the corrected predicate, not the current any-human default) — must be preserved and evaluated against the unified rows. Boundaries: only adapt the approval readers/writers; AIBOM artifact-generation behavior owned by the staged supply-chain plan and meta-spec kind-conditional approval modes owned by the meta-spec-reconciliation plan are out of scope.

## Acceptance Criteria

- [ ] One port type and one SQL table back spec approvals: AppState exposes a single approval store, no production code path reads or writes spec approval state through a second store, and the old dual ports are gone or reduced to the unified one.
- [ ] Approving via POST /api/v1/specs/:path/approve is a single write that both consumers see: GET /api/v1/specs shows the resolved status AND verify_spec_ref accepts the (path, sha) — revoke flips both. No code path can make the resolver and the merge gate disagree.
- [ ] Existing rows from both tables survive migration on SQLite and PostgreSQL and remain queryable (approvals, judgments, history).
- [ ] The existing approval-semantics test suite (mode resolution, attestation/stack_hash validity, revoke/reject, signed approvals, judgments identity key) passes against the unified store, plus the owner-default cases from approver-owner-default.

## Verification intent

Divergence probe the two-store design permits and the unified design cannot: record an approval event directly through the event store exactly the way constraint_check.rs does (same repository call, same fields), leaving the per-SHA ledger store empty. On the base, the §9 resolver counts the event and GET /api/v1/specs shows the spec Approved, while verify_spec_ref (reading the other table) rejects with 'no active approval in the ledger' — the same approval is simultaneously valid and invalid. After unification both consumers read the same rows, so the probe's paired assertions (resolver says Approved AND merge gate accepts) must agree; any residual second store, unsynced writer, or one-sided migration makes the pair fail.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/spec-registry.md`. Source: `52d7b66b7b2f0a4dc1d63a1b544a5a7364f79b40`; plan: `831e7d8656a4b362fd8a8e5e7b832c0c885c4b1a8c08c05832118ddb1e584c0f`.

[
  {
    "id": "4f7e9a6ec00744d40ce24f65",
    "decision": "gap",
    "requirement_quote": "The spec approval ledger in agent-gates.md is now unified with the forge ledger described here. One table, one source of truth.",
    "detail": "The gates-field half of the section is implemented: manifest_gates_for_mr resolves the manifest entry matching the MR's spec_ref path and contributes its gates: entries as gate agents, which are materialized as executed QualityGates carrying persona/min_attestation_level/stack_hash constraints and spawned as reviewing agents — different specs do get different gate agents. The unification half is NOT implemented: spec approvals live in two separate durable stores with separate ports — spec_approvals (SpecApprovalRepository; the per-SHA state-machine ledger the merge gate verify_spec_ref enforces) and spec_approval_events (SpecApprovalEventRepository; the event history the §9 resolver reads) — and approve_spec writes both on every approval. There is no single table: the §9 approval-status resolution and the merge-gate approval check read different tables, and the two records can diverge (the judgment-ledger identity-key fix, visible in approve_spec's comments, had to be applied to one store's writer specifically because the stores are separate).",
    "evidence": [
      {
        "file": "crates/gyre-server/src/gate_executor.rs",
        "quote": "pub(crate) fn manifest_gates_for_mr(\n    manifest: Option<&crate::spec_registry::SpecManifest>,\n    spec_ref: Option<&str>,\n) -> Vec<ManifestGateSpec> {",
        "detail": "The gates: field feeds the gate chain as specced: the manifest entry matching the MR's spec_ref contributes its gate configs, which are materialized and executed (spawned gate agents receive persona, min_attestation_level, and stack_hash)."
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "    pub spec_approvals: Arc<dyn gyre_ports::SpecApprovalRepository>,",
        "detail": "First of two approval stores in AppState — the per-SHA ledger (spec_approvals table) that verify_spec_ref/verify_signed_spec_ref enforce at merge."
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "    pub spec_approval_history: Arc<dyn gyre_ports::SpecApprovalEventRepository>,",
        "detail": "Second approval store in AppState — the event history (spec_approval_events table) that resolve_new_approval_status reads for §9 status resolution. Two ports, two tables: not 'one table, one source of truth'."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "    let _ = state.spec_approval_history.record(&event).await;",
        "detail": "Every approval is dual-written: the same approval is recorded into the event store here and into the spec_approvals ledger immediately below (`if let Err(e) = state.spec_approvals.create(&ledger_entry).await`), each with different field sets and identity keys — the unification this section requires is absent."
      }
    ]
  }
]
