# Gyre Agent Protocol

This document covers the full agent lifecycle: spawn, operation, and completion.

---

## Agent Spawn / Complete API (M3.2)

`POST /api/v1/agents/spawn` -- single-call agent provisioning:

```json
// Request
{
  "name": "worker-1",               // required; must match [a-zA-Z0-9._-]{1,63} -- shell metacharacters rejected with 400 (M19.5-A)
  "repo_id": "<repo-uuid>",
  "task_id": "<task-uuid>",
  "branch": "feat/my-feature",
  "parent_id": "<orchestrator-agent-uuid>",    // optional
  "compute_target_id": "<target-uuid>",        // optional -- remote compute target
  "persona": "security",                       // optional -- bare persona name; recorded as the agent's persona (spec-registry.md §16) and injected as GYRE_PERSONA in the container env. Must resolve to an Approved persona in the repo/workspace/tenant chain (same fail-closed posture as persona_slug), else the spawn is rejected with 400
  "disconnected_behavior": "Pause"             // optional -- "Pause" (default) | "ContinueOffline" | "Abort"; controls stale-agent-detector behavior when agent loses connectivity
}

// Response 201
{
  "agent": {
    "id": "...", "name": "worker-1", "status": "Active",
    "spawned_by": "<caller-agent-id or user-id>",   // M13.2: who initiated spawn
    ...
  },
  "token": "<signed-EdDSA-JWT>",   // M18: starts with "ey", 3 dot-separated parts; claims: sub=agent_id, task_id, spawned_by, exp. G10: when spawned on a real process, also embeds wl_pid, wl_hostname, wl_compute_target, wl_stack_hash. M19.4: container-spawned agents additionally embed wl_container_id, wl_image_hash. Verify via /.well-known/jwks.json. Legacy UUID tokens still accepted from POST /api/v1/agents.
  "worktree_path": "/path/to/worktree",   // the agent's actual working directory — the jj workspace ({repo}/workspaces/{agent_id}) when jj provisioning succeeded, else the plain-git worktree ({repo}/worktrees/{branch}); equals the path listed by GET /repos/:id/worktrees
  "clone_url": "http://localhost:3000/git/project/repo.git",
  "branch": "feat/my-feature",
  "jj_change_id": "<jj-change-id-or-null>",   // present when jj is initialized in worktree (best-effort)
  "container_id": "<docker-container-id-or-null>",   // present when agent was launched in a container via GYRE_DEFAULT_COMPUTE_TARGET=container or a container compute_target_id (M19.1)
  "meta_spec_set_sha": "<sha256-or-null>"   // SHA256 of the workspace's bound meta-spec set at spawn time; null when workspace has no meta-spec-set configured; used for commit provenance (M32)
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

**Idempotent (M13.7):** Calling complete a second time returns **202 Accepted** rather than an error -- safe to retry on network failure or agent restart.

**Token revocation (M13.7):** The agent's bearer token is revoked in the database on successful completion. Any subsequent API call with the same token will be rejected with 401. Agents must not reuse a token after completing.

**Commit provenance (M13.2):** When an agent pushes via Smart HTTP, the server automatically captures:
- `spawned_by` -- the identity of whoever called `POST /api/v1/agents/spawn` for this agent
- `X-Gyre-Model-Context` -- optional request header agents may set on the `git-receive-pack` call to record model/context metadata (e.g. `claude-opus-4@anthropic`)

These fields appear on `AgentCommit` records returned by `GET /api/v1/repos/{id}/agent-commits`.

**Spec binding on MR create (M12.3):** `POST /api/v1/merge-requests` accepts an optional `spec_ref` field in the request body -- a string of the form `"specs/system/agent-gates.md@<40-char-sha>"` -- to cryptographically bind the MR to the spec version it implements. A `GateFailure` domain event is broadcast if an `AgentReview` or `AgentValidation` gate fails.

---

## Container Agent Environment Variables (M24)

When spawning an agent into a container compute target, the server pre-mints the JWT (so it can be passed in at container start) and injects the following environment variables into the container:

| Variable | Value | Purpose |
|---|---|---|
| `GYRE_SERVER_URL` | Server base URL | API endpoint for heartbeat, complete, logs |
| `GYRE_AUTH_TOKEN` | Pre-minted EdDSA JWT | Bearer token for all API calls -- revoked on `complete` |
| `GYRE_CLONE_URL` | Git Smart HTTP URL | Clone URL for the assigned repo |
| `GYRE_BRANCH` | Branch name | Branch to clone and work on |
| `GYRE_AGENT_ID` | Agent UUID | Identity for API calls |
| `GYRE_TASK_ID` | Task UUID | Assigned task reference |
| `GYRE_REPO_ID` | Repository UUID | Repo being worked on |
| `GYRE_META_SPEC_PROMPT` | _(when meta-specs apply)_ | Rendered meta-spec system prompt (agent-runtime.md §5): required tenant + workspace meta-specs and spec-level bindings at pinned versions |
| `GYRE_META_SPEC_SET_SHA` | _(when meta-specs apply)_ | SHA-256 of the assembled prompt set (provenance; matches the spawn response's `meta_spec_set_sha`) |
| `GYRE_PROTOCOL_PROMPT` | Protocol section text | System-managed protocol (agent-runtime.md §5): Ralph loop rules, MCP tools, heartbeat, escalation, completion — also served on demand via the MCP `system://protocol` prompt |
| `GYRE_ITERATION` | `0`, `1`, ... | Ralph-loop iteration for this session (agent-runtime.md §1 Phase 7) |
| `GYRE_PROMPT_PERSONA` | _(optional)_ | Persona system prompt | Persona prompt (Platform Model §4.3): the resolved persona's prompt, injected only when a persona is bound to the agent (spawn request `persona_slug`, resolved repo → workspace → tenant, must be approved). Note: the MCP `system://persona` prompt serves the assembled meta-spec set per agent-runtime.md §5 |
| `GYRE_PROMPT_PROTOCOL` | Protocol norms text | `system://protocol` (Platform Model §4.3): interaction norms, Ralph loop definition, escalation rules |
| `GYRE_PROMPT_CONTEXT` | Task/worktree context text | `system://context` (Platform Model §4.3): current task (title, spec ref, description), worktree branch/path, repo info |
| `GYRE_PROMPT_CONSTRAINTS` | Constraints text | `system://constraints` (Platform Model §4.3): budget limits + remaining budget, agent lifetime cap, scope boundaries |
| `GYRE_AGENT_COMMAND` | _(optional)_ | Command for the entrypoint to exec after setup (e.g. a CI script) |
| `GYRE_CRED_{NAME}` | _(per secret)_ | Resolved secrets from the secrets repository (Platform Model §7): tenant/workspace/repo/task scoped, nearest scope wins on name collision, expired secrets excluded. Held in memory by the `cred-proxy` sidecar and scrubbed before the agent process starts -- raw values never appear in the agent process env |
| `GYRE_CRED_PROXY` | `http://127.0.0.1:8765` | Address of the `cred-proxy` sidecar (M27); injected so entrypoint and agent-runner can reference the proxy address |
| `ANTHROPIC_BASE_URL` | `http://127.0.0.1:8765` | Routes Anthropic SDK calls through cred-proxy; raw API key never exposed to agent process (M27) |
| `ANTHROPIC_API_KEY` | `proxy-managed` | Placeholder so Anthropic SDK initialises; cred-proxy injects the real `x-api-key` header per request (M27) |

