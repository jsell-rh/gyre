# AGENTS.md - Gyre Agent Entry Point

Gyre is an autonomous software development platform built in Rust.
It orchestrates AI agents through a structured Ralph loop: spec, implement, review, merge.

For full specs: [specs/index.md](specs/index.md)

---

## Crate Structure (Hexagonal Architecture)

```
crates/
  gyre-common/     # Shared types, errors, Id - no external deps
  gyre-ports/      # Port traits (interfaces) - no infrastructure deps
  gyre-domain/     # Pure domain logic - depends ONLY on ports + common
  gyre-adapters/   # Adapter implementations (SQLite, etc.) - implements ports
  gyre-server/     # Binary: HTTP/WS server - wires domain + adapters
  gyre-cli/        # Binary: CLI + TUI - thin client
```

**Hexagonal boundary invariant:**
- `gyre-domain` MUST NOT import `gyre-adapters` or any infrastructure crate.
- Violation is caught by `scripts/check-arch.sh` and CI (will fail the build).

Dependency flow:
```
gyre-server ──> gyre-domain ──> gyre-ports ──> gyre-common
gyre-server ──> gyre-adapters ──> gyre-ports ──> gyre-common
gyre-cli    ──> gyre-common
```

---

## Key Commands

```bash
# Build everything
cargo build --all

# Build release binaries
cargo build --release -p gyre-server -p gyre-cli

# Run all Rust tests
cargo test --all

# Run frontend component tests (vitest — requires Node/npm)
cd web && npm test && cd ..

# Run Playwright E2E tests (M17.5 — auto-starts gyre-server on port 2222)
cd web && npm run test:e2e && cd ..

# Format check
cargo fmt --all -- --check

# Lint (warnings are errors)
cargo clippy --all-targets --all-features -- -D warnings

# Architecture lint (enforces hexagonal boundaries)
bash scripts/check-arch.sh

# Auto-format
cargo fmt --all

# Watch mode (requires cargo-watch)
cargo watch -x "test --all"

# Run the E2E Ralph loop integration test (requires git on PATH)
cargo test -p gyre-server --test e2e_ralph_loop

# Run M17 integration test suites individually (all require git on PATH)
cargo test -p gyre-server --test api_integration      # 68 REST API contract tests
cargo test -p gyre-server --test auth_integration     # 21 auth + RBAC tests
cargo test -p gyre-server --test git_integration      # 12 git smart HTTP + merge queue tests
```

### M17 Integration Test Suites

Five integration test files in `crates/gyre-server/tests/` each start a live server on a random port:

| File | Tests | Coverage |
|---|---|---|
| `e2e_ralph_loop.rs` | 1 | Full Ralph loop end-to-end: spawn → clone → push → MR → merge |
| `api_integration.rs` | 68 | REST API contract tests for all endpoints (M17.2) |
| `auth_integration.rs` | 21 | Auth matrix: valid tokens, invalid tokens, RBAC role enforcement (M17.4) |
| `git_integration.rs` | 12 | Smart HTTP clone/push, push gates, merge queue, commit provenance (M17.3) |
| `m18_oidc_integration.rs` | 8 | OIDC discovery document, JWKS Ed25519 JWK, JWT spawn token, JWT auth, token-info claims, JWT revocation after complete (M18) |

All tests bind to `127.0.0.1:0` (random port) and run safely in parallel. Require `git` on `PATH`.

> **Note for CI / integration tests:** Always use `git push origin HEAD:main` (not `git push origin main`) when pushing to an empty repo. GitHub Actions runners default to `init.defaultBranch=master`, so the local unborn branch may be named `master` even if the remote expects `main`.

### E2E Integration Test (`e2e_ralph_loop`)

`crates/gyre-server/tests/e2e_ralph_loop.rs` is a 338-line integration test that proves the full Ralph loop works end-to-end via real HTTP and git operations:

1. Spawns a live `gyre-server` on a random port
2. Creates a project, repo, and task via REST API
3. Calls `POST /api/v1/agents/spawn` to get a per-agent token + worktree
4. Clones the repo over Smart HTTP (`/git/...`) using the agent token
5. Creates a commit and pushes it back via Smart HTTP
6. Calls `POST /api/v1/agents/{id}/complete` to open a MR and transition to review
7. Enqueues the MR and waits for the merge processor to auto-merge
8. Verifies the commit appears on the target branch

Requires `git` on `PATH`. Test binds to `127.0.0.1:0` (random port) so runs safely in parallel.

---

## Running the Server

```bash
# Dev mode (defaults: port 3000, token gyre-dev-token, in-memory DB)
cargo run -p gyre-server

# With custom settings
GYRE_PORT=8080 GYRE_AUTH_TOKEN=my-token GYRE_DATABASE_URL=sqlite:///tmp/gyre.db RUST_LOG=debug \
  cargo run -p gyre-server

# Release build
cargo build --release -p gyre-server && ./target/release/gyre-server
```

