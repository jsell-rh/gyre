# Gyre API Reference

All REST and git HTTP endpoints require a Bearer token in the `Authorization` header.

```
Authorization: Bearer <token>
```

See [server-config.md](server-config.md) for authentication mechanisms and environment variables.

> **Breaking changes since M32:**
> - **M33**: Project entity removed. Workspace is now the primary grouping entity.
> - **M34 Slice 6**: Git URL format changed from `/git/{project}/{repo}/...` to `/git/{workspace_slug}/{repo_name}/...`
> - **M34 Slice 5**: `POST /api/v1/specs/approve` and `POST /api/v1/specs/revoke` removed — use path-scoped `POST /api/v1/specs/{path}/approve` and `POST /api/v1/specs/{path}/revoke` instead.

---

## Server Endpoints

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
| `POST/GET` | `/api/v1/tenants` | Create / list tenants (**Admin only**); tenant is the top-level isolation boundary (M34) |
| `GET/PUT/DELETE` | `/api/v1/tenants/{id}` | Read / update / delete tenant (**Admin only**) (M34) |
| `POST/GET` | `/api/v1/tenants/{id}/trust-anchors` | Create (**Admin only**) / list trust anchors for a tenant; body: `{id, issuer, jwks_uri, anchor_type: "user"|"agent"|"addon", constraints?}`; trust anchors are identity issuers the verification algorithm trusts (TASK-006, authorization-provenance §1.1) |
| `GET/PUT/DELETE` | `/api/v1/tenants/{id}/trust-anchors/{aid}` | Get / update / delete a specific trust anchor (**Admin only**); PUT body: `{issuer?, jwks_uri?, anchor_type?, constraints?}` — partial update (TASK-006) |
| `POST/GET` | `/api/v1/workspaces` | Create (**Admin only**, H-15) / list workspaces (`?tenant_id=` filter); workspace groups repos under a shared budget and quota (M22.1) |
| `GET/PUT/DELETE` | `/api/v1/workspaces/{id}` | Read / update (**Admin only**) / delete (**Admin only**) workspace (H-15, M22.1) |
| `POST/GET` | `/api/v1/workspaces/{id}/repos` | Add / list repos in a workspace (M22.1) |
| `GET` | `/api/v1/workspaces/{workspace_id}/tasks` | List tasks scoped to a workspace (M34 Slice 6 — preferred access pattern) |
| `GET` | `/api/v1/workspaces/{workspace_id}/agents` | List agents scoped to a workspace (M34 Slice 6) |
| `GET` | `/api/v1/workspaces/{workspace_id}/merge-requests` | List MRs scoped to a workspace (M34 Slice 6) |
| `POST/GET` | `/api/v1/workspaces/{workspace_id}/messages` | Send a message to all workspace members / list messages; body: `{content, tier: "Directed"|"Telemetry"|"Broadcast", recipient_agent_id?}`; messages are Ed25519-signed at rest (Message Bus Phase 3) |
| `GET` | `/api/v1/workspaces/{workspace_id}/presence` | Workspace presence — `[{user_id, session_id, view, workspace_id, last_seen}]`; stale entries evicted every 30 s (HSI §7, S1.5) |
| `POST/GET` | `/api/v1/personas` | Create (**Admin only**, H-16) / list personas (`?scope=tenant|workspace|repo&scope_id=` filter); `PersonaScope` JSON wire format: `{"kind": "Tenant"|"Workspace"|"Repo", "id": "<uuid>"}` (serde tagged enum — both `kind` and `id` fields required; `id` is the tenant/workspace/repo UUID); Rust type: `Tenant(Id)`, `Workspace(Id)`, `Repo(Id)` (M22.1) |
| `GET/PUT/DELETE` | `/api/v1/personas/{id}` | Read / update (**Admin only**) / delete (**Admin only**) persona -- fields: `name`, `slug`, `scope`, `system_prompt`, `capabilities`, `model`, `temperature`, `max_tokens`, `budget` (H-16, M22.1) |
| `POST` | `/api/v1/personas/{id}/approve` | Approve a persona version — transitions it to active; **Admin only**; records approver + timestamp (VISION-3) |
| `GET` | `/api/v1/personas/resolve` | Resolve the effective persona for a scope: `?scope=tenant|workspace|repo&scope_id=<uuid>` — returns the most specific persona applicable (VISION-3) |
| `POST/GET` | `/api/v1/repos` | Create / list repos (`?workspace_id=`); response includes mirror fields (`is_mirror`, `mirror_url`, `mirror_interval_secs`, `last_mirror_sync`), repo status (`status`: `active`, `archived`, `deleting`). `mirror_url` has credentials redacted (`https://***@host`) (M12.2, M35-lifecycle) |
| `GET/PUT` | `/api/v1/repos/{id}` | Get / update repository; PUT accepts `{name?, description?, default_branch?, workspace_id?}`; response includes `status`, `archived_at`, `workspace_id` fields (M35-lifecycle) |
| `DELETE` | `/api/v1/repos/{id}` | Delete repository (**Admin only**); soft-delete sets `status=deleting`; background job removes bare git directory (M35-lifecycle) |
| `POST` | `/api/v1/repos/{id}/archive` | Archive a repo — sets `status=archived`, cancels open tasks, stops running agents, closes open MRs with `MRStatus::Reverted` (M35-lifecycle) |
| `POST` | `/api/v1/repos/{id}/unarchive` | Unarchive a repo — restores `status=active` (M35-lifecycle) |
| `POST` | `/api/v1/repos/mirror` | Create a pull mirror from an external git URL (bare clone + periodic background sync); body: `{url, name, interval_secs?}`; URL must use `https://` (M12.2) |
| `POST` | `/api/v1/repos/{id}/mirror/sync` | Manually trigger a fetch sync on a mirror repo (M12.2) |
| `GET` | `/api/v1/repos/{id}/branches` | List branches in repository |
| `GET` | `/api/v1/repos/{id}/commits` | Commit log (`?branch=<name>&limit=50`) |
| `GET` | `/api/v1/repos/{id}/diff` | Diff between refs (`?from=<ref>&to=<ref>`) |
| `POST/GET` | `/api/v1/repos/{id}/gates` | Create (**Admin required**) / list quality gates for a repo (`GateType`: `test_command`, `lint_command`, `required_approvals`, `agent_review`, `agent_validation` — serialized as snake_case) (M12.1, M12.3). See [agent-protocol.md](agent-protocol.md) for `AgentReview`/`AgentValidation` env vars. |
| `DELETE` | `/api/v1/repos/{id}/gates/{gate_id}` | Delete a quality gate (M12.1) |
| `POST` | `/api/v1/specs/approve` | Record spec approval: `{path, sha, signature?}` — `sha` must be 40-char hex; **approver identity derived server-side from auth token** (client must not supply `approver_id`) (CISO M12.3-A, M12.3) |
| `GET` | `/api/v1/specs/approvals` | List spec approvals (`?path=<relative-path>` to filter by spec file) (M12.3) |
| `POST` | `/api/v1/specs/revoke` | Revoke a spec approval: `{approval_id, reason}` — caller must be original approver or Admin (returns 403 otherwise); revoker identity derived server-side (client must not supply `revoked_by`) (CISO M12.3-A, M12.3) |
| `GET` | `/api/v1/specs` | List all specs with ledger state — reads `specs/manifest.yaml` + ledger; each entry includes `path`, `title`, `owner`, `sha`, `approval_status`, `drift_status`, `kind?` (M21.1); `?kind=<kind>` filters by spec kind (`meta:persona`, `meta:principle`, `meta:standard`, `meta:process`) (M32) |
| `GET` | `/api/v1/specs/pending` | Specs awaiting approval — ledger entries with `approval_status: Pending` (M21.1) |
| `GET` | `/api/v1/specs/drifted` | Specs with open drift-review tasks — `drift_status: Drifted` (M21.1) |
| `GET` | `/api/v1/specs/index` | Auto-generated markdown index of all specs in manifest (M21.1) |
| `GET` | `/api/v1/specs/{path}` | Get single spec ledger entry by URL-encoded path (M21.1) |
| `POST` | `/api/v1/specs/{path}/approve` | Approve a specific spec version: `{sha, output_constraints?, scope?}` — path-scoped; transitions ledger Pending → Approved; `sha` must be 40-char hex; **approver type (`agent`/`human`) derived server-side from token kind** (JWT bearer = agent, global token/API key = human; client must not supply); approval blocked (400) when an `implements` link exists and parent spec is not yet approved, or when a `conflicts_with` link exists and conflicting spec is already approved (M22.3); **Developer+ required** — ReadOnly callers receive 403; when caller has an active `KeyBinding`, produces a `SignedInput` attestation (TASK-006 Phase 1, audit-only) (M21.1, M21.1-B, M21.1-C) |
| `POST` | `/api/v1/specs/{path}/revoke` | Revoke approval for a specific spec: `{reason}` — path-scoped; caller must be original approver or Admin (M21.1) |
| `POST` | `/api/v1/specs/{path}/reject` | Reject a spec: `{reason}` — transitions Pending → Rejected; caller must be Admin (M21.1) |
| `GET` | `/api/v1/specs/{path}/progress` | Spec implementation progress — linked tasks and MRs with status: `{spec_path, tasks: [...], merge_requests: [...]}` (VISION-1) |
| `GET` | `/api/v1/specs/{path}/history` | Approval event history for a specific spec — list of approval/revocation events with approver, SHA, timestamps, reason (M21.1) |
| `GET` | `/api/v1/specs/{path}/links` | Outbound and inbound spec links for a specific spec — `{links: [{link_type, target_path, direction},...]}` (M22.3) |
| `GET` | `/api/v1/specs/{path}/dependents` | Specs that depend on this one — inbound `depends_on` and `implements` links targeting this spec. Response: `[{id, source_path, link_type, target_path, status, ...}]` (TASK-019, spec-links.md §Querying the Graph) |
| `GET` | `/api/v1/specs/{path}/dependencies` | Specs this spec depends on — outbound `depends_on` and `implements` links from this spec. Response: `[{id, source_path, link_type, target_path, status, ...}]` (TASK-019, spec-links.md §Querying the Graph) |
| `GET` | `/api/v1/specs/stale-links` | All stale links across the tenant — links with `status = "stale"`. Response: `[{source_path, target_path, link_type, stale_since, ...}]` (TASK-019, spec-links.md §Querying the Graph) |
| `GET` | `/api/v1/specs/conflicts` | All active `conflicts_with` links. Response: `[{source_path, target_path, link_type, status, ...}]` (TASK-019, spec-links.md §Querying the Graph) |
| `POST` | `/api/v1/patrol/spec-links` | Accountability agent spec-graph patrol — runs 5 checks (stale links, orphaned supersessions, unresolved conflicts, dangling implementations, deep dependency chains). Body: `{stale_threshold_secs?: u64}` (default 7 days). Response: `{findings: [{type, severity, spec_path, detail, suggested_action}]}`. Error-severity findings create priority-3 notifications for Admin/Developer members. (TASK-023, spec-links.md §Accountability Agent Integration) |
| `GET` | `/api/v1/specs/graph` | Full spec link graph — `{nodes: [{path, title, approval_status},...], edges: [{from, to, link_type},...]}` (M22.3) |
| `POST` | `/api/v1/constraints/validate` | Validate CEL constraint expression syntax: `{constraints: [{name, expression},...], scope?: {allowed_paths?, forbidden_paths?}}` → `{valid: bool, results: [{name, valid, error?},...]}` — compiles each expression with the real CEL parser and validates scope glob-to-CEL conversion; syntax-only, does NOT evaluate against repo state (authorization-provenance §7.6, TASK-007) |
| `POST` | `/api/v1/constraints/dry-run` | Evaluate constraints against repo state (§7.6 dry-run): `{constraints: [{name, expression},...], scope?: {allowed_paths?, forbidden_paths?}, repo_id: string, workspace_id: string}` → `{valid: bool, results: [{name, passed, error?},...]}` — builds a CEL evaluation context from the repo's latest commit diff and workspace config, then evaluates all constraints using the domain evaluator. Returns per-constraint pass/fail results (authorization-provenance §7.6, TASK-007) |
| `GET` | `/api/v1/constraints/strategy` | Preview strategy-implied constraints: `?workspace_id=<id>` → `{constraints: [{name, expression},...]}` — returns the full set of strategy-implied constraints (persona, meta-spec, scope, trust level, attestation policy) that would apply for the given workspace context (authorization-provenance §7.6, TASK-007) |
| `GET/PUT` | `/api/v1/repos/{id}/push-gates` | Get / set active pre-accept push gates for a repo (built-in: ConventionalCommit, TaskRef, NoEmDash); **PUT requires Admin role** (M13.1) |
| `GET/PUT` | `/api/v1/repos/{id}/spec-policy` | Get / set per-repo spec enforcement policy: `{require_spec_ref: bool, require_approved_spec: bool, warn_stale_spec: bool, require_current_spec: bool, enforce_manifest: bool}`. `warn_stale_spec` emits `StaleSpecWarning` domain event when MR spec_ref SHA differs from HEAD; `require_current_spec` blocks merge queue when stale; `enforce_manifest` rejects pushes adding spec files under `specs/` without a `specs/manifest.yaml` entry (spec-registry.md §Manifest Rules). **PUT requires Admin role**. All fields default to `false` (backwards compatible). (M18, TASK-017) |
| `POST` | `/api/v1/repos/{id}/specs/assist` | LLM-assisted spec editing — SSE stream; body: `{spec_path, instruction, draft_content?}`; streams `event: partial` (incremental explanation text), `event: complete` (final `{diff: [{op, path, content}], explanation}` JSON), and `event: error` (`{error, raw_response?}` on invalid LLM output); diff ops: `add`/`remove`/`replace` (S3.3, HSI §11) |
| `POST` | `/api/v1/repos/{id}/specs/save` | Commit spec changes to a feature branch and open an MR; body: `{spec_path, content, message}`; returns `{branch, mr_id}` (S3.3) |
| `POST` | `/api/v1/repos/{id}/prompts/save` | Commit a prompt/spec directly to the default branch; body: `{prompt_path, content, message}` (S3.3) |
| `GET` | `/api/v1/repos/{id}/blame?path={file}` | Per-line agent attribution — which agent last touched each line (M13.4) |
| `GET` | `/api/v1/repos/{id}/hot-files?limit=20` | Files with the most concurrent active agents in the last 24h (M13.4) |
| `GET` | `/api/v1/repos/{id}/review-routing?path={file}` | Ordered list of agents to request review from, ranked by recency and commit count (M13.4) |
| `GET` | `/api/v1/repos/{id}/speculative` | List all speculative merge results for active branches (M13.5) |
| `GET` | `/api/v1/repos/{id}/speculative/{branch}` | Speculative merge result for a specific branch against main (M13.5) |
| `GET` | `/api/v1/repos/{id}/stack-policy` | Get repo's required stack fingerprint for push attestation (M14.2) |
| `PUT` | `/api/v1/repos/{id}/stack-policy` | Set / clear required stack fingerprint (**Admin only**, M14.2) |
| `GET` | `/api/v1/repos/{id}/abac-policy` | Get the ABAC policy list for a repo — array of `AbacPolicy` objects; each policy has `id`, `name`, `rules` (AND within), evaluated as OR across policies (G6) |
| `PUT` | `/api/v1/repos/{id}/abac-policy` | Replace the ABAC policy list (**Admin only**); policies are matched against JWT claims on push and spawn; `rules` is a list of `{claim, operator, value}` match conditions combined with AND; multiple policies in the array are OR'd together (G6) |
| `GET` | `/api/v1/repos/{id}/attestations/{commit_sha}/verification` | Full `VerificationResult` tree for the attestation chain associated with a commit; includes chain structure validation, signature verification, constraint evaluation status (TASK-008, §6.4) |
| `GET` | `/api/v1/repos/{id}/attestations/{commit_sha}/bundle` | `VerificationBundle` for offline verification — contains attestation chain (root to leaf), trust anchors, git diff, and assembly timestamp; can be verified without connecting to the Gyre server (TASK-008, §6.3) |
| `GET` | `/api/v1/repos/{id}/attestations/{commit_sha}/chain` | Attestation chain as directed graph for Explorer visualization — nodes with signer identity, constraint count, verification status; edges show derivation relationships; failed constraints highlighted (TASK-009, §7.6) |
| `GET` | `/api/v1/repos/{id}/aibom` | AI Bill of Materials — per-commit agent attribution + attestation levels (`?from={ref}&to={ref}`); ref names validated to prevent git flag injection (M14.3) |
| `GET` | `/api/v1/repos/{id}/dependencies` | Outgoing dependency edges (`DependencyType`: Code/Spec/Api/Schema/Manual; `DetectionMethod`: auto/manual) (M22.4) |
| `GET` | `/api/v1/repos/{id}/dependents` | Incoming dependency edges (M22.4) |
| `POST` | `/api/v1/repos/{id}/dependencies` | Add a manual dep edge: `{target_repo_id, dep_type, notes?}`; **Admin only** (H-12, M22.4) |
| `DELETE` | `/api/v1/repos/{id}/dependencies/{dep_id}` | Remove a manual dep edge; **Admin only** (H-13, M22.4) |
| `GET` | `/api/v1/repos/{id}/blast-radius` | BFS transitive dependents -- repos affected if this one changes (M22.4) |
| `GET` | `/api/v1/dependencies/graph` | Full tenant-wide dependency DAG: `{nodes, edges}` (M22.4) |
| `GET` | `/api/v1/dependencies/stale` | Stale dependencies tenant-wide (optional `?workspace_id=` filter) — `[{id, source_repo_id, target_repo_id, dependency_type, source_artifact, target_artifact, version_pinned, target_version_current, version_drift, detection_method, status, detected_at, last_verified_at}]` (TASK-021) |
| `GET` | `/api/v1/dependencies/breaking` | List unacknowledged breaking changes tenant-wide — `[{id, dependency_edge_id, source_repo_id, commit_sha, description, detected_at, acknowledged, acknowledged_by?, acknowledged_at?}]` (TASK-020) |
| `POST` | `/api/v1/dependencies/breaking/{id}/acknowledge` | Acknowledge a breaking change, clearing any merge block; returns 204 No Content (TASK-020) |
| `GET` | `/api/v1/workspaces/{id}/dependency-policy` | Per-workspace dependency enforcement policy — `{breaking_change_behavior: block\|warn\|notify, max_version_drift, stale_dependency_alert_days, auto_create_update_tasks}` (TASK-020) |
| `PUT` | `/api/v1/workspaces/{id}/dependency-policy` | Update dependency policy (partial update) — same fields, all optional (TASK-020) |
| `GET` | `/api/v1/repos/{id}/graph` | Full knowledge graph for a repo — `{repo_id, nodes, edges}`; `GraphNode` fields: `id`, `repo_id`, `node_type` (`Package`/`Module`/`Type`/`Interface`/`Function`/`Endpoint`/`Table`), `name`, `qualified_name`, `file_path`, `line_start`/`line_end`, `visibility`, `doc_comment`, `spec_path`, `spec_confidence` (`None`/`Low`/`Medium`/`High`), `last_modified_sha`, `last_modified_by`, `complexity`, `churn_count_30d` (M30) |
| `GET` | `/api/v1/repos/{id}/graph/types` | Type nodes (structs, enums) with their edges (M30) |
| `GET` | `/api/v1/repos/{id}/graph/modules` | Module nodes with containment edges (M30) |
| `GET` | `/api/v1/repos/{id}/graph/node/{node_id}` | Single node + all connected edges — `{node, edges}`; 404 if node not in this repo (M30) |
| `GET` | `/api/v1/repos/{id}/graph/spec/{spec_path}` | Nodes whose `spec_path` matches the given spec (URL-encoded path) with their edges (M30) |
| `GET` | `/api/v1/repos/{id}/graph/concept/{name}` | Concept view — nodes whose `name` or `qualified_name` contains `{name}` (case-insensitive substring) with edges between matching nodes (M30) |
| `GET` | `/api/v1/repos/{id}/graph/timeline` | Architectural deltas — `[{id, repo_id, commit_sha, timestamp, spec_ref?, agent_id?, delta_json}]`; filter with `?since=<epoch>&until=<epoch>`; `delta_json` contains **incremental field-level diffs** (added/removed/changed nodes and edges per commit) enabling time-travel history (migration 000038, incremental extraction) |
| `GET` | `/api/v1/repos/{id}/graph/risks` | Risk metrics per node — `[{node_id, name, qualified_name, churn_rate, fan_out, fan_in, complexity?, spec_covered}]` (M30) |
| `GET` | `/api/v1/repos/{id}/graph/diff` | Graph diff between commits — `{from, to, message, deltas}`; `?from=<ref>&to=<ref>` (defaults: `HEAD~1`/`HEAD`) (M30) |
| `POST` | `/api/v1/repos/{id}/graph/link` | Manually link a node to a spec path: `{node_id, spec_path, confidence?}` (`confidence`: `high`/`medium`/`low`/`none`; default `high`); **Developer+ required** (M30) |
| `GET` | `/api/v1/repos/{id}/graph/predict` | Structural prediction stub — `{repo_id, predictions: []}` (M30) |
| `POST/GET` | `/api/v1/agents` | Register (returns auth_token) / list (`?status=&workspace_id=`) |
| `GET` | `/api/v1/agents/{id}` | Get agent |
| `PUT` | `/api/v1/agents/{id}/status` | Update agent status — `AgentStatus` variants: `Spawning`, `Running`, `Paused`, `Completed`, `Failed`, `Dead`, `Cancelled`; `Paused` used during BCP disconnected mode (M23.3, agent-runtime spec) |
| `POST` | `/api/v1/agents/{id}/usage` | Record LLM usage for an agent — `{model, input_tokens, output_tokens, cost_usd?}`; accumulated in budget tracking; `attestation.meta_specs_used` updated with active workspace meta-spec SHA (agent-runtime spec) |
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
| `GET` | `/api/v1/merge-requests/{id}/timeline` | MR SDLC event timeline — chronological list of events (created, commits pushed, gates run, merged, graph extracted) with timestamps and actor metadata (S2.5, HSI §3) |
| `GET` | `/api/v1/merge-requests/{id}/trace` | Gate-time execution trace for an MR — structured spans capturing gate execution, LLM calls, tool use; `{mr_id, spans: [{span_id, parent_span_id, name, start_ms, end_ms, attributes}]}` (S2.4, HSI §3a) |
| `GET` | `/api/v1/trace-spans/{span_id}/payload` | Full payload for a single trace span — raw input/output data for a gate or LLM call (S2.4) |
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
| `POST` | `/api/v1/agents/spawn` | Spawn agent: create record, generate token, provision worktree, assign task; writes `refs/agents/{id}/head` and `refs/tasks/{task-id}` (M13.6); JWT bearers are evaluated against the target repo's ABAC policy before spawning — returns 403 if no policy matches (G6); returns **429** if workspace or tenant budget limits are exceeded (`max_concurrent_agents`, `max_tokens_per_day`, `max_cost_per_day`) (M22.2) |
| `POST` | `/api/v1/agents/{id}/complete` | Complete agent: open MR, mark task done, clean up worktree; writes `refs/agents/{id}/snapshots/{n}` snapshot ref (M13.6); **idempotent** — returns 202 on double-complete; agent token revoked on success (M13.7) |
| `GET` | `/git/{workspace_slug}/{repo_name}/info/refs` | Smart HTTP git discovery (`?service=git-upload-pack` or `git-receive-pack`) — **M34 Slice 6**: URL format uses workspace slug + repo name (was `{project}/{repo}`) |
| `POST` | `/git/{workspace_slug}/{repo_name}/git-upload-pack` | Smart HTTP git clone / fetch data |
| `POST` | `/git/{workspace_slug}/{repo_name}/git-receive-pack` | Smart HTTP git push data + post-receive hook; SHA values in ref-updates must be valid 40-char hex — non-hex SHAs rejected to prevent argument injection (M-8); pushes to the default branch trigger spec lifecycle task creation (M13.8); **pushes to the default branch trigger automatic polyglot knowledge graph extraction** — `git archive` → language-specific extractor dispatch → persists nodes/edges + records `ArchitecturalDelta` in background; supported languages: **Rust** (syn-based AST), **TypeScript/JavaScript** (tree-sitter), **Python** (tree-sitter, Flask/FastAPI endpoints), **Go** (tree-sitter, packages/structs/interfaces/HTTP handlers) (M30b); optional `X-Gyre-Model-Context` request header captures the agent's model/context for commit provenance (M13.2); JWT bearers are evaluated against the repo's ABAC policy — push rejected with 403 if no policy matches (G6); **auto-detects** `Cargo.toml` path dependencies and creates `DependencyEdge` records for Gyre-hosted repos (M22.4) |
| `GET` | `/api/v1/auth/token-info` | Token introspection — returns token kind (`agent_jwt`, `uuid_token`, `api_key`, `global`) and decoded JWT claims including `task_id`, `spawned_by`, `exp` (M18) |
| `POST` | `/api/v1/auth/key-binding` | Bind ephemeral Ed25519 public key to caller's identity; body: `{public_key: base64, user_signature: base64, ttl_secs?}`; server countersigns as timestamp witness; returns `KeyBindingResponse` with `platform_countersign`; TTL capped at 24h (TASK-006, authorization-provenance §2.3) |
| `GET/PUT` | `/api/v1/users/me` | Current user profile (username, display_name, avatar_url, timezone, locale, global_role, `UserPreferences`); PUT updates fields (M22.8) |
| `GET` | `/api/v1/users/me/agents` | Agents spawned by the current user (M22.8) |
| `GET` | `/api/v1/users/me/tasks` | Tasks assigned to the current user (M22.8) |
| `GET` | `/api/v1/users/me/mrs` | MRs authored by the current user (M22.8) |
| `GET/PUT` | `/api/v1/users/me/notification-preferences` | Get / update notification delivery preferences — per-type channels (email, in-app, webhook), quiet hours, digest frequency (HSI §12) |
| `POST/GET` | `/api/v1/users/me/tokens` | Create / list personal API tokens — `{name, scopes[], expires_at?}`; response includes `token` value only on creation (store it — not retrievable later) (HSI §12) |
| `DELETE` | `/api/v1/users/me/tokens/{id}` | Revoke an API token (HSI §12) |
| `GET` | `/api/v1/users/me/judgments` | Judgment ledger — history of human decisions (approve/reject/trust-adjust) made through the UI; used to personalize future LLM suggestions (HSI §12) |
| `GET` | `/api/v1/users/me/notifications` | Notifications (16 `NotificationType` variants: `MrNeedsReview`, `GateFailure`, `MrMerged`, `SpecChanged`, `AgentCompleted`, etc.; 4 priority levels); **auto-created by server event pipeline**: spec changes, gate failures, agent complete/fail, MR merge, and workspace divergence events all emit notifications automatically — no explicit API call required (M22.8, event-notification pipeline) |
| `PUT` | `/api/v1/users/me/notifications/{id}/read` | Mark notification read (M22.8) |
| `POST` | `/api/v1/notifications/{id}/dismiss` | Dismiss a notification (removes from inbox view) (HSI §2) |
| `POST` | `/api/v1/notifications/{id}/resolve` | Resolve a notification (marks underlying issue addressed) (HSI §2) |
| `GET` | `/api/v1/conversations/{sha}` | Conversation provenance — returns agent conversation history anchored to a commit SHA; `{sha, turns: [{role, content, timestamp, model}]}`; records the agent reasoning that produced the commit (HSI §5, S2.3) |
| `POST/GET` | `/api/v1/workspaces/{id}/members` | Invite (**Admin only**, H-19) / list members; `WorkspaceRole`: Owner, Admin, Developer, Viewer; accept/pending lifecycle (M22.8) |
| `PUT` | `/api/v1/workspaces/{id}/members/{user_id}` | Update a member's `WorkspaceRole`; **Admin only** (H-17, M22.8) |
| `DELETE` | `/api/v1/workspaces/{id}/members/{user_id}` | Remove a member; **Admin only** (H-20, M22.8) |
| `POST/GET` | `/api/v1/workspaces/{id}/teams` | Create (**Admin only**, H-21) / list workspace-scoped teams (M22.8) |
| `PUT/DELETE` | `/api/v1/workspaces/{id}/teams/{team_id}` | Update / delete team; **Admin only** (H-18); `add_member`/`remove_member` idempotent (M22.8) |
| `GET` | `/api/v1/workspaces/{id}/graph` | Cross-repo aggregated knowledge graph for a workspace — all nodes and edges across every repo in the workspace (M30) |
| `GET` | `/api/v1/workspaces/{id}/briefing` | Narrative summary of recent architectural changes — `{workspace_id, since, summary, deltas}`; filter with `?since=<epoch>` (M30) |
| `POST` | `/api/v1/workspaces/{id}/briefing/ask` | SSE Q&A on the workspace briefing — streams LLM-generated answers to a question about recent changes; body: `{question}`; returns `text/event-stream` (S3.2, HSI) |
| `GET` | `/api/v1/workspaces/{id}/graph/concept/{concept_name}` | Workspace-wide concept search — nodes matching `concept_name` across all repos in the workspace (M30) |
| `GET/POST` | `/api/v1/workspaces/{id}/explorer-views` | List / create named explorer views (saved graph perspectives); body: `{name, description?, query?, layout?}` (S3.1) |
| `POST` | `/api/v1/workspaces/{id}/explorer-views/generate` | LLM-generate an explorer view from a natural-language description; body: `{prompt}`; returns a draft `ExplorerView` (S3.1) |
| `GET/PUT/DELETE` | `/api/v1/workspaces/{id}/explorer-views/{view_id}` | Read / update / delete a named explorer view (S3.1) |
| `GET` | `/api/v1/workspaces/{id}/meta-spec-set` | Get workspace's bound meta-spec collection — `{workspace_id, personas: {role: {path, sha}}, principles: [{path, sha}], standards: [{path, sha}], process: [{path, sha}]}`; returns empty set if none configured (M32) |
| `PUT` | `/api/v1/workspaces/{id}/meta-spec-set` | Set workspace meta-spec bindings: same structure as GET response; **Admin only**; 404 if workspace not found (M32) |
| `GET` | `/api/v1/meta-specs/{path}/blast-radius` | Affected workspaces and repos if this meta-spec changes — `{spec_path, affected_workspaces: [{id}], affected_repos: [{id, workspace_id, reason}]}`; path is URL-encoded (M32) |
| `POST` | `/api/v1/workspaces/{id}/meta-specs/preview` | Trigger async preview of a meta-spec change — returns `{preview_id}`; runs reconciliation in background (M32, HSI §1) |
| `GET` | `/api/v1/workspaces/{id}/meta-specs/preview/{preview_id}` | Poll preview status — `{status: pending\|running\|complete\|failed, result?: {affected_agents, drift_count, sample_diffs}}` (M32, HSI §1) |
| `POST/GET` | `/api/v1/meta-specs-registry` | Create / list DB-backed meta-spec registry entries — `{id, name, kind, path, content, version, status: draft\|approved\|deprecated}`; separate from `specs/manifest.yaml`-backed spec ledger (agent-runtime spec) |
| `GET/PUT/DELETE` | `/api/v1/meta-specs-registry/{id}` | Read / update / delete a meta-spec registry entry (**Admin only** for PUT/DELETE) |
| `GET` | `/api/v1/meta-specs-registry/{id}/versions` | List all versions of a meta-spec registry entry |
| `GET` | `/api/v1/meta-specs-registry/{id}/versions/{version}` | Get a specific version snapshot |
| `POST/GET` | `/api/v1/workspaces/{id}/llm/config` | **Admin only** — Create / list per-workspace LLM function overrides; each entry: `{function_name, model, temperature?, max_tokens?, provider: anthropic\|vertex}` (LLM integration) |
| `GET/PUT/DELETE` | `/api/v1/workspaces/{id}/llm/config/{function}` | Get effective config / set override / remove override for a specific LLM function in this workspace (LLM integration) |
| `POST/GET` | `/api/v1/workspaces/{id}/llm/prompts` | **Admin only** — List / manage per-workspace prompt template overrides for LLM functions (LLM integration) |
| `GET/PUT/DELETE` | `/api/v1/workspaces/{id}/llm/prompts/{function}` | Get effective prompt / set override / remove override for a specific LLM function (LLM integration) |
| `GET/PUT` | `/api/v1/admin/llm/config/{function}` | **Admin only** — Get / set tenant-wide default LLM config for a function (applied when no workspace override exists) (LLM integration) |
| `GET/PUT` | `/api/v1/admin/llm/prompts/{function}` | **Admin only** — Get / set tenant-wide default prompt template for a function (LLM integration) |
| `GET` | `/api/v1/federation/trusted-issuers` | List configured trusted remote Gyre instances (base URLs from `GYRE_TRUSTED_ISSUERS`); returns `[]` when federation is disabled (G11) |
| `POST` | `/api/v1/auth/api-keys` | Create API key (Admin role required; returns `gyre_<uuid>` key — stored as SHA-256 hash, visible only once on creation; rotate by creating a new key) |
| `GET` | `/metrics` | Prometheus metrics (request count, duration, active agents, merge queue depth) |
| `GET` | `/api/v1/admin/health` | Admin: server uptime + agent/task/project counts (Admin only) |
| `GET` | `/api/v1/admin/jobs` | Admin: background job status — merge processor, stale agent detector, `spawn_budget_daily_reset` (resets `tokens_used_today`/`cost_today` at midnight UTC), `stale_peer_detector` (marks WireGuard peers inactive after `GYRE_WG_PEER_TTL` s, runs every 60 s) (Admin only) |
| `GET` | `/api/v1/admin/audit` | Admin: searchable activity log (`?agent_id=&event_type=&since=`) (Admin only) |
| `POST` | `/api/v1/admin/agents/{id}/kill` | Admin: force agent to Dead, terminate real OS process via process registry, clean worktrees, block assigned task (Admin only) (M11.1) |
| `POST` | `/api/v1/admin/agents/{id}/reassign` | Admin: reassign agent's current task to another agent (Admin only) |
| `GET` | `/*` | Svelte SPA dashboard (served from `web/dist/`) |
| `POST` | `/mcp` | MCP JSON-RPC 2.0 handler (`initialize`, `tools/list`, tool calls) — requires authentication |
| `GET` | `/mcp/sse` | MCP SSE stream — typed AG-UI activity events — requires authentication |
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