The `docker/gyre-agent/` directory contains a reference `Dockerfile` (Node 22 Alpine + git + curl) and `entrypoint.sh` that validates these vars, configures git credentials via a credential helper (token not embedded in the clone URL), clones the branch, sends an initial heartbeat, then `exec`s `GYRE_AGENT_COMMAND` or -- if unset -- `node /gyre/agent-runner.mjs` for fully autonomous operation. `agent-runner.mjs` connects to the Gyre MCP server, reads the assigned task, implements it, commits, pushes, and calls `gyre_agent_complete`.

**WireGuard mesh (M26):** `setup-wg.sh` in the same directory handles agent-side mesh setup -- generates a Curve25519 keypair, registers the pubkey via `POST /api/v1/network/peers`, fetches the peer list, brings up the `wg0` interface with the allocated `mesh_ip`, and adds routes. Run it after clone and before `agent-runner.mjs` when `GYRE_WG_ENABLED=true`. The peer response includes `mesh_ip: Option<String>` (allocated from `GYRE_WG_CIDR` pool) and `is_stale: bool`.

Build and register:
```bash
docker build -t gyre-agent:latest docker/gyre-agent/

# M25 zero-config: start server with API key -- gyre-agent-default is auto-registered
GYRE_SECRET_ENCRYPTION_KEY=<64-hex-chars> cargo run -p gyre-server  # secrets encrypted at rest

# Or manually create a container compute target (Admin -> Compute -> Add) with type=container
# Agent containers need bridge networking to reach server:
# Set config: {"image": "gyre-agent:latest", "network": "bridge"}
```

