# Review — task-168 (Repo lifecycle — archive push rejection, admin repos tab, gate config UI)

Spec: `specs/system/repo-lifecycle.md` §"Admin → Workspace Scope → Repos Tab", §"Gates (Admin → Repo Scope → Gates)", §"6. Domain Changes — API Summary".
Candidate under review: `04e036343` (base `1e61f663` merged at `d18a6d790`; rebase repair of finding `12011b0bc4364353bb5bd863d4ad229a`).
Verdict: **approved** — all three coverage sections are implemented in production code; independent probes support every acceptance criterion.

## Probes run (evidence: `/tmp/stage/review-evidence/task-168-review/`)

- `bash scripts/check-migration-versions.sh` → OK (no duplicate Diesel versions; `gate_position` renumbered to 000062 past base-owned 000061 `dep_breaking_policies`).
- `bash scripts/check-arch.sh`, `check-migration-sql-portability.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`, `check-forged-scope-fields.sh`, `check-inert-enforcement.sh`, `check-fabricated-scope-defaults.sh`, `check-forwarded-header-trust.sh`, `check-lossy-secret-conversion.sh`, `check-scope-literal-defaults.sh`, `check-conditional-test-guards.sh` → all exit 0.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → exit 0.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` → exit 0 (lib unit tests **and** `tests/git_integration.rs` type-check, including all new test code).
- `cargo test -p gyre-adapters quality_gate` → 1 passed (`save_upsert_updates_all_config_columns` now flips and re-asserts `position` persistence on real SQLite).
- `cargo test -p gyre-domain repository` → 3 passed.
- `cd web && npm ci && npm test` → 108 files, **2168 passed / 41 skipped**, exit 0 (includes the new RepoSettings gate toggle/configure/drag-reorder tests and WorkspaceSettings repos-tab tests).
- `bash scripts/check-task-commit-attribution.sh` → fails **only against the working tree**, because the executor's stage-assignment restore reset `specs/tasks/task-168.md` to `commits: []` in the working tree. Verified clean at the committed candidate: every flagged task-labeled SHA (3399157f, 26fc7b32, 0237af23, 219780c5, 4443b66a, 0106d3a3, d8e78bb6, df5b3246, b6576925, 06e11cbc, 1da8dd39, 4c11b264, 4b5d9656, 5d5e4046, 0a2df7c6, 568d27f2, 68805172) is present in the committed frontmatter (0 missing). Not a candidate defect.

## Verified per acceptance criterion

- **Archived repos reject pushes (§API Summary).** `git_info_refs` rejects the `git-receive-pack` advertisement for archived repos with a 200 pkt-line stream carrying `ERR push rejected: repository is archived` — the git-smart-HTTP service-error convention, so `git push` actually prints the reason; `git_receive_pack` (POST) keeps a defense-in-depth 403 placed after `resolve_repo_by_slug()` and before packfile processing (mirrors the pre-existing mirror-rejection pattern). Clone/fetch (upload-pack) deliberately still served (read-only, not invisible). Tests are mutation-hard: the advertisement test asserts the exact `001f# service=git-receive-pack\n0000` framing plus the ERR line (fails if the check is removed — a normal advertisement carries refs, not ERR); the POST test asserts 403 + "archived"; `push_to_archived_repo_rejected` in `tests/git_integration.rs` runs a real `git push` against a live server and asserts failure with an "archived" reason. `RepoStatus` Display/serde yields "Active"/"Archived", matching the frontend badge check.
- **Admin Repos tab (workspace scope).** `WorkspaceSettings.svelte` Repos tab lists all workspace repos with name, Active/Archived status badge, active-agent count (from `api.agents({workspaceId})`, `status === 'active'` filter), and last activity (max of repo `updated_at`, agent spawns, MR timestamps), plus "+ New Repo" and "Import Repo" buttons opening the existing create/mirror forms; clicking a repo navigates to repo scope via the new `goToRepo` context (backed by the existing `goToRepo(repo, tab)` in App.svelte). Enrichment fetches degrade gracefully (`.catch(() => [])`).
- **Gates panel (repo scope).** `RepoSettings.svelte` gates tab: list (name, type, enabled/disabled toggle), "+ Add Gate" with type selector, per-gate configuration (command / required approvals / reviewer persona / timeout per gate type), delete, and HTML5 drag-to-reorder that rewrites 1-based positions via `PUT /api/v1/repos/:id/gates/:gate_id` and reloads from the server on failure. Backend `update_gate` is a real partial update with per-type field validation (command only on command gates, persona only on agent gates, non-empty names), cross-repo containment (`repo_id` mismatch → 404), and is registered `put(...)` on the already-ABAC-mapped route (PUT → "write" on "gate" via middleware). Ordering is `(position, created_at)` consistently across SQLite, PostgreSQL, and mem adapters; the `position` column lands via migration `2026-10-08-000062_gate_position` (portable `ALTER TABLE ... ADD COLUMN ... INTEGER NOT NULL DEFAULT 0`; backfill 0 preserves pre-existing created_at order).

## Deferred to the independent verifier (sandbox wrapper blocks gyre-server codegen: "gyre-server code generation plus the model exceeds 4 GiB")

- `cargo test -p gyre-server --lib git_http` (three archived-repo tests + pkt-line framing tests)
- `cargo test -p gyre-server --lib api::gates` (position ordering, update/toggle/persist, 404s, type validation)
- `cargo test -p gyre-server --test git_integration push_to_archived_repo_rejected` (real `git push`)
- Full `cargo test --all` + web rebuild per the standard verification contract

All of the above type-check (`cargo check -p gyre-server --lib --tests` exit 0); behavioral correctness is additionally supported by inspection (assertions target observable output/state that would fail with the checks removed) and the green adapter-level position-persistence test.

## Non-blocking observations

- `scripts/check-assertionless-tests.sh` exits 2 in this sandbox on both candidate and base: it uses gawk-only 3-arg `match()` and the system awk is mawk; the first test-bearing file alphabetically (`crates/gyre-adapters/src/call_graph.rs`) triggers the fatal. Pre-existing infrastructure issue, not a candidate defect — verifier should run it under gawk. All new tests in this diff contain real assertions (inspected).
- `specs/coverage/system/repo-lifecycle.md` rows 3/11/19 remain `task-assigned`; per repo convention (git history) the coverage matrix is updated by the audit stage (`audit(...)` commits), not by implementers.
- `web/dist/` is unchanged from base in this diff — generated output owned by the deterministic verifier (rebuilt and committed at integration); authored source and tests reviewed instead.
