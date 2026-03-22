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
| `POST/GET` | `/api/v1/projects` | Create / list projects |
| `GET/PUT/DELETE` | `/api/v1/projects/{id}` | Read / update / delete project |
| `POST/GET` | `/api/v1/repos` | Create / list repos (`?project_id=`); response includes mirror fields (`is_mirror`, `mirror_url`, `mirror_interval_secs`, `last_mirror_sync`). `mirror_url` has credentials redacted (`https://***@host`); `path` in create body is ignored — server-computed as `{repos_root}/{project_id}/{name}.git` (M12.2) |
| `GET` | `/api/v1/repos/{id}` | Get repository (includes mirror fields); `mirror_url` has credentials redacted (H-5) |
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
| `POST` | `/api/v1/specs/{path}/approve` | Approve a specific spec version: `{sha}` — path-scoped; transitions ledger Pending → Approved; `sha` must be 40-char hex; **approver type (`agent`/`human`) derived server-side from token kind** (JWT bearer = agent, global token/API key = human; client must not supply); **Developer+ required** — ReadOnly callers receive 403 (M21.1, M21.1-B, M21.1-C) |
| `POST` | `/api/v1/specs/{path}/revoke` | Revoke approval for a specific spec: `{reason}` — path-scoped; caller must be original approver or Admin (M21.1) |
| `GET` | `/api/v1/specs/{path}/history` | Approval event history for a specific spec — list of approval/revocation events with approver, SHA, timestamps, reason (M21.1) |
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
| `POST/GET` | `/api/v1/agents` | Register (returns auth_token) / list (`?status=`) |
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
| `GET` | `/ws/agents/{id}/tty` | WebSocket TTY attach — auth via first-message Bearer token; replays buffered logs then streams live PTY output (M11.2) |
| `POST/GET` | `/api/v1/tasks` | Create / list (`?status=&assigned_to=&parent_task_id=`) |
| `GET/PUT` | `/api/v1/tasks/{id}` | Read / update task |
| `PUT` | `/api/v1/tasks/{id}/status` | Transition task status |
| `POST/GET` | `/api/v1/merge-requests` | Create / list (`?status=&repository_id=`) |
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
| `POST` | `/api/v1/agents/spawn` | Spawn agent: create record, generate token, provision worktree, assign task; writes `refs/agents/{id}/head` and `refs/ralph/{task-id}/implement` (M13.6); JWT bearers are evaluated against the target repo's ABAC policy before spawning — returns 403 if no policy matches (G6) |
| `POST` | `/api/v1/agents/{id}/complete` | Complete agent: open MR, mark task done, clean up worktree; writes `refs/agents/{id}/snapshots/{n}` snapshot ref (M13.6); **idempotent** — returns 202 on double-complete; agent token revoked on success (M13.7) |
| `GET` | `/git/{project}/{repo}/info/refs` | Smart HTTP git discovery (`?service=git-upload-pack` or `git-receive-pack`) |
| `POST` | `/git/{project}/{repo}/git-upload-pack` | Smart HTTP git clone / fetch data |
| `POST` | `/git/{project}/{repo}/git-receive-pack` | Smart HTTP git push data + post-receive hook; SHA values in ref-updates must be valid 40-char hex — non-hex SHAs rejected to prevent argument injection (M-8); pushes to the default branch trigger spec lifecycle task creation (M13.8); optional `X-Gyre-Model-Context` request header captures the agent's model/context for commit provenance (M13.2); JWT bearers are evaluated against the repo's ABAC policy — push rejected with 403 if no policy matches (G6) |
| `GET` | `/api/v1/auth/token-info` | Token introspection — returns token kind (`agent_jwt`, `uuid_token`, `api_key`, `global`) and decoded JWT claims including `task_id`, `spawned_by`, `exp` (M18) |
| `GET` | `/api/v1/federation/trusted-issuers` | List configured trusted remote Gyre instances (base URLs from `GYRE_TRUSTED_ISSUERS`); returns `[]` when federation is disabled (G11) |
| `POST` | `/api/v1/auth/api-keys` | Create API key (Admin role required; returns `gyre_<uuid>` key — stored as SHA-256 hash, visible only once on creation; rotate by creating a new key) |
| `GET` | `/metrics` | Prometheus metrics (request count, duration, active agents, merge queue depth) |
| `GET` | `/api/v1/admin/health` | Admin: server uptime + agent/task/project counts (Admin only) |
| `GET` | `/api/v1/admin/jobs` | Admin: background job status — merge processor + stale agent detector (Admin only) |
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
| `POST` | `/api/v1/costs` | Record a cost entry (agent_id, task_id, cost_type, amount) |
| `GET` | `/api/v1/costs` | Query cost entries (`?agent_id=&task_id=&since=`) |
| `GET` | `/api/v1/costs/summary` | Aggregated cost totals by agent |
| `POST` | `/api/v1/admin/jobs/{name}/run` | Manually trigger a named background job (Admin only) |
| `POST` | `/api/v1/admin/snapshot` | Create point-in-time DB snapshot (Admin only) |
| `GET` | `/api/v1/admin/snapshots` | List all snapshots (Admin only) |
| `POST` | `/api/v1/admin/restore` | Restore DB from a named snapshot (Admin only) |
| `DELETE` | `/api/v1/admin/snapshots/{id}` | Delete a snapshot (Admin only) |
| `GET` | `/api/v1/admin/export` | Export all entities as JSON (Admin only) |
| `GET/PUT` | `/api/v1/admin/retention` | List / update retention policies (Admin only) |
| `POST/GET` | `/api/v1/admin/siem` | Create / list SIEM forwarding targets (Admin only) |
| `PUT/DELETE` | `/api/v1/admin/siem/{id}` | Update / delete a SIEM target (Admin only) |
| `POST/GET` | `/api/v1/admin/compute-targets` | Create / list remote compute targets (`target_type`: `"local"`, `"ssh"`, `"container"` — Docker/Podman, auto-detected via `which`). **Container security defaults (G8):** `--network=none` (no outbound network — opt in via `network` field), `--memory=2g --pids-limit=512` (resource limits — override via `memory_limit`/`pids_limit`), `--user=65534:65534` (nobody:nogroup — override via `user`). (Admin only) |
| `GET/DELETE` | `/api/v1/admin/compute-targets/{id}` | Get / delete a compute target (Admin only) |
| `POST` | `/api/v1/admin/compute-targets/{id}/tunnel` | Open an SSH tunnel for a compute target: `{direction: "forward"|"reverse", local_port, remote_port, local_host?, remote_host?}` (`local_host` and `remote_host` default to `"localhost"`). Reverse tunnels (`-R`) let air-gapped agents dial out so the server can reach them through NAT. (G12, Admin only) |
| `GET` | `/api/v1/admin/compute-targets/{id}/tunnel` | List active SSH tunnels for a compute target (G12, Admin only) |
| `DELETE` | `/api/v1/admin/compute-targets/{id}/tunnel/{tid}` | Close an SSH tunnel — sends SIGTERM to the `ssh -N` process (G12, Admin only) |
| `POST` | `/api/v1/admin/seed` | Idempotent demo data seed: 2 projects, 3 repos, 4 agents, 6 tasks, 2 MRs, 1 queue entry, 5 activity events. Returns `{already_seeded:true}` on repeat. AdminOnly. (M9.1) |
| `POST` | `/api/v1/release/prepare` | Admin: compute next semver version from conventional commits + generate changelog with agent/task attribution; optionally open a release MR. Request: `{repo_id, branch?, from?, create_mr?, mr_title?}`; `branch` and `from` validated against git argument injection — must not start with `-` or contain `..` (M16-A). Response: `{next_version, changelog, commit_count, mr?}` (M16) |
| `POST/GET` | `/api/v1/audit/events` | Record / query eBPF audit events (`?agent_id=&event_type=&since=`) |
| `GET` | `/api/v1/audit/stream` | SSE stream of live audit events |
| `GET` | `/api/v1/audit/stats` | Audit event statistics and counts |
| `POST/GET` | `/api/v1/network/peers` | Register / list WireGuard mesh peers |
| `GET` | `/api/v1/network/peers/agent/{agent_id}` | Get peer record for a specific agent |
| `DELETE` | `/api/v1/network/peers/{id}` | Remove a peer from the mesh |
| `GET` | `/api/v1/network/derp-map` | Get DERP relay map for WireGuard coordination |

