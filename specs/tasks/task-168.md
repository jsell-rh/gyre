---
title: "Repo lifecycle — archive push rejection, admin repos tab, gate config UI"
spec_ref: "repo-lifecycle.md §API Summary"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "repo-lifecycle.md §Admin → Workspace Scope → Repos Tab"
  - "repo-lifecycle.md §Gates (Admin → Repo Scope → Gates)"
  - "repo-lifecycle.md §API Summary"
commits: ["219780c5aef6f0a9fc88da61185949decc8e97a8", "4443b66a81b9a29d6cfbf808f624cc76af19c1a6", "0106d3a3adfe0e4c7c175d81e0d301583a7b3255", "d8e78bb6c440c7ba24103f18be30dfbcd669f3b5", "730f9df11e81dfe1644d2b464cec1352f72bda35", "df5b32464a082ce008236a2732536755538ebe03", "9ebec6c2713ce824aa5ca9f14df74dcecb074437", "904586888416cb5c5aec355a70875b271e005a90", "36e401fbd899a48fc15a4bf430f25ef904747e1e", "b657692571ca3e6f53528afeec1578dfb46c9e7a", "06e11cbcf59b1fdd0247411c5e8910c4c6190714", "1da8dd39d018de0a101f550198ce224d077329f6", "4c11b2644f2435a87e60a38b8b8f356722562f14", "4b5d96563755dec7fbd825880c23661c604f7a8d", "5d5e4046f0af0cf074348b584d632e36f3f6b570", "0a2df7c6a80141bc885937dc3543e13055bf8ee3", "568d27f2cc4ca542ebe3b138872b710dd693a5e0", "68805172e3c0abb0315928d7f24d3ba2aaa16673", "0237af23cb7403ef2489b1247a52f2601f1bab66"]
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

All three coverage sections are implemented in production code (earlier
checkpoints `80d6563`/`6194782`, the `0106d3a` test-framing fix, and the
`5e617c7`/`4443b66`/`219780c`/`40b0f8e`/`25fe58fa` repair rounds — see
Repair rounds). Current tree: merge of assignment base `fefde294` at
`c0b6a74e` plus the process commit recording this round, then fix `0237af2`
restoring the audit locale block (review finding `2956134c` — see Repair
rounds; evidence for the fix under `/tmp/stage/review-evidence/`, suffix
`audit-locale-fix`).

### Git push rejection for archived repos (§API Summary)

- `git_info_refs` rejects the `git-receive-pack` advertisement for archived
  repos with a 200 response whose pkt-line stream carries
  `ERR push rejected: repository is archived` — the git-smart-HTTP convention
  so `git push` actually prints the reason (a bare 403 body is never shown by
  git). `git_receive_pack` (POST) keeps a defense-in-depth 403 before the
  packfile is processed; clone/fetch (`git-upload-pack`) still works on
  archived repos — read-only, not invisible.
- Evidence on the current tree: git_http unit tests 41/41 passed
  (`cargo-git-http-basefefde294.log`, includes the three archived-repo
  pkt-line/403/clone-still-works tests), and integration test
  `push_to_archived_repo_rejected` in `crates/gyre-server/tests/git_integration.rs`
  which runs a **real `git push`** against a live server and asserts the push
  fails with an "archived" reason (1/1 passed,
  `cargo-integration-archived-push-basefefde294.log`).

### Admin workspace-scope Repos tab (§Admin → Workspace Scope → Repos Tab)

- `WorkspaceSettings.svelte` has a `repos` tab: table of all workspace repos
  with name, Active/Archived status badge, active agent count, last-activity
  timestamp, default branch, and mirror URL; "+ New Repo" and "Import Repo"
  buttons with creation/mirror forms; clicking a repo navigates to repo scope
  via the `goToRepo` context.
- Evidence: full frontend suite on the current tree — 1948 passed / 41
  skipped across 89 files (`vitest-task168-repair-basefefde294.log`, exit 0),
  including both repos-tab test suites in
  `web/src/__tests__/WorkspaceSettings.test.js` + `RepoSettings.test.js`
  (137/137 in those two files).

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
- Evidence: gates API unit tests (position ordering, update validation) —
  26 gates-filtered tests + 7 recovery tests passed on the current tree
  (`cargo-gates-recovery-basefefde294.log`).

### Repair rounds

- `5e617c7`: resolved base `110180fb` (only generated `web/dist` conflicts);
  retargeted a fabricated-scope exemption line number (count unchanged).
- `4443b66`: resolved base `2f215b1e` — renumbered our migration to
  sequence `000057` after main landed `000056_spec_assertion_results`.
- `219780c`: resolved base `1d9547de`; formatted rustfmt changed-lines
  violations and deleted the obsolete `git_http.rs` fabricated-scope exemption
  (the base fixed the site itself; frozen count 5 → 4).
- `40b0f8e`: resolved base `66a5fc67` — additive main-side work only; all
  task-168 source survived byte-for-byte.
- **Round `25fe58fa`**: resolved assignment base `124a852` (task-193 spec
  approval attestation). Union `TAB_IDS` (base's `policies` tab plus our
  `repos`); renumbered `2026-10-08-000057_gate_position` → `000058` after
  base landed `000057_spec_approval_attestation`.
