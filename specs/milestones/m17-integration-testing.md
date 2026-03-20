# M17: Comprehensive Integration & E2E Testing

## Goal

Verify that everything built in M0–M16 actually works end-to-end with mechanically enforced
test coverage. Move from "it compiles and unit tests pass" to "every API contract, auth flow,
git operation, merge lifecycle, and UI interaction is exercised by automated tests that block
merge when they break."

The north star: a green CI run on `main` means the system is production-ready, not just
compile-clean.

---

## Background

Gyre has 606+ Rust unit tests and 86 frontend vitest component tests. These cover individual
functions and components in isolation. What is missing:

- **REST API contract tests**: does every endpoint return the documented status codes, shapes,
  and error responses?
- **Git protocol tests**: do clone, push, auth, and gate enforcement work end-to-end over real
  TCP connections?
- **Merge queue system tests**: does the full enqueue → gate execution → auto-merge pipeline
  work with real repos and real commands?
- **Auth integration tests**: do all four auth mechanisms reject unauthorized callers and
  enforce RBAC correctly?
- **Frontend E2E tests**: does the SPA actually load, render data, and let users perform CRUD
  operations without JavaScript errors?
- **CI enforcement**: are all of the above required to pass before a PR can merge?

M17 closes all these gaps. Existing unit tests are not removed or replaced — they continue to
run. M17 adds integration and E2E layers on top.

---

## Reference

- Existing e2e test: `crates/gyre-server/tests/e2e_ralph_loop.rs` (full Ralph loop)
- Existing WS test: `crates/gyre-cli/tests/ws_integration.rs`
- Endpoint table: `CLAUDE.md` (Server Endpoints section)
- Auth spec: `CLAUDE.md` (Authentication section)
- Merge queue: `specs/milestones/m12-quality-gates.md`
- Agent gates: `specs/system/agent-gates.md`

---

## Deliverables

### M17.1: REST API Integration Tests

**Location:** `crates/gyre-server/tests/api_integration.rs`

Cover every `/api/v1/` endpoint documented in `CLAUDE.md`. Each endpoint gets:
1. A **happy path** test with valid input — assert status code and response shape.
2. An **auth rejection** test — no token or wrong token returns `401`.
3. An **invalid input** test — malformed body or missing required fields returns `4xx` with
   an error payload containing an `"error"` key.

#### Endpoint Coverage Checklist

