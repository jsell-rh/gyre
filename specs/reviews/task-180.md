# Review: TASK-180 — Executable Spec Assertions

**Reviewer:** Verifier
**Date:** 2026-10-09 (R13, resumed round — mutation probes completed)
**Comparison base:** 66422bd4b99de70536cce8422ec23db5eaec082d
**HEAD:** 51f29e95645e6a7b394d900aa96d9963c14ee744
**Verdict:** complete

---

## Findings

No material gaps. All §9 acceptance criteria are enforced by production code wired
through the real push entry points, and the notification behaviors are
mutation-verified.

### Behavior verification

- [x] **Parser.** `parse_assertions` (gyre-domain/src/spec_assertions.rs) extracts
  `<!-- gyre:assert ... -->` comments; attribute form maps all three §9 types
  (`no_dependency` → `Module/NotDependsOn`, `implements` → `Type/Implements`,
  `all_have` → `NodesOfType/HasProperty`). Malformed attribute assertions are
  skipped, not silently passed. The exact §9 example block parses to 3 assertions
  with correct lines (`parse_attribute_spec_example_block`).
- [x] **Evaluator, fail-closed semantics.** `all_have` fails when zero subject
  nodes exist (`eval_attribute_all_have_no_subject_nodes_fails` — no vacuous
  pass) and on unknown property (`eval_attribute_all_have_unknown_property_fails`).
  `no_dependency`/`implements` are real edge queries against nodes+edges
  (`eval_depends_on` negate branch, `eval_implements`), not structural checks.
  `NodeType::from_str_name` (gyre-common/src/graph.rs) accepts the spec's
  PascalCase `"Endpoint"` plus the snake_case wire name for every variant.
- [x] **Push integration — all three push paths wired.** `extract_and_store_graph`
  now takes `Arc<dyn SpecAssertionResultRepository>` + `PushNotificationScope` and
  runs `check_spec_assertions_on_push` post-extraction (graph_extraction.rs:384).
  Callers updated: git HTTP receive-pack (git_http.rs:755-770), mirror sync
  (mirror_sync.rs:73-93), and initial mirror clone (repos.rs:486-527). This is
  durability through the real entry point, not a test-only path.
- [x] **Persistence.** New port `SpecAssertionResultRepository` with SQLite and
  PostgreSQL adapters + migration 2026-10-08-000056 (next unused sequence;
  portable SQL — no dialect-only functions). Replace semantics: prior rows for
  `(repo_id, spec_path)` are deleted inside a transaction before insert, so
  assertions removed from a spec don't linger (mem adapter enforces identical
  semantics — `check-mem-port-contracts` passes). Repo delete cleans up orphaned
  rows (`delete_repo_removes_spec_assertion_results` test).
- [x] **Priority-9 Inbox notifications.** `NotificationType::SpecAssertionFailure`
  defaults to priority 9 (notification.rs:135). `check_spec_assertions_on_push`
  creates one notification per user per push (Admin/Developer/Owner in the repo's
  workspace; Viewer excluded — asserted in test) carrying the failing spec path,
  commit SHA, and failure list; `entity_ref` links the first failing spec so
  Inbox "Update Spec" opens it (Inbox.svelte:634-643 `handleViewSpec` uses
  `body.spec_path`). Duplicate suppression: same-commit re-extraction
  (mirror-sync cycles) does not re-notify; a new commit does (both asserted).
- [x] **API endpoints.** `GET /api/v1/repos/:id/specs/:path/assertions` returns
  persisted last-push results (URL-encoded path, canonical spec identity,
  prefixed form tolerated); unknown repo 404s. `POST .../spec-assertions/check`
  (live check) pre-existed and is unchanged in behavior.
- [x] **Inline view ✓/✗.** ExplorerView.svelte:1666-1674 renders
  `result.passed ? '✔' : '✘'` per assertion with pass/fail styling and a summary
  (`N failing, M passing`); live check falls back to persisted GET results when
  the check endpoint is unavailable (ExplorerView.svelte:205-216).
- [x] **ABAC — strengthened, not weakened.** The previously exempt
  `/api/v1/repos/:id/spec-assertions/check` route moved from
  `abac-route-registry-exemptions.txt` into the resolver
  (`"spec"`, action `write`) with `FROZEN_EXEMPTION_COUNT` 53→52 (shrinkage is
  permitted; growth fails). The new GET route is resolver-mapped
  (`"spec"`, action `None` — read). No duplicate resolver entries (first-match
  shadowing checked: single entry per pattern). `check-abac-route-registry.sh` passes.
- [x] **No fabricated scope.** Mirror-sync paths resolve workspace/tenant from
  the repo's own workspace record (`workspace_tenant_id.map(...)` — no
  `"default"` literal fallback); the scope is `None` and notification silently
  skipped when the workspace cannot be resolved, matching the
  `check-fabricated-scope-defaults` invariant.

### Mutation evidence (both kill the test)

Isolated worktree `/tmp/gyre-mut` (revision 51f29e9) with private
`CARGO_TARGET_DIR=/tmp/gyre-mut-target`; evidence in
`/tmp/stage/review-evidence/task180-mutation-runs.md`:

- **Mutation #1** (prior round, handoff-reported): `Notification::new` default
  priority for `SpecAssertionFailure` changed →
  `push_check_creates_priority9_notifications_for_failed_assertions` FAILED.
  Priority-9 is genuinely asserted.
- **Mutation #2** (this round, re-run after build completed):
  `already_recorded_for_commit = !prior_results.is_empty()` → `true`
  (suppresses all notifications) → test FAILED at "developer must be notified"
  (left: 0, right: 1). Sibling persistence test correctly still passed.
  Mutation reverted; clean-source baseline in the same worktree/target dir:
  `2 passed; 0 failed`. Worktree left clean (`git status --short` empty).

### Focused test runs (main worktree, /tmp/gyre-target)

- `cargo test -p gyre-domain --lib spec_assertions` — 48 passed.
- `cargo test -p gyre-server --lib spec_assertions` — 6 passed.
- `cargo test -p gyre-server --lib delete_repo_removes` — 1 passed.
- `cargo test -p gyre-server --lib push_check` — 2 passed (baseline, mut worktree).
- `cd web && npx vitest run src/__tests__/Inbox.test.js` — 28 passed.
- Invariant gates: check-arch, check-mem-port-contracts, check-in-memory-state-stores,
  check-migration-versions, check-migration-sql-portability, check-dead-message-kinds,
  check-abac-route-registry — all pass.

### Commit attribution

All 8 attributed commits exist, are descendants of the comparison base, and are
the only commits touching product files (crates/web/scripts/docs) in
66422bd4..51f29e9 — no unlisted product commits.