### Server Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/.well-known/openid-configuration` | OIDC discovery document — issuer, JWKS URI, supported algorithms (no auth required) (M18) |
| `GET` | `/.well-known/jwks.json` | Ed25519 JWK Set for JWT signature verification (no auth required) (M18) |
| `GET` | `/health` | Returns `{"status":"ok","version":"0.1.0"}` |
| `GET` | `/ws` | WebSocket upgrade (requires `Auth` handshake first) |
| `GET` | `/api/v1/version` | Returns `{"name":"gyre","version":"0.1.0","milestone":"M0"}` |
| `GET` | `/api/v1/activity` | Query activity log (`?since=&limit=&agent_id=&event_type=`) |
| `POST/GET` | `/api/v1/projects` | Create / list projects (`?workspace_id=` optional filter) |
| `GET/PUT/DELETE` | `/api/v1/projects/{id}` | Read / update / delete project |
| `POST/GET` | `/api/v1/workspaces` | Create (**Admin only**, H-15) / list workspaces (`?tenant_id=` filter); workspace groups repos under a shared budget and quota (M22.1) |
| `GET/PUT/DELETE` | `/api/v1/workspaces/{id}` | Read / update (**Admin only**) / delete (**Admin only**) workspace (H-15, M22.1) |
| `POST/GET` | `/api/v1/workspaces/{id}/repos` | Add / list repos in a workspace (M22.1) |
| `POST/GET` | `/api/v1/personas` | Create (**Admin only**, H-16) / list personas (`?scope=tenant|workspace|repo&scope_id=` filter); `PersonaScope` JSON wire format: `{"kind": "Tenant"|"Workspace"|"Repo", "id": "<uuid>"}` (serde tagged enum — both `kind` and `id` fields required; `id` is the tenant/workspace/repo UUID); Rust type: `Tenant(Id)`, `Workspace(Id)`, `Repo(Id)` (M22.1) |
| `GET/PUT/DELETE` | `/api/v1/personas/{id}` | Read / update (**Admin only**) / delete (**Admin only**) persona -- fields: `name`, `slug`, `scope`, `system_prompt`, `capabilities`, `model`, `temperature`, `max_tokens`, `budget` (H-16, M22.1) |
| `POST/GET` | `/api/v1/repos` | Create / list repos (`?project_id=&workspace_id=`); response includes mirror fields (`is_mirror`, `mirror_url`, `mirror_interval_secs`, `last_mirror_sync`). `mirror_url` has credentials redacted (`https://***@host`); `path` in create body is ignored — server-computed as `{repos_root}/{project_id}/{name}.git` (M12.2) |
| `GET` | `/api/v1/repos/{id}` | Get repository (includes mirror fields); `mirror_url` has credentials redacted (H-5); response includes `workspace_id: Option<Id>` when repo belongs to a workspace (M22.1) |
| `POST` | `/api/v1/repos/mirror` | Create a pull mirror from an external git URL (bare clone + periodic background sync); URL must use `https://` (M12.2) |
| `POST` | `/api/v1/repos/{id}/mirror/sync` | Manually trigger a fetch sync on a mirror repo (M12.2) |
| `GET` | `/api/v1/repos/{id}/branches` | List branches in repository |
| `GET` | `/api/v1/repos/{id}/commits` | Commit log (`?branch=<name>&limit=50`) |
| `GET` | `/api/v1/repos/{id}/diff` | Diff between refs (`?from=<ref>&to=<ref>`) |
| `POST/GET` | `/api/v1/repos/{id}/gates` | Create (**Admin required**) / list quality gates for a repo (`GateType`: `test_command`, `lint_command`, `required_approvals`, `agent_review`, `agent_validation` — serialized as snake_case) (M12.1, M12.3). See **Gate Agent Protocol** below for `AgentReview`/`AgentValidation` env vars. |
| `DELETE` | `/api/v1/repos/{id}/gates/{gate_id}` | Delete a quality gate (M12.1) |
| `POST` | `/api/v1/specs/approve` | Record spec approval: `{path, sha, signature?}` — `sha` must be 40-char hex; **approver identity derived server-side from auth token** (client must not supply `approver_id`) (CISO M12.3-A, M12.3) |
| `GET` | `/api/v1/specs/approvals` | List spec approvals (`?path=<relative-path>` to filter by spec file) (M12.3) |
| `POST` | `/api/v1/specs/revoke` | Revoke a spec approval: `{approval_id, reason}` — caller must be original approver or Admin (returns 403 otherwise); revoker identity derived server-side (client must not supply `revoked_by`) (CISO M12.3-A, M12.3) |
| `GET` | `/api/v1/specs` | List all specs with ledger state — reads `specs/manifest.yaml` + ledger; each entry includes `path`, `title`, `owner`, `sha`, `approval_status`, `drift_status` (M21.1) |
| `GET` | `/api/v1/specs/pending` | Specs awaiting approval — ledger entries with `approval_status: Pending` (M21.1) |
| `GET` | `/api/v1/specs/drifted` | Specs with open drift-review tasks — `drift_status: Drifted` (M21.1) |
| `GET` | `/api/v1/specs/index` | Auto-generated markdown index of all specs in manifest (M21.1) |
| `GET` | `/api/v1/specs/{path}` | Get single spec ledger entry by URL-encoded path (M21.1) |
| `POST` | `/api/v1/specs/{path}/approve` | Approve a specific spec version: `{sha}` — path-scoped; transitions ledger Pending → Approved; `sha` must be 40-char hex; **approver type (`agent`/`human`) derived server-side from token kind** (JWT bearer = agent, global token/API key = human; client must not supply); approval blocked (400) when an `implements` link exists and parent spec is not yet approved, or when a `conflicts_with` link exists and conflicting spec is already approved (M22.3); **Developer+ required** — ReadOnly callers receive 403 (M21.1, M21.1-B, M21.1-C) |
| `POST` | `/api/v1/specs/{path}/revoke` | Revoke approval for a specific spec: `{reason}` — path-scoped; caller must be original approver or Admin (M21.1) |
| `GET` | `/api/v1/specs/{path}/history` | Approval event history for a specific spec — list of approval/revocation events with approver, SHA, timestamps, reason (M21.1) |
| `GET` | `/api/v1/specs/{path}/links` | Outbound and inbound spec links for a specific spec — `{links: [{link_type, target_path, direction},...]}` (M22.3) |
| `GET` | `/api/v1/specs/graph` | Full spec link graph — `{nodes: [{path, title, approval_status},...], edges: [{from, to, link_type},...]}` (M22.3) |
| `GET/PUT` | `/api/v1/repos/{id}/push-gates` | Get / set active pre-accept push gates for a repo (built-in: ConventionalCommit, TaskRef, NoEmDash); **PUT requires Admin role** (M13.1) |
| `GET/PUT` | `/api/v1/repos/{id}/spec-policy` | Get / set per-repo spec enforcement policy: `{require_spec_ref: bool, require_approved_spec: bool, warn_stale_spec: bool, require_current_spec: bool}`. `warn_stale_spec` emits `StaleSpecWarning` domain event when MR spec_ref SHA differs from HEAD; `require_current_spec` blocks merge queue when stale. **PUT requires Admin role**. All fields default to `false` (backwards compatible). (M18) |
| `GET` | `/api/v1/repos/{id}/blame?path={file}` | Per-line agent attribution — which agent last touched each line (M13.4) |
| `GET` | `/api/v1/repos/{id}/hot-files?limit=20` | Files with the most concurrent active agents in the last 24h (M13.4) |
| `GET` | `/api/v1/repos/{id}/review-routing?path={file}` | Ordered list of agents to request review from, ranked by recency and commit count (M13.4) |
| `GET` | `/api/v1/repos/{id}/speculative` | List all speculative merge results for active branches (M13.5) |
| `GET` | `/api/v1/repos/{id}/speculative/{branch}` | Speculative merge result for a specific branch against main (M13.5) |
| `GET` | `/api/v1/repos/{id}/stack-policy` | Get repo's required stack fingerprint for push attestation (M14.2) |
| `PUT` | `/api/v1/repos/{id}/stack-policy` | Set / clear required stack fingerprint (**Admin only**, M14.2) |
| `GET` | `/api/v1/repos/{id}/abac-policy` | Get the ABAC policy list for a repo — array of `AbacPolicy` objects; each policy has `id`, `name`, `rules` (AND within), evaluated as OR across policies (G6) |
| `PUT` | `/api/v1/repos/{id}/abac-policy` | Replace the ABAC policy list (**Admin only**); policies are matched against JWT claims on push and spawn; `rules` is a list of `{claim, operator, value}` match conditions combined with AND; multiple policies in the array are OR'd together (G6) |
| `GET` | `/api/v1/repos/{id}/aibom` | AI Bill of Materials — per-commit agent attribution + attestation levels (`?from={ref}&to={ref}`); ref names validated to prevent git flag injection (M14.3) |
| `GET` | `/api/v1/repos/{id}/dependencies` | Outgoing dependency edges (`DependencyType`: Code/Spec/Api/Schema/Manual; `DetectionMethod`: auto/manual) (M22.4) |
| `GET` | `/api/v1/repos/{id}/dependents` | Incoming dependency edges (M22.4) |
| `POST` | `/api/v1/repos/{id}/dependencies` | Add a manual dep edge: `{target_repo_id, dep_type, notes?}`; **Admin only** (H-12, M22.4) |
| `DELETE` | `/api/v1/repos/{id}/dependencies/{dep_id}` | Remove a manual dep edge; **Admin only** (H-13, M22.4) |
| `GET` | `/api/v1/repos/{id}/blast-radius` | BFS transitive dependents -- repos affected if this one changes (M22.4) |
| `GET` | `/api/v1/dependencies/graph` | Full tenant-wide dependency DAG: `{nodes, edges}` (M22.4) |
| `POST/GET` | `/api/v1/agents` | Register (returns auth_token) / list (`?status=&workspace_id=`) |
| `GET` | `/api/v1/agents/{id}` | Get agent |
| `PUT` | `/api/v1/agents/{id}/status` | Update agent status |
| `PUT` | `/api/v1/agents/{id}/heartbeat` | Agent heartbeat; on Linux, verifies PID liveness via `/proc/{pid}` and logs a warning if the process is no longer running (G10) |
| `POST/GET` | `/api/v1/agents/{id}/messages` | Send/poll agent messages |
| `POST` | `/api/v1/agents/{id}/logs` | Append a log line to the agent's log buffer (M11.2) |
| `GET` | `/api/v1/agents/{id}/logs` | Paginated agent log lines (`?limit=100&offset=0`) (M11.2) |
| `GET` | `/api/v1/agents/{id}/logs/stream` | SSE live feed of new log lines for an agent (M11.2) |
| `GET` | `/api/v1/agents/{id}/touched-paths` | All repo branches and file paths written to by this agent (M13.4) |
| `POST` | `/api/v1/agents/{id}/stack` | Agent self-reports its runtime stack fingerprint at spawn (M14.1) |
| `GET` | `/api/v1/agents/{id}/stack` | Query agent's registered stack fingerprint (M14.1) |
| `GET` | `/api/v1/agents/{id}/workload` | Current workload attestation — `{pid, hostname, compute_target, stack_hash, alive}`: captured at spawn; `alive` re-checked via `/proc/{pid}` on Linux (G10) |
| `GET` | `/api/v1/agents/{id}/container` | Container audit record for this agent -- `ContainerAuditRecord`: `container_id`, `image`, `image_hash`, `runtime` (e.g. `"docker"`), `started_at`, `stopped_at?`, `exit_code?`; 404 if agent was not container-spawned (M19.3) |
| `GET` | `/ws/agents/{id}/tty` | WebSocket TTY attach — auth via first-message Bearer token; replays buffered logs then streams live PTY output (M11.2) |
| `POST/GET` | `/api/v1/tasks` | Create / list (`?status=&assigned_to=&parent_task_id=&workspace_id=`); canonical `status` values (snake_case): `backlog`, `in_progress`, `review`, `done`, `blocked` |
| `GET/PUT` | `/api/v1/tasks/{id}` | Read / update task |
| `PUT` | `/api/v1/tasks/{id}/status` | Transition task status |
| `POST/GET` | `/api/v1/merge-requests` | Create / list (`?status=&repository_id=&workspace_id=`) |
| `GET` | `/api/v1/merge-requests/{id}` | Get merge request |
| `PUT` | `/api/v1/merge-requests/{id}/status` | Transition MR status |
| `POST/GET` | `/api/v1/merge-requests/{id}/comments` | Add / list review comments |
| `POST/GET` | `/api/v1/merge-requests/{id}/reviews` | Submit / list reviews (approve/request changes) |
| `GET` | `/api/v1/merge-requests/{id}/diff` | Get MR diff |
| `GET` | `/api/v1/merge-requests/{id}/gates` | Get quality gate execution results for an MR (M12.1) |
| `GET` | `/api/v1/merge-requests/{id}/attestation` | Get signed merge attestation bundle for a merged MR — fields: `attestation_version`, `mr_id`, `merge_commit_sha`, `merged_at`, `gate_results`, `spec_ref`, `spec_fully_approved`, `author_agent_id`; returns 404 if not yet merged or attestation pending (G5) |
| `PUT` | `/api/v1/merge-requests/{id}/dependencies` | Set MR dependency list: `{depends_on: [<mr-uuid>,...], reason?}` — validates all dep IDs exist, rejects self-dependency and cycles (400); queue skips MRs with unmerged deps; **Developer+ required** — ReadOnly callers receive 403 (CISO P147-A, TASK-100). **Branch lineage auto-detection:** on MR creation, the server uses `git merge-base` to check if the source branch descends from another open MR's source branch and auto-populates `depends_on` (branch refs validated to prevent arg injection). |
| `GET` | `/api/v1/merge-requests/{id}/dependencies` | Get MR dependencies and dependents: `{mr_id, depends_on: [...], dependents: [...]}` (TASK-100) |
| `DELETE` | `/api/v1/merge-requests/{id}/dependencies/{dep_id}` | Remove a single dependency from an MR; 404 if dep_id not in depends_on; **Developer+ required** (CISO P147-A, TASK-100) |
| `PUT` | `/api/v1/merge-requests/{id}/atomic-group` | Set atomic group membership: `{group: "<name>"}` (or `null` to clear) — all group members must be ready before any is dequeued; **Developer+ required** (CISO P147-A, TASK-100) |
| `POST` | `/api/v1/merge-queue/enqueue` | Add approved MR to merge queue; triggers gate execution per repo gates (M12.1) |
| `GET` | `/api/v1/merge-queue` | List merge queue entries (priority ordered) |
| `DELETE` | `/api/v1/merge-queue/{id}` | Cancel queued entry |
| `GET` | `/api/v1/merge-queue/graph` | Return full merge queue DAG: `{nodes: [{mr_id, title, status, priority},...], edges: [{from, to},...]}` (TASK-100) |
| `POST` | `/api/v1/repos/{id}/commits/record` | Record agent-commit mapping |
| `GET` | `/api/v1/repos/{id}/agent-commits` | Query commits by agent (`?agent_id=`) |
| `POST/GET` | `/api/v1/repos/{id}/worktrees` | Create / list worktrees; POST: JWT bearers evaluated against repo ABAC policy — returns 403 if no policy matches (G6-A) |
| `DELETE` | `/api/v1/repos/{id}/worktrees/{wt_id}` | Delete worktree |
| `POST` | `/api/v1/agents/spawn` | Spawn agent: create record, generate token, provision worktree, assign task; writes `refs/agents/{id}/head` and `refs/ralph/{task-id}/implement` (M13.6); JWT bearers are evaluated against the target repo's ABAC policy before spawning — returns 403 if no policy matches (G6); returns **429** if workspace or tenant budget limits are exceeded (`max_concurrent_agents`, `max_tokens_per_day`, `max_cost_per_day`) (M22.2) |
| `POST` | `/api/v1/agents/{id}/complete` | Complete agent: open MR, mark task done, clean up worktree; writes `refs/agents/{id}/snapshots/{n}` snapshot ref (M13.6); **idempotent** — returns 202 on double-complete; agent token revoked on success (M13.7) |
| `GET` | `/git/{project}/{repo}/info/refs` | Smart HTTP git discovery (`?service=git-upload-pack` or `git-receive-pack`) |
| `POST` | `/git/{project}/{repo}/git-upload-pack` | Smart HTTP git clone / fetch data |
| `POST` | `/git/{project}/{repo}/git-receive-pack` | Smart HTTP git push data + post-receive hook; SHA values in ref-updates must be valid 40-char hex — non-hex SHAs rejected to prevent argument injection (M-8); pushes to the default branch trigger spec lifecycle task creation (M13.8); optional `X-Gyre-Model-Context` request header captures the agent's model/context for commit provenance (M13.2); JWT bearers are evaluated against the repo's ABAC policy — push rejected with 403 if no policy matches (G6); **auto-detects** `Cargo.toml` path dependencies and creates `DependencyEdge` records for Gyre-hosted repos (M22.4) |
| `GET` | `/api/v1/auth/token-info` | Token introspection — returns token kind (`agent_jwt`, `uuid_token`, `api_key`, `global`) and decoded JWT claims including `task_id`, `spawned_by`, `exp` (M18) |
| `GET/PUT` | `/api/v1/users/me` | Current user profile (username, display_name, avatar_url, timezone, locale, global_role, `UserPreferences`); PUT updates fields (M22.8) |
| `GET` | `/api/v1/users/me/agents` | Agents spawned by the current user (M22.8) |
| `GET` | `/api/v1/users/me/tasks` | Tasks assigned to the current user (M22.8) |
| `GET` | `/api/v1/users/me/mrs` | MRs authored by the current user (M22.8) |
| `GET` | `/api/v1/users/me/notifications` | Notifications (16 `NotificationType` variants: `MrNeedsReview`, `GateFailure`, `MrMerged`, etc.; 4 priority levels); auto-created on agent complete, gate failure, and MR merge (M22.8) |
| `PUT` | `/api/v1/users/me/notifications/{id}/read` | Mark notification read (M22.8) |
| `POST/GET` | `/api/v1/workspaces/{id}/members` | Invite (**Admin only**, H-19) / list members; `WorkspaceRole`: Owner, Admin, Developer, Viewer; accept/pending lifecycle (M22.8) |
| `PUT` | `/api/v1/workspaces/{id}/members/{user_id}` | Update a member's `WorkspaceRole`; **Admin only** (H-17, M22.8) |
| `DELETE` | `/api/v1/workspaces/{id}/members/{user_id}` | Remove a member; **Admin only** (H-20, M22.8) |
| `POST/GET` | `/api/v1/workspaces/{id}/teams` | Create (**Admin only**, H-21) / list workspace-scoped teams (M22.8) |
| `PUT/DELETE` | `/api/v1/workspaces/{id}/teams/{team_id}` | Update / delete team; **Admin only** (H-18); `add_member`/`remove_member` idempotent (M22.8) |
| `GET` | `/api/v1/federation/trusted-issuers` | List configured trusted remote Gyre instances (base URLs from `GYRE_TRUSTED_ISSUERS`); returns `[]` when federation is disabled (G11) |
| `POST` | `/api/v1/auth/api-keys` | Create API key (Admin role required; returns `gyre_<uuid>` key — stored as SHA-256 hash, visible only once on creation; rotate by creating a new key) |
| `GET` | `/metrics` | Prometheus metrics (request count, duration, active agents, merge queue depth) |
| `GET` | `/api/v1/admin/health` | Admin: server uptime + agent/task/project counts (Admin only) |
| `GET` | `/api/v1/admin/jobs` | Admin: background job status — merge processor, stale agent detector, `spawn_budget_daily_reset` (resets `tokens_used_today`/`cost_today` at midnight UTC), `stale_peer_detector` (marks WireGuard peers inactive after `GYRE_WG_PEER_TTL` s, runs every 60 s) (Admin only) |
| `GET` | `/api/v1/admin/audit` | Admin: searchable activity log (`?agent_id=&event_type=&since=`) (Admin only) |
| `POST` | `/api/v1/admin/agents/{id}/kill` | Admin: force agent to Dead, terminate real OS process via process registry, clean worktrees, block assigned task (Admin only) (M11.1) |
| `POST` | `/api/v1/admin/agents/{id}/reassign` | Admin: reassign agent's current task to another agent (Admin only) |
| `GET` | `/*` | Svelte SPA dashboard (served from `web/dist/`) |
| `POST` | `/mcp` | MCP JSON-RPC 2.0 handler (`initialize`, `tools/list`, tool calls) |
| `GET` | `/mcp/sse` | MCP SSE stream — typed AG-UI activity events |
| `GET` | `/api/v1/agents/discover` | Discover active agents by capability (`?capability=<str>`) |
| `PUT` | `/api/v1/agents/{id}/card` | Publish / update an agent's A2A AgentCard |
| `POST` | `/api/v1/compose/apply` | Apply agent-compose spec (JSON or YAML), creates agent tree in dependency order |
| `GET` | `/api/v1/compose/status` | Get current compose session: agent states |
| `POST` | `/api/v1/compose/teardown` | Stop all compose agents and remove session |
| `POST` | `/api/v1/repos/{id}/jj/init` | Initialize jj (Jujutsu) in colocated mode on a repo |
| `GET` | `/api/v1/repos/{id}/jj/log` | List recent jj changes (`?limit=N`) |
| `POST` | `/api/v1/repos/{id}/jj/new` | Create a new anonymous jj change (WIP commit); JWT bearers evaluated against repo ABAC policy — returns 403 if no policy matches (G6-A) |
| `POST` | `/api/v1/repos/{id}/jj/squash` | Squash working copy into parent change; returns `200 JSON` `CommitSignature` `{sha, signature (base64 Ed25519), key_id, algorithm, mode, timestamp}` — use `GET /commits/{sha}/signature` to verify later (M13.8); JWT bearers evaluated against repo ABAC policy — returns 403 if no policy matches (G6-A) |
| `POST` | `/api/v1/repos/{id}/jj/undo` | Undo the last jj operation; JWT bearers evaluated against repo ABAC policy — returns 403 if no policy matches (G6-A) |
| `POST` | `/api/v1/repos/{id}/jj/bookmark` | Create a jj bookmark (branch) pointing to a change; JWT bearers evaluated against repo ABAC policy — returns 403 if no policy matches (G6-A) |
| `GET` | `/api/v1/repos/{id}/commits/{sha}/signature` | Look up and verify the `CommitSignature` for a given commit SHA; 404 if SHA not in store (M13.8) |
| `GET` | `/healthz` | Liveness probe — `{status, checks}` JSON |
| `GET` | `/readyz` | Readiness probe — `{status, checks}` JSON |
| `POST` | `/api/v1/analytics/events` | Record an analytics event |
| `GET` | `/api/v1/analytics/events` | Query analytics events (`?event_name=&agent_id=&since=`) |
| `GET` | `/api/v1/analytics/count` | Count events by name (aggregated) |
| `GET` | `/api/v1/analytics/daily` | Daily event counts (time-series) |
| `GET` | `/api/v1/analytics/usage` | Event count, unique agent count, and trend (`up`/`down`/`flat` vs prior equal-length period); `?event_name=&since=&until=` (M23) |
| `GET` | `/api/v1/analytics/compare` | Before/after pivot comparison: `before_count`, `after_count`, `change_pct` (null when before=0), `improved`; `?event_name=&before=&pivot=&after=` (M23) |
| `GET` | `/api/v1/analytics/top` | Top N event names by count; `?limit=10&since=` (M23) |
| `POST` | `/api/v1/costs` | Record a cost entry (agent_id, task_id, cost_type, amount) |
| `GET` | `/api/v1/costs` | Query cost entries (`?agent_id=&task_id=&since=`) |
| `GET` | `/api/v1/costs/summary` | Aggregated cost totals by agent |
| `GET` | `/api/v1/workspaces/{id}/budget` | Current `BudgetConfig` (limits) + `BudgetUsage` (real-time snapshot) for a project-scoped workspace; `id` is the project UUID (M22.2) |
| `PUT` | `/api/v1/workspaces/{id}/budget` | Set workspace budget limits: `{max_tokens_per_day?, max_cost_per_day?, max_concurrent_agents?, max_agent_lifetime_secs?}`; returns 400 if any limit exceeds the tenant ceiling (cascade validation); **Admin only** (M22.2) |
| `GET` | `/api/v1/budget/summary` | Tenant-wide `BudgetConfig` + `BudgetUsage` plus per-workspace breakdown; **Admin only** (M22.2) |
| `GET` | `/api/v1/search` | Full-text search (`?q=&entity_type=&workspace_id=&limit=20`); results: `[{entity_type, id, title, snippet, score}]` (M22.7) |
| `POST` | `/api/v1/search/reindex` | Trigger full entity reindex; **Admin only** (H-14, M22.7) |
| `POST/GET` | `/api/v1/policies` | Create / list declarative ABAC policies; 8 operators (Equals, NotEquals, In, NotIn, GreaterThan, LessThan, Contains, Exists); first-match-wins; default-deny (M22.6) |
| `GET/PUT/DELETE` | `/api/v1/policies/{id}` | Read / update / delete policy (M22.6) |
| `POST` | `/api/v1/policies/evaluate` | Dry-run evaluation: `{context}` -> `{decision: Allow|Deny, matched_policy?, reason}` (M22.6) |
| `GET` | `/api/v1/policies/decisions` | Decision audit log (`?policy_id=&effect=&since=`) (M22.6) |
| `GET` | `/api/v1/policies/effective` | Effective permissions explorer for a given attribute context (M22.6) |
| `POST` | `/api/v1/admin/jobs/{name}/run` | Manually trigger a named background job (Admin only) |
| `POST` | `/api/v1/admin/snapshot` | Create point-in-time DB snapshot (Admin only) |
| `GET` | `/api/v1/admin/snapshots` | List all snapshots (Admin only) |
| `POST` | `/api/v1/admin/restore` | Restore DB from a named snapshot (Admin only) |
| `DELETE` | `/api/v1/admin/snapshots/{id}` | Delete a snapshot (Admin only) |
| `GET` | `/api/v1/admin/export` | Export all entities as JSON (Admin only) |
| `GET/PUT` | `/api/v1/admin/retention` | List / update retention policies (Admin only) |
| `POST/GET` | `/api/v1/admin/siem` | Create / list SIEM forwarding targets (Admin only) |
| `PUT/DELETE` | `/api/v1/admin/siem/{id}` | Update / delete a SIEM target (Admin only) |
| `POST/GET` | `/api/v1/admin/compute-targets` | Create / list remote compute targets (`target_type`: `"local"`, `"ssh"`, `"container"` — Docker/Podman, auto-detected via `which`). **SSH targets** accept `host` field and optionally `container_mode: true` to run agents in containers on the remote SSH host. **Container security defaults (G8):** `--network=none` (default for all container types — G8 security invariant). Agent containers needing server access (clone/heartbeat/complete) must opt in via `"network": "bridge"` in the compute target config. Git credentials are passed via a credential helper script (not embedded in the clone URL). `GYRE_AGENT_COMMAND` is launched via `exec` (not `eval`) for a clean process tree. `--memory=2g --pids-limit=512` (resource limits — override via `memory_limit`/`pids_limit`), `--user=65534:65534` (nobody:nogroup — override via `user`). `config` JSON also accepts `command` (entrypoint binary, default `/gyre/entrypoint.sh`) and `args` (argument list) to configure the container entrypoint. (Admin only, M24) |
| `GET/DELETE` | `/api/v1/admin/compute-targets/{id}` | Get / delete a compute target (Admin only) |
| `POST` | `/api/v1/admin/compute-targets/{id}/tunnel` | Open an SSH tunnel for a compute target: `{direction: "forward"|"reverse", local_port, remote_port, local_host?, remote_host?}` (`local_host` and `remote_host` default to `"localhost"`). Reverse tunnels (`-R`) let air-gapped agents dial out so the server can reach them through NAT. (G12, Admin only) |
| `GET` | `/api/v1/admin/compute-targets/{id}/tunnel` | List active SSH tunnels for a compute target (G12, Admin only) |
| `DELETE` | `/api/v1/admin/compute-targets/{id}/tunnel/{tid}` | Close an SSH tunnel — sends SIGTERM to the `ssh -N` process (G12, Admin only) |
| `POST` | `/api/v1/admin/seed` | Idempotent demo data seed: 2 projects, 3 repos, 4 agents, 6 tasks, 2 MRs, 1 queue entry, 5 activity events. Returns `{already_seeded:true}` on repeat. AdminOnly. (M9.1) |
| `GET` | `/api/v1/admin/bcp/targets` | BCP targets: reads `GYRE_RTO` and `GYRE_RPO` env vars; returns recovery time/point objectives in seconds (Admin only) (M23) |
| `POST` | `/api/v1/admin/bcp/drill` | BCP drill: triggers a real snapshot + verify cycle; returns `{snapshot_id, verified, duration_ms}` (Admin only) (M23) |
| `GET` | `/scim/v2/ServiceProviderConfig` | SCIM 2.0 discovery — supported features, auth schemes (no gyre auth required for discovery) (M23) |
| `GET` | `/scim/v2/Schemas` | SCIM 2.0 schema definitions for User resource type (M23) |
| `GET` | `/scim/v2/ResourceTypes` | SCIM 2.0 resource type registry (M23) |
| `GET` | `/scim/v2/Users` | SCIM 2.0 list users (`?startIndex=&count=&filter=`); auth via `GYRE_SCIM_TOKEN` Bearer (M23) |
| `POST` | `/scim/v2/Users` | SCIM 2.0 provision a new user; auth via `GYRE_SCIM_TOKEN` Bearer (M23) |
| `GET` | `/scim/v2/Users/{id}` | SCIM 2.0 get user by SCIM id; auth via `GYRE_SCIM_TOKEN` Bearer (M23) |
| `PUT` | `/scim/v2/Users/{id}` | SCIM 2.0 replace user attributes; auth via `GYRE_SCIM_TOKEN` Bearer (M23) |
| `DELETE` | `/scim/v2/Users/{id}` | SCIM 2.0 deprovision user; auth via `GYRE_SCIM_TOKEN` Bearer (M23) |
| `POST` | `/api/v1/release/prepare` | Admin: compute next semver version from conventional commits + generate changelog with agent/task attribution; optionally open a release MR. Request: `{repo_id, branch?, from?, create_mr?, mr_title?}`; `branch` and `from` validated against git argument injection — must not start with `-` or contain `..` (M16-A). Response: `{next_version, changelog, commit_count, mr?}` (M16) |
| `POST/GET` | `/api/v1/audit/events` | Record / query eBPF audit events (`?agent_id=&event_type=&since=`) |
| `GET` | `/api/v1/audit/stream` | SSE stream of live audit events |
| `GET` | `/api/v1/audit/stats` | Audit event statistics and counts |
| `POST/GET` | `/api/v1/network/peers` | Register / list WireGuard mesh peers |
| `GET` | `/api/v1/network/peers/agent/{agent_id}` | Get peer record for a specific agent |
| `PUT` | `/api/v1/network/peers/{id}` | Update peer endpoint (roaming): `{endpoint: "host:port"}` — JWT caller must own the peer (agent_id match); updates `last_seen` (M26.2) |
| `DELETE` | `/api/v1/network/peers/{id}` | Remove a peer from the mesh |
| `GET` | `/api/v1/network/derp-map` | Get DERP relay map for WireGuard coordination |