---

## Custom Git Ref Namespaces (M13.6)

The server writes refs into reserved namespaces on each lifecycle event:

| Event | Ref written | Purpose |
|---|---|---|
| `spawn` | `refs/agents/{agent-id}/head` | Points to the HEAD commit at spawn; survives branch force-pushes |
| `spawn` | `refs/tasks/{task-id}` | Marks the branch for this task's implementation |
| `complete` | `refs/agents/{agent-id}/snapshots/{n}` | Immutable snapshot of the branch tip at completion (n increments per call) |
| `merge` | `refs/notes/attestations` | Signed `MergeAttestation` JSON attached as git note to the merge commit SHA; inspect via `git notes --ref=refs/notes/attestations show <sha>` (G5) |

These refs survive agent restarts. Query them via standard git: `git ls-remote <clone-url> 'refs/agents/*'`.

---

## Gate Agent Protocol (M12.1)

When the merge queue executes an `AgentReview` or `AgentValidation` gate, it spawns the configured command as a subprocess with these environment variables:

| Variable | Value |
|---|---|
| `GYRE_SERVER_URL` | Server base URL |
| `GYRE_REVIEW_TOKEN` / `GYRE_VALIDATION_TOKEN` | Scoped per-run Bearer token -- revoked on process exit |
| `GYRE_MR_ID` | UUID of the MR being reviewed |
| `GYRE_GATE_ID` | UUID of the gate triggering this run |
| `GYRE_GATE_AGENT_ID` | Identity to use when submitting reviews via API |
| `GYRE_DIFF_URL` | URL to fetch the MR diff |
| `GYRE_SPEC_REF` | Spec reference bound to the MR (if any) |
| `GYRE_SPEC_URL` | URL to fetch the spec content at the MR's **pinned** spec_ref SHA (not HEAD) — `GET /api/v1/merge-requests/{id}/spec`. Empty when the MR has no spec_ref. Spec-alignment review (agent-gates.md §Integration with Agent Gates) must compare the diff against this pinned version |
| `GYRE_PERSONA` | Persona file path for the gate |
| `GYRE_VALIDATION_TYPE` | AgentValidation only: domain-specific check identifier from the gate config (empty string when unset) |

**`AgentReview` protocol:** exit with any code; server checks for an Approved/ChangesRequested review submitted by `GYRE_GATE_AGENT_ID` after the process exits.

**`AgentValidation` protocol:** exit 0 = pass, non-zero = fail.

**Security:** each execution gets a unique `gyre_gate_<uuid>` token (revoked on completion even on crash/timeout); command split on whitespace -- no `sh -c` shell wrapper; 5-minute default timeout prevents hung gate agents. Review tokens carry OIDC scope `review:submit` (read MR context, submit verdict, read-only -- push denied); validation tokens carry `validation:report` (read MR context only -- no review submission, push denied). Scope checks are exact-match on the capability string.