| Group | Endpoints | Happy | Auth | Invalid |
|---|---|---|---|---|
| Health | `GET /health`, `GET /healthz`, `GET /readyz`, `GET /api/v1/version` | ✓ | — | — |
| Projects | `POST/GET /api/v1/projects`, `GET/PUT/DELETE /api/v1/projects/{id}` | ✓ | ✓ | ✓ |
| Repos | `POST/GET /api/v1/repos`, `GET /api/v1/repos/{id}`, `DELETE /api/v1/repos/{id}` | ✓ | ✓ | ✓ |
| Repo Branches | `GET /api/v1/repos/{id}/branches` | ✓ | ✓ | — |
| Repo Commits | `GET /api/v1/repos/{id}/commits` | ✓ | ✓ | — |
| Repo Diff | `GET /api/v1/repos/{id}/diff` | ✓ | ✓ | ✓ |
| Repo Gates | `POST/GET /api/v1/repos/{id}/gates`, `DELETE .../gates/{gate_id}` | ✓ | ✓ | ✓ |
| Push Gates | `GET/PUT /api/v1/repos/{id}/push-gates` | ✓ | ✓ | ✓ |
| Blame/Hot-Files | `GET /api/v1/repos/{id}/blame`, `GET .../hot-files`, `GET .../review-routing` | ✓ | ✓ | ✓ |
| Speculative | `GET /api/v1/repos/{id}/speculative`, `GET .../speculative/{branch}` | ✓ | ✓ | — |
| Stack Policy | `GET/PUT /api/v1/repos/{id}/stack-policy` | ✓ | ✓ | ✓ |
| AIBOM | `GET /api/v1/repos/{id}/aibom` | ✓ | ✓ | ✓ |
| Mirror | `POST /api/v1/repos/mirror`, `POST .../mirror/sync` | ✓ | ✓ | ✓ |
| Worktrees | `POST/GET /api/v1/repos/{id}/worktrees`, `DELETE .../worktrees/{id}` | ✓ | ✓ | — |
| Agent Commits | `POST/GET /api/v1/repos/{id}/commits/record`, `GET .../agent-commits` | ✓ | ✓ | — |
| JJ | `POST/GET /api/v1/repos/{id}/jj/*` | ✓ | ✓ | — |
| Agents | `POST/GET /api/v1/agents`, `GET/PUT .../agents/{id}`, heartbeat, messages | ✓ | ✓ | ✓ |
| Agent Logs | `POST/GET /api/v1/agents/{id}/logs`, `GET .../logs/stream` (SSE) | ✓ | ✓ | — |
| Agent Stack | `POST/GET /api/v1/agents/{id}/stack` | ✓ | ✓ | — |
| Agent Card | `PUT /api/v1/agents/{id}/card`, `GET .../discover` | ✓ | ✓ | — |
| Spawn/Complete | `POST /api/v1/agents/spawn`, `POST .../agents/{id}/complete` | ✓ | ✓ | ✓ |
| Tasks | `POST/GET /api/v1/tasks`, `GET/PUT .../tasks/{id}`, status transition | ✓ | ✓ | ✓ |
| Merge Requests | `POST/GET /api/v1/merge-requests`, comments, reviews, diff | ✓ | ✓ | ✓ |
| Merge Queue | `POST /api/v1/merge-queue/enqueue`, `GET`, `DELETE .../merge-queue/{id}` | ✓ | ✓ | — |
| MR Gates | `GET /api/v1/merge-requests/{id}/gates` | ✓ | ✓ | — |
| Auth | `POST /api/v1/auth/api-keys` | ✓ | ✓ | ✓ |
| Analytics | `POST/GET /api/v1/analytics/events`, `/count`, `/daily` | ✓ | ✓ | ✓ |
| Costs | `POST/GET /api/v1/costs`, `/summary` | ✓ | ✓ | ✓ |
| Compose | `POST /api/v1/compose/apply`, `GET .../status`, `POST .../teardown` | ✓ | ✓ | ✓ |
| Admin | `GET .../admin/health`, `/jobs`, `/audit`, `/seed`, `/export` | ✓ | ✓ | — |
| Admin Agents | `POST .../admin/agents/{id}/kill`, `/reassign` | ✓ | ✓ | — |
| Admin Snapshots | `POST/GET/DELETE /api/v1/admin/snapshots`, `/restore` | ✓ | ✓ | — |
| Admin SIEM | `POST/GET/PUT/DELETE /api/v1/admin/siem` | ✓ | ✓ | — |
| Admin Retention | `GET/PUT /api/v1/admin/retention` | ✓ | ✓ | — |
| Compute Targets | `POST/GET/GET/DELETE /api/v1/admin/compute-targets` | ✓ | ✓ | — |
| Audit | `POST/GET /api/v1/audit/events`, `/stream`, `/stats` | ✓ | ✓ | — |
| Network | `POST/GET /api/v1/network/peers`, `/agent/{id}`, `DELETE .../peers/{id}` | ✓ | ✓ | — |
| MCP | `POST /mcp` (initialize, tools/list) | ✓ | ✓ | — |
| Metrics | `GET /metrics` | ✓ | — | — |

#### Test Infrastructure

```rust
// crates/gyre-server/tests/common/mod.rs
pub struct TestServer {
    pub base_url: String,
    pub client: reqwest::Client,
    pub auth_token: &'static str,
    _state: Arc<AppState>,
}

impl TestServer {
    /// Spawn a live server on a random port with in-memory DB.
    pub async fn new() -> Self { ... }

    /// GET helper with auth header.
    pub async fn get(&self, path: &str) -> reqwest::Response { ... }

    /// POST helper with JSON body + auth header.
    pub async fn post<T: serde::Serialize>(&self, path: &str, body: &T) -> reqwest::Response { ... }

    /// POST helper with no auth (for 401 tests).
    pub async fn post_unauth<T: serde::Serialize>(&self, path: &str, body: &T) -> reqwest::Response { ... }

    /// Pre-create a project+repo+task, return their IDs.
    pub async fn seed_project_repo_task(&self) -> (String, String, String) { ... }
}
```

#### Acceptance Criteria

- [ ] Every endpoint in the table above has a happy-path test
- [ ] Every protected endpoint has an auth-rejection test returning 401
- [ ] Every endpoint accepting a request body has an invalid-input test returning 4xx with
  `{"error": "..."}` or similar
