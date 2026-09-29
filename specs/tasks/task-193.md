---
title: "Mode-based approval status resolution with attestation/stack_hash validity"
spec_ref: "spec-registry.md §9 Approval Status Resolution"
depends_on: []
progress: not-started
coverage_sections:
  - "spec-registry.md §9"
commits: []
---

## Spec Excerpt

From `specs/system/spec-registry.md` §9 (Approval Status Resolution):

> The ledger's `approval_status` is computed from the `approval_mode` and the approvals in `spec_approvals`:
>
> | Mode | Status = Approved When |
> |---|---|
> | `human_only` | At least one valid human approval exists for `current_sha` |
> | `agent_only` | At least one valid agent approval exists for `current_sha` with matching attestation constraints |
> | `human_and_agent` | At least one valid human AND at least one valid agent approval exist for `current_sha` |
>
> An approval is **valid** when:
> - `spec_sha` matches `current_sha` (not stale)
> - `revoked_at` is null (not revoked)
> - For agents: `attestation_level >= min_attestation_level` from manifest
> - For agents: `stack_hash` matches manifest's required `stack_hash` (if specified)

Related, §5 (Agent Approver / Gate Schema):

> When `stack_hash` is set, the forge verifies the agent's OIDC token contains a matching `stack_hash` claim. This prevents a weakened persona from producing approvals that satisfy policies written for the original persona. When `stack_hash` is null, any attested agent running the named persona is accepted.

Attestation levels (§5): `1=raw`, `2=CLI`, `3=Gyre-managed`.

## Problem (current state)

`approve_spec` in `crates/gyre-server/src/api/specs.rs:672-708` is hollow:

```rust
// Update ledger approval_status based on new approval.
// For simplicity: any valid approval for the current SHA sets status to Approved.
if let Some(mut entry) = state.spec_ledger.find_by_path(&spec_path).await? {
    if entry.current_sha == req.sha {
        entry.approval_status = ApprovalStatus::Approved;
        ...
```

It sets `Approved` on ANY single approval for the current SHA. It never:
- reads the spec's `approval_mode` from the manifest,
- requires both a human AND an agent for `human_and_agent` (the default mode),
- validates agent `attestation_level >= min_attestation_level`,
- validates agent `stack_hash` matches the manifest's required `stack_hash`.

Supporting gaps that MUST be closed to make §9 real:
- `SpecApprovalEvent` (`crates/gyre-domain/src/spec_ledger.rs:60`) carries `approver_type`, `approver_id`, `persona` but **not** `stack_hash` or `attestation_level`. The persisted `spec_approval_events` table (`crates/gyre-adapters/src/schema.rs:555`, sqlite `spec_approval_event.rs`, postgres `spec_approval_event.rs`) also lacks these two columns. The spec §8 `spec_approvals` schema requires them. Without them, agent validity checks cannot be evaluated against persisted approvals.
- The manifest is not currently loaded inside `approve_spec`. It must be read from the repo's git HEAD (see `get_spec` at `specs.rs:378` and `crate::spec_registry::read_git_file` for the pattern; the ledger entry carries `repo_id`).

## Implementation Plan

1. **Extend the approval record data model** (real persistence, both adapters):
   - Add `stack_hash: Option<String>` and `attestation_level: Option<i32>` to `SpecApprovalEvent` (`crates/gyre-domain/src/spec_ledger.rs`).
   - Add columns `stack_hash TEXT NULL` and `attestation_level INTEGER NULL` to the `spec_approval_events` table: update `crates/gyre-adapters/src/schema.rs`, add forward migrations in `crates/gyre-adapters/src/sqlite/migrations.rs` and the postgres migration path, and update the Queryable/Insertable rows + mapping in both `crates/gyre-adapters/src/sqlite/spec_approval_event.rs` and `crates/gyre-adapters/src/postgres/spec_approval_event.rs`. Update `MemSpecApprovalEventRepository` (`crates/gyre-server/src/mem.rs`) to round-trip the new fields.

2. **Populate the fields at approval time** in `approve_spec` (`specs.rs` ~line 494):
   - For agent approvals (`auth.jwt_claims.is_some()`): set `stack_hash` from the `wl_stack_hash` JWT claim, and derive `attestation_level` from the agent's stored `WorkloadAttestation` (`crate::workload_attestation::WorkloadAttestation`, keyed in `kv_store` namespace `"workload_attestations"` by agent id): map compute target to the spec taxonomy — container-backed (has `container_id`) ⇒ `3` (Gyre-managed), local/SSH CLI-spawned ⇒ `2` (CLI), otherwise ⇒ `1` (raw). Document the mapping in a code comment citing §5.
   - For human approvals: leave both `None`.