**Gate agent lifecycle (task-135, agent-gates.md §Gate Agent Lifecycle):** gate agents are ephemeral (one run, then token teardown), read-only, and identity-scoped as above; their reviews are **attributed** — a review submitted with a scoped reviewer token records `reviewer_persona` and `gate_id`, derived server-side from the token's gate, never from the request body. Their **context windows are captured for audit**: before the subprocess runs, the exact bundle the agent will see (persona system prompt, MR diff, spec content at the pinned `spec_ref` SHA, MR title, task description/acceptance criteria) is zstd-compressed and stored in the conversation repository, retrievable for replay at `GET /api/v1/conversations/{sha}`; a `gate_agent_context` audit event records the conversation SHA, gate, persona, and MR. Capture is skipped (logged) when the workspace's tenant scope cannot be resolved — never stored under a fabricated tenant.

**Gate chains (task-135, agent-gates.md §Gate Chains):** a repo's gates form an ordered chain (`position` column; `GET /repos/{id}/gates` and the adapters return chain order). `PUT /repos/{id}/gate-chain` replaces the whole chain from TOML (`[[gates]]` entries with `name`, `type`, `command`, `persona`, `min_approvals`, `required`, ...); same-named gates keep their ids, absent gates are deleted, and invalid chains are rejected before any mutation. Gates with `required = false` are advisory: results are recorded and failures attach an advisory comment on the MR (attributed to `gate-agent:{gate_id}`) but never block merging.

**Manifest-declared gates (spec-registry.md §16, task-195):** when an MR's `spec_ref` names a spec whose `specs/manifest.yaml` entry declares `gates:` (e.g. `[security, accountability]`), the forge spawns a gate agent per persona at enqueue time instead of running a subprocess — no command is involved. For each manifest gate:

1. The server creates a real `Agent` record (`gate-<persona>-<mr-id>`, workspace-scoped, `spawned_by: "server"`), records the persona in the `agent_personas` namespace, and registers a scoped EdDSA JWT in `agent_tokens` carrying OIDC scope `review:submit` (revoked when the gate resolves). Because the token is review:submit-scoped, `submit_review` binds the recorded reviewer to the verified token subject — the persona / `min_attestation_level` / `stack_hash` constraints are evaluated against the identity the server minted, never a request-body reviewer id.
2. The gate agent's runtime process is launched through the platform's compute-target machinery — the same execution path `POST /api/v1/agents/spawn` uses (`api::spawn::launch_agent_workload`: workspace-assigned target → tenant default → local; command from the target config, `GYRE_AGENT_COMMAND`, or `/gyre/entrypoint.sh`). The launched runtime receives its scoped token via `GYRE_AUTH_TOKEN` (the credential delivery channel every spawned agent uses) plus its review context (`GYRE_MR_ID`, `GYRE_GATE_ID`, `GYRE_PERSONA`, `GYRE_REPO_ID`, `GYRE_DIFF_URL`, `GYRE_SPEC_REF`, `GYRE_MIN_ATTESTATION_LEVEL`, `GYRE_STACK_HASH`), and a workload attestation is recorded for the agent at launch — so its own reviews derive at attestation level ≥ 1 and can satisfy the manifest's constraints (higher levels follow the normal mechanisms: stack attestation for level 2, container id + image for level 3).
3. A `ReviewRequest` message is dispatched to the agent carrying `{mr_id, repo_id, spec_path, spec_ref, persona, min_attestation_level, stack_hash}` — the gate agent runtime receives its review context directly in its launch env (`GYRE_MR_ID`, `GYRE_PERSONA`, `GYRE_SPEC_REF`, …) and can poll the message via the MCP surface (`gyre_message_poll`, which derives the inbox from the verified token subject — inherently own-inbox), fetch the MR diff, review, and submit its verdict via the standard review API (`POST /api/v1/merge-requests/{id}/reviews`, `reviewer_agent_id` = its own agent id — enforced by the reviewer identity binding, which applies to every agent-token principal, not just scoped gate agents). The REST inbox route (`GET /api/v1/agents/{id}/messages`) is kept off the reviewer allow-list as least privilege / defense in depth: the handler actually routed there (`messages::poll_messages`) does enforce own-inbox for non-admin callers, but a single-purpose review token has no need for it — its ReviewRequest delivery is the launch env plus `/mcp`. Reviews are attributed per reviewer; each reviewer's **latest** verdict is the one that counts.
4. The gate passes only on an `Approved` review from an agent running the gate persona whose workload attestation satisfies `min_attestation_level` (and an exact `stack_hash` match when pinned); a final `ChangesRequested` from the persona fails it; otherwise it waits up to the gate timeout, then fails closed (blocking the merge).
5. The gate agent is torn down after its gate resolves — its runtime process/container/Pod is killed through the compute backend that owns its handle (`spawned_backends` routing, the same invariant the admin kill path enforces: container ids go to `docker rm --force`, Pods to `kubectl delete pod`, remote pids to the remote host — never a local-only pid kill), the `spawned_backends` entry is removed, its token revoked with an `AgentTokenRevoked` audit event, and its status set to `Stopped` — ephemeral and single-purpose (agent-gates.md §Gate Agent Lifecycle). Re-enqueueing the MR reuses the still-running gate agent rather than double-spawning.

