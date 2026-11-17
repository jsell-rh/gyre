---
title: "Repo lifecycle — archive push rejection, admin repos tab, gate config UI"
spec_ref: "repo-lifecycle.md §API Summary"
depends_on: []
progress: complete
coverage_sections:
  - "repo-lifecycle.md §Admin → Workspace Scope → Repos Tab"
  - "repo-lifecycle.md §Gates (Admin → Repo Scope → Gates)"
  - "repo-lifecycle.md §API Summary"
commits: ["3fd5b97a0a997e2f58072cddd0727b08fb81ef65", "c328b9e0c8b982894b5960435b05335c50ba405c", "3399157f03203b1bb57e7924670917b78d4f544a", "26fc7b32667bf8d3121ec07f837d1a960d0681d7", "0237af23cb7403ef2489b1247a52f2601f1bab66", "219780c5aef6f0a9fc88da61185949decc8e97a8", "4443b66a81b9a29d6cfbf808f624cc76af19c1a6", "0106d3a3adfe0e4c7c175d81e0d301583a7b3255", "d8e78bb6c440c7ba24103f18be30dfbcd669f3b5", "730f9df11e81dfe1644d2b464cec1352f72bda35", "df5b32464a082ce008236a2732536755538ebe03", "9ebec6c2713ce824aa5ca9f14df74dcecb074437", "904586888416cb5c5aec355a70875b271e005a90", "36e401fbd899a48fc15a4bf430f25ef904747e1e", "b657692571ca3e6f53528afeec1578dfb46c9e7a", "06e11cbcf59b1fdd0247411c5e8910c4c6190714", "1da8dd39d018de0a101f550198ce224d077329f6", "4c11b2644f2435a87e60a38b8b8f356722562f14", "4b5d96563755dec7fbd825880c23661c604f7a8d", "5d5e4046f0af0cf074348b584d632e36f3f6b570", "0a2df7c6a80141bc885937dc3543e13055bf8ee3", "568d27f2cc4ca542ebe3b138872b710dd693a5e0", "68805172e3c0abb0315928d7f24d3ba2aaa16673", "db2196eb8234245db6f176a2953dc411db07102c", "159b7408a12cef7990486878ffa33bf07c880c61"]
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

All three coverage sections are implemented in production code (implementation
chain in `commits:`; rebase repairs merged at `d18a6d790` and `d673353f2`;
`159b7408` is task-195's post-merge repair of this task's `QualityGate.position`
addition — recorded here because its subject references task-168 and the
attribution check scopes every referenced task). Acceptance boxes are left
exactly as assigned — checking them in place mutates
the protected contract text; completion is recorded here and in `progress:`.

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

### Gate configuration UI (§Gates (Admin → Repo Scope → Gates))

- `RepoSettings.svelte` has a repo-scope **Gates** tab: lists configured
  gates (name, type, enabled toggle), "+ Add Gate" with a type selector,
  per-gate configuration editing (command, required flag) via
  `PUT /api/v1/repos/:id/gates/:gate_id`, delete, and drag-to-reorder that
  persists 1-based positions through the same PUT, backed by the `position`
  column (`2026-10-08-000062_gate_position` migration).

### Round: base merge e402e1ed (rebase repair, finding 48e7f1d483d24f0eba516ecb8097da99)

- Resolved the merge against current main (task-121 workspace membership /
  repo access enforcement) at merge commit `d673353f2`. The only source
  conflict was `crates/gyre-server/src/git_http.rs` (4 regions); every region
  was resolved as the union of both sides — task-168's archived-repo push
  rejection (info/refs ERR pkt-line + receive-pack 403) is kept, and main's
  `authorize_repo_access()` membership/override enforcement runs on
  clone/fetch (Read) and push (Write) after the archived check. The
  `web/dist` asset conflict was resolved toward the staged merge state; dist
  is pipeline-owned and regenerated by final integration.
- Fresh merged-head evidence under `/tmp/stage/review-evidence/
  task-168-merge-e402e1ed/`: `SKIP_WEB_BUILD=1 cargo check -p gyre-server`
  exit 0; `check-migration-versions.sh` exit 0 (task-121's
  `2026-11-12-000061_repo_access_overrides` does not collide — Diesel
  versions include the full date prefix); `check-arch.sh` exit 0;
  `cd web && npm ci && npm test` exit 0 (110 files, 2218 passed / 41 skipped).
- Server test compilation exceeds this sandbox's 4 GiB budget (build-wrapper
  restriction, not a code defect) — verifier must run
  `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib git_http` and
  `SKIP_WEB_BUILD=1 cargo test -p gyre-server --test git_integration
  push_to_archived_repo_rejected` (both passed at the pre-merge head
  `fe352c1` under independent approval).

### Round: contract repair (finding 7bb204fe4b55471fa94c5b0a65690cb5)

- The protected task text drifted in the prior checkpoint (the Gates coverage
  section had been renamed `§Admin → Repo Scope → Gates`). The executor
  restored the assigned contract; this round reconciles the document without
  touching production code: the assigned contract text and coverage-section
  names are preserved verbatim, `progress`/`commits` bookkeeping and these
  `## Shipped` notes (exact heading, `###` subheadings) are restored on top.
- No source changes: the source tree is byte-identical to the checkpointed
  candidate `4f6f0bce5` (this round adds only task-doc commits on top).
  Presence probes for this round are recorded under `/tmp/stage/review-evidence/task-168-contract-repair/`
  (archived-rejection sites in `git_http.rs`, the
  `push_to_archived_repo_rejected` integration test, and the
  Repos/Gates UI surfaces). Server build/test probes are unchanged from the
  base-merge round above — the verifier should re-run the two focused server
  test commands listed there at the final head.