- [ ] No test shares state with another test (each gets a fresh in-memory server)
- [ ] Tests run in parallel with `cargo test --all`
- [ ] Total test count increases by 200+ (one happy + one auth + one invalid per endpoint group)

---

### M17.2: Git Smart HTTP Integration Tests

**Location:** `crates/gyre-server/tests/git_http_integration.rs`

The existing `e2e_ralph_loop.rs` proves the happy path. M17.2 extends coverage to error
cases, gate enforcement, provenance recording, and ref namespace integrity.

#### Test Cases

**Clone & fetch:**
- `clone_with_valid_token` — clone succeeds, working copy has correct content
- `clone_with_invalid_token` — `git clone` returns exit code non-zero (401 from server)
- `clone_with_no_token` — rejected; error message mentions authorization
- `fetch_after_push` — clone, push commit, fetch from second worktree, new commit visible

**Push:**
- `push_valid_commit` — commit pushed, visible via `GET /api/v1/repos/{id}/commits`
- `push_rejected_non_hex_sha` — forged ref-update with non-hex SHA returns 400 (M-8)
- `push_with_conventional_commit_gate` — enable ConventionalCommit push gate; push with
  non-conventional message is rejected (422 / ref-update failure) with gate name in error
- `push_with_task_ref_gate` — enable TaskRef push gate; push without TASK- ref rejected
- `push_with_no_em_dash_gate` — push commit containing `—` (em-dash) is rejected
- `push_multiple_gates_all_pass` — commit that satisfies all three built-in gates is accepted

**Post-receive provenance:**
- `push_records_provenance` — after push, `GET /api/v1/repos/{id}/agent-commits` returns
  the commit with correct `agent_id`, `task_id`, `ralph_step`
- `push_to_main_creates_spec_task` — push spec file under `specs/system/` to default branch
  triggers automatic task creation (M13.8); verify via `GET /api/v1/tasks`

**Ref namespaces (M13.6):**
- `spawn_writes_agent_head_ref` — after `POST /agents/spawn`, `git ls-remote` shows
  `refs/agents/{id}/head`
- `spawn_writes_ralph_ref` — after spawn, `refs/ralph/{task-id}/implement` exists
- `complete_writes_snapshot_ref` — after `POST /agents/{id}/complete`,
  `refs/agents/{id}/snapshots/1` exists
- `complete_is_idempotent` — second complete call returns 202, snapshot count increments to 2

#### Acceptance Criteria

- [ ] All test cases above pass with a real git binary on PATH
- [ ] Gate rejection tests verify the push is not recorded (commit not in log)
- [ ] Provenance tests verify the `agent_commits` record matches the pushed SHA
- [ ] Tests bind to `127.0.0.1:0` (random port) — safe to run in parallel

---

### M17.3: Merge Queue System Tests

**Location:** `crates/gyre-server/tests/merge_queue_integration.rs`

#### Test Cases

**Enqueue and auto-merge:**
- `enqueue_approved_mr_merges` — create repo + MR, approve it, enqueue it; merge processor
  auto-merges within 5 seconds; verify commit on target branch
- `enqueue_unapproved_mr_stays_queued` — MR without approval stays in queue, does not merge
- `cancel_queued_entry` — `DELETE /merge-queue/{id}` removes it; it does not merge

**Quality gates (M12.1):**
- `gate_testcommand_pass` — configure a `TestCommand` gate with `echo ok`; enqueue MR;
  gate executes, passes, MR merges
- `gate_testcommand_fail` — configure a `TestCommand` gate with `exit 1`; enqueue MR;
  gate fails, MR stays blocked, `GET .../gates` shows `Failed` status
- `gate_lintcommand_pass` — `LintCommand` gate with passing command
- `gate_multiple_gates_all_must_pass` — two gates, one fails; MR does not merge
- `gate_results_queryable` — `GET /api/v1/merge-requests/{id}/gates` returns all results
  with correct status, started_at, finished_at fields

**Agent gates (M12.3):**
- `agent_review_gate_approved` — configure `AgentReview` gate; gate agent submits `Approved`
  review via API; merge proceeds
- `agent_review_gate_changes_requested` — gate agent submits `ChangesRequested`; MR blocked;
  `GateFailure` domain event broadcast over WebSocket

**Merge queue priority ordering:**
- `higher_priority_mr_merges_first` — enqueue two MRs; higher-priority one merges first
  (verify order of merge commits on target branch)

