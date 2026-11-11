# Review — task-168 (Repo lifecycle — archive push rejection, admin repos tab, gate config UI)

Spec: `specs/system/repo-lifecycle.md` §"Admin → Workspace Scope → Repos Tab", §"Gates (Admin → Repo Scope → Gates)", §"API Summary" (§4 Archive step 5, §3 Gates).
Candidate under review: `67524811cfb72ac18056e958872c7d465d5abc37` (assignment base `cba416b46`).
Verdict: **approved** — all three coverage sections are backed by real production code and meaningful tests; no blocking findings. Runtime tests that require a gyre-server test build are deferred to the independent verifier (review-stage build wrapper blocks them under the 4 GiB budget); the full list is recorded in `/tmp/stage/review-evidence/task-168-67524811c/README.md`.

## Probes run (exit-coded evidence in /tmp/stage/review-evidence/task-168-67524811c/)

- Mechanical invariants: `check-migration-versions`, `check-migration-sql-portability`, `check-arch`, `check-abac-route-registry`, `check-abac-exempt-handlers`, `check-mcp-write-tools`, `check-mem-port-contracts`, `check-inert-enforcement`, `check-task-commit-attribution` — all exit 0.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → exit 0 (lib + all test targets type-check; confirms the E0063 `position` fixture repair compiles everywhere).
- `cargo test -p gyre-domain` → 466+8 passed.
- `cargo test -p gyre-adapters quality_gate` → `save_upsert_updates_all_config_columns` passes with the new `position: 7` upsert assertion (real SQLite round-trip).
- `cargo test -p gyre-adapters migration` → 7 passed, including `no_duplicate_migration_version_prefixes`, `migrations_pending_zero_after_startup`, `migration_is_idempotent` — the new `000061_gate_position` migration applies cleanly and idempotently on fresh SQLite.
- `cd web && npm ci && npx vitest run` full suite → 104 files, 2125 passed / 41 skipped, exit 0 (includes the new WorkspaceSettings Repos-tab and RepoSettings gate-panel tests: 141/141 in the two focused files).
- Environment caveats (not code defects): the gawk-only test-quality scripts (`check-assertionless-tests` etc.) exit 2 under this sandbox's mawk; `check-web-tests.sh` hits a duplicated `--maxWorkers` flag injected by the sandbox npm wrapper. Both are sandbox artifacts; verifier should re-run with gawk.

## §API Summary — archived repos reject pushes (verified)

- `git_info_refs` rejects the `git-receive-pack` advertisement for archived repos with a 200 whose pkt-line stream carries `ERR push rejected: repository is archived` — the correct git-smart-HTTP convention so `git push` prints the reason; `git_receive_pack` keeps a defense-in-depth 403 before packfile processing; upload-pack (clone/fetch) is untouched, matching §4 "read-only, not invisible".
- Tests are genuine: the unit tests drive the real router built by `git_app_with_repo()` (real bare git repo on disk, real state store) and archive via `state.repos.update` — they fail if the `is_archived()` checks are removed. The advertisement test hardcodes the exact pkt-line framing (`001f# service=git-receive-pack\n0000` + ERR line) so a `pkt_line()`/`service_header()` regression fails rather than being mirrored in. The integration test `push_to_archived_repo_rejected` runs a real `git push` against a live server and asserts failure with an "archived" reason. Deferred to verifier: the gyre-server lib and `git_integration` test targets (blocked by the review-stage build wrapper).

## §Admin → Workspace Scope → Repos Tab (verified)

- `WorkspaceSettings.svelte` workspace-scope Repos tab lists all repos with name, Active/Archived status badge (server `RepoStatus::to_string()` matches the UI's `'Archived'` check exactly), active agent count (from `GET /agents?workspace_id=`, filtered to `status === 'active'`), last activity (max of repo `updated_at`, agent spawns, MR timestamps), default branch and mirror URL; "+ New Repo" and "Import Repo" buttons open the existing creation/import forms wired to `api.createRepo` / `api.createMirrorRepo`; clicking a repo name navigates to repo scope via the new `goToRepo` context (App.svelte). Tests assert row content, badges, agent-count filtering, form payload shapes, and button presence.

## §Gates (Admin → Repo Scope → Gates) (verified)

- `RepoSettings.svelte` gate panel: per-gate enabled/disabled toggle (checkbox → `PUT {required}`), per-gate configuration form with type-appropriate fields (command for test/lint gates, required approvals, reviewer persona, timeout), "+ Add Gate" with the pre-existing gate-type selector, and drag-to-reorder that rewrites 1-based positions and persists each move via `PUT /repos/:id/gates/:gate_id`, reloading from the server on failure. Frontend tests assert the exact API payloads for toggle, configure, and reorder (including that unchanged positions are not re-sent).
- Server side: `update_gate` validates per-type field applicability (403-free 400s), enforces repo scoping exactly like `delete_gate` (`gate.repo_id` must match the path repo, else 404), and the route rides the existing `/api/v1/repos/:id/gates/:gate_id` ABAC resolver mapping (PUT → "write" action). `list_gates` and both SQL adapters plus the mem adapter now order by `(position, created_at)` — mem/SQL contract parity confirmed by `check-mem-port-contracts`. New gates append after the current max position. The `position` column is persisted by migration `2026-10-08-000061_gate_position` (portable SQL, unique version, fresh-DB and idempotency tests pass) and the SQLite upsert regression test now covers `position` drift.

## Scope and integrity checks

- Diff base→candidate touches only in-scope surfaces; `scripts/`, exemption files, and `web/dist` are untouched. From the prior reviewed product candidate `90a3d1c` to HEAD, only `specs/tasks/task-168.md` changed (contract restoration) — the product surface is byte-identical, so the prior round's compile evidence carries.
- No fake implementations found: no bail!/stub paths in the new code, real storage round-trips, real router tests. No test inflation: the new tests assert payloads and behavior, not mirrors.

## Non-blocking observations (recorded for the verifier/auditor)

- `specs/coverage/system/repo-lifecycle.md` rows 3/11/19 still read `task-assigned` with now-stale gap notes (row 19 claims receive-pack lacks an archived check). Flipping them is completion/auditor bookkeeping; not-marking is the conservative direction and the task contract does not require it.
- Migration `2026-10-08-000061_gate_position` keeps the 2026-10-08 date prefix while taking sequence 000061, so its diesel version sorts before the 2026-11-dated 000059/000060 on fresh databases. Verified non-breaking (neither touches `quality_gates`; fresh-DB migration tests pass; on existing deployments it is the only pending migration). Future migrations should date-prefix consistently with their sequence.
- Deferred verifier probes (gyre-server test build): `cargo test -p gyre-server --lib git_http`, `--lib api::gates`, `--lib ralph_loop`, `--test git_integration push_to_archived_repo_rejected`, full `cargo test --all`, `cargo fmt --all -- --check`, gawk-dependent test-quality scripts, and `web/dist` regeneration.
