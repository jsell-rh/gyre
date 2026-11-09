# Review — task-165 (Supply chain — gyre-stack.lock parsing and attestation level enforcement)

Spec: `specs/system/supply-chain.md` §gyre-stack.lock, §Attestation Levels, §Level 3: Gyre-Managed Runtime, §Policy per Level.
Candidate: `7e16a555c21e03a01e88df733048385a59c4e0a3` (assigned base `fefde2946f3a0af1f212588f54591516fee0f7b2`), includes repair rounds through `57bc4ea7e` (rustfmt) and `27227f6b6` (§7.5 payload schema).
Verdict: **approved**.

## Evidence (all probes at the exact candidate; logs under `/tmp/stage/review-evidence/`)

Mechanical invariants (all exit 0): check-arch, check-event-payload, check-migration-versions, check-migration-sql-portability, check-relative-path-defaults, check-scope-literal-defaults, check-fabricated-scope-defaults, check-lossy-secret-conversion, check-forged-scope-fields, check-forwarded-header-trust, check-in-memory-state-stores, check-inert-enforcement, check-dead-message-kinds, check-mem-port-contracts, check-abac-route-registry, check-mcp-write-tools, check-byte-slice-truncation, check-fail-open-ref-resolution, check-task-commit-attribution. `check-rustfmt-diff.py` vs base: changed lines clean (11 files).

Focused suites (via `/tmp/stage/bin/cargo`): `gyre-domain stack` 9/9; `gyre-server api::stack_attest` 14/14; `gyre-server git_http::` 46/46 (all 8 task-165 push tests); `gyre-server constraint_check` 31/31 (consumes the changed `parse_stack_policy`); `gyre-adapters sqlite::agent` 17/17 (migration 000059 applies, column in create/update/select); `gyre-cli --test ws_integration` 1/1 (the previously sandbox-restricted listener test passes here — current `capabilities.json` reports listeners supported); `cargo check -p gyre-cli` clean.

Independent kill-checks (temporary edit → test → `git checkout --` restore; tree verified clean, 0 modified, after each):

1. min-level check disabled (`if false && level < policy.min_attestation_level`) → `push_below_min_attestation_level_blocked` FAILS (killcheck-min-level.log).
2. drift branch disabled → `lockfile_drift_blocked_in_block_mode` FAILS (killcheck-drift.log).
3. L3 container-identity branch disabled → `level3_detected_from_workload_attestation` FAILS (killcheck-level3.log).

The block-mode tests are not self-confirming: each enforcement branch, when disabled, is caught.

## Verified against the task contract

- **Lockfile schema/parse**: `StackLockfile` (gyre-domain/src/stack.rs) — TOML round-trip, `verify_integrity` (fingerprint recomputation), `check_drift` (names differing components); 9 unit tests.
- **`gyre stack lock` CLI**: fetches `GET /agents/:id/stack`, writes TOML lockfile; compiles clean.
- **Level 1/2/3 resolution**: `resolve_push_attestation_level` (git_http.rs) — L3 via spawn-written workload attestation with container identity (real production path, spawn.rs M19.4) or JWT wl claims; L2 via registered stack; L1 otherwise; stored on the agent record (migration 000059, exposed on GET /agents/:id) and mapped to provenance labels (fixes the old mislabeling where stack+policy ⇒ "server-verified").
- **Policy per level**: `RepoStackPolicy.min_attestation_level` (1..=3 validated) + block/warn enforcement mode; below-minimum → 403 + `undo_ref_updates` + PushRejected (block, kill-checked) or full §7.5-schema ConstraintViolation event + accept (warn; test asserts schema, both sides of the comparison, stored level).
- **Drift detection**: lockfile read from the pushed tree via `read_git_file` at the new SHA, integrity-checked, compared against the pushing agent's attested stack; not policy-gated (no-policy lockfile → warn-mode flag, tested); kill-checked.

## Non-blocking observations (recorded for completeness)

- No dedicated SQLite/Postgres assertion of a non-None `attestation_level` value round-trip; the mem-backed git_http tests prove value persistence (Some(1)/Some(2)/Some(3)) and the sqlite tests prove schema/migration plus the column in every statement. Mapping is trivial `Option<u8>`↔`Option<i32>`, identical in both adapters.
- `parse_stack_policy` syncs `required_level = min_attestation_level` (documented on the struct; the PUT handler normalizes both), so the two level fields cannot diverge via the API — a deliberate, documented design decision.
- Known partials are honestly disclosed in coverage rows 7/8/9: lockfile not approver-signed (task plan's schema has no signature field), L3 trust basis is server-observed container identity rather than SPIFFE/eBPF (plan scoped L3 detection to the container-identity check), and the JWT workload-claim branch has no production minting site (defense-in-depth). All consistent with the task's implementation plan; none are hidden.

`cargo test --all` is owned by the verification stage; the focused suites above cover every task-touched surface.