### Authentication

All REST and git HTTP endpoints require a Bearer token in the `Authorization` header. **SSE endpoints** (`/api/v1/agents/{id}/logs/stream`, `/api/v1/audit/stream`) also accept a `?token=<token>` query parameter as a fallback, since `EventSource` cannot set request headers:

```
Authorization: Bearer <token>
```

Four auth mechanisms are accepted (checked in priority order):

| Mechanism | How to obtain | Scope |
|---|---|---|
| `GYRE_AUTH_TOKEN` env var | Server config (default: `gyre-dev-token`) | Global admin — all endpoints |
| JWT agent token (EdDSA) | Returned by `POST /api/v1/agents/spawn` (starts with `ey`, 3 dot-separated parts) | Agent-scoped; signed + expiry validated + revocation checked; verify via `/.well-known/jwks.json`; TTL via `GYRE_AGENT_JWT_TTL` (M18) |
| Federated JWT (remote EdDSA) | JWT minted by a trusted remote Gyre instance in `GYRE_TRUSTED_ISSUERS` | Agent-scoped; verified via remote OIDC discovery + JWKS (no local registration); `agent_id = "<remote-host>/<sub>"`; JWKS cached 5 min per issuer (G11) |
| Per-agent UUID token | Returned by `POST /api/v1/agents` (legacy); still accepted for backwards compatibility | Agent-scoped operations |
| API key (`gyre_<uuid>`) | `POST /api/v1/auth/api-keys` (Admin only) | Same as the user's role |
| JWT (Keycloak OIDC) | Keycloak token exchange | Role from `realm_access` JWT claim |