- **This round (merge `c0b6a74e`)**: resolved assignment base `fefde294`
  (task-089 HSI Explorer Generated Views). Conflicts and resolutions:
  - `fabricated-scope-defaults-exemptions.txt`: base fixed the `auth.rs`
    JWT claims fallback (task-103) and lowered the frozen count 4 → 3 — took
    base's side, entry removed.
  - `WorkspaceSettings.svelte`: base independently shipped a simpler repos
    tab (HSI §1.5), so the auto-merge produced TWO repos script/markup
    sections (duplicate `let` declarations — uncompilable) plus a `TAB_IDS`
    collision. Resolution: **one** repos implementation — ours (agent counts,
    last activity, status badge, repo navigation) extended with base's
    default-branch and mirror-URL columns; base's duplicate script block,
    effect branch, and markup deleted; `TAB_IDS` is the union (10 tabs:
    base's `secrets` tab added, `repos` after `budget`); the two `repos`
    locale objects in `en.json` merged into one union object (duplicate JSON
    keys are last-wins, so base's were dead).
  - `WorkspaceSettings.test.js`: the merged file carries both repos-tab
    suites; base's suite retargeted to the surviving implementation's
    testids (`ws-`-prefixed) with identical behavioral assertions (create →
    `api.createRepo`, import → `api.createMirrorRepo` with derived name,
    row content includes mirror URL — now a real column). Two keyboard-nav
    tests asserted `repos` as the *last* tab; with the union tab order the
    last tab is `audit`, so they now assert `#ws-tab-audit` (same
    wrap-to-last / End-key behavior). Tab-count test 9 → 10.
  - `RepoSettings.test.js`: mock union (our gate/MR mocks + base's secrets
    mocks).
  - `recovery.rs`: base's new test `QualityGate` literals gained our
    `position: 0` field (E0063 fix, matches the production default).
  - Renumbered `2026-10-08-000058_gate_position` → `000059` after base
    landed `000058_user_entity_profile_fields` (duplicate Diesel versions
    silently never run). rustfmt clean.
- **Review finding `2956134c` (audit locale regression, fix `0237af2`)**: the
  `c0b6a74e` merge-resolution unified the two `repos` locale objects but
  dropped the base's `workspace_settings.audit` block, leaving all 11
  audit-tab `$t()` keys in `WorkspaceSettings.svelte` unresolved — the
  Audit tab rendered raw dotted keys while the test suite stayed green
  (tests assert testids, not localized text). Fix: restored the base's
  11-key `audit` object verbatim into `en.json` (between `compute` and
  `save_failed_settings`, matching base key order/formatting — a surgical
  13-line insertion; JSON round-trip rewrites were rejected because the
  file mixes literal Unicode and `\uXXXX` escapes and json.dump cannot
  preserve that mixture). Evidence: the reviewer's probe re-implemented
  (`probe-ws-i18n-fixed.log`) — all 106 `workspace_settings.*` keys resolve,
  plus a repo-wide sweep of all 1244 static `$t()` references across 66
  svelte files (`probe-i18n-repo-wide.log`), and
  `WorkspaceSettings.test.js` 78/78 passed on the fixed tree.
- **Review finding `fddd589d` (stale committed `web/dist`)**: acknowledged and
  verified at the source level. `web/dist` is a generated artifact — in this
  runtime the executor restores it from the assignment base and final
  integration rebuilds and publishes it remotely, so it is deliberately not
  committed here. The task-168 UI is fully present in `web/src` (the source
  RustEmbed/build.rs build from): the `cargo check -p gyre-server` in this
  round triggered `crates/gyre-server/build.rs` (`npm ci` + vite) from this
  exact merged source and the web build succeeded, embedding the Repos tab
  and Gates panel. A `SKIP_WEB_BUILD=1` build against a stale dist is a
  deployment concern owned by the pipeline's remote publication stage, which
  rebuilds dist from this source.

### Invariants

All 17 mechanical invariant checks pass at merge head `c0b6a74e` on the merged
tree (`invariants-basefefde294.log` + per-check logs under
`/tmp/stage/review-evidence/`) — `check-arch`,
`check-abac-route-registry`, `check-migration-versions` (no duplicates
after the `000059` renumber), `check-migration-sql-portability`,
`check-mem-port-contracts`, `check-inert-enforcement`,
`check-dead-message-kinds`, `check-fabricated-scope-defaults`
(3 entries, frozen count 3 — base fixed the auth.rs site),
`check-byte-slice-truncation`,
`check-relative-path-defaults`, `check-fail-open-ref-resolution`,
`check-lossy-secret-conversion`, `check-scope-literal-defaults`,
`check-forwarded-header-trust`, `check-in-memory-state-stores`,
`check-unbounded-external-http`, `check-forged-scope-fields` — plus
`check-task-commit-attribution.sh` (passed post-commit). All 18 re-run at
the audit-locale-fix head `0237af2` on the working tree — 18/18 PASS
(`invariants-audit-locale-fix.log`).
