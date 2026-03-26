# Agent Runtime

> This spec defines the complete agent lifecycle — from spec approval through task completion — including compute target selection, prompt assembly from meta-specs, the Ralph loop, and budget enforcement. It consolidates and supersedes scattered runtime definitions across `ralph-loop.md`, `platform-model.md` §4, and `agent-protocol.md`.

## The Model

Agents are work executors. Humans never spawn agents directly — the system spawns them in response to approved specs. Every agent runs with a frozen set of meta-spec prompts, operates on a single task, and iterates through the Ralph loop until gates pass or max iterations are reached.

```
Human writes spec (LLM assistant available)
  → Human approves spec (always required, per vision.md §3)
    → Approved spec SHA triggers inbox message to orchestrator agent
      → Orchestrator decomposes spec into tasks
        → For each task: worker agent spawned
          → Agent implements, commits, pushes, signals "done"
            → "Done" signal triggers MR creation
              → Gates run automatically (including spec-vs-code agent review)
                → Pass: MR merges, task marked completed
                → Fail: agent re-spawned (clean context + failure messages), loops
```

---

## 1. Agent Lifecycle

### Phase 1: Spec Triggers Orchestration

When a spec is approved (new SHA recorded in the spec approval ledger), the server creates a `SpecChanged` message on the message bus. The workspace orchestrator agent — which is always running or spawned on demand — receives this message in its inbox.

The orchestrator reads the spec content, decomposes it into tasks, and creates each task via the `task.create` MCP tool. The orchestrator is itself an agent with a special built-in persona (`workspace-orchestrator`). It does not implement code — it decomposes and delegates.

**Orchestrator spawn:** If no orchestrator is active for the workspace, the server spawns one automatically when a `SpecChanged` message has no active consumer. The orchestrator uses the workspace's configured compute target.

### Phase 2: Agent Spawn

For each task, the orchestrator calls `agent.spawn` (via MCP). The server:

1. Creates the agent record (`Active` status)
2. Mints an EdDSA JWT scoped to the agent's repo
3. Assembles the prompt set (§2)
4. Creates a git worktree on the task's branch
5. Provisions the agent on the workspace's compute target (§3)
6. Injects environment variables (server URL, token, branch, task ID, repo ID)

The agent process starts, clones the repo, reads its task via MCP, and begins implementation.

### Phase 3: Implementation

The agent works autonomously:
- Reads task details and spec refs via MCP
- Edits files, runs tests, makes commits
- Pushes to its branch
- Calls `gyre_agent_complete` when done

The agent has no knowledge of other agents, the merge queue, or the broader system state. It sees its task, its spec, and its repo.

### Phase 4: MR and Gates

When the agent signals completion:
1. Server creates a MR from the agent's branch to the default branch
2. Gates run automatically (configured per repo in `agent-gates.md`):
   - **Test gates:** run test commands (exit 0 = pass)
   - **Lint gates:** run linters
   - **Spec review gate:** spawns a reviewer agent with reviewer persona to evaluate MR diff against the spec at its pinned SHA
   - **TraceCapture gate:** captures OTel spans during test execution (observational, always passes)
3. All gate results are recorded on the MR

### Phase 5: Ralph Loop

**If all gates pass:** MR merges. Task marked `Completed`. Agent marked `Idle`. JWT revoked.

**If any gate fails:**
1. Task remains `InProgress`
2. Gate failure details are written as messages to the agent's inbox
3. The server re-spawns the agent:
   - **Same persona, same meta-spec set, same task assignment**
   - **NO conversation history** — fresh context window (eliminates context rot)
   - **Gate failure messages in inbox** — the agent reads these on startup and addresses them
4. The agent re-implements, commits, pushes, signals done
5. Gates run again
6. Loop continues until convergence or `max_iterations` reached

**Max iterations reached:** Task marked `Blocked`. Orchestrator notified. Orchestrator may re-decompose the task, adjust the spec, or escalate to a human via a priority-1 notification ("Agent needs clarification").

**Key property:** Each iteration is a fresh session. The agent has no memory of previous attempts — only the gate failure messages and the current state of the code on its branch. This is deliberate: fresh context prevents the agent from repeating the same mistakes and forces it to reason from first principles each time.

### Phase 6: System-Initiated Agents

Some agents are spawned by the system, not by an orchestrator:

| Agent Type | Trigger | Purpose | Lifecycle |
|---|---|---|---|
| **Gate reviewer** | MR enters gate queue | Review MR diff against spec | Single-shot: submit verdict, torn down |
| **Interrogation** | Human clicks "Ask Why" | Explore agent reasoning post-hoc | Single-shot: conversation ends, torn down (30-min JWT TTL) |
| **Reconciliation** | Meta-spec change published | Re-implement affected specs | Ralph loop (same as worker agents) |

