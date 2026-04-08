# TASK-044: Authorization Provenance — HSI Constraint Editing UI and Provenance Chain Visualization

**Spec reference:** `authorization-provenance.md` §7.6 (Human-System Interface amendments)

**Progress:** `not-started`

**Commits:**
(none yet)

---

## Context

The authorization-provenance spec (§7.6) explicitly amends the Human-System Interface with two UI features:

1. **Constraint editing in the spec approval dialog** — When a human approves a spec, they should be able to see strategy-implied constraints (read-only), add explicit CEL constraints, define scope constraints via file-tree globs, and dry-run the constraint set.

2. **Provenance chain visualization in the Explorer** — Clicking a commit should show its attestation chain as a directed graph with signer identity, constraint count, and verification status per node.

All backend infrastructure is complete:
- Domain types: TASK-003 (complete)
- Storage adapters: TASK-004 (complete)
- CEL evaluator: TASK-005 (complete)
- Key binding + signed input: TASK-006 (complete)
- Strategy-implied constraints: TASK-007 (complete)
- Enforcement: TASK-008 (complete)
- Full chains + AIBOM: TASK-009 (complete)

The server already exposes the endpoints needed:
- `POST /api/v1/constraints/validate` — validate CEL expression syntax
- `POST /api/v1/constraints/dry-run` — evaluate constraints against repo state
- `GET /api/v1/constraints/strategy` — preview strategy-implied constraints
- `GET /api/v1/repos/:id/attestations/:commit_sha/chain` — attestation chain data
- `GET /api/v1/repos/:id/attestations/:commit_sha/verification` — verification result tree

## Spec Excerpt

> **Amendment (§7.6):** Spec approval UI gains constraint editing:
>
> - The approval dialog shows strategy-implied constraints (derived from workspace config, trust level, stack policy) as read-only.
> - The approver can add explicit constraints via a CEL expression editor with autocomplete for available context fields.
> - The approver can define scope constraints via a file-tree picker (glob patterns).
> - A dry-run button evaluates the constraint set against the current repo state to preview what would pass/fail.
>
> The Explorer (§3) gains a provenance chain visualization:
> - Clicking a commit shows its attestation chain as a directed graph.
> - Each node shows: signer identity, constraint count, verification status.
> - Failed constraints are highlighted with the failing expression and the value that caused the failure.

## Requirements

### Part A: Constraint Editing in Spec Approval

1. **Strategy-implied constraint preview**: When the approval dialog opens, call `GET /api/v1/constraints/strategy` with the spec path and workspace context. Display the returned constraints as read-only items with their CEL expressions.

2. **Explicit constraint editor**: Add a section where the approver can enter CEL expressions. Each constraint has a `name` (human-readable label) and `expression` (CEL). Validate syntax on blur via `POST /api/v1/constraints/validate`.

3. **Scope constraint editor**: Add glob pattern inputs for `allowed_paths` and `forbidden_paths`. Show the repo's file tree for reference.

4. **Dry-run button**: Call `POST /api/v1/constraints/dry-run` with the assembled constraint set. Display results as pass/fail per constraint with the failing expression and context value highlighted.

5. **Wire to approval request**: When the user clicks "Approve", include `output_constraints` and `scope` in the `POST /api/v1/specs/:path/approve` request body.

### Part B: Provenance Chain Visualization

1. **Commit attestation chain**: When a commit is selected in the Explorer (e.g., in the Code tab's commit list or via provenance drill-down), fetch `GET /api/v1/repos/:id/attestations/:commit_sha/chain`.

2. **Chain graph rendering**: Render the attestation chain as a directed graph (root `SignedInput` at top, derived inputs below, leaf attestation at bottom). Each node shows:
   - Signer identity (user or agent)
   - Chain depth
   - Constraint count
   - Verification status (green check / red X)

3. **Failed constraint highlighting**: If verification failed, highlight the failing node in red. Show the failing constraint's CEL expression and the actual value that caused the failure (from the `VerificationResult` tree).

4. **Integration point**: This can be a new panel/tab in `NodeDetailPanel` or `ExplorerCodeTab`, accessible when viewing commit provenance.

## Existing Code References

- `web/src/components/ExplorerView.svelte` — main explorer container
- `web/src/components/ExplorerCodeTab.svelte` — code/blame/provenance view
- `web/src/lib/NodeDetailPanel.svelte` — detail panel
- `crates/gyre-server/src/api/provenance.rs` — chain/verification/bundle endpoints
- `crates/gyre-server/src/api/specs.rs` — spec approval endpoint (already accepts `output_constraints` and `scope`)
- `crates/gyre-common/src/attestation.rs` — Attestation, VerificationResult types

## Agent Instructions

When working on this task:
1. Update this file's progress to `in-progress` when you start.
2. Add commit SHAs to the "Commits" section above as you go.
3. When complete, update progress to `ready-for-review`.
