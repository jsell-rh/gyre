# Review — task-193 (Mode-based spec approval status resolution with attestation/stack_hash validity)

Spec: `specs/system/spec-registry.md` §9 Approval Status Resolution (plus §5 agent-approver
stack_hash semantics, §17 `requires_approval: false`).
Comparison base: `59ff5d01069a22108a40cf370780a4e04f36f854` → candidate `98add3efba783be623a736dd65124140d41df72d`.
Diff: 13 files — domain event fields, diesel migration `2026-10-08-000056_spec_approval_attestation`
(next unused sequence, portable `ALTER TABLE ADD COLUMN`), `schema.rs` + SQLite/Postgres row mappings,
`spec_registry.rs` (`read_manifest` + pure resolver + 13 unit tests), `api/specs.rs` (attestation capture
from verified JWT claims, `resolve_new_approval_status` wiring into approve/revoke, transition-gated
`SpecApproved` emission, 4 E2E tests), `constraint_check.rs` (`derive_attestation_level` → `pub(crate)`),
exemption-coordinate/repair-round script updates, task frontmatter. Working tree clean at the candidate;
no uncommitted repairs.

Verdict: **complete**.

## Diff inspection

- **Domain + persistence are real and additive.** `SpecApprovalEvent` gains
  `attestation_level: Option<u32>` / `stack_hash: Option<String>`; migration 000056 is the next
  unused sequence (after 000055, no duplicates — gate exit 0), uses portable `ALTER TABLE … ADD COLUMN`
  (portability gate exit 0), and both SQLite and Postgres adapters map the new columns both directions.
  The mem adapter stores the struct by clone, so fields round-trip there too. No data dropped.
- **Attestation capture is from verified sources, never invented.** `approve_spec` classifies the
  approver by the *verified* JWT's `scope == "agent"` claim (agent JWTs are EdDSA-signed with
  `scope: "agent"` at mint; the auth middleware validates signature+expiry and only honors tokens
  registered in `agent_tokens` before `jwt_claims` is populated — auth.rs:531-556). Agent approvals
  record `stack_hash` from the `wl_stack_hash` claim and `attestation_level` derived from the agent's
  KV workload-attestation record via the same `derive_attestation_level` used by constraint checks
  (levels 0–3 from stack fingerprint / container_id / image_hash). Humans (API key, global token, or
  non-agent-scope JWT) record `None`/`None`.
- **The resolver is pure and correctly specified.** `resolve_approval_status(entry, defaults,
  current_sha, events)`: event valid iff `spec_sha == current_sha` && active; agent validity
  additionally requires persona ∈ `agent_approvers[]`, `attestation_level >= min_attestation_level`
  (default 1), and exact `stack_hash` match when pinned; human validity requires `approver_type ==
  "human"` and `approver_id ∈ human_approvers` when non-empty; modes human_only / agent_only /
  human_and_agent resolved per §9; `requires_approval: false` → Approved (§17). Signature matches
  what tasks 194/195 reuse; `read_manifest(repo_path, sha) -> Option<SpecManifest>` has the exact
  specified signature and `read_manifest_paths` delegates to it.
- **Wiring covers both mutation paths.** `resolve_new_approval_status` feeds the resolver the full
  `list_by_path` history and is called from both `approve_spec` (only when `entry.current_sha ==
  req.sha`) and `revoke_spec_approval` (recompute instead of blind demote — a revoke that leaves a
  valid pair intact no longer demotes). Manifest-less repos fall back to legacy single-approval
  semantics over real events, logged at debug — explicitly allowed by the plan.
- **`SpecApproved` fires only on the transition into Approved** (prior != Approved && new ==
  Approved), so partial approvals of a `human_and_agent` pair no longer trigger the signal chain;
  repeat approvals don't re-emit.