### Authentication

All REST and git HTTP endpoints require a Bearer token in the `Authorization` header:

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
| `GYRE_AGENT_JWT_TTL` | `3600` | Lifetime in seconds for EdDSA JWT agent tokens issued by `POST /api/v1/agents/spawn`. After expiry, token is rejected even if not explicitly revoked. (M18) |
| `GYRE_SIGSTORE_MODE` | `local` | Commit signing backend for `jj squash`: `local` signs with the forge's Ed25519 key; `fulcio` is reserved for future external Fulcio CA integration (logs a warning, does not block). (M13.8) |
| `GYRE_TRUSTED_ISSUERS` | _(disabled)_ | Comma-separated base URLs of trusted remote Gyre instances (e.g. `https://gyre-2.example.com`). Enables G11 federation: JWTs minted by these instances are verified via remote OIDC discovery + JWKS (cached 5 min). Federated agents receive `Agent` role; `agent_id = "<remote-host>/<sub>"`. (G11) |
| `GYRE_RATE_LIMIT` | `100` | Requests per second allowed per IP before 429 (M7.3) |
| `GYRE_AUDIT_SIMULATE` | _(disabled)_ | Set to `true` to run the audit event simulator on startup (M7.1) |
| `GYRE_PROCFS_MONITOR` | _(enabled)_ | Set to `false` to disable the procfs-based agent process monitor (G7). Polls `/proc/{pid}/fd/` and `/proc/{pid}/net/tcp` every 5 s per live agent PID; emits real `FileAccess` and `NetworkConnect` audit events. No-op on non-Linux platforms. |
| `GYRE_REPOS_PATH` | `./repos/` | Directory for bare git repositories on disk. Created on startup if absent. (M10.3) |
| `GYRE_DATABASE_URL` | _(unset — in-memory)_ | Database URL. `sqlite://gyre.db` for SQLite or `postgres://user:pass@host/db` for PostgreSQL. When set, all port traits persist via Diesel ORM with auto-migrations. Unset = in-memory (default, stateless). (M10.1, M15.1, M15.2) |

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
  "name": "worker-1",
  "repo_id": "<repo-uuid>",
  "task_id": "<task-uuid>",
  "branch": "feat/my-feature",
  "parent_id": "<orchestrator-agent-uuid>",    // optional
  "compute_target_id": "<target-uuid>"         // optional — remote compute target
}