**MR dependency ordering (merge-dependencies spec):**
- `dependent_mr_waits_for_parent` — if MR B depends on MR A, B does not merge until A merges
- `dependency_chain_resolves_in_order` — A → B → C; merges happen in A, B, C order

**Domain events:**
- `merge_emits_mr_status_changed_event` — subscribe to WebSocket before merge; verify
  `DomainEvent MrStatusChanged merged` received within timeout
- `gate_failure_emits_gate_failure_event` — subscribe to WS; trigger gate failure; verify
  `GateFailure` event received with correct `mr_id` and `gate_name`

#### Acceptance Criteria

- [ ] All test cases above pass using in-memory server + real background merge processor
- [ ] Gate execution uses real subprocess (`std::process::Command`) for `TestCommand` gates
- [ ] WebSocket domain event assertions use a 5-second timeout with tokio::time::timeout
- [ ] Tests do not share state (each spawns its own server + merge processor)

---

### M17.4: Auth Integration Tests

**Location:** `crates/gyre-server/tests/auth_integration.rs`

#### Auth Mechanism Tests

All four mechanisms must be individually verified:

1. **Global token (`GYRE_AUTH_TOKEN`):**
   - `global_token_grants_admin_access` — call Admin-only endpoint (`GET /api/v1/admin/health`)
     with the server's `GYRE_AUTH_TOKEN`; returns 200
   - `wrong_global_token_rejected` — different token returns 401

2. **Per-agent token:**
   - `agent_token_allows_agent_operations` — register agent, use returned token for
     `PUT /agents/{id}/heartbeat`; returns 200
   - `agent_token_blocked_after_complete` — complete the agent; subsequent API call with same
     token returns 401 (token revocation M13.7)
   - `agent_token_cannot_reach_admin` — agent token on `GET /admin/health` returns 403

3. **API key (`gyre_<uuid>`):**
   - `api_key_creation_requires_admin` — non-admin cannot create API keys (403)
   - `api_key_valid_after_creation` — create key with admin; use `gyre_<uuid>` to call
     protected endpoint; returns 200
   - `api_key_shown_once` — second `GET` of the same key ID does not reveal the raw key
     (server only stores hash)

4. **JWT (Keycloak OIDC):**
   - `jwt_admin_role_grants_admin_access` — issue a mock JWT with `realm_access.roles: ["Admin"]`;
     call admin endpoint; returns 200 (requires test OIDC server or signed mock JWT)
   - `jwt_readonly_role_blocks_write` — JWT with `ReadOnly` role; `POST /projects` returns 403
   - `jwt_developer_role_allows_create` — JWT with `Developer` role; `POST /projects` returns 201
   - `jwt_expired_token_rejected` — expired JWT returns 401

#### RBAC Role Hierarchy Tests

- `admin_can_do_everything` — Admin role: all endpoints accessible
- `developer_can_crud` — Developer: CRUD on projects/repos/tasks/MRs; no admin
- `agent_can_push_and_heartbeat` — Agent role: git push, heartbeat, task assignment
- `readonly_blocks_writes` — ReadOnly: all GET endpoints accessible, all mutating endpoints
  return 403
- `role_hierarchy_admin_supersedes_developer` — Admin has Developer permissions (no need to
  grant both roles)

#### Token Revocation Tests

- `revoked_token_immediately_rejected` — revoke agent token; next request within 1 second
  returns 401 (not cached stale state)
- `double_complete_returns_202` — complete an agent twice; second call returns 202, not 500

#### Acceptance Criteria

- [ ] All four auth mechanisms tested with real HTTP requests (no mock middleware)
- [ ] RBAC matrix fully covered: 4 roles × representative endpoint set
- [ ] Token revocation propagates immediately (no sleep-and-retry)
- [ ] JWT tests use a real JWT signed with a test key (not just mocked middleware)

---

### M17.5: Frontend E2E Tests

**Location:** `web/tests/e2e/` (Playwright test suite)

**Requirement:** Node 18+, Playwright 1.x (`npm install -D @playwright/test`).

The server must be running (or Playwright launches it) before E2E tests run.

#### Setup

```js
// web/playwright.config.js
import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: './tests/e2e',
  use: { baseURL: 'http://localhost:2222' },
  webServer: {
    command: 'cargo run -p gyre-server',
    url: 'http://localhost:2222/health',
    reuseExistingServer: true,
    timeout: 30_000,
  },
});
```

#### Test Cases

