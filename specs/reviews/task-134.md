# Review — task-134 (agent-gates.md §Part 1 — AgentReview and AgentValidation gate types with review protocol)

Spec: `specs/system/agent-gates.md` §Gate Types (Extended) + §AgentReview Gate (coverage rows 4 and 5).
Candidate under review: `9163afc9b` (merge of task-177 base `774737638` via `a3b639877`, plus docs/process commits). Assigned base: `774737638`.
Verdict: **needs-revision** — one blocking finding (duplicate Diesel migration version introduced by the rebase onto `7747376`).

## Round 1

Probe environment: this sandbox (`/tmp/stage/bin/cargo`, listener probe OK — real loopback e2e ran, no skips). All probes run on the exact candidate tree; evidence under `/tmp/stage/review-evidence/`.

### Verified working (no findings)

The protocol behavior itself is real and well-tested; every focused suite passes on the candidate:

- `cargo test -p gyre-server gate_executor` → **37/37** (incl. both e2e tests driving the fixture `review_agent_driver.sh` through a live axum server with a real bare repo and a spec pinned at a pre-tip SHA, and both TTL regression tests)
- `cargo test -p gyre-server --test task134_review_probe` → **7/7**; `auth::` **46/46**; `git_http::` **40/40**; `abac_middleware::` **12/12**; `gates::` **16/16**; `merge_requests::` **33/33**; `cargo test -p gyre-adapters read_file_at_commit` → **3/3**
- `scripts/check-fabricated-scope-defaults.sh` (count frozen at 1, tightest form), `check-mem-port-contracts.sh`, `check-arch.sh` → all OK

Specifics checked against the acceptance criteria:

- **Scoped token, layered enforcement (real):** `mint_scoped` (auth.rs) issues `review:submit` / `validation:report` JWTs with `jti`; `git_receive_pack` rejects both scopes before repo resolution (403); the ABAC middleware allow-lists only each capability's routes (exact-match scope comparison); `submit_review` binds the reviewer identity to the token subject for scoped tokens (forged `reviewer_agent_id` ignored — proven by the driver fixture and `scoped_review_token_reviewer_identity_is_bound`). The `lookalike_scope_does_not_inherit_review_capability` test is genuinely discriminating (substring matching would fail it).
- **Context gathering (real):** full diff via `git_ops.diff`, spec at the pinned SHA via the new `read_file_at_commit` port (unresolvable SHA is `Err`, absent file at a resolvable SHA is `Ok(None)` and fails the gate with a distinct message), MR title, task description via MR → author agent → task; persona nearest-wins repo → workspace → tenant with no fabricated tenant scope. E2e asserts each context artifact is delivered to the agent process.
- **Fail-closed semantics:** no-command gates fail (both types); exit-0-without-verdict fails; only the gate's own agent's verdict counts; token revoked (`kv_remove`) on every exit path; temp spec/diff files removed.
- **Verdict mapping:** Approved → Passed, ChangesRequested → Failed with the review body surfaced in gate output.

### Findings

- [ ] **F1 (major, code — blocking): duplicate Diesel migration version `20261009000056` breaks `cargo test --all`, the CI mechanical check, and SQLite production startup.** The candidate introduces `crates/gyre-adapters/migrations/2026-10-09-000056_gate_validation_type/` (recovered task work), but the assigned base `774737638` already carries `2026-10-09-000056_user_tenant_id/` — the rebase onto task-177's base created the collision and it was not detected. Three reproduced consequences on the candidate tree (evidence: `migration-version-check.txt`, `adapters-lib-tests-full.txt`):
  1. `bash scripts/check-migration-versions.sh` → **FAIL rc=1** (duplicate `20261009000056`: gate validation type vs user tenant id). The identical check on base `774737638` (clean worktree) → **OK rc=0**, so the failure is introduced by the candidate.
  2. `cargo test -p gyre-adapters --lib` → **393 passed, 3 failed**:
     - `sqlite::quality_gate::tests::save_upsert_updates_all_config_columns` — the candidate's own new regression test panics: `table quality_gates has no column named validation_type` (the duplicate-version resolution silently dropped the `gate_validation_type` migration on a fresh DB);
     - `sqlite::tests::no_duplicate_migration_version_prefixes` — the crate's in-code guard fails on the same duplicate;
     - `sqlite::tests::migration_000058_backfills_unique_url_safe_usernames` — `run_pending_migrations` errors with `UNIQUE constraint failed: __diesel_schema_migrations.version` (diesel records one row per version; the second same-version migration aborts startup migration).
  3. Production impact: `GYRE_DATABASE_URL=sqlite://...` deployments run these embedded migrations at startup (`sqlite/mod.rs` `run_pending_migrations`); depending on resolution order the server either fails to migrate outright or starts with no `validation_type` column, after which **every** quality-gate save fails (`NewQualityGateRow`/upsert unconditionally references the column) — the server test suites stay green only because `build_state` without `GYRE_DATABASE_URL` uses the in-memory stores.
  This violates the acceptance criterion "`cargo test --all` passes" and the repo-wide migration-numbering invariant (AGENTS.md; `scripts/check-migration-versions.sh`). Remedy: renumber the newer migration directory to the next unused 6-digit sequence (`ls crates/gyre-adapters/migrations | sort | tail -3` → currently 000058 is the highest, so `...-000059_gate_validation_type` with a date not already occupied by that number), then re-run the three failing tests plus the mechanical check.
- [ ] **F2 (process, non-blocking once F1 is fixed): the Shipped section's test evidence is stale for the merged tree.** It claims the SQLite/Postgres adapters persist `validation_type` "with an upsert round-trip test" — that exact test fails on the candidate (F1.2), so the claim was carried forward from a pre-rebase tree without re-running the adapter suite against the final merge. Not an independent defect (same root cause), but the rebase-onto-`7747376` narrative in `specs/tasks/task-134.md` should have re-verified adapter-level evidence, since server-side suites cannot see SQLite persistence at all.

### Notes for the verifier

- The protocol implementation (scoped tokens, ABAC allow-list, identity binding, verdict mapping, teardown, fail-closed paths) is otherwise production-quality and independently corroborated by this review's probes; after the migration renumber, re-run at minimum: `cargo test -p gyre-adapters --lib`, `bash scripts/check-migration-versions.sh`, and the full `cargo test --all`.