**User roles** (M4.2, populated from Keycloak `realm_access.roles` JWT claim):

| Role | Permissions |
|---|---|
| `Admin` | All operations including API key creation and user management |
| `Developer` | Full CRUD on projects, repos, tasks, MRs |
| `Agent` | Spawn/complete agent ops, task assignment, git push |
| `ReadOnly` | GET-only access |

The git HTTP endpoints (`/git/...`) accept all four auth mechanisms so that `gyre clone` / `gyre push` can use the per-agent token stored in `~/.gyre/config`.

**RBAC enforcement (M4.3):** Role-checking axum extractors (`RequireDeveloper`, `RequireAgent`, `RequireReadOnly`) enforce role hierarchy Admin > Developer > Agent > ReadOnly. Returns `403 {"error":"insufficient permissions"}` on failure. Admin-only endpoints additionally use the `AdminOnly` extractor.

**ABAC enforcement (G6):** For endpoints that enforce attribute-based access control (git push, agent spawn), JWT bearer tokens are additionally evaluated against the repo's `AbacPolicy` list. Each policy is a set of claim-match rules combined with AND; policies are OR'd — access is granted if any one policy fully matches. The global `GYRE_AUTH_TOKEN`, per-agent UUID tokens, and API keys bypass ABAC and are granted access by RBAC alone; only JWT bearers (agent JWTs, Keycloak JWTs, federated JWTs) are subject to policy evaluation.

### Server Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `GYRE_PORT` | `3000` | TCP port to listen on |
| `GYRE_AUTH_TOKEN` | `gyre-dev-token` | Bearer token clients must send to authenticate |
| `GYRE_BASE_URL` | `http://localhost:<port>` | Public base URL (used in clone URLs returned by spawn API) |
| `GYRE_LOG_FORMAT` | _(human-readable)_ | Set to `json` for structured JSON log output (M4.1) |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | _(disabled)_ | OTLP/gRPC collector URL, e.g. `http://otel-collector:4317` (M4.1) |
| `GYRE_OIDC_ISSUER` | _(disabled)_ | Keycloak realm URL, e.g. `http://keycloak:8080/realms/gyre` — enables JWT auth (M4.2) |
| `GYRE_OIDC_AUDIENCE` | _(none)_ | Optional JWT audience claim for Keycloak token validation (M4.2) |
| `RUST_LOG` | `info` | Log level filter (e.g. `debug`, `gyre_server=trace`) |
| `GYRE_SNAPSHOT_PATH` | `./snapshots/` | Directory for DB snapshot files (`POST /api/v1/admin/snapshot`) |
| `GYRE_MAX_BODY_SIZE` | `10485760` (10 MB) | Maximum HTTP request body size in bytes (M7.3) |
| `GYRE_CORS_ORIGINS` | `http://localhost:3000,...` | Comma-separated allowed CORS origins. Default: localhost:2222, localhost:3000, localhost:5173 **plus `http://localhost:{GYRE_PORT}` appended automatically when not already present**. Set to `*` to allow all (not recommended for production). (M7.3, M-5) |
| `GYRE_AGENT_JWT_TTL` | `300` | Lifetime in seconds for EdDSA JWT agent tokens issued by `POST /api/v1/agents/spawn`. After expiry, token is rejected even if not explicitly revoked. Reduced from 3600 to 300 in M27.5. (M18, M27) |
| `GYRE_SIGSTORE_MODE` | `local` | Commit signing backend for `jj squash`: `local` signs with the forge's Ed25519 key; `fulcio` is reserved for future external Fulcio CA integration (logs a warning, does not block). (M13.8) |
| `GYRE_TRUSTED_ISSUERS` | _(disabled)_ | Comma-separated base URLs of trusted remote Gyre instances (e.g. `https://gyre-2.example.com`). Enables G11 federation: JWTs minted by these instances are verified via remote OIDC discovery + JWKS (cached 5 min). Federated agents receive `Agent` role; `agent_id = "<remote-host>/<sub>"`. (G11) |
| `GYRE_RATE_LIMIT` | `100` | Requests per second allowed per IP before 429 (M7.3) |
| `GYRE_AUDIT_SIMULATE` | _(disabled)_ | Set to `true` to run the audit event simulator on startup (M7.1) |
| `GYRE_DEFAULT_COMPUTE_TARGET` | `local` | Default compute target type when no `compute_target_id` is supplied on spawn: `local` (subprocess) or `container` (Docker/Podman with G8 security defaults); requires Docker or Podman on `PATH` when set to `container` (M19.1) |
| `GYRE_PROCFS_MONITOR` | _(enabled)_ | Set to `false` to disable the procfs-based agent process monitor (G7). Polls `/proc/{pid}/fd/` and `/proc/{pid}/net/tcp` every 5 s per live agent PID; emits real `FileAccess` and `NetworkConnect` audit events. No-op on non-Linux platforms. |
| `GYRE_REPOS_PATH` | `./repos/` | Directory for bare git repositories on disk. Created on startup if absent. (M10.3) |
| `GYRE_GIT_PATH` | `git` | Path to the `git` binary. Defaults to `git` (resolved via `PATH`). Override for NixOS/container environments where git is at a fixed store path (e.g. `/nix/store/.../bin/git`). Used by smart HTTP handlers, merge processor, and spec lifecycle hooks. |
| `GYRE_DATABASE_URL` | _(unset — in-memory)_ | Database URL. `sqlite://gyre.db` for SQLite or `postgres://user:pass@host/db` for PostgreSQL. When set, all port traits persist via Diesel ORM with auto-migrations. Unset = in-memory (default, stateless). (M10.1, M15.1, M15.2) |
| `GYRE_SCIM_TOKEN` | _(unset — SCIM disabled)_ | Bearer token SCIM clients must send to `/scim/v2/` endpoints. When unset, SCIM provisioning endpoints return 401. Separate from `GYRE_AUTH_TOKEN`. (M23) |
| `GYRE_RTO` | _(unset)_ | Recovery Time Objective in seconds; returned by `GET /api/v1/admin/bcp/targets` (M23) |
| `GYRE_RPO` | _(unset)_ | Recovery Point Objective in seconds; returned by `GET /api/v1/admin/bcp/targets` (M23) |
| `GYRE_AGENT_CREDENTIALS` | _(unset)_ | Comma-separated `KEY=value` pairs injected into every container agent spawn (e.g. `ANTHROPIC_API_KEY=sk-ant-xxx`). **M27:** credentials are injected as `GYRE_CRED_KEY=value` and held by the `cred-proxy` sidecar — raw values are never in the agent process env. Anthropic API calls are routed through the proxy via `ANTHROPIC_BASE_URL`. On startup, if Docker/Podman is on `PATH`, the server auto-registers a `gyre-agent-default` container compute target. (M25, M27) |
| `GYRE_AGENT_GCP_SA_JSON` | _(unset)_ | GCP service account JSON (full JSON string) for Vertex AI provider. Injected as `GYRE_CRED_GCP_SA_JSON` and held by `cred-proxy` which emulates the GCE metadata server on `127.0.0.1:8080` for OAuth2 token exchange. Agent env gets `GCE_METADATA_HOST=127.0.0.1:8080`. (M27) |
| `GYRE_CRED_ALLOWED_HOSTS` | `api.anthropic.com,gitlab.com,api.github.com` | Comma-separated allowlist of destination hostnames the `cred-proxy` sidecar will forward requests to. `POST /proxy` calls to unlisted hosts receive 403. Prevents SSRF via the credential proxy. (M27-A) |

