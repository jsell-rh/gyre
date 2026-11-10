---
title: "Repo lifecycle — archive push rejection, admin repos tab, gate config UI"
spec_ref: "repo-lifecycle.md §API Summary"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "repo-lifecycle.md §Admin → Workspace Scope → Repos Tab"
  - "repo-lifecycle.md §Gates (Admin → Repo Scope → Gates)"
  - "repo-lifecycle.md §API Summary"
commits: ["0237af23cb7403ef2489b1247a52f2601f1bab66", "219780c5aef6f0a9fc88da61185949decc8e97a8", "4443b66a81b9a29d6cfbf808f624cc76af19c1a6", "0106d3a3adfe0e4c7c175d81e0d301583a7b3255", "d8e78bb6c440c7ba24103f18be30dfbcd669f3b5", "730f9df11e81dfe1644d2b464cec1352f72bda35", "df5b32464a082ce008236a2732536755538ebe03", "9ebec6c2713ce824aa5ca9f14df74dcecb074437", "904586888416cb5c5aec355a70875b271e005a90", "36e401fbd899a48fc15a4bf430f25ef904747e1e", "b657692571ca3e6f53528afeec1578dfb46c9e7a", "06e11cbcf59b1fdd0247411c5e8910c4c6190714", "1da8dd39d018de0a101f550198ce224d077329f6", "4c11b2644f2435a87e60a38b8b8f356722562f14", "4b5d96563755dec7fbd825880c23661c604f7a8d", "5d5e4046f0af0cf074348b584d632e36f3f6b570", "0a2df7c6a80141bc885937dc3543e13055bf8ee3", "568d27f2cc4ca542ebe3b138872b710dd693a5e0", "68805172e3c0abb0315928d7f24d3ba2aaa16673"]
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

All three coverage sections are implemented in production code. Current tree:
merge of assignment base `b5a87208` (task-108 admin user impersonation) at
`f425d2c6`, on top of the earlier checkpoints (`80d6563`/`6194782`, the
`0106d3a` test-framing fix, the `5e617c7`/`4443b66`/`219780c`/`40b0f8e`/
`25fe58fa`/`c0b6a74e` repair rounds, and the `0237af2` audit-locale fix for
review finding `2956134c`). Evidence for this round under
`/tmp/stage/review-evidence/`, suffix `baseb5a87208`.

### Git push rejection for archived repos (§API Summary)

- `git_info_refs` rejects the `git-receive-pack` advertisement for archived
  repos with a 200 response whose pkt-line stream carries
  `ERR push rejected: repository is archived` — the git-smart-HTTP convention
  so `git push` actually prints the reason (a bare 403 body is never shown by
  git). `git_receive_pack` (POST) keeps a defense-in-depth 403 before the
  packfile is processed; clone/fetch (`git-upload-pack`) still works on
  archived repos — read-only, not invisible.
- Evidence on the current tree: git_http unit tests 57/57 passed
  (`cargo-git-http-baseb5a87208.log`, includes the three archived-repo
  pkt-line/403/clone-still-works tests), and integration test
  `push_to_archived_repo_rejected` in `crates/gyre-server/tests/git_integration.rs`
  which runs a **real `git push`** against a live server and asserts the push
  fails with an "archived" reason (1/1 passed,
  `cargo-archived-push-baseb5a87208.log`).

### Admin workspace-scope Repos tab (§Admin → Workspace Scope → Repos Tab)

- `WorkspaceSettings.svelte` has a `repos` tab: table of all workspace repos
  with name, Active/Archived status badge, active agent count, last-activity
  timestamp, default branch, and mirror URL; "+ New Repo" and "Import Repo"
  buttons with creation/mirror forms; clicking a repo navigates to repo scope
  via the `goToRepo` context.
- Evidence on the current tree: `WorkspaceSettings.test.js` +
  `RepoSettings.test.js` — 137/137 passed
  (`vitest-task168-settings-baseb5a87208.log`); the full frontend suite
  (1948 passed / 41 skipped, 89 files) passed at the pre-merge head
  (`vitest-task168-repair-basefefde294.log`), and this merge did not touch
  the settings components (only `api.js`/`en.json` auto-merged cleanly).

