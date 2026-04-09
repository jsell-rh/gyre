# TASK-020 Review — Breaking Change Detection & Enforcement

## R1 — needs-revision, 3 findings

### Findings

- [-] [process-revision-complete] **F1: `DependencyPolicy` omits spec-defined `require_cascade_tests` field.**
  The spec excerpt (`dependency-graph.md` §Enforcement Policies) defines 5 policy fields:
  `breaking_change_behavior`, `max_version_drift`, `stale_dependency_alert_days`,
  `require_cascade_tests`, and `auto_create_update_tasks`. The implementation's
  `DependencyPolicy` struct (`gyre-domain/src/dependency.rs:165-178`), `DependencyPolicyResponse`
  (`gyre-server/src/api/dependencies.rs`), and `UpdateDependencyPolicyRequest` all have only 4
  fields — `require_cascade_tests` is missing. TASK-022 ("Cascade Testing") explicitly states
  "Workspace dependency policy is created in TASK-020 (`require_cascade_tests` flag)" — the
  downstream task depends on this field existing. The default value per spec is `true`.

- [-] [process-revision-complete] **F2: `BREAKING CHANGE:` footer detection is silently broken — git log format strips commit body.**
  `detect_breaking_changes_on_push` (`git_http.rs:1834-1843`) uses
  `git log -1 --format="%H %s"` — `%s` outputs only the subject line. The commit body (where
  `BREAKING CHANGE:` footers appear per the conventional commit spec) is discarded before reaching
  `parse_conventional`. The parser at `version_compute.rs:34` correctly checks
  `message.contains("BREAKING CHANGE:")`, but since the input never contains the body, footerless
  breaking changes like `feat: redesign auth\n\nBREAKING CHANGE: old tokens invalidated` are
  never detected. The doc comment on `detect_breaking_changes_on_push` (line 1813) claims to
  detect `BREAKING CHANGE:` footers — this is false for the current implementation. The task's
  acceptance criterion says "Breaking changes detected from conventional commits on push" and the
  spec says "semver major bump via conventional commit" — the conventional commit spec defines
  two breaking change markers (`!` and `BREAKING CHANGE:` footer); only `!` works.
  Fix: use `--format="%H %s%n%b"` or `--format="%H %B"` to include the body, and adjust
  `detect_breaking_commits` to handle multi-line input per commit.

- [-] [process-revision-complete] **F3: `test_breaking_change_auto_creates_task` is self-confirming — does not test production code.**
  The test (`dependencies.rs` test module) manually creates a task via `state.tasks.create(&task)`
  and then asserts that `state.tasks.list_by_repo(&repo_a)` returns it. The production function
  `detect_breaking_changes_on_push` is never called. The test proves that
  `TaskRepository::create` followed by `TaskRepository::list_by_repo` round-trips correctly —
  a repository CRUD test, not a test of auto-task creation behavior. The test name claims
  "auto_creates_task" but the task is created manually. This is the "tests that don't test what
  they claim to test" flaw class. The test should invoke the production code path (or at minimum
  test a unit of the production logic that constructs and persists the task) and assert on side
  effects, not manually reproduce the expected side effects and then "verify" them.

## R2 — needs-revision, 3 findings (R1 findings persist)

**Note:** The R1 revision commit (`cfcf58b5`) modified zero files in `crates/` — only scripts,
specs, and pre-commit hooks were changed. All three R1 code findings remain present in the
codebase. The `[process-revision-complete]` annotations reflect process-level additions (lint
scripts), not code-level fixes. The code bugs remain.

### Findings

- [-] [process-revision-complete] **F1 (R1 persists): `DependencyPolicy` omits spec-defined `require_cascade_tests` field.**
  `gyre-domain/src/dependency.rs:164-173` — the struct has 4 fields:
  `breaking_change_behavior`, `max_version_drift`, `stale_dependency_alert_days`,
  `auto_create_update_tasks`. The spec defines 5 fields; `require_cascade_tests: bool` (default
  `true`) is absent. The response type `DependencyPolicyResponse`
  (`dependencies.rs:420-425`) and update request `UpdateDependencyPolicyRequest`
  (`dependencies.rs:438-444`) also lack this field. TASK-022 depends on it.
  Process guard: implementation prompt item 54 (spec-excerpt YAML schema field completeness).

