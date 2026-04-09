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