**Auth flow:**
- `auth_token_modal_stores_token` — open Token modal (topbar auth dot), enter token, save;
  subsequent API calls use the new token
- `wrong_token_shows_error_state` — enter bad token; topbar auth dot turns red; dashboard
  shows error state

**Dashboard home:**
- `dashboard_loads_metric_cards` — navigate to `/`; four metric cards (active agents, open
  tasks, pending MRs, queue depth) visible and contain numbers
- `seed_demo_data_button_works` — click "Seed Demo Data"; toast confirms success; metric
  counts increase

**Projects:**
- `create_project_via_modal` — click "New Project", fill form, submit; new project card
  appears in list without page reload
- `project_list_shows_empty_state` — with no projects, EmptyState component renders

**Repos:**
- `create_repo_in_project` — navigate to project, click "Add Repo", fill form, submit;
  repo appears in list
- `repo_detail_shows_clone_url` — navigate to repo; clone URL bar visible and contains
  `http://` prefix
- `copy_clone_url` — click copy button; clipboard contains the clone URL

**Task board:**
- `create_task_appears_in_kanban` — click "New Task", fill title and priority; card appears
  in the correct kanban column
- `task_status_filter_works` — click status filter pill; only tasks with that status visible

**Merge requests:**
- `mr_detail_shows_diff` — navigate to MR detail; diff panel visible with syntax highlighting
- `mr_side_by_side_diff` — diff panel shows two columns (before/after)

**Agent management:**
- `agent_list_shows_status_badges` — navigate to Agents; each agent card shows a status badge
- `spawn_agent_modal_validates_fields` — open "Spawn Agent" modal; submit with empty name;
  inline validation error shown
- `agent_logs_tab_shows_output` — navigate to agent → Logs tab; log lines displayed (or
  empty state if no logs)

**MCP Tool Catalog:**
- `mcp_catalog_shows_8_tools` — navigate to "MCP Tools" sidebar item; exactly 8 tool cards
  displayed
- `mcp_tool_card_expands_schema` — click a tool card; JSON schema section expands

**Settings:**
- `settings_shows_server_info` — navigate to Settings; server name and version from
  `/api/v1/version` displayed

**Merge Queue:**
- `merge_queue_shows_entries` — enqueue an MR via API; navigate to Merge Queue view; entry
  appears with status

**Global Search:**
- `cmd_k_opens_search` — press `Meta+K`; search overlay opens
- `search_for_agent_returns_result` — type agent name; result card appears

**Analytics & Cost:**
- `analytics_bar_chart_renders` — navigate to Analytics; bar chart visible (no JS errors)
- `cost_summary_table_renders` — navigate to Cost; summary table visible

#### Accessibility baseline:
- `no_axe_violations_on_dashboard` — run `axe-core` on the dashboard home page; no critical
  or serious violations

#### Acceptance Criteria

- [ ] All test cases above pass against a live server with seed data loaded
- [ ] Tests run headless in CI (Playwright headless mode)
- [ ] A failing test produces a screenshot artifact (Playwright `--screenshot=only-on-failure`)
- [ ] No test has `page.waitForTimeout` — use `waitForSelector` / `waitForResponse` instead
- [ ] E2E suite runs in under 5 minutes on a single machine

---

### M17.6: CI Enforcement

**Location:** `.github/workflows/ci.yml` (update existing), plus new
`.github/workflows/e2e.yml`

#### Rust Integration Test Job

Add a dedicated CI job that runs the integration test suites:

```yaml
# In ci.yml
integration-tests:
  runs-on: ubuntu-latest
  needs: [build]
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - uses: Swatinem/rust-cache@v2
    - name: Install git
      run: sudo apt-get install -y git
    - name: Run integration tests
      run: cargo test --all --test '*_integration' --test 'e2e_*'
      env:
        RUST_LOG: warn
        GYRE_AUTH_TOKEN: ci-test-token
```

#### Frontend E2E Job

```yaml
# .github/workflows/e2e.yml
name: E2E Tests
on: [push, pull_request]
jobs:
  e2e:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - uses: actions/setup-node@v4
        with: { node-version: '20' }
      - run: cargo build --release -p gyre-server
      - run: cd web && npm ci && npx playwright install --with-deps chromium
      - run: cd web && npx playwright test
        env:
          GYRE_AUTH_TOKEN: e2e-test-token
          GYRE_PORT: 2222
      - uses: actions/upload-artifact@v4
        if: failure()
        with:
          name: playwright-screenshots
          path: web/test-results/
```