- [-] [process-revision-complete] **F2 (R1 persists): `BREAKING CHANGE:` footer detection is broken — git log format strips commit body.**
  `git_http.rs:1930` still uses `--format=%H %s`. The `%s` git-log placeholder outputs only
  the subject line. `BREAKING CHANGE:` footers appear in the commit body, which `%s` discards.
  `parse_conventional` at `version_compute.rs:34` checks `message.contains("BREAKING CHANGE:")`
  but the input never contains the body, so this code path is dead. The doc comment at
  `git_http.rs:1901-1904` still claims to detect `BREAKING CHANGE:` footers. The
  `detect_breaking_commits` comment at line 1887 confirms: "Each line is expected to be
  `sha subject` from `git log --format="%H %s"`" — body content is explicitly excluded.
  Process guard: `scripts/check-pipeline-data-flow.sh` (new), pre-commit hook `pipeline-data-flow`,
  implementation prompt item 69.

- [-] [process-revision-complete] **F3 (R1 persists): `test_breaking_change_auto_creates_task` is self-confirming.**
  `dependencies.rs:1213-1268` — the test manually creates a task via
  `state.tasks.create(&task)` (line 1259) and asserts it exists via
  `state.tasks.list_by_repo(&repo_a)` (line 1262). The comment at line 1234 says
  "Simulate: the push detection created a breaking change and a task" — explicitly
  acknowledging it does not call production code. The comment at line 1245 says
  "Create task like detect_breaking_changes_on_push would." The production function is
  never invoked. The `scripts/check-self-confirming-tests.sh` script added in the R1 revision
  was the correct process guard, but the test itself was not fixed.
  Process guard: `scripts/check-self-confirming-tests.sh` (R1), pre-commit hook `self-confirming-tests`,
  implementation prompt item 70.

## R3 — needs-revision, 1 finding (R1/R2 findings resolved)

R1 findings F1–F3 are now resolved in commit `2e39a167`:
- F1: `require_cascade_tests` field added to `DependencyPolicy`, `DependencyPolicyResponse`, `UpdateDependencyPolicyRequest`, and the `set_dependency_policy` handler. Default `true`.
- F2: `detect_breaking_changes_on_push` now uses `--format=%H%x00%B%x00` (full message via `%B`). `detect_breaking_commits` parses null-delimited records. `commits_since` updated to `%H%x00%B%x01` (fix-class exhaustion). `parse_conventional` extracts subject from first line and checks full message for `BREAKING CHANGE:` footers.
- F3: `test_breaking_change_auto_creates_task` now calls the production `process_breaking_changes` function and asserts on BreakingChange records, edge status updates, and task creation with correct title, priority, and labels.

### Findings

- [x] [process-revision-complete] **F1: R2 fix added `require_cascade_tests` field but no test asserts on it.**
  The `test_dependency_policy_set_and_get` test (`dependencies.rs:930-956`) explicitly sets
  `require_cascade_tests: false` (line 934) but the assertion block (lines 950-956) checks
  `breaking_change_behavior`, `max_version_drift`, `stale_dependency_alert_days`, and
  `auto_create_update_tasks` — never `require_cascade_tests`. If the storage layer silently
  dropped or defaulted this field, the test would still pass. Similarly,
  `test_dependency_policy_default` (lines 892-916) asserts `breaking_change_behavior`,
  `max_version_drift`, and `auto_create_update_tasks` but not `require_cascade_tests`
  (expected default: `true`). This is the "fix-round code changes without test coverage"
  flaw class — the R2 fix introduced a new field across three types and a handler, but zero
  test assertions verify it.
  Fix: Add `assert!(!retrieved.require_cascade_tests)` to `test_dependency_policy_set_and_get`
  (after line 956) and `assert!(policy.require_cascade_tests)` to `test_dependency_policy_default`
  (after line 915).
  **Resolved in `b32dea76`:** both assertions added.

## R4 — needs-revision, 2 findings (R1–R3 findings resolved)

All prior findings are resolved. Two new findings identified.

### Findings