### WebSocket Protocol (`gyre-common::WsMessage`)

All WS messages are JSON with a `"type"` discriminant. Auth must be the first message.
See `crates/gyre-common/src/protocol.rs` for the full type definitions.

```json
// 1. Auth handshake (required first):
{"type":"Auth","token":"gyre-dev-token"}
{"type":"AuthResult","success":true,"message":"authenticated"}

// 2. Liveness probe:
{"type":"Ping","timestamp":1234567890}
{"type":"Pong","timestamp":1234567890}

// 3. Record an activity event (server stores + broadcasts to all clients):
{"type":"ActivityEvent","event_id":"abc","agent_id":"server","event_type":"RUN_STARTED","description":"Task started","timestamp":1234567890}

// 4. Query activity log over WebSocket:
{"type":"ActivityQuery","since":1234567800,"limit":50}
{"type":"ActivityResponse","events":[...]}

// 5. Domain event push (server → client, M10.2) — emitted automatically on mutations:
{"type":"DomainEvent","event":"AgentCreated","id":"<uuid>"}
{"type":"DomainEvent","event":"AgentStatusChanged","id":"<uuid>","status":"Active"}
{"type":"DomainEvent","event":"AgentContainerSpawned","id":"<agent-uuid>","container_id":"<docker-container-id>","image":"<image-ref>","image_hash":"<sha256-digest>"}
{"type":"DomainEvent","event":"TaskCreated","id":"<uuid>"}
{"type":"DomainEvent","event":"TaskTransitioned","id":"<uuid>","status":"in_progress"}
{"type":"DomainEvent","event":"MrCreated","id":"<uuid>"}
{"type":"DomainEvent","event":"MrStatusChanged","id":"<uuid>","status":"merged"}
{"type":"DomainEvent","event":"ActivityRecorded","id":"<uuid>","event_type":"RUN_STARTED"}
{"type":"DomainEvent","event":"QueueUpdated"}
{"type":"DomainEvent","event":"DataSeeded"}
{"type":"DomainEvent","event":"PushRejected","repo_id":"<uuid>","branch":"<ref>","reason":"<gate-name>"}
{"type":"DomainEvent","event":"SpeculativeConflict","repo_id":"<uuid>","branch":"<ref>"}
{"type":"DomainEvent","event":"SpeculativeMergeClean","repo_id":"<uuid>","branch":"<ref>"}
{"type":"DomainEvent","event":"HotFilesChanged","repo_id":"<uuid>"}
{"type":"DomainEvent","event":"SpecChanged","repo_id":"<uuid>","spec_path":"specs/system/foo.md","change_kind":"added","task_id":"<uuid>"}
{"type":"DomainEvent","event":"GateFailure","mr_id":"<uuid>","gate_name":"<name>","gate_type":"agent_review","status":"failed","output":"<gate output>","spec_ref":"specs/system/agent-gates.md@<sha>","gate_agent_id":"<uuid>"}
{"type":"DomainEvent","event":"StaleSpecWarning","repo_id":"<uuid>","mr_id":"<uuid>","spec_path":"<relative-spec-path>","current_sha":"<40-char-hex>","mr_sha":"<40-char-hex>"}
```

The in-memory `ActivityStore` holds up to 1000 events (oldest dropped when full).
The same events are also queryable via `GET /api/v1/activity?since=<ts>&limit=<n>`.

**Domain events (M10.2):** After authenticating, clients receive server-push `DomainEvent` frames whenever agents, tasks, or MRs are mutated via REST. No client request needed — the server broadcasts to all connected sessions automatically. See `crates/gyre-server/src/domain_events.rs` for the full enum.

#### AG-UI Event Taxonomy (`gyre-common::AgEventType`)

`event_type` in `ActivityEvent` is a typed `AgEventType` enum (M5.1). Accepted values:

| Value | Meaning |
|---|---|
| `TOOL_CALL_START` | Agent began invoking a tool |
| `TOOL_CALL_END` | Tool call completed |
| `TEXT_MESSAGE_CONTENT` | Agent produced text output |
| `RUN_STARTED` | Agent task run started |
| `RUN_FINISHED` | Agent task run finished |
| `STATE_CHANGED` | Agent or task state transition |
| `ERROR` | Error occurred |
| `<custom>` | Any other string maps to `Custom(String)` |

#### Audit Event Taxonomy (`gyre-domain::AuditEventType`)

`event_type` in audit events flowing through `GET /api/v1/audit/events` and `GET /api/v1/audit/stream` is a typed `AuditEventType` enum. Accepted values (snake_case strings):

| Value | Meaning |
|---|---|
| `file_access` | Agent accessed a file path (procfs monitor, G7) |
| `network_connect` | Agent made a network connection (procfs monitor, G7) |
| `process_exec` | Agent exec'd a subprocess |
| `container_started` | Container successfully started for an agent (M23) |
| `container_stopped` | Container exited cleanly (M23) |
| `container_crashed` | Container exited with non-zero code or was force-killed (M23) |
| `container_oom` | Container OOM-killed by the kernel (M23) |
| `container_network_blocked` | Outbound network attempt blocked by `--network=none` (G8, M23) |

### MCP Server (M5.1)

Gyre exposes an MCP (Model Context Protocol) server at `/mcp`. Agents can discover and call Gyre capabilities as MCP tools.

**Endpoints:**
- `POST /mcp` — JSON-RPC 2.0. Methods: `initialize`, `tools/list`, `tools/call`
- `GET /mcp/sse` — SSE stream of typed AG-UI activity events

**Authentication:** Same Bearer token as REST API (`Authorization: Bearer <token>`).

**Available tools** (from `tools/list`):

| Tool | Description |
|---|---|
| `gyre_create_task` | Create a new task |
| `gyre_list_tasks` | Query tasks (`status`, `assigned_to` filters) |
| `gyre_update_task` | Update task fields or status |
| `gyre_create_mr` | Create a merge request |
| `gyre_list_mrs` | List merge requests (`status`, `repository_id` filters) |
| `gyre_record_activity` | Log a typed AG-UI activity event |
| `gyre_agent_heartbeat` | Send agent heartbeat |
| `gyre_agent_complete` | Signal task completion (opens MR, cleans worktree) |
| `gyre_search` | Full-text search across all entities (`q`, `entity_type`, `workspace_id`, `limit` params) (M22.7) |
| `gyre_analytics_query` | Decision-support analytics (`query_type`: `usage`\|`compare`\|`top`); wraps the three M23 analytics endpoints (M23) |

Example MCP `initialize` call:
```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{"name":"my-agent","version":"1.0"}}}
```

### A2A Protocol (M5.2)

Agents publish **Agent Cards** announcing their capabilities and can discover peers.

**AgentCard schema** (`PUT /api/v1/agents/{id}/card`):
```json
{
  "agent_id": "<uuid>",
  "name": "worker-1",
  "description": "Implements backend tasks",
  "capabilities": ["rust", "api-design"],
  "protocols": ["mcp", "a2a"],
  "endpoint": "http://worker-1:3000"
}
```

**Discovery** (`GET /api/v1/agents/discover?capability=rust`): returns Agent Cards for all `Active` agents matching the optional capability filter.

**Typed messages** (`POST /api/v1/agents/{id}/messages`): the `payload` field may carry a structured `MessageType`:

| Type | Use |
|---|---|
| `TaskAssignment` | Delegate a task to a peer agent |
| `ReviewRequest` | Request code review from a peer |
| `StatusUpdate` | Broadcast progress update |
| `Escalation` | Escalate a blocked situation |
| `FreeText` | Unstructured message |

### Agent Compose Spec (M5.2)

Declarative multi-agent team blueprints. Apply via `POST /api/v1/compose/apply` with JSON body or YAML body (`Content-Type: application/yaml`).

**Example `agent-compose.yaml`:**
```yaml
agents:
  - name: orchestrator
    role: Orchestrator
    capabilities: [planning, decomposition]
    task:
      title: "Implement feature X"
      description: "Break down and delegate feature X"
      priority: high

  - name: backend-worker
    role: Developer
    parent: orchestrator
    capabilities: [rust, api-design]
    task:
      title: "Implement REST endpoints"
      priority: medium

  - name: reviewer
    role: Reviewer
    parent: orchestrator
    capabilities: [code-review]
```

Agents are created in dependency order (parents before children). Parent links are set automatically. After apply, poll `GET /api/v1/compose/status` for agent states. Call `POST /api/v1/compose/teardown` to stop all agents when done.


### Agent Spawn / Complete API (M3.2)

`POST /api/v1/agents/spawn` -- single-call agent provisioning:

```json
// Request
{
  "name": "worker-1",               // required; must match [a-zA-Z0-9._-]{1,63} -- shell metacharacters rejected with 400 (M19.5-A)
  "repo_id": "<repo-uuid>",
  "task_id": "<task-uuid>",
  "branch": "feat/my-feature",
  "parent_id": "<orchestrator-agent-uuid>",    // optional
  "compute_target_id": "<target-uuid>",        // optional — remote compute target
  "disconnected_behavior": "Pause"             // optional — "Pause" (default) | "ContinueOffline" | "Abort"; controls stale-agent-detector behavior when agent loses connectivity
}

// Response 201
{
  "agent": {
    "id": "...", "name": "worker-1", "status": "Active",
    "spawned_by": "<caller-agent-id or user-id>",   // M13.2: who initiated spawn
    ...
  },
  "token": "<signed-EdDSA-JWT>",   // M18: starts with "ey", 3 dot-separated parts; claims: sub=agent_id, task_id, spawned_by, exp. G10: when spawned on a real process, also embeds wl_pid, wl_hostname, wl_compute_target, wl_stack_hash. M19.4: container-spawned agents additionally embed wl_container_id, wl_image_hash. Verify via /.well-known/jwks.json. Legacy UUID tokens still accepted from POST /api/v1/agents.
  "worktree_path": "/path/to/worktree",
  "clone_url": "http://localhost:3000/git/project/repo.git",
  "branch": "feat/my-feature",
  "jj_change_id": "<jj-change-id-or-null>",   // present when jj is initialized in worktree (best-effort)
  "container_id": "<docker-container-id-or-null>"   // present when agent was launched in a container via GYRE_DEFAULT_COMPUTE_TARGET=container or a container compute_target_id (M19.1)
}
```

