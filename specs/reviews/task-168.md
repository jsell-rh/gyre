# Review: TASK-168 — Repo lifecycle (archive push rejection, admin repos tab, gate config UI)

**Reviewer:** Independent review
**Date:** 2026-10-09
**Comparison base:** 2f215b1e58d633e666852bb1fc42f842d89ae68c
**Candidate:** 8309cc9bcca6e37a660dedf572f360a6bfee9447
**Verdict:** complete (approved)

---

## Findings

No material gaps. All three coverage sections are implemented in production
code, verified end-to-end, and mutation-tested. Evidence logs under
`/tmp/stage/review-evidence/`.

### §API Summary — archived repos reject pushes

- `git_info_refs` (crates/gyre-server/src/git_http.rs) rejects the
  `git-receive-pack` advertisement for archived repos with a 200 response
  whose pkt-line stream carries `ERR push rejected: repository is archived`
  — the correct git-smart-HTTP convention (a bare 403 body is never shown by
  `git push`). The repo is resolved via `resolve_repo_by_slug` (same
  tenant/workspace scoping as the previous path resolver; only the entity is
  additionally loaded), and `repo.is_archived()` (gyre-domain repository.rs,
  real `RepoStatus::Archived` comparison) gates it.
- `git_receive_pack` (POST) keeps a defense-in-depth 403 before the packfile
  is processed.
- Clone/fetch still works on archived repos (read-only, not invisible) —
  asserted by `info_refs_upload_pack_archived_repo_still_serves_clone_advertisement`.
- **End-to-end:** `push_to_archived_repo_rejected`
  (tests/git_integration.rs) archives a repo via the real API, clones it with
  a real `git` client, commits, and runs a real `git push` — the push fails
  with an "archived" reason. Passed against a live server
  (cargo-integration-archived-rerun.log).
- **Mutation-verified:** disabling both archived checks in an isolated
  worktree made both unit tests and the integration test fail
  ("push to archived repo should be rejected, but it succeeded") —
  mutation-archived-lib.log, mutation-archived-integration.log. The tests
  kill the bug; they are not self-confirming. The pkt-line framing assertion
  pins the exact wire bytes (`001f# service=git-receive-pack\n0000`), so a
  framing regression also fails it.

### §Admin → Workspace Scope → Repos Tab

- `WorkspaceSettings.svelte` gains a `repos` tab: table of all workspace
  repos (name, Active/Archived status badge, active-agent count, last
  activity) with "+ New Repo" and "Import Repo" forms wired to
  `api.createRepo`/`api.createMirrorRepo`, and click-through to repo scope
  via the new `goToRepo` context (set in App.svelte).
- The `status` field round-trips from the API (`RepoResponse.status:
  r.status.to_string()` → "Archived"/"Active"), so the badge is not reading a
  field the server never sends.
- 6 new vitest cases assert the tab renders, `api.workspaceRepos('ws-1')` is
  called, rows/badges/agent counts render from mocked data, and the
  create/import forms issue the exact expected API payloads.

### §Gates (Admin → Repo Scope → Gates)

- `RepoSettings.svelte` gates panel: gates listed via
  `GET /api/v1/repos/:id/gates` ordered by `position`; per-gate enabled/
  disabled toggle (`required`), per-gate configuration form with
  per-type fields (test/lint command, required approvals, reviewer persona,
  timeout), "+ Add Gate" with gate-type selector (pre-existing create form),
  and drag-to-reorder persisting 1-based positions via the new
  `PUT /api/v1/repos/:id/gates/:gate_id` (api.updateRepoGate). Reorder
  failure reloads from the server rather than keeping a phantom local order.
- Backend `gates::update_gate`: partial update with per-gate-type field
  validation (command only on TestCommand/LintCommand, approvals only on
  RequiredApprovals, persona only on AgentReview/AgentValidation),
  cross-repo gate access returns 404, type immutable. Route is
  `put(update_gate).delete(delete_gate)` on the already-resolver-registered
  pattern `/api/v1/repos/:id/gates/:gate_id` (ABAC mapping pre-existing for
  the DELETE; check-abac-route-registry passes).
- Gate ordering enforced at three layers — handler sort, SQLite/PG adapters
  (`ORDER BY position, created_at`), and the mem store — all keyed
  `(position, created_at)`. Migration `2026-10-08-000057_gate_position`
  (renumbered past the base-merge's `000056_spec_assertion_results`;
  `check-migration-versions` passes, no duplicates) is SQLite+PG portable
  (`ADD COLUMN position INTEGER NOT NULL DEFAULT 0`).
- **Mutation-verified:** with the handler sort alone disabled the ordering
  test still passed (store layer holds the contract); with handler + mem
  store sorts both disabled it failed — the test verifies the ordering
  behavior, not one implementation detail (mutation-gates-sort*.log).
- Frontend tests assert exact `api.updateRepoGate` payloads for toggle,
  config save (per-type field routing), and reorder (position renumbering,
  unchanged gate not re-PUT).

### Test suites

- `cargo test -p gyre-server --lib` (full, clean rebuild from candidate
  sources): **1257 passed, 0 failed** (cargo-server-lib-full-clean.log).
- `cargo test -p gyre-server --test git_integration push_to_archived_repo_rejected`: **1 passed**.
- `cd web && npm ci && npm test` (full): **65 files, 1615 passed, 41 skipped**
  (vitest-full.log); focused WorkspaceSettings + RepoSettings runs: 114/114.
- All 21 mechanical invariant checks pass on the candidate, including
  `check-fabricated-scope-defaults` (the exemption retarget to
  git_http.rs:1535 verified against the actual statement location on this
  tree) and `check-task-commit-attribution`.

### Repair-round verification (base merges 110180fb, 2f215b1e)

- Migration renumber verified: `000056_spec_assertion_results` (base) and
  `000057_gate_position` (candidate) coexist; `check-migration-versions` OK.
- `web/dist` rebuilt from merged source: bundle contains
  `getSpecAssertionResults` (base's feature) and the new repos-tab /
  gate-edit testids (candidate's features) — no stale dist.

### Process note: review-tooling contamination (not a candidate defect)

An intermediate round of this review observed 4 consecutive failures of
`list_gates_orders_by_position` in the full lib suite. These were traced to
review tooling, not the candidate: mutation-probe worktrees had been built
against the shared `/tmp/gyre-target`, and cargo's mtime freshness check made
the clean main checkout silently run the mutated binary afterwards (proven by
a controlled reproduction and the replayed `mem.rs:2280` "variable does not
need to be mutable" warning, which only exists when the sort line is absent).
After removing the worktrees and `cargo clean -p gyre-server`, a full rebuild
from the candidate's own sources passes 1257/1257. Full incident record and
logs: `/tmp/stage/review-evidence/contamination-incident.md`. Future mutation
probes must use a private `CARGO_TARGET_DIR`.