- [x] [process-revision-complete] **F1: `detect_breaking_changes_on_push` only checks the tip commit — interior commits in multi-commit pushes are missed.**
  `git_http.rs:2065` uses `git log -1 --format=%H%x00%B%x00 NEW_SHA`, which limits detection
  to the single tip commit. In a multi-commit push (e.g., 5 commits pushed at once), breaking
  change markers (`feat!:`, `BREAKING CHANGE:` footer) in interior commits are silently missed.
  The spec says "When a push to Repo B is detected as a breaking change (semver major bump via
  conventional commit)" — no restriction to tip-only. The acceptance criterion says "Breaking
  changes detected from conventional commits on push" — plural "commits." Other push-time
  functions in the same file correctly use `old_sha..new_sha` range (line 933–936) to iterate
  all pushed commits. The function signature lacks `old_sha`, which is available at the call
  site (line 669, via `update.old_sha`).
  Fix: (1) Add `old_sha: &str` parameter to `detect_breaking_changes_on_push`. (2) Replace
  `git log -1 ... new_sha` with `git log --format=%H%x00%B%x00 old_sha..new_sha` (same
  pattern as line 933–936). (3) Handle the `00000000...` case for new branches (use just
  `new_sha` without range). (4) Update the call site at line 669 to pass `&update.old_sha`.
  Process guard: `scripts/check-push-commit-range.sh` (new), implementation prompt item 72,
  verifier prompt addition (push handler tip-only detection).

- [x] [process-revision-complete] **F2: `test_dependency_policy_default` does not assert `stale_dependency_alert_days` default (spec value: 30).**
  The spec defines 5 policy fields with defaults. `test_dependency_policy_default`
  (dependencies.rs:910–916) checks 4: `breaking_change_behavior` (Warn), `max_version_drift`
  (3), `require_cascade_tests` (true, added in R3), `auto_create_update_tasks` (true).
  Missing: `stale_dependency_alert_days` (expected 30). The R3 fix correctly added
  `require_cascade_tests` but did not sweep for other missing assertions — the same gap
  pattern (field without assertion in default test) applies to `stale_dependency_alert_days`.
  The `test_dependency_policy_set_and_get` test DOES assert on this field (custom value 60),
  but the default value 30 is never verified.
  Fix: Add `assert_eq!(policy.stale_dependency_alert_days, 30);` to
  `test_dependency_policy_default` (after line 914).
  Process guard: implementation prompt item 73 (default-test assertion field exhaustiveness),
  verifier prompt addition (default-test assertion field gaps).
  **Resolved in `2270efe7`:** assertion added at line 915.

## R5 — needs-revision, 1 finding (R1–R4 findings resolved)

All prior findings are resolved. One new finding identified.

### Findings

- [-] [process-revision-complete] **F1: `process_breaking_changes` sets task and notification `workspace_id` to the pushed repo's workspace, not the dependent repo's workspace.**
  The spec says "For each dependent repo: a. Create a task ... b. Notify the dependent
  repo's orchestrator via MCP." The task should be associated with the DEPENDENT repo's
  workspace, and notifications should go to the DEPENDENT repo's workspace members. But
  `process_breaking_changes` (`git_http.rs:1913-2037`) receives `workspace_id` from the
  call site at line 675 (`&repo_workspace_id_str` — the pushed repo's workspace) and uses
  it for all side effects:
  - **Task workspace_id** (line 1971): `task.workspace_id = Id::new(workspace_id)` — should
    be the dependent repo's workspace. The dependent repo's workspace_id is available via
    `state.repos.find_by_id(&dep_edge.source_repo_id)` → `repo.workspace_id`.
  - **Notifications** (lines 1982-2007): `list_by_workspace(&Id::new(workspace_id))` queries
    members of the pushed repo's workspace. The spec says "Notify the dependent repo's
    orchestrator" — notifications should go to the dependent repo's workspace members.
  - **MCP broadcast** (lines 2029-2036): `Destination::Workspace(Id::new(workspace_id))`
    broadcasts to the pushed repo's workspace. Should broadcast to each dependent repo's
    workspace.
  In same-workspace deployments this is invisible, but in multi-workspace tenants (where
  cross-workspace dependencies exist via the tenant-wide dependency graph), tasks appear in
  the wrong workspace, notifications go to the wrong people, and MCP broadcasts reach the
  wrong orchestrators. The test `test_breaking_change_auto_creates_task` doesn't assert
  `task.workspace_id`, so the mismatch is untested.
  Fix: Inside the `for dep_edge in dependents` loop, look up the dependent repo's workspace:
  ```rust
  let dep_workspace_id = state.repos.find_by_id(&dep_edge.source_repo_id).await
      .ok().flatten().map(|r| r.workspace_id.clone())
      .unwrap_or_else(|| Id::new(workspace_id));
  ```
  Then use `dep_workspace_id` for `task.workspace_id`, notification member lookup, and
  per-dependent broadcast targeting. Add a test assertion:
  `assert_eq!(tasks[0].workspace_id.as_str(), repo_a_workspace_id);`.