System-initiated agents use the same compute target as their workspace. They receive the same meta-spec prompt assembly (§2) — the distinction is in their persona and scope, not their runtime.

---

## 2. Meta-Spec Prompt Assembly

### Meta-Specs Are Prompts

Meta-specs — personas, principles, standards, process norms — are the instructions passed to agents. They are "specs for agent behavior." When you edit a persona, you are editing what the agent receives in its system prompt.

There is no separate "prompt configuration." The meta-spec registry IS the prompt configuration.

### Registry Levels

Meta-specs exist at two levels in the registry, plus spec-level binding:

```
Tenant registry (org-wide conventions)
  └── Workspace registry (team conventions)
        └── Spec-level bindings (per-spec selections from either registry)
```

Each registry entry:
```rust
pub struct MetaSpec {
    pub id: Id,
    pub kind: MetaSpecKind,           // Persona, Principle, Standard, Process
    pub name: String,
    pub content: String,              // the actual prompt text
    pub version: u32,                 // auto-incremented on each edit
    pub content_sha: String,          // SHA-256 of content (content-addressable)
    pub scope: MetaSpecScope,         // Tenant | Workspace
    pub scope_id: Id,                 // tenant_id or workspace_id
    pub required: bool,               // if true, always injected — cannot be opted out
    pub created_by: Id,
    pub created_at: u64,
    pub updated_at: u64,
}
```

**DB-backed, not repo-backed.** Meta-specs span repos and workspaces. They use content-addressable SHA-256 hashes (computed from `content`) for versioning, not git SHAs. All edits are stored in the database. Full version history is retained in a `meta_spec_versions` table.

### Required vs Optional

- **Required** meta-specs (tenant or workspace level): always injected into every agent spawned in that scope. Admins set the `required` flag. Spec authors cannot opt out.
- **Optional** meta-specs: available in the registry for spec authors to explicitly select via spec-level bindings. Not auto-applied.

### Spec-Level Binding

Specs reference meta-specs they want applied during implementation:

```json
{
  "meta_spec_bindings": [
    {"meta_spec_id": "uuid-1", "pinned_version": 3},
    {"meta_spec_id": "uuid-2", "pinned_version": 1}
  ]
}
```

Stored in the spec registry alongside other spec metadata. The spec author selects these via the UI (Specs view → Edit → Meta-spec bindings panel). The UI shows available meta-specs from the tenant and workspace registries, with required ones pre-selected and locked (cannot be removed).

### Injection Order

When an agent is spawned for a task bound to a spec, the prompt set is assembled:

```
1. All REQUIRED tenant meta-specs (ordered by kind: persona → principle → standard → process)
2. All REQUIRED workspace meta-specs (same ordering)
3. Spec-level bindings (author's explicit selections, at pinned versions)
```

All are concatenated into the agent's system prompt context. No "concern" matching or deduplication — ordered inclusion.

### Versioning and Attestation

When a meta-spec is edited:
1. New `version` number assigned, new `content_sha` computed from content
2. Old version retained in `meta_spec_versions` table (full history, immutable)
3. Specs that pin the old version are not automatically updated

**Attestation:** The merge attestation bundle records the full set of meta-spec SHAs used:
```json
{
  "spec_ref": "specs/system/auth.md@abc123",
  "meta_specs_used": [
    {"id": "uuid-1", "kind": "persona", "content_sha": "sha256:...", "version": 3, "required": true, "scope": "tenant"},
    {"id": "uuid-2", "kind": "standard", "content_sha": "sha256:...", "version": 1, "required": false, "scope": "workspace"}
  ],
  "agent_id": "...",
  "task_id": "..."
}
```

This is appended to the existing `MergeAttestation` bundle (amending `agent-gates.md`).

### Stale Pin Detection

A background job (or check on spec access) detects when a spec pins an old meta-spec version:

- Meta-spec `uuid-1` is at version 5, but Spec X pins version 3
- System creates a soft notification (Inbox priority 6, "Meta-spec drift alert"):
  - "Spec X uses persona v3, but v5 is available. Review and update pin."
- Human updates the pin → spec metadata changes → spec SHA changes → spec needs re-approval
- Approved → orchestrator receives `SpecChanged` → re-decomposes → agents re-implement with new prompts

The invalidation cascades through the existing spec lifecycle. No new mechanism needed.

### Bootstrap

Gyre ships with default meta-specs seeded at first startup:

| Kind | Name | Scope | Required | Purpose |
|---|---|---|---|---|
| Persona | `default-worker` | Tenant | No | General-purpose implementation agent |
| Persona | `workspace-orchestrator` | Tenant | No | Task decomposition and delegation |
| Persona | `spec-reviewer` | Tenant | No | Spec-vs-code gate review |
| Principle | `conventional-commits` | Tenant | Yes | Commit message conventions |
| Standard | `test-coverage` | Tenant | No | Test writing standards |

These can be edited, cloned, or replaced by the user. The `required` flag can be changed by tenant/workspace admins.

### API

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/meta-specs` | GET | List meta-specs (filtered by `?scope=`, `?scope_id=`, `?kind=`, `?required=`) |
| `POST /api/v1/meta-specs` | POST | Create meta-spec (admin at scope level) |
| `GET /api/v1/meta-specs/:id` | GET | Get meta-spec with current content |
| `PUT /api/v1/meta-specs/:id` | PUT | Update meta-spec (creates new version, old version retained) |
| `DELETE /api/v1/meta-specs/:id` | DELETE | Delete meta-spec (fails if any spec bindings reference it) |
| `GET /api/v1/meta-specs/:id/versions` | GET | List all versions of a meta-spec |
| `GET /api/v1/meta-specs/:id/versions/:version` | GET | Get specific version content |

---

## 3. Compute Target Model

### Abstraction

Compute targets are pluggable backends for agent execution. The system supports multiple backends behind a single trait:

```rust
#[async_trait]
pub trait ComputeTarget: Send + Sync {
    fn name(&self) -> &str;
    fn target_type(&self) -> ComputeTargetType;   // Container, Ssh, Kubernetes
    async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle>;
    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()>;
    async fn is_alive(&self, handle: &ProcessHandle) -> Result<bool>;
}
```

All backends run the same agent image. The image is the unit of consistency — same runtime, same tools, same entrypoint, different orchestration layer.

### Supported Backends

| Backend | Mechanism | Use Case |
|---|---|---|
| **Container** (Docker/Podman) | `docker run` with security defaults | Local development, CI |
| **SSH** | SSH to remote host, `docker run` there | Remote build machines, air-gapped environments |
| **Kubernetes** | Create Pod with agent image | Production scale, multi-tenant |

**Container security defaults** (all backends):
- `--network=none` (opt-in to bridge via compute target config)
- `--memory=2g`, `--pids-limit=512`
- `--user=65534:65534` (non-root)
- Worktree mounted as volume

### Nix-Based Image Build

The agent container image is built with Nix for reproducibility:

```
docker/gyre-agent/
  flake.nix              # Single definition, builds all targets
  flake.lock             # Pinned dependencies
  agent-runner.mjs       # Agent entrypoint (Claude Agent SDK)
  entrypoint.sh          # Bootstrap script (git clone, cred-proxy, heartbeat)
  cred-proxy.mjs         # M27 credential opacity proxy
```

**Build commands:**
- `nix build .#agent-image` — OCI image (default)
- `nix build .#agent-image-docker` — Docker-loadable tarball
- `nix build .#agent-image-tar` — Tarball for air-gapped transfer

The Nix flake pins all dependencies (Node.js version, Claude Agent SDK version, git, curl) for bit-reproducible builds. The image hash is recorded in workload attestation (`wl_image_hash` JWT claim).

One definition, all targets. The `Dockerfile` is replaced by the Nix flake.

### Tenant and Workspace Configuration

**Tenant level:** Defines which compute targets are available.

```rust
pub struct ComputeTargetConfig {
    pub id: Id,
    pub tenant_id: Id,
    pub name: String,
    pub target_type: ComputeTargetType,    // Container, Ssh, Kubernetes
    pub config: serde_json::Value,         // Backend-specific config (host, namespace, etc.)
    pub is_default: bool,                  // Tenant default target
    pub created_at: u64,
}
```

**Workspace level:** Selects ONE compute target from the tenant's list.

The workspace's `compute_target_id` field determines where all agents in that workspace run. If unset, falls back to the tenant's default target. If no tenant default, falls back to local container auto-detection (Docker or Podman).

**UI:** Workspace Admin → Settings → Compute Target dropdown. One selection. No per-agent configuration — the workspace decides, agents inherit.

**API:**
| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/compute-targets` | GET | List available targets for this tenant |
| `POST /api/v1/compute-targets` | POST | Register a new target (tenant admin) |
| `GET /api/v1/compute-targets/:id` | GET | Get target details |
| `PUT /api/v1/compute-targets/:id` | PUT | Update target config |
| `DELETE /api/v1/compute-targets/:id` | DELETE | Remove target (fails if workspaces reference it) |

---

## 4. Budget Enforcement

### Cascade

```
Tenant budget (absolute ceiling)
  └── Workspace budget (cannot exceed tenant)
        └── Per-agent enforcement (charged to workspace)