---

## Authentication

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

---

## WebSocket Protocol (`gyre-common::WsMessage`)

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

// 5. Domain event push (server -> client, M10.2) -- emitted automatically on mutations:
{"type":"DomainEvent","event":"AgentCreated","id":"<uuid>"}
{"type":"DomainEvent","event":"AgentStatusChanged","id":"<uuid>","status":"Active"}
{"type":"DomainEvent","event":"AgentContainerSpawned","id":"<agent-uuid>","container_id":"<docker-container-id>","image":"<image-ref>","image_hash":"<sha256-digest>"}
{"type":"DomainEvent","event":"TaskCreated","id":"<uuid>"}
{"type":"DomainEvent","event":"TaskTransitioned","id":"<uuid>","status":"in_progress"}
{"type":"DomainEvent","event":"MrCreated","id":"<uuid>"}
{"type":"DomainEvent","event":"MrStatusChanged","id":"<uuid>","status":"merged"}
{"type":"DomainEvent","event":"QueueUpdated"}
{"type":"DomainEvent","event":"PushRejected","repo_id":"<uuid>","branch":"<ref>","reason":"<gate-name>"}
{"type":"DomainEvent","event":"SpecChanged","repo_id":"<uuid>","spec_path":"specs/system/foo.md","change_kind":"added","task_id":"<uuid>"}
{"type":"DomainEvent","event":"GateFailure","mr_id":"<uuid>","gate_name":"<name>","gate_type":"agent_review","status":"failed","output":"<gate output>","spec_ref":"specs/system/agent-gates.md@<sha>","gate_agent_id":"<uuid>"}
{"type":"DomainEvent","event":"StaleSpecWarning","repo_id":"<uuid>","mr_id":"<uuid>","spec_path":"<relative-spec-path>","current_sha":"<40-char-hex>","mr_sha":"<40-char-hex>"}
```

The in-memory `ActivityStore` holds up to 1000 events (oldest dropped when full).
The same events are also queryable via `GET /api/v1/activity?since=<ts>&limit=<n>`.

### AG-UI Event Taxonomy

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

### Audit Event Taxonomy

`event_type` in audit events is a typed `AuditEventType` enum. Accepted values (snake_case):

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

---

## MCP Server (M5.1)

Gyre exposes an MCP (Model Context Protocol) server at `/mcp`. Agents can discover and call Gyre capabilities as MCP tools.

**Endpoints:**
- `POST /mcp` — JSON-RPC 2.0. Methods: `initialize`, `tools/list`, `tools/call`. Requires authentication.
- `GET /mcp/sse` — SSE stream of typed AG-UI activity events. Requires authentication.

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
| `gyre_message_send` | Send a Directed or Custom message to an agent/workspace. Params: `to` (destination), `kind`, `payload`, `tier`. Derives workspace from agent JWT. |
| `gyre_message_poll` | Poll own inbox for Directed messages. Params: `after_ts`, `after_id`, `limit`, `unacked_only`. Derives agent_id from JWT. |
| `gyre_message_ack` | Acknowledge a received message. Params: `message_id`. Derives agent_id from JWT. |
| `graph_concept` | Search knowledge graph by concept name. Params: `concept` (required), `repo_id` or `workspace_id` (one required), `depth` (optional, default 2). Returns matching nodes and edges. (HSI §11) |
| `spec_assist` | LLM-assisted spec editing. Params: `repo_id`, `spec_path`, `instruction` (all required), `draft_content` (optional). Returns validated `{diff: [{op, path, content}], explanation}` JSON; diff ops: `add`/`remove`/`replace`. Rate limited: 10 req/60s per user/workspace. (HSI §11) |

**Available resources** (from `resources/list`):

| Resource | URI Template | Description |
|---|---|---|
| `spec://` | `spec://{path}` | Read spec markdown files |
| `agents://` | `agents://{workspace_id}` | List active agents in a workspace |
| `queue://` | `queue://{repo_id}` | Merge queue entries for a repository |
| `conversation://context` | — | Interrogation agent conversation history (HSI §4) |
| `briefing://` | `briefing://{workspace_id}` | Workspace briefing narrative: completed MRs, in-progress tasks, metrics (HSI §9). Optional `?since=<epoch>` query param. |
| `notifications://` | `notifications://{workspace_id}` | Inbox notifications for authenticated user (HSI §11). Optional `?min_priority=&max_priority=` query params. |
| `trace://` | `trace://{mr_id}` | SDLC system trace for a merge request: spans, root_spans (HSI §3a). |

Example MCP `initialize` call:
```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{"name":"my-agent","version":"1.0"}}}
```

---

## A2A Protocol (M5.2)

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
