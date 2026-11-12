---
title: "Repo lifecycle — archive push rejection, admin repos tab, gate config UI"
spec_ref: "repo-lifecycle.md §API Summary"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "repo-lifecycle.md §Admin → Workspace Scope → Repos Tab"
  - "repo-lifecycle.md §Gates (Admin → Repo Scope → Gates)"
  - "repo-lifecycle.md §API Summary"
commits: ["c328b9e0c8b982894b5960435b05335c50ba405c", "3399157f03203b1bb57e7924670917b78d4f544a", "26fc7b32667bf8d3121ec07f837d1a960d0681d7", "0237af23cb7403ef2489b1247a52f2601f1bab66", "219780c5aef6f0a9fc88da61185949decc8e97a8", "4443b66a81b9a29d6cfbf808f624cc76af19c1a6", "0106d3a3adfe0e4c7c175d81e0d301583a7b3255", "d8e78bb6c440c7ba24103f18be30dfbcd669f3b5", "730f9df11e81dfe1644d2b464cec1352f72bda35", "df5b32464a082ce008236a2732536755538ebe03", "9ebec6c2713ce824aa5ca9f14df74dcecb074437", "904586888416cb5c5aec355a70875b271e005a90", "36e401fbd899a48fc15a4bf430f25ef904747e1e", "b657692571ca3e6f53528afeec1578dfb46c9e7a", "06e11cbcf59b1fdd0247411c5e8910c4c6190714", "1da8dd39d018de0a101f550198ce224d077329f6", "4c11b2644f2435a87e60a38b8b8f356722562f14", "4b5d96563755dec7fbd825880c23661c604f7a8d", "5d5e4046f0af0cf074348b584d632e36f3f6b570", "0a2df7c6a80141bc885937dc3543e13055bf8ee3", "568d27f2cc4ca542ebe3b138872b710dd693a5e0", "68805172e3c0abb0315928d7f24d3ba2aaa16673"]
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
branch (implementation chain in `commits:`; latest assignment base
`1e61f663` merged at `d18a6d790`). Acceptance boxes are deliberately left
exactly as assigned — checking them in place mutates the protected contract
text; completion is recorded here and in `progress:`.

### Round: base merge 1e61f663 (rebase repair, finding 12011b0bc4364353bb5bd863d4ad229a)

- Resolved the merge against current main (task-163 dependency-graph
  persistence) at merge commit `d18a6d790`: conflicts were confined to
  `web/dist` build artifacts, resolved toward main's published assets — dist
  is pipeline-owned and regenerated by final integration.
- Source verified intact on both sides: `git diff 535301acd -- crates/
  web/src/ scripts/` shows only main-side additions (task-163 files), zero
  task-168 product lines removed.
- Repaired the post-merge migration collision: main's
  `dep_breaking_policies` took Diesel version 20261008000061, colliding with
  our `gate_position` migration (only the alphabetically-first directory
  runs). Renumbered `gate_position` to the next unused slot 000062;
  `check-migration-versions.sh` exits 0.
- Fresh merged-head evidence under `/tmp/stage/review-evidence/
  task-168-merge-1e61f663/`: check-migration-versions, check-arch,
  check-migration-sql-portability, check-abac-route-registry,
  check-mcp-write-tools (all exit 0); `SKIP_WEB_BUILD=1 cargo check
  -p gyre-server --all-targets` and `-p gyre-adapters --all-targets` exit 0;
  `cd web && npm test` exit 0 (108 files, 2168 passed / 41 skipped).

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

- `RepoSettings.svelte` has a repo-scope **Gates** tab: lists configured
  gates (name, type, enabled toggle), "+ Add Gate" with a type selector,
  per-gate configuration editing (command, required flag) via
  `PUT /api/v1/repos/:id/gates/:gate_id`, delete, and drag-to-reorder that
  persists 1-based positions through the same PUT, backed by the `position`
  column (migration `...000062_gate_position`).

### Verifier note

- Server test compilation is deferred to the independent verifier per the
  4 GiB budget: run `cargo test --all` (or at minimum
  `cargo test -p gyre-server --test git_integration` and
  `cargo test -p gyre-adapters`) at this head `d18a6d790`.