#### Gate Requirements

Both jobs are **required status checks** on the `main` branch:
- `integration-tests` — blocks merge if any integration test fails
- `e2e` — blocks merge if any Playwright test fails

These join the existing required checks:
- `test` (cargo test --all, all unit tests)
- `clippy` (lint)
- `fmt` (formatting)
- `arch-lint` (hexagonal boundary check)

#### Acceptance Criteria

- [ ] `integration-tests` job defined in CI and required to pass before merge to main
- [ ] `e2e` job defined in CI and required to pass before merge to main
- [ ] Both jobs use `Swatinem/rust-cache` for fast incremental builds
- [ ] Integration test job passes on the first green run in CI
- [ ] E2E job produces screenshot artifacts on failure for debugging
- [ ] Total CI time for full suite (unit + integration + E2E) stays under 15 minutes

---

## Test Infrastructure Reference

### Shared Test Helpers (`crates/gyre-server/tests/common/mod.rs`)

```rust
pub struct TestServer { ... }  // See M17.1 above

/// Create a bare git repo, get the clone URL, return (repo_id, clone_url).
pub async fn create_test_repo(srv: &TestServer) -> (String, String) { ... }

/// Run git clone with Bearer auth. Returns path to local clone.
pub async fn git_clone_with_auth(clone_url: &str, token: &str) -> tempfile::TempDir { ... }

/// Make a commit in the given dir and return the SHA.
pub fn make_commit(dir: &std::path::Path, message: &str, file: &str, content: &str) -> String { ... }

/// Push using Bearer auth. Returns success/failure.
pub fn git_push_with_auth(dir: &std::path::Path, clone_url: &str, token: &str, branch: &str) -> bool { ... }

/// Subscribe to WebSocket and collect DomainEvents until predicate is satisfied or timeout.
pub async fn wait_for_domain_event<F>(
    ws_url: &str,
    token: &str,
    predicate: F,
    timeout_secs: u64,
) -> Option<serde_json::Value>
where F: Fn(&serde_json::Value) -> bool { ... }
```

### Mock JWT Helper (`crates/gyre-server/tests/common/jwt.rs`)

```rust
/// Generate a signed JWT for the given role using an ephemeral test key.
/// The server must be started with this key's JWKS URL.
pub fn test_jwt(role: &str, subject: &str) -> String { ... }

/// Spawn a minimal JWKS HTTP server serving the test public key. Returns URL.
pub async fn test_jwks_server() -> (String, tokio::task::JoinHandle<()>) { ... }
```

### Frontend Test Fixtures (`web/tests/e2e/fixtures/`)

```js
// fixtures/seeded.js — Playwright fixture that calls POST /api/v1/admin/seed before tests
export const seededPage = base.extend({
  page: async ({ page }, use) => {
    await fetch('http://localhost:2222/api/v1/admin/seed', {
      method: 'POST',
      headers: { Authorization: 'Bearer e2e-test-token' },
    });
    await use(page);
  },
});
```

---

## Acceptance Criteria (M17 Overall)

- [ ] `cargo test --all` runs unit + integration tests in one command, all green
- [ ] `cargo test --all --test '*_integration'` runs only integration tests
- [ ] `cd web && npx playwright test` runs only E2E tests
- [ ] All tests pass on a fresh clone with only `git`, `cargo`, and `node` installed
- [ ] CI fails fast: if any required check fails, the PR cannot merge
- [ ] Total Rust test count ≥ 800 after M17 (606 existing + ~200 integration)
- [ ] Total frontend test count ≥ 120 (86 vitest + ~35 Playwright)
- [ ] No flaky tests — each test is deterministic and self-contained (no shared state, no
  sleep-based waits)
- [ ] Test coverage report generated (`cargo llvm-cov`) and uploaded as CI artifact

---

## Security Considerations

- Integration tests that test auth rejection must assert the exact status code (401 vs 403)
  and must not inadvertently test against a server running without auth enforcement.
- The mock JWT helper must use an ephemeral key pair generated per test run — never commit
  a test private key.
- E2E tests run against a server started with a dedicated test token that is not the default
  `gyre-dev-token` — prevents confusion between dev and test environments.
- Gate integration tests that run shell commands (`TestCommand`) must not run arbitrary
  user-supplied commands — test helpers use only hardcoded commands (`echo ok`, `exit 1`,
  `true`, `false`).
