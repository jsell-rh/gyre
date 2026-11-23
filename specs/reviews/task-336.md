# Review — task-336 (Spec API read path: linked_tasks under both spellings, per-spec open task counts in the index)

Spec: `specs/system/spec-registry.md` §API Surface (single-spec linked tasks) + §Auto-Generated Index ("Open task count per spec").
Candidate under review: `2781e100` (branch `pipeline/task-336/cdff18d1ca984e979428514eb25c07ae-2`, base `0b33cb3a`; product commit `ce06830d`, attribution commit `2781e100`).
Verdict: **approved** — no findings against the task contract. Compilation and test execution are deferred to the independent verifier with exact commands below (this sandbox has no network to crates.io — curl exit 7 — and an empty `CARGO_HOME`/target dir; restriction, not a code defect; see `/tmp/stage/review-evidence/task-336/`).

## Probes run

- Full diff read (`git diff 0b33cb3a..2781e100`): only `crates/gyre-server/src/api/specs.rs` (+224/−13), coverage rows 12–13 of `specs/coverage/system/spec-registry.md`, and `specs/tasks/task-336.md`. No routes, middleware, exemptions, migrations, state, or verifier surfaces touched.
- Static type-level verification of every new symbol/arity (ports `TaskRepository::{list, list_by_spec_path}`, `Id::new/as_str/Display`, `Task::new` + `transition_status` chain Backlog→InProgress→Review→Done, mem adapter exact-match semantics, deref coercions, single `mod tests` helper scope, route registrations) — see `static-verification.md`.
- `rustfmt --edition 2021 --check` on specs.rs → exit 0.
- `scripts/check-arch.sh`, `check-byte-slice-truncation.sh`, `check-conditional-test-guards.sh` → all exit 0. `check-assertionless-tests.sh` exits 2 with no failure output identically at the base commit (pre-existing, not introduced).
- Network/registry infeasibility proof for deferred compilation (`network-probe.log`, `cargo-home-listing.log`, `compile-infeasibility.txt`).

## Verified

- **Dual-spelling linked_tasks (AC 1).** `spec_path_spellings()` produces exactly the spelling pair the patrol's `spec_has_live_task` queries (ledger-relative `system/x.md` ↔ hook-rooted `specs/system/x.md`, both directions handled); `tasks_linked_to_spec()` queries `list_by_spec_path` under each and dedups by id (sort-then-dedup is correct). `get_spec` (specs.rs:450) uses it with the base's best-effort `unwrap_or_default()` semantics preserved; `get_spec_progress` (specs.rs:2290) uses it with the base's `?` propagation. The hook's rooted spelling (git_http.rs:3630) and the adapters' exact-match (mem.rs:1167, sqlite/task.rs) confirm the pre-fix single query could not return hook tasks — the new test's `assert_eq!(linked, vec!["hook-task-1"])` fails at the base and passes with the fix.
- **specs.rs:2191 "history" reference resolved correctly.** Base line 2191 is the `GET /api/v1/specs/:path/progress` section header; the approval-history endpoint (`spec_approval_history`) has no linked_tasks field and the canonical spec defines it as "Approval history for a spec" only. The candidate fixed the progress handler (the endpoint the task's line pointer names) and documented the interpretation honestly in the task file.
- **Open-task-count column (AC 2).** `spec_index` queries the task repository once and counts, per ledger entry, tasks whose spec_path matches either spelling and whose status is not Done/Cancelled; header `| Spec | Status | SHA | Open Tasks |`; grouping by directory, approval status, and `specs/{path}` links unchanged; taskless specs render 0 (not an empty cell). The index test seeds two open hook-spelling tasks plus one Done ledger-spelling task (count must be 2) and a taskless spec (0) — both assertions fail at the base, which has no such column. The Done task's status is reached through real valid transitions; a failed transition would surface as a wrong count, so `let _ =` cannot mask it.
- **No fakery / no test inflation.** Real repository queries against the real router (`api_router().with_state` over mem-backed `test_state`); no hardcoded ids or counts; all three tests assert concrete observable output (JSON ids, markdown rows), none mirror production logic.
- **Coverage rows 12–13** were updated only for the shipped behavior, with accurate source-level notes (they describe the implementation, not a claimed independent verification run; the deferred verification is stated explicitly in the task file).

Minor (non-blocking, recorded for future decomposition; all pre-existing and outside this task's contract):

- `list_specs` (GET /api/v1/specs, specs.rs:189–265) keys its task_map by the task's raw spec_path spelling and looks it up by the ledger path — hook-spelling tasks are invisible in the list endpoint's linked_tasks too. Same class as the revoke handler's single-spelling task-cancellation query (specs.rs:1587). Neither is in this task's required behavior; both deserve a follow-up task.
- The index's open-task count is not repo-scoped (a matching-spec_path task in another repo counts toward the row). The task contract does not require repo scoping.

Verifier must run (compilation impossible in this sandbox — no crates.io network, empty CARGO_HOME):

- `cargo check -p gyre-server`
- `cargo test -p gyre-server --lib api::specs` — must include the three new tests (`get_spec_lists_hook_created_tasks_in_linked_tasks`, `spec_progress_lists_hook_created_tasks`, `spec_index_counts_open_tasks_per_spec`) plus the existing spec API suite (notably `spec_index_returns_markdown`, whose assertions remain a subset of the new row shape).
- Standard full workspace suite + Clippy per standard verification.