```

Budget is tracked per workspace. Individual agents don't have separate budgets — their usage charges against the workspace total.

### Enforcement Levels

| Threshold | Action | Mechanism |
|---|---|---|
| **80%** | Warn | `BudgetWarning` notification created (Inbox priority 7). Agents continue running. |
| **100%** | Graceful stop | All active agents in the workspace receive a `BudgetExhausted` message in their inbox. Agents have 60 seconds to commit current work and call `agent_complete`. After 60 seconds, agents are killed via `ComputeTarget::kill_process()`. |
| **Beyond limit** | Hard kill | `ComputeTarget::kill_process()` called immediately. Worktree preserved for recovery. No MR created. Task remains `InProgress` for human review. |

### What's Tracked

| Cost Type | Source | Unit |
|---|---|---|
| `llm_tokens` | Agent LLM calls (via cred-proxy usage reports) | Input + output tokens |
| `llm_cost_usd` | Agent LLM calls (via cred-proxy) | Dollar amount (model-specific pricing) |
| `compute_minutes` | Agent wall-clock time (spawn to complete/kill) | Minutes |
| `llm_query` | User-initiated LLM calls (briefing/ask, explorer/generate, specs/assist) | Per-call |

The credential proxy (`cred-proxy.mjs`) reports token usage back to the server after each LLM call via `POST /api/v1/agents/:id/usage`. The server aggregates usage per workspace and checks against workspace limits on every report.

### Budget Reset

Budgets with time-based limits (e.g., `max_tokens_per_day`, `max_cost_per_day`) reset on a rolling 24-hour window. The reset is handled by a background job that runs every hour and clears expired usage entries.

---

## 5. Agent Prompt Structure

When an agent is spawned, it receives this prompt structure:

```
[System prompt — assembled from meta-specs per §2]

  Required tenant meta-specs:
    - {persona content}
    - {principle content}
    - {standard content}

  Required workspace meta-specs:
    - {additional principles/standards}

  Spec-level bindings:
    - {author-selected meta-specs at pinned versions}

[Task context — injected via MCP prompts at spawn]

  - Task ID, description, acceptance criteria
  - Spec ref (content at pinned SHA)
  - Branch name, repo context
  - Gate failure messages from previous iterations (if re-spawn)
  - Inbox messages (orchestrator directives, budget warnings)

[Protocol — system-managed, not user-editable]

  - Ralph loop rules: implement, commit, push, signal done
  - MCP tool usage (available tools and their purposes)
  - Heartbeat requirement (every 60 seconds)
  - Escalation rules (when to call agent.escalate)
  - Completion signaling (gyre_agent_complete)
```

The **protocol** section is system-managed and not part of the meta-spec registry. It defines how agents interact with Gyre infrastructure. Users don't edit this.

The **meta-spec sections** are user-managed and define what kind of work the agent produces.

The **task context** is assembled at spawn time from the task, spec, and any gate failure messages.

---

## Relationship to Existing Specs

**Supersedes:**
- `ralph-loop.md` — replaced by this spec's §1 and §5 (complete lifecycle including gate-driven re-spawn)
- `docs/agent-protocol.md` — consolidated into this spec

**Amends:**

| Spec | Amendment |
|---|---|
| `platform-model.md` §4 | Agent spawn semantics move to this spec. `platform-model.md` retains domain types (Agent, Task, MR structs) but defers lifecycle to `agent-runtime.md`. |
| `meta-spec-reconciliation.md` | Meta-spec registry model (tenant/workspace levels, `required` flag, DB-backed versioning) defined here. Reconciliation spec defers to this for registry semantics. |
| `agent-gates.md` | Gate failure → Ralph loop re-spawn defined here. `agent-gates.md` retains gate type definitions and execution mechanics. `MergeAttestation` amended to include `meta_specs_used` array. |
| `hierarchy-enforcement.md` §4 | Add compute target CRUD endpoints to route table. Add `meta_spec_versions`, `meta_spec_bindings`, `compute_targets` tables to tenant-filter configuration. |
| `human-system-interface.md` §5 | Attestation bundle schema amended to include `meta_specs_used` array with full content SHAs. |
| `human-system-interface.md` §1 | Meta-specs view at workspace scope uses the meta-spec registry API defined here, not the git-backed spec list. |

**Depends on:**
- `agent-gates.md` — gate types and execution
- `message-bus.md` — agent inbox, orchestrator messages
- `abac-policy-engine.md` — agent token scoping
- `hierarchy-enforcement.md` — tenant/workspace hierarchy
- `vision.md` — humans direct via specs, agents execute
