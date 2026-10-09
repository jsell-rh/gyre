---
title: "Mode-based spec approval status resolution with attestation/stack_hash validity"
spec_ref: "spec-registry.md §9 Approval Status Resolution"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "spec-registry.md §9"
commits: ["1e55eef5973023193199d136a844f072d25f1b36", "b1ead356c8c96561b8b88374922312ca82831824", "5d4ebb171e2f01c7d5fdd6898c5c008f854ac74a", "e42c374d89663ff8bac7990edb2853942534fdf2", "0e6c1b88e05e10e2ab8c40eb958d28e27579a6d6", "56b51b3fa52d5d978ec8ca9519884ddc31bf58a9"]
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

Also §5 (Agent Approver / Gate Schema): "When `stack_hash` is set, the forge verifies the agent's OIDC token contains a matching `stack_hash` claim … When `stack_hash` is null, any attested agent running the named persona is accepted."

## Problem

Hollow. `approve_spec` (`crates/gyre-server/src/api/specs.rs:672-678`) sets `approval_status = Approved` on **any** single valid approval for the current SHA (code comment: "For simplicity: any valid approval for the current SHA sets status to Approved"). This means:

- `human_and_agent` mode never actually requires **both** a human and an agent approval.
- `agent_only` mode accepts an agent approval without checking `attestation_level >= min_attestation_level` or a required `stack_hash` from the manifest.
- The manifest's `approval.human_approvers` / `approval.agent_approvers` constraints are ignored entirely.

