# Review — task-193 (Mode-based spec approval status resolution with attestation/stack_hash validity)

Spec: `specs/system/spec-registry.md` §9 Approval Status Resolution (mode table + approval-validity
rules), with §5 stack_hash claim wording and §17 `requires_approval: false`.
Assigned base `73a31e0b`, candidate `fa73443b4b362ce5cde3fb590c4703033a42fe4e`.
Commits under review (scoping per `commits:` frontmatter): `1e55eef`, `b1ead35`, `5d4ebb1`,
`e42c374`, `0e6c1b8`, `56b51b3` (plus process commits for the merge with the updated base and the
dist revert).
Verdict: **complete**.

## Durable finding from the prior round — resolved

The accidental `web/dist` bundle rebuild is gone: `git diff 73a31e0b fa73443b -- web/` is empty
(candidate dist bit-identical to base), and `git diff --check 73a31e0b fa73443b` exits 0.
Task-193's own source files are bit-identical to the previously reviewed candidate `c7e47667`
(0-line diffs on all seven touched source files); the only deltas since are the base merge and the
dist revert.

## Round 3 probes (this checkout, branch `pipeline/task-193/361900e1…-539` at `fa73443b`)

All logs under `/tmp/stage/review-evidence/`.

- `cargo build --all` → exit 0.
- `cargo test -p gyre-server --lib test_resolve` → **13 passed, 0 failed** (per-mode,
  attestation-too-low, persona-not-configured, stack_hash-mismatch, stack_hash-missing,
  stale-sha, revoked, requires_approval:false).
- `cargo test -p gyre-server --lib -- api::specs::tests` → **55 passed, 0 failed**, incl. the four
  new E2E tests (`human_and_agent_requires_both_approvals_e2e`,
  `agent_only_rejects_attestation_below_minimum_e2e`,
  `agent_only_requires_exact_stack_hash_match_e2e`,
  `agent_attestation_fields_round_trip_via_history_api`) which drive the real axum router with a
  real committed git repo (real `git rev-parse HEAD:specs/…` blob SHAs), real Ed25519-minted agent
  JWTs registered in `agent_tokens` (so auth-middleware signature validation + revocation check
  runs), and real KV-stored workload attestations.
- `cargo test -p gyre-adapters --lib spec_approval_event` → **3 passed, 0 failed**
  (`agent_attestation_fields_round_trip`, `human_approval_keeps_attestation_fields_null`,
  `record_and_list_by_path_round_trip`) — SQLite persistence of the new columns.
- `cargo test -p gyre-domain` → **363 passed, 0 failed**.
- Gates: `check-arch.sh`, `check-migration-versions.sh`, `check-migration-sql-portability.sh`,
  `check-mem-port-contracts.sh`, `check-task-commit-attribution.sh` → all OK.
- Migration `2026-10-08-000056_spec_approval_attestation` is the next unused sequence
  (…000055 is the previous); portable `ALTER TABLE … ADD COLUMN`, no data dropped; both SQLite
  and Postgres row mappings updated in `schema.rs`.

**Mutation probe (the decisive test-quality check).** With `resolve_approval_status` temporarily
reverted to the legacy "any active approval for current_sha → Approved" semantics (file restored
afterwards — md5 verified identical, `git status` clean):

- 7 of 13 resolver unit tests **FAIL** (`test_resolve_human_only_pending_when_only_agent_approved`,
  `test_resolve_human_and_agent_requires_both`,
  `test_resolve_agent_only_pending_when_attestation_below_minimum`,
  `test_resolve_agent_only_pending_when_persona_not_configured`,
  `test_resolve_agent_only_pending_when_stack_hash_mismatches`,
  `test_resolve_agent_only_pending_when_stack_hash_missing`,
  `test_resolve_human_only_pending_when_approver_not_in_list`).
- 3 E2E tests **FAIL** (`human_and_agent_requires_both_approvals_e2e`,
  `agent_only_rejects_attestation_below_minimum_e2e`,
  `agent_only_requires_exact_stack_hash_match_e2e`).

So both the unit suite and the router-level E2E suite are genuinely anchored to the §9 mode
logic — reverting the enforcement is caught at both layers, not just the pure function.

Note: `cargo build --all` without `SKIP_WEB_BUILD=1` rebuilds `web/dist` as a side effect in this
checkout (build.rs). That is an environment artifact of the probe, not part of the candidate; the
working tree was restored (`git checkout -- web/dist && git clean -fd web/dist`) and verified clean
before recording the verdict. This is also plausibly the mechanism behind the original accidental
dist rebuild in the prior round.

## Verified working (no findings)

