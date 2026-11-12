---
title: "Repo lifecycle — archive push rejection, admin repos tab, gate config UI"
spec_ref: "repo-lifecycle.md §API Summary"
depends_on: []
progress: complete
coverage_sections:
  - "repo-lifecycle.md §Admin → Workspace Scope → Repos Tab"
  - "repo-lifecycle.md §Gates (Admin → Repo Scope → Gates)"
  - "repo-lifecycle.md §API Summary"
commits: ["3399157f03203b1bb57e7924670917b78d4f544a", "26fc7b32667bf8d3121ec07f837d1a960d0681d7", "0237af23cb7403ef2489b1247a52f2601f1bab66", "219780c5aef6f0a9fc88da61185949decc8e97a8", "4443b66a81b9a29d6cfbf808f624cc76af19c1a6", "0106d3a3adfe0e4c7c175d81e0d301583a7b3255", "d8e78bb6c440c7ba24103f18be30dfbcd669f3b5", "730f9df11e81dfe1644d2b464cec1352f72bda35", "df5b32464a082ce008236a2732536755538ebe03", "9ebec6c2713ce824aa5ca9f14df74dcecb074437", "904586888416cb5c5aec355a70875b271e005a90", "36e401fbd899a48fc15a4bf430f25ef904747e1e", "b657692571ca3e6f53528afeec1578dfb46c9e7a", "06e11cbcf59b1fdd0247411c5e8910c4c6190714", "1da8dd39d018de0a101f550198ce224d077329f6", "4c11b2644f2435a87e60a38b8b8f356722562f14", "4b5d96563755dec7fbd825880c23661c604f7a8d", "5d5e4046f0af0cf074348b584d632e36f3f6b570", "0a2df7c6a80141bc885937dc3543e13055bf8ee3", "568d27f2cc4ca542ebe3b138872b710dd693a5e0", "68805172e3c0abb0315928d7f24d3ba2aaa16673", "c328b9e0c8b982894b5960435b05335c50ba405c"]
---

## Spec Excerpt

### §API Summary (repo-lifecycle.md)

Git push rejection: archived repos should reject pushes with an appropriate error.

### §Admin → Workspace Scope → Repos Tab (repo-lifecycle.md)

Repo management is a tab in the Admin view at workspace scope. The **Repos tab** shows:
- List of all repos (name, status, agent count, last activity)
- "+ New Repo" button
- "Import Repo" button
- Click a repo → navigates to repo scope

### §Gates (Admin → Repo Scope → Gates) (repo-lifecycle.md)

- List of configured gates (name, type, enabled/disabled toggle)
- "+ Add Gate" button with gate type selector
- Per-gate configuration (test command, lint command, reviewer persona)
- Drag to reorder

## Implementation Plan

1. **Git push rejection for archived repos**: In `crates/gyre-server/src/git_http.rs`, the `git_receive_pack` handler (POST endpoint) should check the repo's `status` field. If `RepoStatus::Archived`, return an error response before processing the pack. The error message should explain that the repo is archived. Check `resolve_repo_by_slug()` — it already loads the repo entity; add a status check after resolution.

2. **Admin Repos tab**: Create a `RepoList.svelte` component (or extend the existing workspace settings) that shows all repos in the workspace with: name, status badge (Active/Archived), active agent count, last activity timestamp. Include "+ New Repo" and "Import Repo" buttons that open the existing creation forms. Wire into the Admin workspace-scope tab bar.

3. **Gate configuration UI**: Create a `GateConfig.svelte` component for repo-scope admin that:
   - Lists configured gates via `GET /api/v1/repos/:id/gates`
   - Shows each gate's name, type, and enabled status with a toggle
   - Provides "+ Add Gate" button with a type selector dropdown
   - Shows per-gate configuration fields (test command, lint command, etc.)
   - Saves changes via `PUT /api/v1/repos/:id/gates/:gate_id`
   - Wire into the repo-scope admin view

## Acceptance Criteria

- [ ] `git push` to an archived repo returns an error (not silently accepted)
- [ ] Admin workspace scope has a "Repos" tab listing all repos
- [ ] Repos tab shows status badges and "+ New Repo" / "Import Repo" buttons
- [ ] Repo scope admin has a "Gates" panel showing gate configuration
- [ ] Gates can be added, toggled, and configured through the UI
- [ ] `cargo test --all` and `cd web && npm test` pass

## Agent Instructions

Read `specs/system/repo-lifecycle.md` §"1. Where Repo Management Lives", §"3. Repo Configuration — Gates", and §"6. Domain Changes — API Summary". For the git push rejection: the handler is in `crates/gyre-server/src/git_http.rs` — look for `git_receive_pack` or the POST handler for `/git/:workspace_slug/:repo_name/git-receive-pack`. The repo status check should go after `resolve_repo_by_slug()`. For UI: follow existing Svelte 5 patterns in `web/src/` (e.g., WorkspaceSettings.svelte, RepoSettings.svelte). The gate API routes are at `GET/POST /api/v1/repos/:id/gates`.