`POST /api/v1/agents/{id}/complete` -- teardown after work is done:

```json
// Request
{
  "branch": "feat/my-feature",
  "title": "Add my feature",
  "target_branch": "main"
}

// Response 200 -- returns the opened MergeRequest
```

The server automatically: opens the MR, marks the task done, removes the git worktree, and marks the agent Idle.

**Idempotent (M13.7):** Calling complete a second time returns **202 Accepted** rather than an error — safe to retry on network failure or agent restart.

**Token revocation (M13.7):** The agent's bearer token is revoked in the database on successful completion. Any subsequent API call with the same token will be rejected with 401. Agents must not reuse a token after completing.

**Commit provenance (M13.2):** When an agent pushes via Smart HTTP, the server automatically captures:
- `spawned_by` — the identity of whoever called `POST /api/v1/agents/spawn` for this agent
- `X-Gyre-Model-Context` — optional request header agents may set on the `git-receive-pack` call to record model/context metadata (e.g. `claude-opus-4@anthropic`)

These fields appear on `AgentCommit` records returned by `GET /api/v1/repos/{id}/agent-commits`.

**Spec binding on MR create (M12.3):** `POST /api/v1/merge-requests` accepts an optional `spec_ref` field in the request body — a string of the form `"specs/system/agent-gates.md@<40-char-sha>"` — to cryptographically bind the MR to the spec version it implements. A `GateFailure` domain event is broadcast if an `AgentReview` or `AgentValidation` gate fails.

**Container Agent Environment Variables (M24):** When spawning an agent into a container compute target, the server pre-mints the JWT (so it can be passed in at container start) and injects the following environment variables into the container:

| Variable | Value | Purpose |
|---|---|---|
| `GYRE_SERVER_URL` | Server base URL | API endpoint for heartbeat, complete, logs |
| `GYRE_AUTH_TOKEN` | Pre-minted EdDSA JWT | Bearer token for all API calls — revoked on `complete` |
| `GYRE_CLONE_URL` | Git Smart HTTP URL | Clone URL for the assigned repo |
| `GYRE_BRANCH` | Branch name | Branch to clone and work on |
| `GYRE_AGENT_ID` | Agent UUID | Identity for API calls |
| `GYRE_TASK_ID` | Task UUID | Assigned task reference |
| `GYRE_REPO_ID` | Repository UUID | Repo being worked on |
| `GYRE_AGENT_COMMAND` | _(optional)_ | Command for the entrypoint to exec after setup (e.g. a CI script) |
| `GYRE_CRED_PROXY` | `http://127.0.0.1:8765` | Address of the `cred-proxy` sidecar (M27); injected so entrypoint and agent-runner can reference the proxy address |
| `ANTHROPIC_BASE_URL` | `http://127.0.0.1:8765` | Routes Anthropic SDK calls through cred-proxy; raw API key never exposed to agent process (M27) |
| `ANTHROPIC_API_KEY` | `proxy-managed` | Placeholder so Anthropic SDK initialises; cred-proxy injects the real `x-api-key` header per request (M27) |

The `docker/gyre-agent/` directory contains a reference `Dockerfile` (Node 22 Alpine + git + curl) and `entrypoint.sh` that validates these vars, configures git credentials via a credential helper (token not embedded in the clone URL), clones the branch, sends an initial heartbeat, then `exec`s `GYRE_AGENT_COMMAND` or — if unset — `node /gyre/agent-runner.mjs` for fully autonomous operation. `agent-runner.mjs` connects to the Gyre MCP server, reads the assigned task, implements it, commits, pushes, and calls `gyre_agent_complete`. **WireGuard mesh (M26):** `setup-wg.sh` in the same directory handles agent-side mesh setup — generates a Curve25519 keypair, registers the pubkey via `POST /api/v1/network/peers`, fetches the peer list, brings up the `wg0` interface with the allocated `mesh_ip`, and adds routes. Run it after clone and before `agent-runner.mjs` when `GYRE_WG_ENABLED=true`. The peer response includes `mesh_ip: Option<String>` (allocated from `GYRE_WG_CIDR` pool) and `is_stale: bool`. Build and register:
```bash
docker build -t gyre-agent:latest docker/gyre-agent/

# M25 zero-config: start server with API key — gyre-agent-default is auto-registered
GYRE_AGENT_CREDENTIALS=ANTHROPIC_API_KEY=sk-ant-xxx cargo run -p gyre-server

# Or manually create a container compute target (Admin -> Compute -> Add) with type=container
# Agent containers need bridge networking to reach server:
# Set config: {"image": "gyre-agent:latest", "network": "bridge"}
```

**Custom git ref namespaces (M13.6):** The server writes refs into reserved namespaces on each lifecycle event:

| Event | Ref written | Purpose |
|---|---|---|
| `spawn` | `refs/agents/{agent-id}/head` | Points to the HEAD commit at spawn; survives branch force-pushes |
| `spawn` | `refs/ralph/{task-id}/implement` | Marks the implement step of the Ralph loop for this task |
| `complete` | `refs/agents/{agent-id}/snapshots/{n}` | Immutable snapshot of the branch tip at completion (n increments per call) |
| `merge` | `refs/notes/attestations` | Signed `MergeAttestation` JSON attached as git note to the merge commit SHA; inspect via `git notes --ref=refs/notes/attestations show <sha>` (G5) |

These refs survive agent restarts. Query them via standard git: `git ls-remote <clone-url> 'refs/agents/*'`.

### Gate Agent Protocol (M12.1)

When the merge queue executes an `AgentReview` or `AgentValidation` gate, it spawns the configured command as a subprocess with these environment variables:

| Variable | Value |
|---|---|
| `GYRE_SERVER_URL` | Server base URL |
| `GYRE_REVIEW_TOKEN` / `GYRE_VALIDATION_TOKEN` | Scoped per-run Bearer token — revoked on process exit |
| `GYRE_MR_ID` | UUID of the MR being reviewed |
| `GYRE_GATE_ID` | UUID of the gate triggering this run |
| `GYRE_GATE_AGENT_ID` | Identity to use when submitting reviews via API |
| `GYRE_DIFF_URL` | URL to fetch the MR diff |
| `GYRE_SPEC_REF` | Spec reference bound to the MR (if any) |
| `GYRE_PERSONA` | Persona file path for the gate |

**`AgentReview` protocol:** exit with any code; server checks for an Approved/ChangesRequested review submitted by `GYRE_GATE_AGENT_ID` after the process exits.

**`AgentValidation` protocol:** exit 0 = pass, non-zero = fail.

**Security:** each execution gets a unique `gyre_gate_<uuid>` token (revoked on completion even on crash/timeout); command split on whitespace — no `sh -c` shell wrapper; 5-minute default timeout prevents hung gate agents.

**Merge processor dep failure handling (P5):** before processing each queued entry, the merge processor checks dependency health: if a dependency MR is `Closed`, the queue entry is marked `Failed` and a High-priority task `"Dependency MR-{id} was closed, reassess MR-{dependent}"` is auto-created; if a dependency has 3+ gate failures, an escalation warning is logged.

### Spec Lifecycle Automation (M13.8)

When an agent pushes to the **default branch** of any repo, the post-receive hook scans for changes to watched spec paths. If spec files are added, modified, deleted, or renamed, the server automatically creates a task and broadcasts a `SpecChanged` domain event.

**Watched paths** (changes outside these prefixes are ignored):
- `specs/system/`
- `specs/development/`

**Auto-created task titles and labels by change type:**

| `git diff --name-status` | Task title | Labels | Priority |
|---|---|---|---|
| `A` (Added) | `Implement spec: <path>` | `spec-implementation`, `auto-created` | Medium |
| `M` (Modified) | `Review spec change: <path>` | `spec-drift-review`, `auto-created` | High |
| `D` (Deleted) | `Handle spec removal: <path>` | `spec-deprecated`, `auto-created` | High |
| `R` (Renamed) | `Update spec references: <old> -> <new>` | `spec-housekeeping`, `auto-created` | Medium |

The task description records the spec path and repo ID. The `SpecChanged` domain event is broadcast over WebSocket immediately after the task is created, so dashboards and listeners can react in real time.

**No action required** from agents pushing spec changes — task creation is automatic and idempotent within a single push. Multiple spec files changed in one push create one task per file.

**Auto-revocation of spec approvals:** When a watched spec file is **modified**, **deleted**, or **renamed** in a push to the default branch, all active approvals for that path are automatically revoked. `revoked_by` is set to `"system:spec-lifecycle"` and `revocation_reason` records the push branch. For renames, approvals on the old path are revoked; the new path starts with no approvals.

> `web/dist/` is committed so the server can serve the SPA without requiring `npm` at build
> time. Agents and CI do not need Node installed to build or run `gyre-server`.

### Dashboard (M3.4 + M4.3 + M5 + M6 + M7 + M8.3 + M9)

The Svelte SPA at `GET /*` includes a dashboard with agent management UI:

**Navigation is path-based** — navigate directly to `/<view>` in the URL bar or click the sidebar. Valid view paths: `dashboard`, `activity`, `agents`, `tasks`, `projects`, `merge-queue`, `mcp-catalog`, `compose`, `analytics`, `costs`, `audit`, `spec-approvals`, `specs`, `admin`, `settings`, `workspaces`, `personas`, `budget`, `dependencies`, `spec-graph`, `profile`. Browser back/forward buttons work correctly via `history.pushState`/`popstate`. Legacy `#<view>` hash URLs are still supported on initial load for backwards compatibility. Example: `http://localhost:3000/agents`

**Entity deep-link URLs (M28)** — in addition to sidebar view paths, entity-scoped URLs restore a specific record on direct navigation or page reload: `/repos/:id`, `/tasks/:id`, `/merge-requests/:id`, `/workspaces/:id`. On mount, the app fetches the entity by ID and loads the correct detail view. Back/forward still works via `popstate`.