// Response 201
{
  "agent": {
    "id": "...", "name": "worker-1", "status": "Active",
    "spawned_by": "<caller-agent-id or user-id>",   // M13.2: who initiated spawn
    ...
  },
  "token": "<signed-EdDSA-JWT>",   // M18: starts with "ey", 3 dot-separated parts; claims: sub=agent_id, task_id, spawned_by, exp. G10: when spawned on a real process, also embeds wl_pid, wl_hostname, wl_compute_target, wl_stack_hash. Verify via /.well-known/jwks.json. Legacy UUID tokens still accepted from POST /api/v1/agents.
  "worktree_path": "/path/to/worktree",
  "clone_url": "http://localhost:3000/git/project/repo.git",
  "branch": "feat/my-feature",
  "jj_change_id": "<jj-change-id-or-null>"   // present when jj is initialized in worktree (best-effort)
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

- **Agent List**: shows all registered agents with status. **"Spawn Agent" button** opens a modal to provision a new sub-agent (name, repo, task, branch dropdowns). On success, displays the agent token and clone URL for use by the spawned agent.
- **Repo Detail**: shows a clone URL bar with one-click copy, pre-filled with the correct `Authorization: Bearer` git credential command.
- **Admin Panel** (M4.3 + M8.3, Admin role required): tab-based navigation (Health / Jobs / Audit / Agents / SIEM / Compute / Network / Snapshots / Retention) via `Tabs` component. Health tab: uptime, agent/task/project metric cards. Jobs tab: merge processor + stale agent detector status table. Audit tab: searchable activity feed with agent_id / event_type filters. Agents tab: Kill and Reassign action buttons per agent; **Spawn Log** inline timeline per row shows each spawn step with status badge, timestamp, and detail (expand/collapse). SIEM tab: table of forwarding targets with add/edit/delete; modal form (URL, format JSON/CEF/LEEF, event filter, enabled toggle). Compute tab: table of compute targets (local/docker/ssh) with create/delete; modal with name, type, host fields. Network tab: WireGuard peer registry table with register/remove actions; DERP relay map JSON viewer below the table.

Access at `http://localhost:3000` after starting the server. Admin Panel requires `Admin` role via Keycloak JWT (`GYRE_OIDC_ISSUER`) or the global `GYRE_AUTH_TOKEN`.
- **MCP Tool Catalog** (M5.1 + M8.3, sidebar: "MCP Tools"): card grid layout — one card per tool with name, description, and collapsible JSON schema. Lists all 8 MCP tools available on `/mcp`.
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
- **Agent List** (M8.2 + M11.2): 3-column card grid with table-view toggle, status filter pills, skeleton grid on load, slide-in detail panel with tabbed Info/Logs/Terminal view. Logs tab shows scrollable monospace agent output with live SSE streaming; Terminal tab streams live PTY output via `/ws/agents/{id}/tty` (M11.2).
- **Task Board** (M8.2 + M9.2 + M20): kanban columns with semantic color-coded top borders per status, `Badge` component for priority, `EmptyState` per empty column, skeleton loading. "New Task" button opens Modal (title, description, priority, status) -> POST `/api/v1/tasks`; card appears in the correct column immediately. M20: **cards are clickable** and navigate to **Task Detail view** — Info tab (all task fields: title, description, priority, status, assigned_to, parent) + Artifacts tab (linked PR, Ralph refs).
- **Project List** (M8.2 + M9.2): responsive card grid, skeleton loading, `EmptyState` when no projects exist. "New Project" button opens Modal (name + description) -> POST `/api/v1/projects`. Selecting a project shows "Add Repo" button -> Modal -> POST `/api/v1/repos`. Toast notifications on success/error.
- **Repo Detail** (M8.2 + M20): uses `lib/Tabs` + `lib/Table` components, `Badge` for MR status, relative timestamps, `EmptyState` per empty tab. New M20 tabs: **Policy tab** — ABAC policy editor (`GET/PUT /api/v1/repos/{id}/abac-policy`, Admin) with claim/operator/value rule list + add/remove; spec-policy toggles (`GET/PUT /api/v1/repos/{id}/spec-policy`) for `require_spec_ref`, `require_approved_spec`, `warn_stale_spec`, `require_current_spec`. **Activity tab** — hot files panel (`GET /api/v1/repos/{id}/hot-files`) with agent-weighted counts; click a file to load per-line blame attribution (`GET /api/v1/repos/{id}/blame?path=`). **Gates tab** — quality gate table with delete action + inline create-gate form (name, type, command); push-gate toggles for `ConventionalCommit`, `TaskRef`, `NoEmDash` (`GET/PUT /api/v1/repos/{id}/push-gates`). **Commits tab enhancements** — agent attribution column (`GET /api/v1/repos/{id}/agent-commits`) + Ed25519 signature badge per commit (`GET /api/v1/repos/{id}/commits/{sha}/signature`). **Branches tab enhancement** — speculative merge status badge per branch (`GET /api/v1/repos/{id}/speculative`): conflict/clean chip.
- **Merge Request Detail** (M8.3 + M12.3 + M20): two-column layout — diff panel left, metadata + status timeline right. Diff panel upgraded to side-by-side view with syntax highlighting (M12.3). Status timeline shows each MR lifecycle step with timestamps and reviewer info. M20 additions: **Dependencies panel** in sidebar shows `depends_on` list with inline remove (x) buttons, add-dep input, and read-only "Required by" dependents list (`GET/PUT/DELETE /api/v1/merge-requests/{id}/dependencies`); **spec_ref chip** shows bound spec path + short SHA when present; **atomic_group badge** in Details section.
- **Merge Queue View** (M8.3 + M20): visual flow lanes per queue position with progress bars, estimated wait indicators, and per-entry action buttons (cancel). M20: **DAG toggle** in header switches to dependency graph view (`GET /api/v1/merge-queue/graph`) with blocked-by dependency chips (orange left border) and green ready indicator per entry.
- **Settings** (M8.3): server info card (name, version, milestone fetched from `/api/v1/version`), pulsing WebSocket connection indicator (connected / connecting / disconnected / error with semantic colors), configuration reference table, Gyre branding card, language selector (current locale; add locales by dropping JSON files in `web/src/locales/`).
- **Auth Token UI** (M9.3 + M20): auth status dot in topbar (green = authenticated, red = error). Click opens Token modal to view/change the API token stored in `localStorage`; saving reconnects the WebSocket. All REST and MCP calls inject `Authorization: Bearer {token}`. Defaults to `gyre-dev-token` when no token is stored. M20: modal fetches `GET /api/v1/auth/token-info` on open and displays token kind, agent ID, task ID, scope, and expiry timestamp.

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
| Spec Registry (manifest + ledger) | [specs/system/spec-registry.md](specs/system/spec-registry.md) |
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
| Database & Migrations | [specs/development/database-migrations.md](specs/development/database-migrations.md) |
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