Additionally, `SpecApprovalEvent` (`crates/gyre-domain/src/spec_ledger.rs:60-75`) carries `approver_type` and `persona` but **no `attestation_level` / `stack_hash`** — so the agent-validity checks §9 requires cannot be evaluated from recorded events. These must be captured at approval time (from the approving agent's JWT/OIDC claims) and persisted.

## Implementation Plan

1. **Capture agent attestation on approval events.**
   - Add `attestation_level: Option<u32>` and `stack_hash: Option<String>` to `SpecApprovalEvent` (`crates/gyre-domain/src/spec_ledger.rs`). The `spec_approvals` table in the ledger schema already models these columns (`attestation_level`, `stack_hash`) — verify the `spec_approval_history` adapter table (SQLite + Postgres) has matching columns; add a diesel migration + `schema.rs` update + adapter row mapping if absent. Do NOT drop existing data.
   - In `approve_spec` (`specs.rs:486-505`), when the approver is an agent (`auth.jwt_claims.is_some()`), populate `attestation_level` and `stack_hash` from the agent's verified JWT claims (see how `auth.jwt_claims` is structured; reuse the same claim names the agent runtime issues — grep `stack_hash` / `attestation_level` in the auth/JWT code). Humans leave both `None`.

2. **Read the manifest entry for the spec at approval time.**
   - Add a helper `pub async fn read_manifest(repo_path: &str, sha: &str) -> Option<SpecManifest>` in `crates/gyre-server/src/spec_registry.rs` (reuse `read_git_file` exactly as `read_manifest_paths` does; parse via `parse_manifest`). Keep `read_manifest_paths` working (it MAY delegate to the new helper).
   - In `approve_spec`, resolve the ledger entry's `repo_id` → repo → `repo_path` (via `state.repos`), then `read_manifest(repo_path, &entry.current_sha)` and find the `SpecEntry` whose `path` matches `spec_path` (manifest paths are relative to `specs/`; `spec_path` is the `specs/…`-prefixed path — normalize consistently, matching how `sync_spec_ledger` maps them).

3. **Replace the "any valid approval" logic (specs.rs:672-678) with mode-based resolution.**
   - Write a pure resolver function (unit-testable, place in `spec_registry.rs` or a sibling module) with signature roughly:
     `fn resolve_approval_status(entry: &SpecEntry, defaults: &ManifestDefaults, current_sha: &str, events: &[SpecApprovalEvent]) -> ApprovalStatus`.
   - An event is **valid** iff `event.spec_sha == current_sha` AND `event.is_active()` (revoked_at is None).
   - Agent-approval validity additionally requires: the event's `persona` matches a configured `agent_approvers[].persona`; `attestation_level >= that approver's min_attestation_level` (default 1); and if that approver sets `stack_hash`, `event.stack_hash == Some(required)`.
   - Human-approval validity: `approver_type == "human"` (optionally, when `human_approvers` is non-empty, require `approver_id` ∈ that list — implement per spec's "from `human_approvers`" wording).
   - Approved when: `human_only` → ≥1 valid human; `agent_only` → ≥1 valid agent; `human_and_agent` → ≥1 valid human AND ≥1 valid agent. Otherwise keep `Pending`.
   - If `effective_requires_approval` is false for the entry, treat as Approved for any SHA (matches §17 "requires_approval: false → any SHA accepted").
   - Feed the resolver with the active events from `state.spec_approval_history.list_by_path(&spec_path)` (the full history; filter to `current_sha` inside the resolver). When the manifest cannot be read (no repo/manifest), fall back to the existing single-approval behavior so approvals in manifest-less repos still work — log at debug.
   - Only emit the `SpecApproved` message-bus event (specs.rs:680-707) when the resolver transitions the entry to `Approved` (i.e., all required approvals are now present), not on every partial approval.

## Acceptance Criteria

- `SpecApprovalEvent` persists `attestation_level` and `stack_hash`; an agent approval round-trips these from JWT claims through the repository and back.
- A `human_and_agent` spec is **not** marked `Approved` after only a human approval, nor after only an agent approval; it becomes `Approved` only once both a valid human and a valid agent approval exist for `current_sha`.
- An `agent_only` spec with `min_attestation_level: 3` is **not** `Approved` by an agent approval recorded with `attestation_level: 2`; it is `Approved` when the level is ≥ 3.
- A spec whose manifest approver sets a `stack_hash` is **not** `Approved` by an agent whose recorded `stack_hash` differs; it is `Approved` on an exact match.
- Unit tests on the pure `resolve_approval_status` resolver that FAIL if the mode/attestation/stack_hash logic is reverted to "any valid approval": at minimum one test per mode plus one attestation-too-low and one stack_hash-mismatch case.
- `cargo build --all`, touched-crate tests, and `bash scripts/check-arch.sh` pass.

## Agent Instructions

- The resolver MUST be a pure function over `(SpecEntry, ManifestDefaults, current_sha, events)` so it is testable without a DB; wire it into `approve_spec`. Do not embed the logic inline in the async handler in a way that can't be unit-tested.
- Do NOT invent attestation values to make a mode pass. Capture real values from the agent's JWT claims; humans stay `None`.
- Verify the diesel table mapping for the approval-history table in `crates/gyre-adapters/src/schema.rs` before adding columns; write a real migration.
- This task introduces `read_manifest()` in `spec_registry.rs`; tasks 194 and 195 reuse it — keep the signature `read_manifest(repo_path, sha) -> Option<SpecManifest>`.
- Run only touched-crate tests plus `scripts/check-arch.sh`; skip formatters and the full workspace suite.

## Shipped

§9 mode-based approval resolution is implemented and verified at `32c94f62` (crates bit-identical to the reviewed checkpoint `3e467f37`; merge `32c94f62` only brought in the updated base `8c2d1775`).

**Behavior:**
- `SpecApprovalEvent` now carries `attestation_level: Option<u32>` and `stack_hash: Option<String>` (`gyre-domain/src/spec_ledger.rs`), persisted via diesel migration `2026-10-08-000056_spec_approval_attestation` (portable `ALTER TABLE ADD COLUMN`, both SQLite + Postgres adapters map the new columns; mem adapter stores the struct). Schema updated, no data dropped.
- `approve_spec` (`api/specs.rs`) captures agent attestation from the approver's **verified JWT claims**: `stack_hash` from the `wl_stack_hash` claim; `attestation_level` derived from the agent's real workload-attestation record via the same `derive_attestation_level` used by constraint checks (supply-chain levels 0–3). Humans (API-key/global-token auth, or non-agent-scope JWT) record `None`/`None`. Values are never invented.
- New `read_manifest(repo_path, sha) -> Option<SpecManifest>` in `spec_registry.rs` (signature as specified for tasks 194/195; `read_manifest_paths` delegates to it). The resolver reads the manifest at `HEAD` because `current_sha` is a blob SHA, not a commit.
- Pure resolver `resolve_approval_status(entry, defaults, current_sha, events)` in `spec_registry.rs`: event valid iff `spec_sha == current_sha` && active; agent validity additionally requires persona ∈ `agent_approvers`, `attestation_level >= min_attestation_level` (default 1), and exact `stack_hash` match when pinned; human validity requires `approver_type == "human"` and `approver_id ∈ human_approvers` when that list is non-empty. Modes: `human_only` → ≥1 valid human; `agent_only` → ≥1 valid agent; `human_and_agent` → both. `requires_approval: false` → Approved regardless (§17). Wired into `approve_spec` and `revoke_spec_approval` via `resolve_new_approval_status` (manifest-less repos fall back to legacy single-approval semantics, logged at debug).
- `SpecApproved` message-bus event fires only on the transition *into* Approved (partial approvals of a `human_and_agent` pair no longer trigger the signal chain).

**Test evidence (all at `32c94f62`, logs in /tmp/stage/review-evidence/):**
- 13 resolver unit tests pass, including per-mode tests that fail under a reverted "any valid approval" resolver, attestation-too-low (`test_resolve_agent_only_pending_when_attestation_below_minimum`), stack_hash-mismatch and stack_hash-missing.
- 4 E2E tests through the real HTTP router with real committed git repos and real minted agent JWTs + workload attestations: `human_and_agent_requires_both_approvals_e2e` (Pending after agent-only, Approved after human completes the pair), `agent_only_rejects_attestation_below_minimum_e2e` (level 2 vs min 3 → Pending; level 3 → Approved), `agent_only_requires_exact_stack_hash_match_e2e` (wrong stack → Pending; pinned stack → Approved), `agent_attestation_fields_round_trip_via_history_api` (JWT-derived level 3 + `sha256:pinned` persist and read back).
- Adapters round-trip: 3/3 (`agent_attestation_fields_round_trip`, `human_approval_keeps_attestation_fields_null`, `record_and_list_by_path_round_trip`). gyre-domain: 363/363. gyre-server spec_registry: 55/55; api::specs: 55/55.
- `cargo build --all` exit 0. `scripts/check-arch.sh`, `check-migration-versions.sh`, `check-migration-sql-portability.sh`, `check-mem-port-contracts.sh` all pass.

Sandbox note: no TCP listener support (capabilities.json), so no live-server HTTP check was performed here; the E2E tests exercise the full axum router (auth middleware → handler → SQLite-backed stores) in-process, which is the strongest available transport-level verification. Exact-head GitHub CI remains mandatory.
