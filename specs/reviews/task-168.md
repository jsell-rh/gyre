# Review — task-168 (repo-lifecycle — archive push rejection, admin repos tab, gate config UI)

Candidate: `7b367f39d737b923718b025ba518b8641e106cda` (base `cfeefbda`, branch
`pipeline/task-168/9ef9a926d3e44c23b9f27d2efe588dcf-1`). Verdict: **approved**.

Covers the three task sections of `specs/system/repo-lifecycle.md`: §API Summary
(archived-repo git push rejection), §Admin → Workspace Scope → Repos Tab, and
§Gates (Admin → Repo Scope → Gates).

## Evidence (probes run this round, logs in /tmp/stage/review-evidence)

- `cargo check -p gyre-server` — clean, 0 errors (cold build, 7m32s).
- `cargo check -p gyre-server --tests` — clean, 0 errors (test targets compile,
  including the new git_http/gates unit tests and git_integration Test 13).
- `cargo test -p gyre-adapters --lib quality_gate` —
  **1 passed** (`sqlite::quality_gate::tests::save_upsert_updates_all_config_columns`),
  447 filtered. This is a real storage probe: `SqliteStorage::new` runs the
  embedded migrations, so the pass proves `2026-10-08-000061_gate_position`
  (ALTER TABLE quality_gates ADD COLUMN position) executes on SQLite AND that
  the Diesel upsert `.set(...)` persists `position: 7` (the task-134
  column-drift regression guard, extended by this task to flip `position`).
- `cd web && npm ci && npm test` — **104 files, 2089 passed, 41 skipped, 0
  failed** (full frontend suite, includes the new RepoSettings gate
  toggle/configure/drag tests and WorkspaceSettings repos-tab tests).
- Mechanical gates on the candidate tree: check-arch, check-abac-route-registry,
  check-abac-exempt-handlers (75 handlers), check-mem-port-contracts,
  check-migration-versions, check-migration-sql-portability,
  check-forged-scope-fields, check-in-memory-state-stores,
  check-relative-path-defaults, check-inert-enforcement,
  check-conditional-test-guards, check-dead-message-kinds — all pass.
  check-task-commit-attribution passes against the **committed** candidate
  (verified in a clean worktree; the checkout's working tree carries a
  pre-existing unstaged revert of specs/tasks/task-168.md to the pristine
  assignment body — `commits: []` — which makes the script fail there; that
  working-tree state is not part of the candidate).
- check-assertionless-tests.sh exits 2 with no violation output on BOTH base
  and candidate — pre-existing script/environment failure, not introduced.

## Section findings

### §API Summary — archived-repo push rejection (real, defense in depth)

`git_info_refs` (git_http.rs): for `service=git-receive-pack` on an archived
repo it returns a 200 advertisement whose pkt-line stream carries
`ERR push rejected: repository is archived` — the git-smart-HTTP service-error
convention so `git push` actually prints the reason (a plain 403 body is never
shown by git). `git_receive_pack` (POST) keeps a 403 before the packfile is
read, after `resolve_repo_by_slug()`, mirroring the pre-existing mirror-repo
403. Clone/fetch (`git-upload-pack`) is deliberately untouched — archived is
read-only, not invisible (§4 step 5), and a test pins that.
`Repo::archive()/is_archived()` are the real domain methods (repository.rs).
Unit tests hardcode the exact pkt-line framing (`001f# service=…\n0000`) so a
framing regression in `pkt_line()`/`service_header()` fails rather than being
mirrored in; the 403 test asserts status + "archived" in body.

### §Admin → Workspace Scope → Repos Tab (real data, real navigation)

WorkspaceSettings.svelte `repos` tab lists every workspace repo with name,
Active/Archived status badge (matches `RepoStatus` serde "Active"/"Archived"),
active-agent count and last-activity (computed from real `GET /agents?workspace_id=`
— verified the server lowercases `AgentStatus` and enriches `repo_id` from
worktree tracking — plus `GET /merge-requests?workspace_id=` and repo
`updated_at`), default branch, and mirror URL. "+ New Repo" / "Import Repo"
open forms wired to `api.createRepo` / `api.createMirrorRepo`; repo name links
through the new `goToRepo` context (App.svelte) into repo scope. Frontend
tests assert badge text, the active-only agent count (1 active + 1 idle → "1"),
and both creation/import API call shapes.

