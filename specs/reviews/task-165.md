# Review — task-165 (Supply chain — gyre-stack.lock parsing and attestation level enforcement)

Spec: `specs/system/supply-chain.md` §gyre-stack.lock, §Attestation Levels, §Level 3, §Policy per Level.
Candidate: `0b0449c36eb3f5c00af2b749a87a7bc52a0acd4a` (base `b5a872087899e8bc4a18092ab56990a26e9ca9af`).
Verdict: **approved** — independent probes and three kill-checks confirm real enforcement; no blocking findings.

## Probes (all at the exact candidate HEAD; logs under `/tmp/stage/review-evidence/`, prefix `task165-rv-`)

- `cargo test -p gyre-server git_http::` → **62 passed, 0 failed** — includes all eight
  task-165 push tests (below-minimum block + warn, meeting-minimum accept +
  level stored, L3-from-workload-attestation, drift block/match/no-policy-warn,
  tampered-lockfile integrity rejection) and the base's ledger/lineage tests.
- `cargo test -p gyre-server api::stack_attest` → **14 passed**; `cargo test -p gyre-domain stack` →
  **9 passed**; `cargo test -p gyre-adapters sqlite::agent` → **17 passed**; `cargo test -p gyre-cli` →
  **150 passed**; `cargo test -p gyre-cli --test ws_integration test_auth_and_ping_roundtrip` →
  **1 passed** (this sandbox permits TcpListener — the shipped notes' errno-95 concern was
  environment-specific and does not reproduce here).
- Static gates at the candidate — arch, event-payload, migration-versions,
  migration-sql-portability, abac-route-registry, inert-enforcement,
  dead-message-kinds, mcp-write-tools, mem-port-contracts, relative-path-defaults,
  fabricated-scope-defaults, scope-literal-defaults, lossy-secret-conversion,
  fail-open-ref-resolution, in-memory-state-stores, forged-scope-fields,
  task-commit-attribution — **all exit 0**.
- `cargo fmt -p gyre-server -p gyre-cli -p gyre-domain -p gyre-adapters -- --check` → clean;
  `python3 scripts/check-rustfmt-diff.py <base>` → exit 0 ("changed lines clean, 11 Rust files
  checked" — the CI fmt job's exact check). Workspace-wide `cargo fmt --all` is rejected by the
  executor's wrapper here; recorded as deferred to verification, not a code issue.

## Kill-checks (each temporary edit reverted; `git status` clean, HEAD unchanged afterwards)

1. **Min-level comparison disabled** (`if false && level < policy.min_attestation_level`):
   `push_below_min_attestation_level_blocked` and
   `push_below_min_level_warn_mode_flagged_and_accepted` both **FAIL** (log:
   `task165-rv-killcheck-minlevel.log`).
2. **Drift check disabled** (guard forced false): `lockfile_drift_blocked_in_block_mode`,
   `lockfile_drift_without_policy_warned_and_accepted`, and
   `tampered_lockfile_rejected_by_integrity_check` all **FAIL** (log:
   `task165-rv-killcheck-drift.log`).
3. **Level resolution neutralized** (forced L1): `level3_detected_from_workload_attestation` and
   `push_meeting_min_level_accepted_and_level_stored` both **FAIL** (log:
   `task165-rv-killcheck-level3.log`).

The tests are not self-confirming — each production behavior under test is load-bearing for its test.

## Verified working

- **Lockfile schema** (`gyre-domain/src/stack.rs`): real TOML schema (fingerprint, agents_md_hash,
  hooks, mcp_servers, model, cli_version, settings_hash, persona_hash, lock_timestamp) with
  parse/to_toml round-trip, `verify_integrity()` (fingerprint recomputation), and `check_drift()`
  naming differing components. Placement in gyre-domain gives CLI and server one fingerprint
  definition (the server's former private copy was replaced by a re-export, not duplicated).
- **`gyre stack lock`** (`gyre-cli/src/main.rs`): fetches the registered stack from the existing
  `GET /api/v1/agents/:id/stack` endpoint (verified registered in `api/mod.rs`) and writes the
  lockfile. `cargo check -p gyre-cli` clean; CLI suite 150/150.
- **Level resolution** (`git_http.rs` `resolve_push_attestation_level`): L3 = server-recorded
  workload attestation with container identity — real production wiring, `spawn.rs` writes
  `workload_attestations` with `container_id`/`image_hash` for container compute targets
  (`attest_agent_with_container`); the JWT `wl_container_id`+`wl_image_hash` branch exists as a
  secondary path (claims defined in `AgentJwtClaims` but not minted in production — honestly
  recorded in coverage row 9 and shipped notes). L2 = registered stack in KV `agent_stacks`;
  L1 = default. Stored on the agent record via migration `2026-10-10-000059` (next unused
  sequence confirmed by the migration-versions gate), round-trips through sqlite + postgres
  adapters, exposed on `GET /api/v1/agents/:id`.
- **Per-level policy** (`api/stack_attest.rs`): `RepoStackPolicy.min_attestation_level` (1..=3,
  validated, 400 on bad input) + `enforcement: block|warn` (validated), Admin-only PUT (403 +
  RBAC-denied audit for non-Admin, pre-existing NEW-39 regression test retained), legacy KV
  entries derive the fields from `required_level` (tested).
- **Push enforcement** (`git_http.rs` `enforce_stack_policy`): below-minimum → Block: 403 +
  `undo_ref_updates` + PushRejected event (the block test verifies the ref is actually deleted);
  Warn: accept + ConstraintViolation event carrying the full authorization-provenance §7.5
  payload schema (attestation_id, constraint_name, expression, context_snapshot, action,
  timestamp — asserted field-by-field by a shared helper; event-payload gate exit 0).
- **Drift detection**: lockfile read from the pushed tree via `read_git_file` at the new SHA,
  integrity-checked (tampered component + stale fingerprint → rejected in block mode), then
  compared against the pushing agent's attested stack. Not gated on a repo policy — a lockfile
  without a policy gets warn-mode flagging (ConstraintViolation + accept), tested.
- **Coverage honesty**: rows 7/8/9 record the real partials (lockfile not approver-signed; L3 is
  server-observed container identity, not SPIFFE/eBPF; JWT workload-claim minting unwired) rather
  than claiming full spec depth. These match the task's implementation plan, which scoped the
  lockfile schema to the component list (no signatures) and L3 detection to the
  container-identity check.

## Non-blocking notes (design choices within the task plan, no action required)

- `set_stack_policy` stores `required_level` and `min_attestation_level` as one synced value, so
  an admin cannot set them divergently; the sync is documented in-code and the legacy derivation
  (required_level 3 → min 3) is the stated intent.
- The drift comparison runs only when the pusher has an attested stack (L2+); an L3 container
  agent with no registered `agent_stacks` entry skips it — consistent with plan step 5, which
  compares the lockfile against "the submitted stack attestation" (no attestation → nothing to
  compare).
- `cargo test --all` is deferred to the verification stage per the review protocol
  (workspace-wide invocations are wrapper-rejected here); focused per-package suites covering
  every touched crate are green at the candidate.
