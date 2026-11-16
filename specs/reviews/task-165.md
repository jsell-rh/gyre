# Review: task-165 — Supply chain (gyre-stack.lock + attestation level enforcement)

**Candidate:** `afdc8d2a380440ce419531f2e01a4356ff1b93ff` (base `cfeefbda975211a6ac24a17e5920be6f8b23008b`)
**Reviewer:** independent review round (pi runtime), 2026-11-11
**Verdict:** approved — task contract satisfied by real production code; no blocking findings.

## What was reviewed

Full diff base→candidate (18 files, ~1.8k insertions): `gyre-domain/src/stack.rs`
(new), `gyre-server/src/git_http.rs` (enforcement + 8 push tests),
`gyre-server/src/api/stack_attest.rs` (policy schema), `gyre-server/src/api/agents.rs`,
adapters (sqlite/postgres/mem agent round-trip + migration 000059),
`gyre-cli` (`gyre stack lock`), docs, coverage matrix.

## Independent evidence (saved under /tmp/stage/review-evidence)

- `cargo test -p gyre-domain --lib` → **472 passed** (incl. 9 new stack tests:
  TOML round-trip, tamper/integrity, drift component-naming, invalid TOML).
- `cargo test -p gyre-adapters --lib sqlite` → **311 passed** (fresh-DB
  migration runs include new `000059_agent_attestation_level`;
  `no_duplicate_migration_version_prefixes` green).
- `cargo check -p gyre-server --tests` and `-p gyre-cli` → clean (server test
  compilation deferred to the verifier per 4 GiB budget rule).
- Mechanical invariants all OK: check-arch, check-migration-versions,
  check-migration-sql-portability, check-abac-route-registry,
  check-inert-enforcement, check-scope-literal-defaults,
  check-relative-path-defaults, check-fabricated-scope-defaults,
  check-lossy-secret-conversion, check-byte-slice-truncation,
  check-in-memory-state-stores, check-dead-message-kinds,
  check-unbounded-external-http, check-mem-port-contracts.

## Findings on the contract

1. **Lockfile schema** — `StackLockfile` (gyre-domain) captures exactly the
   task-plan fields (agents_md_hash, hooks, mcp_servers, model, cli_version,
   settings_hash, persona_hash, lock_timestamp + composite fingerprint);
   real TOML parse/serialize, real SHA-256 fingerprint recompute for
   integrity, drift check names differing components. Domain placement is
   right: CLI and server share the same fingerprint code.
2. **`gyre stack lock`** — real CLI command fetching the registered stack via
   `GET /agents/:id/stack` and writing the lockfile. Thin glue over tested
   domain code; no dedicated CLI test (non-blocking observation below).
3. **Level 1/2/3 resolution** — `resolve_push_attestation_level` (git_http.rs):
   L3 from the server-side spawn-time workload attestation (container_id +
   image_hash, written by spawn.rs for container targets) or verified JWT
   claims; L2 from a registered stack; L1 otherwise. Level persisted on the
   agent record (migration 000059, all three adapters round-trip — verified
   on SQLite at runtime) and exposed via `GET /agents/:id`.
4. **Policy per level** — `RepoStackPolicy.min_attestation_level` (1..=3) +
   `enforcement` (block default / warn), validated on PUT (out-of-range level
   and unknown mode → 400), legacy entries derived conservatively. Enforced
   on every push to a policy-bearing repo (not gate-opt-in): block → 403 +
   refs undone + PushRejected event; warn → full §7.5 ConstraintViolation
   event + accept. The tests assert response codes, rejection bodies, ref
   deletion after undo, event payload schema, and stored levels — they fail
   if the enforcement is disabled (not self-confirming).
5. **Drift detection** — lockfile read from the pushed tree via
   `read_git_file`, integrity-checked, compared to the pushing agent's
   attested stack; correctly ungated from repo policy (warn-mode flag when
   no policy exists, matching the spec's lockfile-as-verification framing).
   Tampered-lockfile rejection covered.

## Tests the verifier must execute (gyre-server suite, deferred per budget)

```
cargo test -p gyre-server --lib git_http::tests::push_below_min_attestation_level_blocked
cargo test -p gyre-server --lib git_http::tests::push_below_min_level_warn_mode_flagged_and_accepted
cargo test -p gyre-server --lib git_http::tests::push_meeting_min_level_accepted_and_level_stored
cargo test -p gyre-server --lib git_http::tests::level3_detected_from_workload_attestation
cargo test -p gyre-server --lib git_http::tests::lockfile_drift_blocked_in_block_mode
cargo test -p gyre-server --lib git_http::tests::lockfile_match_accepted
cargo test -p gyre-server --lib git_http::tests::lockfile_drift_without_policy_warned_and_accepted
cargo test -p gyre-server --lib git_http::tests::tampered_lockfile_rejected_by_integrity_check
cargo test -p gyre-server --lib api::stack_attest
cargo test --all   # task acceptance criterion
```

## Non-blocking observations (recorded, not defects against the task contract)

- **Migration numbering**: `2026-10-10-000059_agent_attestation_level` reuses
  sequence 000059 already used by base's `2026-11-08-000059_notification_entity_source`.
  Diesel versions differ (date-prefixed), both migrations run — proven by the
  fresh-DB adapter tests above and check-migration-versions.sh. The repo
  already carries the same pattern (000057 ×3 across dates). Cosmetic only.
- **Canonical-spec lockfile format divergence**: specs/system/supply-chain.md
  sketches `[stack]`/`[policy]` sections and a signed lockfile; the task plan
  (the decomposed contract for this assignment) specifies the flat schema that
  was implemented. Coverage row 7 honestly records the divergence (unsigned
  lockfile, `[[hooks]]` arrays vs `[hooks.X]` sub-tables) as Partial.
- **L3 JWT-claim branch** (`wl_container_id`/`wl_image_hash`) has no
  production minting site today (spawn pre-mints without workload claims);
  the live L3 signal is the spawn-time KV record. Documented in coverage
  row 9; the KV path is real and tested.
- **`gyre stack lock` has no dedicated test**; the command is fetch +
  `StackLockfile::from_stack` + `to_toml` + write, with the domain half
  under test.