- **Agent List**: shows all registered agents with status. **"Spawn Agent" button** opens a modal to provision a new sub-agent (name, repo, task, compute target dropdowns; branch is a free-text input with **datalist autocomplete** populated from `GET /api/v1/repos/{id}/branches` when a repo is selected — placeholder `feat/my-feature (new branch name)` guides users to enter a new or existing branch name — M24). On success, displays the agent token and clone URL for use by the spawned agent.
- **Repo Detail**: shows a clone URL bar with one-click copy, pre-filled with the correct `Authorization: Bearer` git credential command.
- **Admin Panel** (M4.3 + M8.3, Admin role required): tab-based navigation (Health / Jobs / Audit / Agents / SIEM / Compute / Network / Snapshots / Retention / BCP) via `Tabs` component. Health tab: uptime, agent/task/project metric cards. Jobs tab: merge processor + stale agent detector status table. Audit tab: searchable activity feed with agent_id / event_type filters. Agents tab: Kill and Reassign action buttons per agent; **Spawn Log** inline timeline per row shows each spawn step with status badge, timestamp, and detail (expand/collapse). SIEM tab: table of forwarding targets with add/edit/delete; modal form (URL, format JSON/CEF/LEEF, event filter, enabled toggle). Compute tab: table of compute targets (local/docker/ssh) with create/delete; modal with name, type, host fields. Network tab: WireGuard peer registry table with register/remove actions; DERP relay map JSON viewer below the table. BCP tab: RTO/RPO metric cards (from `GET /api/v1/admin/bcp/targets`) and a **Run BCP Drill** button (`POST /api/v1/admin/bcp/drill`) that triggers a live snapshot+verify cycle.

