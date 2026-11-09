# Review — task-165 (Supply chain — gyre-stack.lock parsing and attestation level enforcement)

Spec: `specs/system/supply-chain.md` §gyre-stack.lock, §Attestation Levels, §Level 3: Gyre-Managed Runtime, §Policy per Level.
Candidate under review: `66677b96` (branch `pipeline/task-165/7de814ab95e745639162e0bbb564e4ad-1`), assigned base `fefde294`.
Verdict: **needs revision** (1 blocking finding, 1 non-blocking note). See `/tmp/stage/verdict.json`.

## Round 1 (this review)

Evidence logs under `/tmp/stage/review-evidence/` (all commands run at candidate `66677b96`, worktree verified clean via `git status --short` after every temporary mutation).

### F1 (BLOCKING) — duplicate Diesel migration version silently drops the task's own migration

`crates/gyre-adapters/migrations/2026-10-10-000056_agent_attestation_level` collides on version `20261010000056` with the base's `2026-10-10-000056_meta_spec_approved_content_hash` (the base migration arrived via merge commit `bdc156e00`, the rebase-resolution round, which did not renumber). This is exactly the flaw class AGENTS.md freezes a gate for: "duplicate versions silently never run".

Empirical proof (log: `task165-review-migration-dup-probe.log`, temporary test module removed afterward, worktree restored clean):

- Fresh DB via `SqliteStorage::new`: `__diesel_schema_migrations` lists `20261010000056` **once**; `pragma_table_info('agents')` has **no** `attestation_level` column; `meta_specs.approved_content_hash` **is** present — the task-165 migration is the one Diesel drops.
- `cargo test -p gyre-adapters sqlite::agent` → **8 passed, 9 FAILED**, every failure `no such column: attestation_level` (log: `task165-review-sqlite-agent-tests.log`; reproduced twice). The diesel `NewAgentRow`/`AgentRow` include the column, so agent INSERT/UPDATE fails on every fresh SQLite **and PostgreSQL** deployment (shared embedded `migrations/` dir) — a production-breaking regression of the entire agent store, well beyond task-165 scope.
- `bash scripts/check-migration-versions.sh` → **exit 1** at the candidate ("duplicate Diesel migration versions detected ... 20261010000056: agent attestation level / meta spec approved content hash"); **exit 0** at base `fefde294` (verified in a temp worktree, since removed). Log: `task165-review-gate-migration-versions.log`.
- Acceptance criterion "`cargo test --all` passes" is therefore unmet.

Why the implementer's evidence missed it: all suites cited this round (`git_http::`, `api::stack_attest`, `gyre-domain stack`, `gyre-cli`) are mem-adapter-backed and never touch the SQL schema. The shipped note "no task-165 production behavior changed; all conflicts were source-level" is false for SQL backends, and the claim that only `ws_integration` fails in this sandbox is inaccurate.

Fix: renumber to the next unused sequence for the date (e.g. `2026-10-10-000059_agent_attestation_level`), then re-run `cargo test -p gyre-adapters sqlite::agent` (and the postgres-equivalent suites in verification) plus the migration gates.

### F2 (non-blocking) — lockfile drift verification is policy-gated, not lockfile-gated

`enforce_stack_policy` (`crates/gyre-server/src/git_http.rs`) performs the gyre-stack.lock integrity/drift check only inside `if let Some(policy) = &policy`. A repo that commits a lockfile but sets no `repo_stack_policies` entry gets no drift verification at push time. supply-chain.md §gyre-stack.lock frames the committed lockfile itself as the drift-verification mechanism, and the task plan item 5 does not condition flagging on a policy. Defensible as policy opt-in (enforcement mode is undefined without a policy), but coverage row 7 does not record the gating. Recommend warn-mode flagging on lockfile-present-without-policy, or recording the gating as a known partial.

### Verified working (no findings)

- **Lockfile schema** — `StackLockfile` (`crates/gyre-domain/src/stack.rs`): real TOML schema (fingerprint + all 8 specced components), `parse`/`to_toml` round-trip, `verify_integrity` recomputes the composite fingerprint from components, `check_drift` names differing components. `cargo test -p gyre-domain stack` → 9 passed (log: `task165-review-domain-stack.log`).
- **Level resolution is honest and unforgeable at its trust basis** — L3 comes from the server-written `workload_attestations` KV record (spawn.rs writes container identity only for server-spawned container targets; heartbeat updates liveness fields only, `api/agents.rs:340-390`; `api/workload.rs` is read-only) or from signature-verified JWT claims (`auth.rs` validates agent JWTs via `agent_signing_key.validate`; the wl_container_id/wl_image_hash branch has no production minting site, which the coverage row 9 records accurately). L2 = registered stack; L1 = nothing. Stored on the agent record and exposed on `GET /agents/:id`.
- **Per-level policy** — `RepoStackPolicy.min_attestation_level` (1..=3, validated, 400 on out-of-range) + `enforcement: block|warn` (400 on unknown), Admin-only PUT (NEW-39 regression test present). `cargo test -p gyre-server api::stack_attest` → 14 passed (log: `task165-review-stackattest-tests.log`).
- **Push enforcement is real and the tests are hard.** `cargo test -p gyre-server git_http::` → 45 passed at the candidate (log: `task165-review-githttp-tests.log`). Mutation probe (log: `task165-review-mutation-probe.log`, source restored and re-verified green in `task165-review-githttp-restored.log`): disabling the below-minimum comparison and the drift/integrity rejection makes exactly the 4 enforcement tests fail (`push_below_min_attestation_level_blocked` 200≠403, `push_below_min_level_warn_mode_flagged_and_accepted`, `lockfile_drift_blocked_in_block_mode` 200≠403, `tampered_lockfile_rejected_by_integrity_check`) — the tests are anchored to the production behavior, not incidental. The block test also asserts the ref is actually undone.
- **CLI** — `gyre stack lock [--output]` fetches the registered stack and writes the lockfile; `cargo test -p gyre-cli` → 136 passed (log: `task165-review-cli-tests.log`).
- **Other gates at the candidate**: migration-sql-portability OK, relative-path-defaults OK, fabricated-scope-defaults OK (logs: `task165-review-gate-*.log`); task-commit-attribution OK (the three frontmatter checkpoint commits carry the code; `735a5e1ac` is docs/specs-only).
- Provenance label mapping (3→server-verified, 2→self-reported, else unattested) wired into `record_pushed_commits`.

### Required from the next round

1. Renumber the migration off version `20261010000056` (blocking; F1) and re-run the SQL-backed suites + `check-migration-versions.sh`.
2. Optionally address or explicitly ledger F2.