3. **Load the manifest for the spec** inside `approve_spec`:
   - Resolve the repo from `entry.repo_id`, read `specs/manifest.yaml` at HEAD via `crate::spec_registry::read_git_file`, parse with `crate::spec_registry::parse_manifest`, and find the matching `SpecEntry`.
   - Obtain `effective_approval_mode()`, `approval.human_approvers`, and `approval.agent_approvers` (`AgentApproverConfig { persona, min_attestation_level, stack_hash }`).
   - If the manifest or entry cannot be read, fail the resolution safely (do NOT default to Approved) — the status stays `Pending`.

4. **Replace the hollow resolution (lines 672-708)** with real mode-based resolution:
   - Fetch all approvals for the spec via `state.spec_approval_history.list_by_path(&spec_path)`.
   - Filter to **valid** approvals for `entry.current_sha`: `spec_sha == current_sha` AND `revoked_at.is_none()`.
   - For agent approvals, additionally require, against the matching `AgentApproverConfig` for that `persona`: `attestation_level >= min_attestation_level` (default `1`), and if the config's `stack_hash` is `Some`, the approval's `stack_hash == config.stack_hash`. An agent approval whose persona is not in `agent_approvers` does not count toward `agent_only`/`human_and_agent` satisfaction.
   - Compute `Approved` per mode: `human_only` ⇒ ≥1 valid human; `agent_only` ⇒ ≥1 valid qualifying agent; `human_and_agent` ⇒ ≥1 valid human AND ≥1 valid qualifying agent.
   - Only when the mode requirement is satisfied: set `entry.approval_status = Approved`, save, and emit the `SpecApproved` message-bus event (keep the existing emit block). Otherwise leave the status `Pending` and do NOT emit `SpecApproved`.
   - Keep the existing supersedes side-effect block (lines 710-763) unchanged; it should run only after a successful Approved transition (preserve current ordering/behavior).

5. **Do not regress** the existing link-based approval gates (lines 429-484) or the SignedInput attestation block.

## Acceptance Criteria

- `human_and_agent` (the default mode) requires BOTH a valid human and a valid qualifying agent approval for `current_sha` before status becomes `Approved`; a single human OR single agent leaves it `Pending`.
- `human_only` reaches `Approved` on one valid human approval; a lone agent approval does not.
- `agent_only` reaches `Approved` only on a valid agent approval whose persona matches an `agent_approvers` entry with `attestation_level >= min_attestation_level` and (when specified) matching `stack_hash`; a below-threshold or wrong-`stack_hash` agent approval leaves it `Pending`.
- Agent approvals persist `stack_hash` and `attestation_level` to the `spec_approval_events` table and survive a round-trip through the SQLite adapter (verified against a real SQLite DB, not the in-memory repo).
- `SpecApproved` is emitted only when the mode requirement is genuinely met.
- Stale approvals (spec_sha != current_sha) and revoked approvals never count toward resolution.
- `cargo build --all` and `bash scripts/check-arch.sh` pass.

## Agent Instructions

- Real work only. The resolution MUST evaluate persisted approvals against the manifest-derived mode and constraints. No `bail!()`, no audit-only, no hardcoded Approved.
- Write hard tests that fail if the mode gating breaks: e.g. a `human_and_agent` spec that stays `Pending` after only a human approval and flips to `Approved` after the qualifying agent approval; an `agent_only` spec that stays `Pending` when the agent's `attestation_level` is below `min_attestation_level` or its `stack_hash` mismatches. Prefer testing at the resolution boundary with a real SQLite-backed approval repo. Do NOT add self-confirming/mirrored-logic tests.
- If any spec detail proves genuinely wrong (not merely inconvenient), amend the spec via the spec lifecycle instead of coding around it.
- Skip project-wide formatters/linters and the full test suite; run only the targeted tests and `cargo build --all` + `scripts/check-arch.sh`.
- Follow hexagonal boundaries: domain/ports carry the new fields and query surface; adapters implement persistence; the server wires resolution. `gyre-domain` MUST NOT import adapters.