Access at `http://localhost:3000` after starting the server. Admin Panel requires `Admin` role via Keycloak JWT (`GYRE_OIDC_ISSUER`) or the global `GYRE_AUTH_TOKEN`.
- **MCP Tool Catalog** (M5.1 + M8.3, sidebar: "MCP Tools"): card grid layout — one card per tool with name, description, and collapsible JSON schema. Lists all 10 MCP tools available on `/mcp` (including `gyre_search` added in M22.7, `gyre_analytics_query` added in M23).
- **Compose View** (M5.2 + M8.3, sidebar: "Compose"): structured section cards with a mono textarea editor. Paste/upload an agent-compose spec (JSON or YAML), apply it, monitor agent states in an interactive tree visualization, and teardown the session.
- **Agent Card Panel** (M5.2 + M8.3): per-agent panel to view and edit the A2A AgentCard (capabilities as `Badge` pills, protocols, endpoint). Improved empty state when no card is published.
- **Analytics View** (M6.1): event counts bar chart and recent events list with property drill-down. Tracks auto-emitted events: `task.status_changed`, `mr.merged`, `agent.spawned`, `agent.completed`, `merge_queue.processed`.
- **Cost View** (M6.1): agent cost breakdown table with total display and per-agent detail drill-down.
- **Admin Panel — M6 additions** (M6.2): snapshot create/restore/delete controls, job history table with Run Now button, retention policy editor, full data export download.
- **Audit View** (M7.1 + M20, sidebar: "Audit"): two-tab view — **Live Stream** tab streams real-time audit events via SSE (`GET /api/v1/audit/stream`) with connected/disconnected indicator; **History** tab shows filtered audit event query with event type and agent ID filters (`GET /api/v1/audit/events`). Aggregate stats card shows event counts by type.
- **Spec Approvals View** (M20, sidebar: "Spec Approvals"): full CRUD for spec approval records — approval table with path, SHA, approver, timestamp; **Approve modal** (path, SHA input); **Revoke modal** (reason input). Uses `GET /api/v1/specs/approvals`, `POST /api/v1/specs/approve`, `POST /api/v1/specs/revoke`.
- **Spec Dashboard** (M21.2, sidebar: "Specs" under Source Control): full spec registry view. Stats cards row (Total / Approved / Pending / Drifted counts live from ledger); filter pills (All / Pending / Approved / Drifted); spec table with path (mono), title, owner, status `Badge` (semantic color), 7-char SHA, relative timestamp; clicking a row opens a slide-in detail panel (380px) with three tabs — **Info** (full ledger metadata: path, title, owner, SHA, approval_mode, drift_status, timestamps), **History** (approval event timeline with approver, SHA, timestamps, revocation reason), **Links** (linked MRs and tasks). **Approve button** opens SHA-confirmation modal → `POST /api/v1/specs/{path}/approve`; **Revoke button** opens reason-input modal → `POST /api/v1/specs/{path}/revoke`. (M21.1/M21.2)
- **SIEM Panel** (M7.1, Admin only): configure SIEM forwarding targets (webhook URL, format, filter), enable/disable per target.
- **Compute Targets** (M7.2, Admin only): register and manage remote compute targets (local, Docker, SSH). Shows target type, host, and status.
- **Network Panel** (M7.3, Admin only): WireGuard peer registry, DERP relay map viewer, per-agent peer status.
- **Dashboard Home** (M8.1 + M9.2): redesigned landing view with metric cards (active agents, open tasks, pending MRs, queue depth), agent health grid with semantic status colors, recent activity feed, merge queue progress bar. Quick Actions row: New Project, New Task, and Seed Demo Data (POST `/api/v1/admin/seed`) buttons for fast task entry without navigating away.
- **Design System** (M8.1): Red Hat brand CSS variables in `web/src/lib/design-system.css` — dark theme with `gray-95` (#151515) background, `red-50` (#ee0000) primary actions, semantic status palette. 13-component library: `Button`, `Badge`, `Card`, `Table`, `Input`, `Modal`, `Toast`, `Tabs`, `Skeleton`, `EmptyState`, `Breadcrumb`, `SearchBar`.
- **Sidebar** (M8.1): grouped nav sections (Overview / Source Control / Agents / Operations / Admin), collapsible to icon-only mode via chevron toggle, server status footer.
- **Global Search** (M8.1): Cmd+K opens `SearchBar` overlay with keyboard navigation across agents, tasks, repos, and MRs.
- **Activity Feed** (M8.2): timeline layout with colored event-type nodes, multi-select filter pills (toggle per event type), relative timestamps, skeleton loading, `EmptyState` when no events match.
- **Agent List** (M8.2 + M11.2 + M19.3): 3-column card grid with table-view toggle, status filter pills, skeleton grid on load, slide-in detail panel with tabbed Info/Logs/Terminal view. Info tab shows agent metadata; when the agent was container-spawned, a **Container** subsection displays `container_id`, `image`, `image_hash`, `runtime`, `started_at`, `stopped_at`, and `exit_code` sourced from `GET /api/v1/agents/{id}/container` (absent for non-container agents, M19.3). Logs tab shows scrollable monospace agent output with live SSE streaming; Terminal tab streams live PTY output via `/ws/agents/{id}/tty` (M11.2).
- **Task Board** (M8.2 + M9.2 + M20): kanban columns with semantic color-coded top borders per status, `Badge` component for priority, `EmptyState` per empty column, skeleton loading. "New Task" button opens Modal (title, description, priority, status) -> POST `/api/v1/tasks`; card appears in the correct column immediately. M20: **cards are clickable** and navigate to **Task Detail view** — Info tab (all task fields: title, description, priority, status, assigned_to, parent) + Artifacts tab (linked PR, Ralph refs).
- **Project List** (M8.2 + M9.2): responsive card grid, skeleton loading, `EmptyState` when no projects exist. "New Project" button opens Modal (name + description) -> POST `/api/v1/projects`. Selecting a project shows "Add Repo" button -> Modal -> POST `/api/v1/repos`. Toast notifications on success/error.
- **Repo Detail** (M8.2 + M20): uses `lib/Tabs` + `lib/Table` components, `Badge` for MR status, relative timestamps, `EmptyState` per empty tab. New M20 tabs: **Policy tab** — ABAC policy editor (`GET/PUT /api/v1/repos/{id}/abac-policy`, Admin) with claim/operator/value rule list + add/remove; spec-policy toggles (`GET/PUT /api/v1/repos/{id}/spec-policy`) for `require_spec_ref`, `require_approved_spec`, `warn_stale_spec`, `require_current_spec`. **Activity tab** — hot files panel (`GET /api/v1/repos/{id}/hot-files`) with agent-weighted counts; click a file to load per-line blame attribution (`GET /api/v1/repos/{id}/blame?path=`). **Gates tab** — quality gate table with delete action + inline create-gate form (name, type, command); push-gate toggles for `ConventionalCommit`, `TaskRef`, `NoEmDash` (`GET/PUT /api/v1/repos/{id}/push-gates`). **Commits tab enhancements** — agent attribution column (`GET /api/v1/repos/{id}/agent-commits`) + Ed25519 signature badge per commit (`GET /api/v1/repos/{id}/commits/{sha}/signature`). **Branches tab enhancement** — speculative merge status badge per branch (`GET /api/v1/repos/{id}/speculative`): conflict/clean chip.
- **Merge Request Detail** (M8.3 + M12.3 + M20): two-column layout — diff panel left, metadata + status timeline right. Diff panel upgraded to side-by-side view with syntax highlighting (M12.3). Status timeline shows each MR lifecycle step with timestamps and reviewer info. M20 additions: **Dependencies panel** in sidebar shows `depends_on` list with inline remove (x) buttons, add-dep input, and read-only "Required by" dependents list (`GET/PUT/DELETE /api/v1/merge-requests/{id}/dependencies`); **spec_ref chip** shows bound spec path + short SHA when present; **atomic_group badge** in Details section.
- **Merge Queue View** (M8.3 + M20): visual flow lanes per queue position with progress bars, estimated wait indicators, and per-entry action buttons (cancel). M20: **DAG toggle** in header switches to dependency graph view (`GET /api/v1/merge-queue/graph`) with blocked-by dependency chips (orange left border) and green ready indicator per entry.
- **Settings** (M8.3): server info card (name, version, milestone fetched from `/api/v1/version`), pulsing WebSocket connection indicator (connected / connecting / disconnected / error with semantic colors), configuration reference table, Gyre branding card, language selector (current locale; add locales by dropping JSON files in `web/src/locales/`).
- **Workspace List** (M22.5, sidebar: "Workspaces" under Overview): workspace switcher grid; create-workspace modal (name + description); click a workspace to drill into detail view.
- **Workspace Detail** (M22.5, drill-in from Workspace List): budget usage progress bars (tokens/day, cost/day, concurrent agents); three tabs: Repos (listed repos in workspace), Members (invite/remove with `WorkspaceRole`), Teams (create/manage).
- **Persona Catalog** (M22.5, sidebar: "Personas" under Agents): card grid with scope badge (`Tenant`/`Workspace`/`Repo`), capabilities list, model/temperature metadata; create-persona modal + delete action. Scope ID field adapts to the selected scope kind: **Workspace** renders a dropdown populated from `GET /api/v1/workspaces`; **Repo** renders a dropdown from `GET /api/v1/repos`; **Tenant** shows a free-text input with hint to use `"default"`. `scopeId` resets when `scopeKind` changes to prevent stale UUIDs.
- **Budget Dashboard** (M22.5, sidebar: "Budget" under Operations): tenant-wide summary cards (total tokens, cost, active agents vs limits); per-workspace breakdown with progress bars showing usage against budget config; calls `GET /api/v1/budget/summary`.
- **Dependency Graph** (M22.5, sidebar: "Dependencies" under Source Control): SVG circular layout of cross-repo `DependencyEdge` records; edge coloring by `DependencyType` (Code/Spec/Api/Schema); click a node to open blast-radius panel (BFS transitive dependents); calls `GET /api/v1/dependencies/graph` and `GET /api/v1/repos/{id}/blast-radius`.
- **Spec Graph** (M22.5, sidebar: "Spec Graph" under Source Control): SVG DAG of `SpecLink` records with link-type colored edges + legend (`implements`, `supersedes`, `depends_on`, `conflicts_with`, `extends`, `references`); node detail panel on click; calls `GET /api/v1/specs/graph`.
- **User Profile** (M22.5, sidebar: "My Profile" under Overview): profile edit form (display_name, avatar_url, timezone, locale, preferences); four tabs: My Agents, My Tasks, My MRs, Notifications (unread badge, mark-read); calls `GET/PUT /api/v1/users/me` and `GET /api/v1/users/me/{agents,tasks,mrs,notifications}`.
- **Global Workspace Selector** (M28, topbar): dropdown fetches all workspaces via `GET /api/v1/workspaces`; selection persists to `localStorage` (`gyre_selected_workspace_id`); passes `workspaceId` to AgentList, TaskBoard, and ProjectList for filtering. "All Workspaces" clears the filter. A **workspace scope chip** showing the selected workspace name also appears in the topbar (M22.5); clicking it navigates to the workspace detail view.
- **Auth Token UI** (M9.3 + M20): auth status dot in topbar (green = authenticated, red = error). Click opens Token modal to view/change the API token stored in `localStorage`; saving reconnects the WebSocket. All REST and MCP calls inject `Authorization: Bearer {token}`. Defaults to `gyre-dev-token` when no token is stored. M20: modal fetches `GET /api/v1/auth/token-info` on open and displays token kind (human-readable: `global` = "Global admin token", `agent_jwt` = "Agent JWT (EdDSA, scoped)", `uuid_token` = "Per-agent UUID token (legacy)", `api_key` = "API key"), agent ID, task ID, scope, and expiry timestamp.

---

## CLI Usage

### Setup (M3.3)

```bash
# Register this CLI instance as a named agent; saves token + agent ID to ~/.gyre/config
gyre init --server http://localhost:3000 --name my-agent --token gyre-dev-token

# Clone a Gyre-hosted repository (uses token from ~/.gyre/config)
gyre clone myproject/myrepo            # clones into ./myrepo/
gyre clone myproject/myrepo --dir /tmp/work

# Push current branch (uses token from ~/.gyre/config)
gyre push                              # pushes to origin
gyre push --remote gyre
```

Config file is stored at `~/.gyre/config` (TOML):

```toml
server = "http://localhost:3000"
token = "<per-agent-auth-token>"
agent_id = "<uuid>"
agent_name = "my-agent"
```

### Agent Operations (M3.3)

```bash
# Show this agent's registered status and current task
gyre status

# List tasks (optional filters)
gyre tasks list
gyre tasks list --status in_progress
gyre tasks list --mine                 # only tasks assigned to this agent

# Assign a task to this agent and mark it in_progress
gyre tasks take <task-id>
```

### Merge Requests (M3.3)

```bash
# Create a merge request for the current branch
gyre mr create --title "My feature" --repo-id <repo-uuid>

# Custom source/target branches
gyre mr create --title "Fix bug" --repo-id <repo-uuid> \
  --source fix/my-bug --target main
```

### Connection / Diagnostics

```bash
# Connect to a running gyre-server (interactive session)
gyre connect --server ws://localhost:3000/ws --token gyre-dev-token

# Ping the server and measure round-trip time
gyre ping --server ws://localhost:3000/ws --token gyre-dev-token

# Check server health via HTTP
gyre health --server http://localhost:3000

# Launch the TUI dashboard (exits on 'q')
gyre tui --server ws://localhost:3000/ws --token gyre-dev-token
```

Default `--server` is `ws://localhost:3000/ws` and default `--token` is `gyre-dev-token`
(matches server defaults, so bare `gyre ping` works against a local dev server).

---

## Branching Convention

| Branch pattern | Purpose |
|---|---|
| `main` | Always green, deployable |
| `feat/<name>` | New features |
| `fix/<name>` | Bug fixes |
| `chore/<name>` | Maintenance, deps, tooling |
| `docs/<name>` | Documentation only |
| `ci/<name>` | CI/CD changes |

Rules:
- Branch from `main`.
- All work lands via PR.
- PRs require CI green before merge.
- No force-push to `main`.

---

## Commit Message Convention

Format: `<type>(<scope>): <description>`

| Type | When to use |
|---|---|
| `feat` | New feature or capability |
| `fix` | Bug fix |
| `docs` | Documentation changes only |
| `style` | Formatting, no logic change |
| `refactor` | Code change that is not a fix or feature |
| `perf` | Performance improvement |
| `test` | Adding or fixing tests |
| `build` | Build system, Cargo.toml changes |
| `ci` | CI/CD pipeline changes |
| `chore` | Dependency updates, tooling |
| `revert` | Revert a previous commit |

Scope is optional but recommended. Use the crate name or subsystem.

Examples:
```
feat(server): add WebSocket endpoint for agent connections
fix(domain): correct task status transition from review to done
docs(agents): update AGENTS.md with new crate structure
ci: cache cargo target directory in GitHub Actions
build(gyre-ports): add async-trait dependency
```

Enforced by `scripts/check-commit-msg.sh` (pre-commit hook on commit-msg stage).

---

## Architecture Decisions

Key specs to read before making changes:

| Topic | Spec |
|---|---|
| Tech stack + hexagonal invariants | [specs/development/architecture.md](specs/development/architecture.md) |
| Design principles (invariants) | [specs/system/design-principles.md](specs/system/design-principles.md) |
| Agent Gates & Spec Binding | [specs/system/agent-gates.md](specs/system/agent-gates.md) |
| Spec Lifecycle Automation | [specs/system/spec-lifecycle.md](specs/system/spec-lifecycle.md) |
| Platform model (ownership, orchestration, personas, governance) | [specs/system/platform-model.md](specs/system/platform-model.md) |
| Spec Registry (manifest + ledger) | [specs/system/spec-registry.md](specs/system/spec-registry.md) |
| Spec links (implements, supersedes, depends_on, conflicts_with, extends, references) | [specs/system/spec-links.md](specs/system/spec-links.md) |
| Cross-repo dependency graph (auto-detect, breaking changes, cascade testing) | [specs/system/dependency-graph.md](specs/system/dependency-graph.md) |
| Meta-Spec Reconciliation (safe iteration on personas, principles, standards) | [specs/system/meta-spec-reconciliation.md](specs/system/meta-spec-reconciliation.md) |
| M0 milestone deliverables | [specs/milestones/m0-walking-skeleton.md](specs/milestones/m0-walking-skeleton.md) |
| M1 milestone deliverables | [specs/milestones/m1-domain-foundation.md](specs/milestones/m1-domain-foundation.md) |
| M2 milestone deliverables | [specs/milestones/m2-source-control.md](specs/milestones/m2-source-control.md) |
| M3 milestone deliverables | [specs/milestones/m3-agent-orchestration.md](specs/milestones/m3-agent-orchestration.md) |
| M4 milestone deliverables | [specs/milestones/m4-identity-observability.md](specs/milestones/m4-identity-observability.md) |
| M5 milestone deliverables | [specs/milestones/m5-agent-protocols.md](specs/milestones/m5-agent-protocols.md) |
| M6 milestone deliverables | [specs/milestones/m6-infrastructure.md](specs/milestones/m6-infrastructure.md) |
| M7 milestone deliverables | [specs/milestones/m7-production-hardening.md](specs/milestones/m7-production-hardening.md) |
| M8 milestone deliverables | [specs/milestones/m8-frontend-excellence.md](specs/milestones/m8-frontend-excellence.md) |
| M9 milestone deliverables | [specs/milestones/m9-functional-ui.md](specs/milestones/m9-functional-ui.md) |
| M10 milestone deliverables | [specs/milestones/m10-persistent-storage.md](specs/milestones/m10-persistent-storage.md) |
| M11 milestone deliverables | [specs/milestones/m11-agent-execution.md](specs/milestones/m11-agent-execution.md) |
| M12 milestone deliverables | [specs/milestones/m12-quality-gates.md](specs/milestones/m12-quality-gates.md) |
| M13 milestone deliverables | [specs/milestones/m13-forge-native.md](specs/milestones/m13-forge-native.md) |
| M14 milestone deliverables | [specs/milestones/m14-supply-chain.md](specs/milestones/m14-supply-chain.md) |
| M15 milestone deliverables | [specs/milestones/m15-diesel-migrations.md](specs/milestones/m15-diesel-migrations.md) |
| M16 milestone deliverables | [specs/milestones/m16-security-hardening.md](specs/milestones/m16-security-hardening.md) |
| M17 milestone deliverables | [specs/milestones/m17-integration-testing.md](specs/milestones/m17-integration-testing.md) |
| M18 milestone deliverables | [specs/milestones/m18-agent-identity.md](specs/milestones/m18-agent-identity.md) |
| M19 milestone deliverables | [specs/milestones/m19-container-runtime.md](specs/milestones/m19-container-runtime.md) |
| M20 milestone deliverables | [specs/milestones/m20-ui-coverage.md](specs/milestones/m20-ui-coverage.md) |
| M21 milestone deliverables | [specs/milestones/m21-spec-registry.md](specs/milestones/m21-spec-registry.md) |
| M22 milestone deliverables | [specs/milestones/m22-platform-entities.md](specs/milestones/m22-platform-entities.md) |
| M25 milestone deliverables | [specs/milestones/m25-agent-runner.md](specs/milestones/m25-agent-runner.md) |
| Database & Migrations | [specs/development/database-migrations.md](specs/development/database-migrations.md) |
| User management & notification system | [specs/system/user-management.md](specs/system/user-management.md) |
| Full-text search (all entities, FTS5/tsvector, MCP tool) | [specs/system/search.md](specs/system/search.md) |
| ABAC policy engine (attribute-based access, scope cascade, audit) | [specs/system/abac-policy-engine.md](specs/system/abac-policy-engine.md) |
| Forge-native advantages | [specs/system/forge-advantages.md](specs/system/forge-advantages.md) |
| Trusted Foundry integration (future pattern) | [specs/system/trusted-foundry-integration.md](specs/system/trusted-foundry-integration.md) |
| Agent experience + legibility | [specs/development/agent-experience.md](specs/development/agent-experience.md) |
| CI, docs, release | [specs/development/ci-docs-release.md](specs/development/ci-docs-release.md) |

---

## Pre-Commit Hooks

Install once per clone:
```bash
pre-commit install
pre-commit install --hook-type commit-msg
```

Hooks run automatically on `git commit`. To run manually:
```bash
pre-commit run --all-files
```

Hook summary:
- `cargo-fmt`: formatting check
- `cargo-clippy`: lint with denied warnings
- `arch-lint`: hexagonal boundary enforcement
- `no-em-dash`: rejects Unicode em-dashes in source
- `conventional-commits`: commit message format (commit-msg stage)
