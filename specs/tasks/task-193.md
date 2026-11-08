---
title: "Mode-based spec approval status resolution with attestation/stack_hash validity"
spec_ref: "spec-registry.md §9 Approval Status Resolution"
depends_on: []
progress: complete
coverage_sections:
  - "spec-registry.md §9"
commits: ["a98676935a6c19fae80377276d37b8378b1ac9b1"]
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

§9 mode-based approval resolution is implemented and verified through the repair rounds below (current head `a9867693` on assignment base `9bef21a4` — the worker lease expired after round 2 and the controller recovered the round-2 source into checkpoint commit `a9867693`; §9 crate content is unchanged since the reviewed checkpoint `3e467f37` — subsequent commits are merges, process records, verification repairs, and the source recovery only).

**Behavior:**
- `SpecApprovalEvent` now carries `attestation_level: Option<u32>` and `stack_hash: Option<String>` (`gyre-domain/src/spec_ledger.rs`), persisted via diesel migration `2026-10-08-000057_spec_approval_attestation` (portable `ALTER TABLE ADD COLUMN`, both SQLite + Postgres adapters map the new columns; mem adapter stores the struct). Schema updated, no data dropped. (Authored as 000056, renumbered to 000057 in the repair round below — the merged base shipped its own 000056 `spec_assertion_results`.)
- `approve_spec` (`api/specs.rs`) captures agent attestation from the approver's **verified JWT claims**: `stack_hash` from the `wl_stack_hash` claim; `attestation_level` derived from the agent's real workload-attestation record via the same `derive_attestation_level` used by constraint checks (supply-chain levels 0–3). Humans (API-key/global-token auth, or non-agent-scope JWT) record `None`/`None`. Values are never invented.
- New `read_manifest(repo_path, sha) -> Option<SpecManifest>` in `spec_registry.rs` (signature as specified for tasks 194/195; `read_manifest_paths` delegates to it). The resolver reads the manifest at `HEAD` because `current_sha` is a blob SHA, not a commit.
- Pure resolver `resolve_approval_status(entry, defaults, current_sha, events)` in `spec_registry.rs`: event valid iff `spec_sha == current_sha` && active; agent validity additionally requires persona ∈ `agent_approvers`, `attestation_level >= min_attestation_level` (default 1), and exact `stack_hash` match when pinned; human validity requires `approver_type == "human"` and `approver_id ∈ human_approvers` when that list is non-empty. Modes: `human_only` → ≥1 valid human; `agent_only` → ≥1 valid agent; `human_and_agent` → both. `requires_approval: false` → Approved regardless (§17). Wired into `approve_spec` and `revoke_spec_approval` via `resolve_new_approval_status` (manifest-less repos fall back to legacy single-approval semantics, logged at debug).
- `SpecApproved` message-bus event fires only on the transition *into* Approved (partial approvals of a `human_and_agent` pair no longer trigger the signal chain).

**Test evidence:**
- 13 resolver unit tests pass, including per-mode tests that fail under a reverted "any valid approval" resolver, attestation-too-low (`test_resolve_agent_only_pending_when_attestation_below_minimum`), stack_hash-mismatch and stack_hash-missing.
- 4 E2E tests through the real HTTP router with real committed git repos and real minted agent JWTs + workload attestations: `human_and_agent_requires_both_approvals_e2e` (Pending after agent-only, Approved after human completes the pair), `agent_only_rejects_attestation_below_minimum_e2e` (level 2 vs min 3 → Pending; level 3 → Approved), `agent_only_requires_exact_stack_hash_match_e2e` (wrong stack → Pending; pinned stack → Approved), `agent_attestation_fields_round_trip_via_history_api` (JWT-derived level 3 + `sha256:pinned` persist and read back).
- Adapters round-trip: 3/3 (`agent_attestation_fields_round_trip`, `human_approval_keeps_attestation_fields_null`, `record_and_list_by_path_round_trip`).

Sandbox note: no TCP listener support (capabilities.json), so no live-server HTTP check was performed here; the E2E tests exercise the full axum router (auth middleware → handler → SQLite-backed stores) in-process, which is the strongest available transport-level verification. Exact-head GitHub CI remains mandatory.

### Verification-repair rounds

**Round 1 (`c143b504`, vs base `59ff5d01`)** — the controller verification run flagged four finding classes; all repaired:

- **rustfmt** (3 files) and **clippy** (`obfuscated_if_else` on the resolver's `match {}.then_some().unwrap_or()` chain; `cloned_ref_to_slice_refs` on `&[ev.clone()]` test slices): fixed; `check-rustfmt-diff.py` and `check-clippy-diff.py` both exit 0 ("changed lines clean").
- **fabricated-scope-defaults** (`specs.rs` reject_spec fanout `entry.repo_id.unwrap_or("default")` — legacy site whose exemption drifted when task-193 inserted code above it): fixed for real, not re-exempted — the spec-rejected notification fanout now resolves `tenant_id` from the workspace record via `workspaces.find_by_id` (the `emit_reconciliation_completed` pattern) and skips + warn-logs when the scope cannot be determined. The `specs.rs:959` exemption entry was deleted and `FROZEN_EXEMPTION_COUNT` lowered 5 → 4, so any re-introduction fails at any line.
- **in-memory-state-stores**: exemption coordinates updated to track the three pre-existing sites (superseded by round 2 below).

Post-repair gates at `c143b504`: `cargo build --all` exit 0; `cargo test -p gyre-server --lib` 1259/1259; `cargo test -p gyre-adapters spec_approval` 3/3; all repo script gates pass. `check-unnamed-tuple-carriers.sh` has pre-existing unexempted hits at both HEAD and the assignment base and is run by neither the verification suite nor CI — left untouched.

**Round 2 (`4bb14cef`, vs base `9bef21a4`)** — after the branch was merged forward onto base `9bef21a4` (merge `145ae217`), the controller verification run (attempt `d9823d48`) flagged two finding classes; both repaired:

- **Duplicate Diesel migration version `20261008000056`**: the merged base ships `2026-10-08-000056_spec_assertion_results`, so the task-193 `spec_approval_attestation` migration was silently dropped (alphabetically-first wins) — its columns never existed on fresh databases. Renumbered to the next unused sequence `2026-10-08-000057_spec_approval_attestation`; both migrations now run (`check-migration-versions.sh` and `check-migration-sql-portability.sh` exit 0).
- **in-memory-state-stores exemption drift**: task-193's resolver section was inserted above the three pre-existing legacy `spec_registry.rs` store aliases, drifting their frozen coordinates 218/220/249 → 333/335/364, which the verification exemption-diff gate rejects as new exemptions. Fixed without touching the frozen file: the resolver section (`read_manifest_paths` delegate, `resolve_approval_status`, `is_valid_human_approval`, `is_valid_agent_approval`) was relocated below the `SpecLinksStore` alias so the aliases sit at their base coordinates 218/220/249 again, and `scripts/in-memory-state-stores-exemptions.txt` was restored to byte-identical base content (frozen count stays 5; porting these legacy stores to adapters remains separate scheduled work, not §9).

Round-2 evidence (logs in /tmp/stage/review-evidence/):

- The relocation is a pure code movement: a difflib multiset proof over the pre/post files shows the only content delta is rebinding `parse_manifest(&yaml)` into a local before the `match` (semantically identical); everything else is byte-identical moved lines. rustfmt parses the file and `check-rustfmt-diff.py 9bef21a4` exits 0 (the single pre-existing rustfmt hit at `resolve_git` is identical in the base).
- All compile-free repo gates pass at `4bb14cef`: check-arch, check-hierarchy, abac-route-registry, abac-exempt-handlers, mcp-write-tools, dead-message-kinds, byte-slice-truncation, relative-path-defaults, fail-open-ref-resolution, mem-port-contracts, fabricated-scope-defaults, lossy-secret-conversion, scope-literal-defaults, inert-enforcement, forged-scope-fields, forwarded-header-trust, unbounded-external-http, in-memory-state-stores, migration-versions, migration-sql-portability — all exit 0.

**Round-2 sandbox transport restriction (recorded for remote verification):** this sandbox instance's egress allowlist permits registry.npmjs.org and github.com but hard-blocks index.crates.io / static.crates.io (`curl https://index.crates.io/config.json` → exit 7, "Immediate connect fail ... Permission denied"), and no local cargo registry cache or vendored sources exist, so `cargo build`/`cargo test` probes could not download the 399 locked crates. This is an infrastructure restriction, not a code defect. Because the round-2 diff is a verified pure line-movement plus a directory rename plus an exemption-file restore to base content, the compile-level risk is nil (Rust items are order-independent; the moved block's imports are unchanged), but the touched-crate suites were NOT re-run in this round and remain owned by deterministic verification: `cargo build --all`, `cargo test -p gyre-server --lib`, `cargo test -p gyre-adapters spec_approval`, and `check-clippy-diff.py 9bef21a4` must be confirmed at exact head by the controller.

**Round 3 (`a9867693`, checkpoint recovery, vs base `9bef21a4`)** — the worker lease expired after round 2; the controller recovered the source into the single checkpoint commit `a9867693` on this branch (the pre-recovery commit chain `1e55eef…4bb14cef` is no longer reachable from the branch; only `c143b504` survives as a loose object). The recovery was verified content-equivalent to the round-2 verified state, not assumed:

- Diff `c143b504` (round-1 verified head, which passed `cargo build --all`, 1259/1259 server tests, 3/3 adapter tests) vs `a9867693` over all §9 files (`spec_registry.rs`, `spec_ledger.rs`, `api/specs.rs`, `constraint_check.rs`, `schema.rs`, both `spec_approval_event.rs` adapters, migrations): `specs.rs`/`spec_ledger.rs`/adapters/constraint_check are byte-identical (0 changed lines); the only `spec_registry.rs` delta is the documented round-2 pure relocation of the resolver section below the `SpecLinksStore` alias plus the semantically-identical `parse_manifest` rebinding; the only schema/migration deltas are the merged base's own `spec_assertion_results` additions; the migration renumber `000056→000057` is present. The full product surface now lives solely in `a9867693`, so that SHA was appended to the `commits:` frontmatter for review scoping.
- All compile-free repo gates re-run at `a9867693`, all exit 0: check-arch, check-hierarchy, abac-route-registry, abac-exempt-handlers, mcp-write-tools, dead-message-kinds, byte-slice-truncation, relative-path-defaults, fail-open-ref-resolution, mem-port-contracts, fabricated-scope-defaults, lossy-secret-conversion, scope-literal-defaults, inert-enforcement, forged-scope-fields, forwarded-header-trust, unbounded-external-http, in-memory-state-stores, migration-versions, migration-sql-portability, plus `check-rustfmt-diff.py 9bef21a4` ("changed lines clean"). Logs in /tmp/stage/review-evidence/.
- The round-2 sandbox transport restriction still holds at exact re-probe (`curl https://index.crates.io/config.json` → exit 7, connection refused; no cargo registry cache), so cargo-level suites remain owned by deterministic verification at exact head: `cargo build --all`, `cargo test -p gyre-server --lib`, `cargo test -p gyre-adapters spec_approval`, `check-clippy-diff.py 9bef21a4`. Since the §9 crate content is byte-identical to the `c143b504` state that passed all of these, the compile risk is nil, but confirmation remains mandatory.