### §Gates (real config surface, ordered)

New `PUT /api/v1/repos/:id/gates/:gate_id` (`update_gate`): repo-scoped gate
lookup with cross-repo mismatch → 404; partial update of name (non-empty),
command (required for test/lint gates, rejected as inapplicable for others),
required_approvals/persona (type-checked per gate type), `required` toggle,
timeout, and `position`. Route registered on the existing path whose ABAC
`RouteResourceMapping` entry ("gate") already covers it (path-based resolver;
PUT maps to "write" via `method_to_action`). `position` column added via
migration `2026-10-08-000061_gate_position` (portable SQL, no duplicate
version; backfills 0), ordering `(position, created_at)` enforced identically
in the SQLite adapter, PG adapter, and mem adapter (port-contract parity);
create defaults to max+1. UI: per-gate enabled/disabled toggle (persists
`required`), "Configure" per-gate form (command / approvals / persona by type,
name, timeout), drag-to-reorder that rewrites 1-based positions via PUT and
reloads from server on failure. The `enabled/disabled` toggle maps to the
existing `required` (blocking vs advisory) semantics defined by
agent-gates.md §"Optional gates" — the domain has no separate enabled flag;
this is the correct existing realization, not a reinterpretation.

### Test quality

Server tests are discriminating: the reorder assertion (PUT review→position 0,
expect order change) fails under created_at-only ordering; the update tests
verify persistence via a fresh list; type-mismatch command updates are
asserted to 400; unknown gate 404s. `git_integration.rs` Test 13 drives a real
server + real `git push` (clone still succeeds, push fails with "archived" in
stderr). Frontend drag test asserts exact position PUTs (g3→2, g2→3, g1 not
re-sent).

## Verifier obligations (gyre-server test compilation deferred per budget rules)

Run in the independent verifier:
- `cargo test -p gyre-server --lib git_http` (includes
  `info_refs_receive_pack_archived_repo_returns_err_pkt_line`,
  `info_refs_upload_pack_archived_repo_still_serves_clone_advertisement`,
  `receive_pack_archived_repo_returns_403`)
- `cargo test -p gyre-server --lib api::gates` (includes
  `list_gates_orders_by_position`, `update_gate_toggles_required_and_updates_command`,
  `update_gate_persona_on_agent_review_gate`,
  `update_gate_command_on_non_command_gate_rejected`,
  `update_unknown_gate_returns_404`)
- `cargo test -p gyre-server --test git_integration push_to_archived_repo_rejected`
- `cargo test --all` (acceptance criterion; `cargo check --tests` already clean)

## Non-blocking observations (for the controller/auditor, not the candidate)

- Coverage rows 3/11/19 in specs/coverage/system/repo-lifecycle.md still read
  `task-assigned | task-168`; the shipped behavior closes their stated gaps.
  Row status is owned by the audit stage per repo convention — flipping them
  here would be self-certification, so their current state is conservative,
  not hollow.
- Committed `web/dist` is byte-identical to base (the final base merge
  resolution took mainline's dist). Default builds regenerate dist via
  build.rs (verified live: "Building web assets (npm run build)… built
  successfully"), and the pipeline's publication stage commits regenerated
  assets (cf. per-task "build(web): regenerate verified assets" commits), so
  only `SKIP_WEB_BUILD=1` Rust-only builds serve the pre-task UI — a known
  pipeline-owned artifact state, per the review brief itself ("the older
  committed web bundle cannot prove current UI code").
- The two verification findings from prior candidate `6c0ef4c` were repaired
  in `26fc7b32` and independently confirmed here: migration renumbered to the
  unique `2026-10-08-000061` (check-migration-versions passes; no duplicate
  Diesel version) and the `position` field restored in `sample_gate()` with
  the upsert regression guard extended (probe passed above).