## Shipped

All three coverage sections are implemented in production code on this
branch (full chain of checkpoints and repair rounds in `commits:`; latest
assignment base `cba416b46` merged at `b436a91a7`). This round was a
**contract repair** (finding `d62945336`): the executor restored the assigned
task document (progress reset, acceptance boxes back to their assigned
unchecked state), and this round re-applies the ready-for-review state
without touching any production file. The acceptance boxes are deliberately
left exactly as assigned — checking them in place mutates the protected
contract text (the trigger for findings `6ed209f6` and `d62945336`);
completion is recorded here and in `progress:`.

### Round: contract restoration, second occurrence (no code change)

- `git diff 90a3d1c --name-only` lists only `specs/tasks/task-168.md`;
  `git diff 90a3d1c -- crates/ web/src scripts/` is empty (0 lines) — the
  working tree is byte-identical on the product surface to candidate
  `90a3d1c`, which passed `SKIP_WEB_BUILD=1 cargo check -p gyre-server
  --all-targets` and `cargo fmt -p gyre-server -- --check` in the prior
  round. Fresh evidence saved under
  `/tmp/stage/review-evidence/task-168-contract-round-2/`.
- No recompile is needed: no Rust, script, or web source file changed this
  round, so the prior round's type-check and fmt evidence carries to this
  head unchanged.

### Round: E0063 verification repair (prior, retained)

- **E0063 in `gyre-server` lib test** — the base merge brought the `position`
  field on `gyre_domain::QualityGate` into a tree where the `ralph_loop.rs`
  `loop_state()` fixture initializer had lost it, failing `cargo check` of the
  gyre-server lib test target. Restored `position: 0` (matching `Default` and
  every other fixture initializer); verified every other `QualityGate`
  initializer in the tree already sets `position`.

### Git push rejection for archived repos (§API Summary)

- `git_info_refs` rejects the `git-receive-pack` advertisement for archived
  repos with a 200 response whose pkt-line stream carries
  `ERR push rejected: repository is archived` — the git-smart-HTTP convention
  so `git push` actually prints the reason. `git_receive_pack` (POST) keeps a
  defense-in-depth 403 before the packfile is processed; clone/fetch still
  works on archived repos (read-only, not invisible).
- Unit tests in `git_http.rs` (archived-repo ERR pkt-line, POST 403,
  clone-still-works) and integration test `push_to_archived_repo_rejected` in
  `crates/gyre-server/tests/git_integration.rs`, which runs a real `git push`
  against a live server and asserts the push fails with an "archived" reason.

### Admin Repos tab (§Admin → Workspace Scope → Repos Tab)

- `WorkspaceSettings.svelte` has a workspace-scope **Repos** tab listing all
  repos with name, status badge (Active/Archived), agent count, and last
  activity, plus "+ New Repo" and "Import Repo" buttons wired to the existing
  creation/import forms; clicking a repo navigates to repo scope.

### Gate configuration UI (§Gates)

- `RepoSettings.svelte` gate panel lists gates ordered by `position`, with
  per-gate name/type/enabled toggle, per-type configuration fields (test
  command, lint command, reviewer persona), "+ Add Gate" with a gate type
  selector, and drag-to-reorder persisting `position` via
  `PUT /api/v1/repos/:id/gates/:gate_id`.
- Server side: `api/gates.rs` sorts by `(position, created_at)`, appends new
  gates after the current max, and accepts `position` updates; covered by
  `list_gates_orders_by_position` and the upsert regression guard in
  `sqlite/quality_gate.rs`.

### Evidence

- This contract round: `git diff 90a3d1c --name-only` → only
  `specs/tasks/task-168.md`; product-surface diff empty (see
  `/tmp/stage/review-evidence/task-168-contract-round-2/`). No recompile
  needed — source byte-identical to the checked candidate.
- Prior round: `SKIP_WEB_BUILD=1 cargo check -p gyre-server --all-targets` →
  clean (lib, lib test, bins, integration tests; the previous round's E0063
  is gone). Exit 0. `cargo fmt -p gyre-server -- --check` → clean. Exit 0.
- `scripts/check-task-commit-attribution.sh` re-run on this head with the
  restored `commits:` frontmatter → exit 0 (all 21 listed SHAs resolve;
  every task-168 product-surface commit is recorded).
- The full `scripts/check-*.sh` sweep was re-run for evidence; failures
  observed in the broader sweep are pre-existing on the base tree (none of
  the failing scripts read `specs/tasks/`, and the product surface is
  unchanged from the reviewed candidate) — recorded in
  `mechanical-checks.txt` under the evidence directory.
- Verifier must run (deferred per 4 GiB budget): full `cargo test --all` —
  notably the `ralph_loop` tests that consume the repaired fixture
  (`gate_failure_respawns_agent_with_fresh_context`,
  `max_iterations_blocks_task_and_fails_agent`, `merge_completes_task`),
  the `gyre-server` git_http unit tests,
  `tests/git_integration.rs::push_to_archived_repo_rejected`, and
  `cd web && npm test`.