### Repo-scope Gates panel (§Gates — Admin → Repo Scope → Gates)

- `RepoSettings.svelte` has a Gates panel: lists gates via
  `GET /api/v1/repos/:id/gates` ordered by `position`, per-gate enabled toggle,
  configuration fields (test/lint command, required approvals, reviewer
  persona, timeout), "+ Add Gate" with gate-type selector, and drag-to-reorder
  saving positions via `PUT /api/v1/repos/:id/gates/:gate_id`.
- Backend: `PUT /api/v1/repos/:id/gates/:gate_id` (`gates::update_gate`) with
  per-gate-type field validation; gate ordering via the `position` column
  (migration `2026-10-08-000059_gate_position`, SQLite + PostgreSQL portable)
  enforced in both adapters and the mem store.
- Evidence on the current tree: 31 gates-filtered + 12 recovery-filtered
  lib tests passed (`cargo test -p gyre-server --lib gates|recovery`).

### Repair rounds

- `5e617c7` (base `110180fb`), `4443b66` (base `2f215b1e`, migration
  renumber to `000057`), `219780c` (base `1d9547de`, rustfmt + exemption
  cleanup), `40b0f8e` (base `66a5fc67`, additive only).
- `25fe58fa` (base `124a852`): union `TAB_IDS`; migration renumber to `000058`.
- `c0b6a74e` (base `fefde294`, task-089 HSI Explorer Generated Views): unified
  the two independently-shipped repos tabs into one implementation (ours
  extended with base's default-branch and mirror-URL columns), union
  `TAB_IDS`/locale objects, merged both repos-tab test suites (base's
  retargeted to the surviving `ws-` testids), migration renumber to `000059`.
- `0237af2` (review finding `2956134c`): restored the base's 11-key
  `workspace_settings.audit` locale block that the `c0b6a74e` locale
  unification dropped (Audit tab rendered raw dotted keys; tests assert
  testids, not localized text, so the suite stayed green). Evidence:
  `probe-ws-i18n-fixed.log`, `probe-i18n-repo-wide.log` (all 1244 static
  `$t()` references across 66 svelte files resolve), 78/78
  `WorkspaceSettings.test.js`.
- Review finding `fddd589d` (stale committed `web/dist`): `web/dist` is a
  generated artifact restored by the executor from the assignment base and
  rebuilt/published remotely by final integration; the task-168 UI is fully
  present in `web/src`, which `build.rs` builds from.
- **This round (`f425d2c6`, base `b5a87208`, task-108 impersonation)**: one
  content conflict in `crates/gyre-server/src/git_http.rs`. Base refactored
  `resolve_repo_by_slug` to take `&auth` (impersonation-aware tenant
  resolution) and added a path-only wrapper `resolve_repo_path_by_slug`; our
  archived-repo checks need the full `Repository` entity. Resolution: call
  the new-signature `resolve_repo_by_slug(&state, &auth, ...)` directly (all
  four call sites in the file already use it), keep the full repo for
  `is_archived()`/`repo.path`, and delete the now-unused
  `resolve_repo_path_by_slug` wrapper (dead code after the merge). rustfmt
  clean; `cargo check -p gyre-server` clean.

### Invariants

All 17 mechanical invariant checks pass at merge head `f425d2c6` on the
merged tree (`invariants-baseb5a87208.log` + per-check logs under
`/tmp/stage/review-evidence/`) — `check-arch`,
`check-abac-route-registry`, `check-migration-versions` (no duplicates;
Diesel versions are date+sequence, so base's `2026-10-09-000056/57` and
`2026-11-08-000058` are distinct from the pre-existing entries),
`check-migration-sql-portability`, `check-mem-port-contracts`,
`check-inert-enforcement`, `check-dead-message-kinds`,
`check-fabricated-scope-defaults`, `check-byte-slice-truncation`,
`check-relative-path-defaults`, `check-fail-open-ref-resolution`,
`check-lossy-secret-conversion`, `check-scope-literal-defaults`,
`check-forwarded-header-trust`, `check-in-memory-state-stores`,
`check-unbounded-external-http`, `check-forged-scope-fields` — plus
`check-task-commit-attribution.sh`.