- **`SpecApprovalEvent` carries and persists `attestation_level`/`stack_hash`.** Domain struct,
  `SpecApprovalEventResponse` (history API), SQLite + Postgres diesel rows and `schema.rs` all map
  the two new nullable columns via migration `2026-10-08-000056`. Round-trip proven at the adapter
  layer (SQLite) and through the history API in the E2E test. Mem adapter stores the struct
  (`check-mem-port-contracts.sh` OK).
- **Attestation values are captured, never invented.** In `approve_spec`, agent-ness is decided by
  verified JWT `scope == "agent"` (agent JWTs are additionally signature/expiry-validated and
  checked against `agent_tokens` for revocation by the auth middleware before the handler runs —
  auth.rs:531-567). `stack_hash` comes from the verified `wl_stack_hash` claim;
  `attestation_level` is derived via the same `derive_attestation_level` used by constraint
  checks from the agent's KV-stored workload attestation record (0-3 supply-chain levels). Humans
  (API-key, global token, non-agent-scope Keycloak JWT) record `None`/`None`. An agent with no
  attestation record derives level 0, which fails every `min_attestation_level` (default 1) —
  fail-closed.
- **Pure resolver matches §9 exactly** (`spec_registry.rs::resolve_approval_status`): valid =
  `spec_sha == current_sha` && active; agent validity additionally requires persona ∈
  `agent_approvers`, `attestation_level >= min_attestation_level` (default 1), and exact
  `stack_hash` match when pinned (missing recorded hash also fails); human validity requires
  `approver_type == "human"` and `approver_id ∈ human_approvers` when non-empty. Modes:
  `human_only` → ≥1 valid human; `agent_only` → ≥1 valid agent; `human_and_agent` → both.
  `requires_approval: false` → Approved (§17). Pure over `(SpecEntry, ManifestDefaults, sha,
  events)` — no DB, unit-testable, as the task instructions require.
- **Handler wiring is correct.** `approve_spec` and `revoke_spec_approval` both recompute status
  via `resolve_new_approval_status` over the full `list_by_path` history (so a revoke that removes
  the last valid approval demotes to Pending, and revoking one half of a satisfied
  human_and_agent pair keeps the other half's status correct). Manifest is read at `HEAD` —
  correct, since `current_sha` is a blob SHA that cannot address `specs/manifest.yaml`; the
  plan's `read_manifest(repo_path, sha)` helper exists with the exact signature reserved for
  tasks 194/195, and `read_manifest_paths` delegates to it. Manifest-less repos fall back to the
  legacy single-active-approval semantics over real events, logged at debug, per plan.
- **`SpecApproved` fires only on the transition into Approved** — partial approvals no longer
  trigger the signal chain; the emit is gated on `prior_status != Approved && new == Approved`.
- **`read_manifest` signature** is exactly `read_manifest(repo_path: &str, sha: &str) ->
  Option<SpecManifest>` as required for tasks 194/195.

## Non-blocking observations (recorded for the verifier, no action required)

- The "Shipped" narrative in `specs/tasks/task-193.md` describes the E2E tests as running against
  "SQLite-backed stores"; `test_state()` actually wires mem adapters for `spec_ledger` /
  `spec_approval_history`. The mislabel does not weaken the evidence: SQLite persistence of the
  new columns is proven by the dedicated adapter round-trip tests, and the handler/resolver
  behavior is transport-proven through the real router. Flagged for narrative accuracy only.
- `persona` on approval events remains caller-supplied (pre-existing behavior, retained per the
  task plan; the plan only mandates JWT capture for `attestation_level`/`stack_hash`). An agent
  cannot exploit this alone: attestation level and pinned stack hash are derived from
  verified-token/KV state, not the request body.
- No live-server HTTP probe was run: the sandbox has no TCP listener support (capabilities.json).
  The in-process router E2E tests are the strongest available transport-level verification;
  exact-head GitHub CI remains the controller's independent check.

## Reproduction

```
git diff 73a31e0b fa73443b -- web/          # empty — durable finding resolved
git diff --check 73a31e0b fa73443b          # exit 0
cargo test -p gyre-server --lib test_resolve       # 13 passed
cargo test -p gyre-server --lib -- api::specs::tests  # 55 passed
cargo test -p gyre-adapters --lib spec_approval_event # 3 passed
bash scripts/check-arch.sh && bash scripts/check-migration-versions.sh \
  && bash scripts/check-migration-sql-portability.sh \
  && bash scripts/check-mem-port-contracts.sh \
  && bash scripts/check-task-commit-attribution.sh   # all OK
# mutation: revert resolve_approval_status to "any active approval for sha" →
#   7/13 resolver unit tests + 3/4 E2E tests FAIL (see mutation-*.log)
```