- **Repair-round changes verified as real fixes, not re-exemptions:** the `reject_spec` fanout now
  resolves `tenant_id` from the workspace record and skips + warn-logs when unresolvable
  (`emit_reconciliation_completed` pattern); the `specs.rs:959` exemption entry was deleted and
  `FROZEN_EXEMPTION_COUNT` lowered 5→4 (gate exit 0). The three `spec_registry.rs` in-memory-store
  exemption lines only track the same three pre-existing sites drifted by inserted code; frozen count
  unchanged at 5 (gate exit 0). Drive-by `spec_index` SHA truncation switched to char-boundary-safe
  `get(..8)`, matching the repo's byte-slice invariant.

## Probes (all at `98add3ef`, evidence under `/tmp/stage/review-evidence/`)

- `cargo test -p gyre-server --lib test_resolve` → **13 passed; 0 failed; exit 0** (per-mode,
  attestation-too-low, stack_hash mismatch/missing, stale SHA, revoked, approver-not-in-list,
  requires_approval:false).
- `cargo test -p gyre-server --lib e2e` + `agent_attestation_fields_round_trip_via_history_api` →
  **4 passed; 0 failed; exit 0** — real committed git repos + manifest, real minted EdDSA agent JWTs
  with workload claims registered in `agent_tokens`, real KV workload attestations, real API-key
  human auth, full axum router in-process: `human_and_agent` Pending after agent-only and Approved
  after the human completes the pair; level-2 agent vs min 3 → Pending, level 3 → Approved; wrong
  stack → Pending, pinned stack → Approved; JWT-derived level 3 + `sha256:pinned` persist and read back.
- `cargo test -p gyre-adapters spec_approval` → **3 passed; 0 failed; exit 0** (agent fields
  round-trip, human fields null, list_by_path round-trip).
- **Mutation probes** (each reverted to byte-identical source afterward; `git diff --exit-code`
  clean, `git status` clean at the candidate):
  - attestation minimum check disabled → `test_resolve_agent_only_pending_when_attestation_below_minimum`
    **FAILED** (exit 101) — the test kills the hollow-attestation regression.
  - stack_hash match check disabled → mismatch **and** missing tests **FAILED** (exit 101).
  - mode logic reverted to "any valid approval" (`||` for HumanAndAgent) →
    `test_resolve_human_and_agent_requires_both` **FAILED** (exit 101).
  - (A first stack-hash probe attempt was invalid — my own edit referenced an undefined variable and
    failed to compile; superseded by the corrected probe above. Log retained.)
- Gates: `check-arch.sh`, `check-fabricated-scope-defaults.sh`, `check-in-memory-state-stores.sh`,
  `check-task-commit-attribution.sh`, `check-migration-versions.sh`,
  `check-migration-sql-portability.sh` — all **exit 0**.

Sandbox note: no TCP listener support (capabilities.json), so no live-server HTTP probe; the E2E
tests exercise the full axum router (auth middleware → handler → SQLite-backed stores) in-process,
the strongest available transport-level verification here. Exact-head GitHub CI remains mandatory.

## Minor observations (not blocking, recorded for completeness)

- `persona` on approval events remains caller-supplied (`req.persona`) — pre-existing at the base,
  not introduced by this diff, and the agent JWT carries no persona claim. Agent-approval validity
  still requires the verified `attestation_level` and `stack_hash`, so persona self-declaration alone
  cannot approve a constrained spec; binding persona to a verified claim (JWT claim or agent-record
  lookup) is a natural follow-up task.
- The resolver reads the manifest at `HEAD` rather than at `entry.current_sha` — correct deviation
  from the plan's literal wording: `current_sha` is a blob SHA, not a commit, so reading the manifest
  "at" it would always fail. Documented in the code.
- Full workspace suite and all-target Clippy deferred to the controller's verification/CI gates,
  consistent with the task instruction to run touched-crate tests plus `check-arch.sh` only.

## Coverage section closure

`spec-registry.md` §9 (coverage row 9, `task-assigned` → task-193): the specced mode-based
resolution with attestation/stack_hash validity is now enforced in production code
(`approve_spec`/`revoke_spec_approval` → `resolve_new_approval_status` → pure
`resolve_approval_status`), backed by persisted attestation fields and mutation-proven unit + E2E
tests. The row is ready for the auditor to promote (rows 6 and 10's "hollow §9" Partial notes also
reference this closure).
