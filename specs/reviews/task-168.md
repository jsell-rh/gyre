# Review — task-168 (Repo lifecycle — archive push rejection, admin repos tab, gate config UI)

Spec: `specs/system/repo-lifecycle.md` — §API Summary (git push rejection for
archived repos), §Admin → Workspace Scope → Repos Tab, §Gates (Admin → Repo
Scope → Gates).
Candidate under review: `c38551dd9e0fab1b2d427fbf8e95f18de10e789d` against
assignment base `fefde294`. This round re-reviews the merge head after the
prior finding `2956134c` (audit-locale regression dropped in the `c0b6a74e`
merge resolution) was fixed by `0237af2`.
Verdict: **approved**.

## Round 2 (this review) — candidate `c38551dd`

Independent probes (evidence under `/tmp/stage/review-evidence/`, suffix
`c38551dd`):

- **Prior finding `2956134c` resolved**: the `workspace_settings.audit` locale
  block is present with all 11 keys (`title`, `filter_label`,
  `all_event_types`, `loading_btn`, `refresh`, `loading`, `empty`, `col_type`,
  `col_actor`, `col_detail`, `col_time`).
- **Repo-wide i18n sweep** (probe re-implemented independently, not reused
  from the executor): 1391 static `$t()` references across 66 svelte files
  all resolve in `web/src/locales/en.json` — no sibling locale regressions of
  the `2956134c` class anywhere in the tree (`probe-i18n-sweep-c38551dd.log`,
  exit 0). No duplicate JSON keys remain in `en.json` (the two merged `repos`
  locale objects were unified, not left as last-wins dead keys).
- **§API Summary — archived push rejection**: `cargo test -p gyre-server
  --lib git_http` → 41/41 passed, including the three archived-repo tests
  (ERR pkt-line advertisement with exact framing asserted, upload-pack
  clone advertisement still served, receive-pack 403). Integration test
  `push_to_archived_repo_rejected` (real `git clone` + commit + `git push`
  against a live server on this sandbox's supported TCP listener) → 1/1
  passed, asserting the push **fails** and stderr names "archived".
  `resolve_repo_by_slug` performs the same tenant/workspace scoping as the
  `resolve_repo_path_by_slug` wrapper it replaces (the wrapper delegates to
  it).
- **§Repos Tab**: `WorkspaceSettings.svelte` `repos` tab shows name,
  Active/Archived status badge, active agent count, last activity, default
  branch, mirror URL; "+ New Repo" and "Import Repo" forms; repo-row click
  navigates to repo scope via the `goToRepo` context newly provided by
  `App.svelte`. The api methods used (`agents({workspaceId})`,
  `mergeRequests`, `workspaceRepos`, `createRepo`, `createMirrorRepo`) all
  exist in `web/src/lib/api.js` with matching signatures.
- **§Gates**: `RepoSettings.svelte` gates panel has per-gate enabled toggle
  (`PUT` `required`), per-gate-type configuration forms (command /
  required_approvals / persona + name + timeout), and drag-to-reorder
  persisting 1-based positions via `PUT /repos/:id/gates/:gate_id` with
  server reload on failure. Backend `gates::update_gate` validates
  per-gate-type fields (command only for test/lint, approvals only for
  required_approvals, persona only for agent gates), rejects gate/repo
  binding mismatch as NotFound, and the route reuses the existing
  `/api/v1/repos/:id/gates/:gate_id` registration already covered by the
  ABAC resolver's `"gate"` resource mapping. `cargo test -p gyre-server
  --lib gates` → 26/26 passed (position ordering, update validation).
  Ordering is enforced consistently in SQLite, Postgres, and the mem store
  (`(position, created_at)`); migration `2026-10-08-000059_gate_position`
  uses portable `ALTER TABLE ... ADD/DROP COLUMN`.
- **Frontend**: `npm ci` then `npx vitest run` on the two touched suites
  (`WorkspaceSettings.test.js`, `RepoSettings.test.js`) → 137/137 passed.
- **Invariants re-run on the candidate tree**: `check-migration-versions`
  (no duplicates), `check-mem-port-contracts`, `check-arch` → all OK.
- **Diff scope**: `git diff fefde294..c38551dd --stat` touches only
  task-168 surfaces (git_http, gates API + adapters + migration, the two
  Svelte components + tests + api.js + en.json, specs/tasks/task-168.md,
  process records); base `fefde294` content (task-089 HSI Explorer etc.)
  survives unchanged outside the conflict files, and the conflict files are
  covered by the i18n sweep and test suites above.

Findings: none.