A repo-level gate and a manifest gate targeting the same persona do not double-run: the manifest gate wins (it carries the manifest's attestation constraints) and the repo-level duplicate for that persona is skipped.

**Merge processor dep failure handling (P5):** before processing each queued entry, the merge processor checks dependency health: if a dependency MR is `Closed`, the queue entry is marked `Failed` and a High-priority task `"Dependency MR-{id} was closed, reassess MR-{dependent}"` is auto-created; if a dependency has 3+ gate failures, an escalation warning is logged.

---

## Agent Compose Spec (M5.2)

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

---

## Spec Lifecycle Automation (M13.8)

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

**Auto-revocation of spec approvals:** When a watched spec file is **modified**, **deleted**, or **renamed** in a push to the default branch, all active approvals for that path are automatically revoked. `revoked_by` is set to `"system:spec-lifecycle"` and `revocation_reason` records the push branch. For renames, approvals on the old path are revoked; the new path starts with no approvals.

---

## Analytics Decisions in the Ralph Loop (analytics.md §Integration with the Ralph Loop)

Analytics close the feedback loop for autonomous development. The loop, end to end:

1. Workspace orchestrator creates delegation tasks
2. Implementation agent ships code, opens MR
3. MR merges → `mr.merged` event recorded
4. Analytics tracks adoption over N days
5. Repo orchestrator evaluates `feature.promote` (equivalent to `GET /api/v1/analytics/decide?decision=feature.promote`)
6. Agent either promotes the flag or rolls back

Steps 5–6 are agent decisions, not server-side automation: the orchestrator calls the
`gyre_analytics_decide` MCP tool in its decision-making loop and acts on the returned
`DecisionResult`. This is the primary way agent code invokes analytics decisions — no raw
HTTP calls required.

**Promote/rollback decision** (built-in decision over the flag's adoption data):

```json
{
  "tool": "gyre_analytics_decide",
  "arguments": {
    "decision": "feature.promote",
    "context": {
      "flag": "new-diff-viewer",
      "baseline_days": 7
    }
  }
}
```

The response carries the `DecisionResult` shape (`decision`, `reason`, `outcome` with
`promote`/`confidence`/`metrics`, `evaluated_at`). A repo orchestrator that receives
`promote: true` promotes the flag; on `feature.rollback` triggers (a gate-failure spike)
it rolls the flag back instead.

**Custom decision** (rule DSL — `custom_rules` present switches to the custom evaluator;
first matching rule wins, `default` applies otherwise):

```json
{
  "tool": "gyre_analytics_decide",
  "arguments": {
    "decision": "spec-approval-pace",
    "context": {},
    "custom_rules": [
      {"metric": "spec.approved", "window": "7d", "threshold": 3, "comparison": "gte", "then": "approve"},
      {"metric": "merge_queue.processed", "filter": {"outcome": "failed"}, "window": "24h", "threshold": 5, "comparison": "gte", "then": "escalate"}
    ],
    "default": "hold"
  }
}
```

Other built-in decisions for loop control: `feature.rollback`, `gate.effectiveness`,
`agent.performance`, `budget.headroom`, `merge_queue.health`. The `context` may scope the
decision with `workspace_id`/`repo_id` (validated against the caller's tenant and token
scope). See `specs/system/analytics.md` §Decision API for Agents for the full decision
catalog and response shapes.
