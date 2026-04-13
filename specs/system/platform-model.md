# Platform Model

> This spec defines Gyre's core ownership hierarchy, persona model, orchestration pattern, agent coordination protocol, resource governance, rollback/recovery, secrets delivery, and bootstrap process. It is the foundational spec that other system specs build on.

## 1. Ownership Hierarchy

### Entities

```
Tenant (enterprise/org boundary)
  ├── Users (with tenant-scoped roles)
  ├── Personas (tenant-global agent definitions)
  ├── Budget (ceiling for all workspaces)
  └── Workspace (governance + coordination boundary)
        ├── Budget (inherited from tenant, overridable downward)
        ├── Team membership
        ├── Workspace Orchestrator (agent)
        ├── Cross-repo policies
        ├── Personas (workspace-scoped, override or extend tenant personas)
        └── Repository (unit of work)
              ├── specs/manifest.yaml
              ├── Code + branches + tags
              ├── Tasks
              ├── Repo Orchestrator (agent)
              ├── Worker Agents
              ├── MRs + Merge Queue + Gates
              ├── Personas (repo-scoped, override or extend workspace personas)
              └── Budget (inherited from workspace)
```

### Tenant

The enterprise boundary. Maps to a Keycloak realm or OIDC issuer.

```rust
pub struct Tenant {
    pub id: Id,
    pub name: String,
    pub slug: String,               // URL-safe identifier
    pub oidc_issuer: Option<String>, // Keycloak realm URL
    pub budget: BudgetConfig,
    pub max_workspaces: Option<u32>,
    pub created_at: u64,
}
```

- Every user belongs to exactly one tenant
- Every workspace belongs to exactly one tenant
- Tenant admins manage users, budgets, and tenant-level personas
- Tenant is the SCIM provisioning boundary

**CLI:** `gyre tenant create`, `gyre tenant list`, `gyre tenant set-budget`

**UI:** Tenant switcher in top nav (for users with multi-tenant access). Tenant admin panel for budget, users, personas.

### Workspace

The governance and coordination boundary. A group of related repos that share budgets, team membership, and cross-repo orchestration.

```rust
pub struct Workspace {
    pub id: Id,
    pub tenant_id: Id,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub budget: BudgetConfig,       // Inherited from tenant, overridable
    pub max_repos: Option<u32>,
    pub max_agents_per_repo: Option<u32>,
    pub trust_level: TrustLevel,    // Supervised, Guided, Autonomous, Custom
    pub llm_model: Option<String>,  // LLM model for workspace queries (default: GYRE_LLM_MODEL env)
    pub created_at: u64,
}

pub enum TrustLevel {
    Supervised,   // Human reviews everything before merge
    Guided,       // Agents merge if gates pass, alert on failures
    Autonomous,   // Only interrupt for exceptions
    Custom,       // Direct ABAC policy manipulation
}
```

- A workspace contains one or more repos
- Cross-repo work is coordinated at the workspace level
- Budget is inherited from tenant; workspace can set lower limits but never higher
- The Workspace Orchestrator agent runs at this level

**Slug uniqueness:** `(tenant_id, slug)` is unique — enforced by DB constraint (see `hierarchy-enforcement.md` §6). The `GET /api/v1/workspaces` endpoint supports `?slug=<slug>` filtering for cross-workspace spec link resolution (see `human-system-interface.md` §6).

**CLI:** `gyre workspace create`, `gyre workspace list`, `gyre workspace add-repo`, `gyre workspace set-budget`

**UI:** Workspace dashboard showing all repos, active agents, budget usage, cross-repo MR dependencies. Workspace settings for budget, team, personas.

### Repository

The unit of work. Where specs, code, agents, tasks, and MRs live. Self-contained.

```rust
pub struct Repository {
    pub id: Id,
    pub workspace_id: Id,
    pub name: String,               // Unique within workspace: (workspace_id, name) constraint
    pub path: String,               // Filesystem path to bare repo
    pub default_branch: String,
    pub budget: BudgetConfig,       // Inherited from workspace
    pub max_agents: Option<u32>,    // Max concurrent agents
    pub is_mirror: bool,
    pub created_at: u64,
}
```

- Specs live in the repo (`specs/manifest.yaml`)
- Tasks are scoped to a repo
- Agents are spawned against a repo
- MRs and merge queue are per-repo
- Gate chains are configured per-repo

**CLI:** `gyre repo create`, `gyre repo list`, `gyre repo set-budget`

**UI:** Repo detail page with branches, commits, MRs, tasks, agents, specs, budget usage.

### Scoping Rules

| Entity | Scoped To | Can Access |
|---|---|---|
| User | Tenant | Any workspace/repo they have membership in |
| Workspace Orchestrator | Workspace | All repos in the workspace (read), cross-repo task/MR creation |
| Repo Orchestrator | Repository | Its repo only. Escalates cross-repo needs to Workspace Orchestrator |
| Worker Agent | Repository | Its repo only. Its assigned worktree. Its task. |
| Persona | Tenant, Workspace, or Repo | Depends on scope (see Persona Model below) |
| Task | Repository | Linked to repo, assigned to agent in that repo |
| MR | Repository | Source/target branches in that repo |
| Spec | Repository | Defined in repo's manifest, approved per-repo policy |
| Budget | Cascades | Tenant -> Workspace -> Repo. Lower levels can't exceed parent. |

### Token Scoping

Agent OIDC tokens encode the scope:

```json
{
  "sub": "agent:worker-42",
  "tenant_id": "tenant-acme",
  "workspace_id": "ws-gyre-platform",
  "repo_id": "repo-gyre-server",
  "task_id": "TASK-007",
  "scope": ["repo:gyre-server:write"],
  "stack_hash": "sha256:...",
  "persona": "security",
  "attestation_level": 3
}
```

A worker agent's token is scoped to its repo. The Workspace Orchestrator's token is scoped to the workspace. The MCP server validates scope on every tool call.

---

## 2. Persona Model

Personas are first-class domain entities stored in the database, not just markdown files. They are as impactful as agents themselves - a persona defines an agent's behavior, judgment, and trustworthiness.

### Persona Entity

```rust
pub struct Persona {
    pub id: Id,
    pub name: String,               // e.g., "security", "repo-orchestrator"
    pub scope: PersonaScope,
    pub scope_id: Option<Id>,       // tenant_id, workspace_id, or repo_id
    pub prompt: String,             // The full persona prompt
    pub version: u32,               // Incremented on each change
    pub content_hash: String,       // SHA-256 of prompt content
    pub owner: Id,                  // User who owns this persona
    pub approval_status: ApprovalStatus,
    pub approved_by: Option<Id>,
    pub approved_at: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
}

pub enum PersonaScope {
    Global,     // Available to all workspaces in the tenant
    Workspace,  // Available to all repos in a workspace
    Repo,       // Available only within a specific repo
}
```

### Scope Resolution

When a gate or agent-compose references a persona by name, the forge resolves it with nearest-scope-wins:

1. Check repo-scoped personas first
2. Check workspace-scoped personas
3. Check tenant-global personas
4. Not found -> error

A repo can override a workspace persona (e.g., repo-specific security review requirements). A workspace can override a tenant persona.

### Persona Lifecycle

- **Creation:** via API or CLI. Persona is created with `approval_status: pending`.
- **Approval:** **human-only, always.** Personas define agent behavior - agents must not approve their own behavioral definitions. This is a hard rule, not configurable.
- **Modification:** creates a new version. Old version is preserved for audit. Approval status resets to `pending`. All agents currently using this persona are **not affected** until they restart (they run with the version they were spawned with).
- **Deletion:** soft-delete only. Preserved for audit. Active agents using the persona continue until teardown.
- **Versioning:** every change increments the version counter. The `content_hash` changes. Any `stack_hash` referencing the old content hash becomes invalid.

### Repo-Defined Personas

A repo can define personas in its codebase (e.g., `specs/personas/domain-reviewer.md`) and register them via the API or as part of bootstrap. The content lives in git (versioned, diffable), but the registration and approval live in the forge's database.

On push, if a persona file changes, the forge:
1. Detects the change (like spec lifecycle)
2. Updates the persona entity with new content + content_hash
3. Resets approval to pending
4. Creates a task: "Persona change requires human approval"
5. Notifies the persona owner

### Built-In Personas

Gyre ships with built-in personas that exist at the tenant level:

| Persona | Purpose | Approval |
|---|---|---|
| `workspace-orchestrator` | Cross-repo coordination, priority management, escalation | Pre-approved (ships with Gyre) |
| `repo-orchestrator` | Ralph loop management, task decomposition, agent dispatch | Pre-approved |
| `accountability` | Spec integrity, drift detection | Pre-approved |
| `security` | Vulnerability scanning, OWASP review | Pre-approved |

Built-in personas can be overridden at workspace or repo scope (with human approval).

### API

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/personas` | GET | List personas (filtered by scope) |
| `POST /api/v1/personas` | POST | Create persona |
| `GET /api/v1/personas/{id}` | GET | Get persona with version history |
| `PUT /api/v1/personas/{id}` | PUT | Update persona (creates new version, resets approval) |
| `POST /api/v1/personas/{id}/approve` | POST | Human-only approval |
| `DELETE /api/v1/personas/{id}` | DELETE | Soft-delete |
| `GET /api/v1/personas/{id}/versions` | GET | Version history |

**CLI:** `gyre persona create`, `gyre persona list`, `gyre persona approve`, `gyre persona show`, `gyre persona edit`

**UI:** Persona management page with prompt editor, version diff viewer, approval workflow, usage report (which agents/gates use this persona).

---

## 3. Two-Level Orchestration

### Workspace Orchestrator

One per workspace. Sees all repos. Handles cross-repo concerns. Uses the `workspace-orchestrator` persona.

**Responsibilities:**
- Observe cross-repo state (blocked repos, dependency chains, budget usage)
- Resolve cross-repo conflicts (two repos competing for same shared library change)
- Route cross-repo work requests (Repo A needs X from Repo B)
- Enforce the spec escalation protocol (see below)
- Allocate budget across repos when contention exists
- Spawn/restart repo orchestrators

**Does NOT:**
- Decompose tasks within a repo (that's the repo orchestrator)
- Review code or specs (that's gate agents)
- Make product decisions (that's the human)

**Token scope:** workspace-level. Can read all repos, create cross-repo tasks and MR dependencies, communicate with all repo orchestrators.

### Repo Orchestrator

One per repo. Manages the Ralph loop for its repo. Uses the `repo-orchestrator` persona.

**Responsibilities:**
- Run the Ralph loop: observe, plan, dispatch, monitor, reconcile
- Decompose specs into tasks
- Spawn worker agents with scoped tokens
- Manage the repo's merge queue
- Escalate cross-repo needs to the Workspace Orchestrator

**Token scope:** repo-level. Can spawn agents, manage tasks/MRs, interact with the forge - all within its repo.

### Cross-Repo Spec Escalation Protocol

When a repo orchestrator needs something from another repo:

```
1. Repo A orchestrator: "I need trait UserPort from Repo B"

2. Repo A checks: does Repo B's spec describe UserPort?

   YES -> Repo A escalates to Workspace Orchestrator:
          "Repo B has UserPort in its spec but hasn't implemented it yet.
           I need it for TASK-042."

          Workspace Orchestrator:
          - Creates cross-repo task in Repo B
          - Creates MR dependency (Repo A's MR depends on Repo B's MR)
          - Notifies Repo B's orchestrator

   NO  -> Repo A escalates to Workspace Orchestrator:
          "Repo B does not have UserPort in its spec. I need a spec change."

          Workspace Orchestrator:
          - Drafts a spec change for Repo B (agent writes it)
          - Escalates to HUMAN for approval (spec change = design decision)
          - Human approves or rejects
          - If approved: spec lifecycle auto-creates task in Repo B
          - Workspace Orchestrator creates the MR dependency
```

**The human gate:** Cross-repo spec changes always require human approval. An agent discovering it needs something from another repo is discovering a design gap - that's a human decision, not an agent decision.

---

## 4. Agent Coordination Protocol (MCP-Based)

All agent-to-server interaction is via MCP tools, resources, and prompts. No direct REST API calls from agents.

### MCP Server

Gyre exposes an MCP server per repo. Agents connect with their scoped OIDC token. The MCP server validates scope on every tool call.

### MCP Tools (Agent-Facing)

| Tool | Scope | Purpose |
|---|---|---|
| `task.list` | repo | List tasks (filtered by status, assignment) |
| `task.create` | repo | Create a task |
| `task.update` | repo | Update task status, assignment |
| `task.take` | repo | Assign task to self |
| `mr.create` | repo | Open a merge request |
| `mr.list` | repo | List MRs |
| `mr.add_dependency` | repo | Declare MR depends on another MR |
| `review.submit` | repo | Submit a review (Approved/ChangesRequested) |
| `review.comment` | repo | Add inline review comment |
| `spec.get` | repo | Read a spec at a specific SHA |
| `spec.list` | repo | List registered specs with approval status |
| `activity.record` | repo | Log an activity event |
| `agent.heartbeat` | repo | Keep-alive signal |
| `agent.complete` | repo | Signal task completion, trigger teardown |
| `agent.escalate` | workspace | Escalate to workspace orchestrator |
| `agent.budget_remaining` | repo | Check remaining budget |
| `git.push` | repo | Push changes (triggers pre-accept validation) |
| `git.status` | repo | Current branch, dirty state |
| `worktree.create` | repo | Create isolated worktree for task |
| `worktree.cleanup` | repo | Remove worktree after completion |
| `message.send` | workspace | Send a Directed or Custom message to an agent |
| `message.poll` | agent | Poll own inbox for new Directed messages |
| `message.ack` | agent | Acknowledge a received message |
| `conversation.upload` | agent | Upload conversation history at completion (base64 zstd blob) |

### MCP Resources (Read-Only Context)

| Resource | Scope | Purpose |
|---|---|---|
| `spec://{path}@{sha}` | repo | Spec content at specific version |
| `manifest://` | repo | Current manifest.yaml |
| `budget://` | repo | Current budget status (used/remaining/limit) |
| `agents://` | repo | List of active agents in this repo |
| `queue://` | repo | Current merge queue state |
| `conversation://context` | agent | Original agent's conversation history (read-only, for interrogation agents — per `human-system-interface.md` §4) |

### MCP Prompts (Injected at Agent Startup)

| Prompt | Purpose |
|---|---|
| `system://persona` | The agent's persona prompt |
| `system://protocol` | Interaction norms, Ralph loop definition, escalation rules |
| `system://context` | Current task, spec refs, acceptance criteria, worktree info |
| `system://constraints` | Budget limits, lifetime, scope boundaries |

These are injected into the agent's system prompt at spawn time. The agent doesn't need to fetch them - they're part of its initial context.

### Token Validation on Every Call

The MCP server extracts the OIDC token from the connection, validates:
- Token is not expired
- `repo_id` claim matches the repo this MCP server serves
- `scope` claim permits the requested tool
- `attestation_level` meets minimum requirements
- Budget has not been exceeded

If any check fails, the tool call returns an error. The agent cannot bypass scope.

---

## 5. Resource Governance

### Budget Model

```rust
pub struct BudgetConfig {
    pub llm_tokens_limit: Option<u64>,      // Total LLM tokens (input + output)
    pub llm_cost_limit_usd: Option<f64>,    // Dollar cost ceiling
    pub max_concurrent_agents: Option<u32>,  // Concurrent agent limit
    pub max_agent_lifetime_secs: Option<u64>, // Per-agent lifetime cap
    pub compute_minutes_limit: Option<u64>,  // Total compute minutes
    pub storage_mb_limit: Option<u64>,       // Repo + worktree storage
}
```

### Cascade Rules

```
Tenant budget (absolute ceiling)
  └── Workspace budget (cannot exceed tenant)
        └── Repo budget (cannot exceed workspace)
              └── Agent budget (cannot exceed repo)
```

- Each level inherits its parent's limits by default
- Each level can set LOWER limits but never HIGHER
- If tenant sets `llm_tokens_limit: 1_000_000`, no workspace can set `1_500_000`
- If a workspace doesn't set a limit, it inherits the tenant's limit
- The forge enforces at every level

### Enforcement Behavior

Configurable per-level:

```rust
pub enum BudgetEnforcementAction {
    Warn,           // Log warning, continue
    GracefulStop,   // Send BudgetExhausted message, grace period, then kill
    HardKill,       // Immediate termination
    QueuePause,     // Pause merge queue until budget is reviewed
}
```

Default behavior:
- **80% of limit:** `Warn` - agent receives a `BudgetWarning` MCP notification
- **100% of limit:** `GracefulStop` - agent receives `BudgetExhausted`, gets 60 seconds to hand off work, then is killed
- Agent's in-progress work is preserved (worktree, branch, partial MR). A task is created: "Agent hit budget limit on TASK-X, needs reassignment or budget increase"

### Budget Tracking

Every MCP tool call that invokes an LLM records token usage:

```rust
pub struct BudgetUsage {
    pub tenant_id: Id,
    pub workspace_id: Id,
    pub repo_id: Option<Id>,       // None for user-initiated LLM queries (briefing/ask, explorer-views/generate)
    pub agent_id: Option<Id>,      // None for user-initiated LLM queries
    pub task_id: Option<Id>,
    pub usage_type: String,        // "agent_run", "llm_query", etc.
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub model: String,
    pub timestamp: u64,
}
```

The forge aggregates usage in real-time. Budget checks happen on every tool call, not just periodically.

### CLI

```bash
gyre budget show                          # Current repo budget usage
gyre budget show --workspace              # Workspace-level usage
gyre budget show --tenant                 # Tenant-level usage
gyre budget set --llm-tokens 500000       # Set repo budget
gyre budget set --workspace --llm-cost 100.00  # Set workspace cost limit
```

### UI

- **Tenant admin panel:** Budget dashboard showing usage across all workspaces. Set/modify tenant budgets. Alert configuration.
- **Workspace dashboard:** Budget usage per repo, bar charts, burn rate projection. Set/modify workspace and repo budgets.
- **Repo detail:** Budget usage per agent, per task. Real-time token counter.
- **Agent detail:** Individual agent budget consumption, projected vs. actual.

---

## 6. Rollback & Recovery

### Post-Merge Validation

After the merge processor merges an MR, a post-merge validation gate runs:

```toml
[[post_merge_gates]]
name = "post-merge-tests"
type = "TestCommand"
command = "cargo test"
required = true
timeout_secs = 300
```

This is separate from pre-merge gates. It runs against the new HEAD of the default branch after the merge commit lands.

### Recovery Protocol

```
MR merged to main
  |
  v
Post-merge gate runs (tests, build, optional smoke test)
  |
  ├── PASS: continue. Merge queue processes next entry.
  |
  └── FAIL:
        1. Merge queue PAUSES (no more merges until main is green)
        2. Forge creates a REVERT commit undoing the merge
        3. Forge pushes revert to main
        4. Post-merge gate re-runs on reverted HEAD
           ├── PASS: main is green again. Merge queue resumes.
           └── FAIL: escalate to human. Something else is wrong.
        5. Original MR is re-opened with status `Reverted` (formal MR status variant — see below)
        6. Author agent receives RevertNotification via MCP
        7. Task created: "MR #{id} reverted: {failure reason}"
        8. The MR's gate results are invalidated (must re-run)
```

**MR Status Enum:**
```rust
pub enum MrStatus {
    Open,       // MR is open, awaiting review/merge
    Merged,     // MR has been merged
    Closed,     // MR was closed without merging
    Reverted,   // MR was merged then reverted (per recovery protocol above)
}
```

**Task Status Enum:**
```rust
pub enum TaskStatus {
    Backlog,     // Created, not yet assigned
    InProgress,  // Assigned to an agent, work underway
    Completed,   // Agent completed the task
    Blocked,     // Waiting on external input (human, dependency)
    Cancelled,   // Spec rejected or task no longer needed (terminal — cannot be re-opened)
}
```

### Agent Behavior During Recovery

- Agents working on worktrees based on main keep working. Their branches will need rebasing after main is fixed, but jj handles this automatically.
- The merge queue is paused, so no new merges land until main is green.
- The Workspace Orchestrator is notified of the pause and can reprioritize work.

### Circuit Breaker

If the same MR is reverted 3 times:
1. MR is removed from the merge queue permanently
2. Task escalated to human: "MR #{id} has failed post-merge validation 3 times"
3. The spec it references may need revisiting

### CLI

```bash
gyre repo status                # Shows if main is green/broken, merge queue paused/running
gyre repo revert MR-042        # Manual revert of a specific MR
gyre repo queue pause           # Manual merge queue pause
gyre repo queue resume          # Manual resume
```

### UI

- Repo detail page shows main branch health (green/red indicator)
- Merge queue shows paused state with reason
- Activity feed shows revert events with links to original MR and failure output

---

## 7. Secrets Delivery

### Principle

Agents must never see their secrets in plaintext. Secrets are injected into the agent's environment by the platform, used opaquely, and revoked on teardown.

### Architecture

```
Agent spawned
  |
  v
Agent authenticates with OIDC token to MCP server
  |
  v
MCP server resolves secrets based on token claims:
  - tenant_id -> tenant-level secrets
  - workspace_id -> workspace-level secrets
  - repo_id -> repo-level secrets
  - task_id -> task-specific secrets (if any)
  |
  v
Secrets injected into agent's environment variables
(agent process inherits them, never reads them from API)
  |
  v
Agent uses secrets opaquely (e.g., DATABASE_URL, API_KEY)
  |
  v
Agent completes or is killed
  |
  v
Secrets access revoked. Ephemeral credentials rotated.
```

### Secret Scoping

Secrets cascade like budgets:

```
Tenant secrets (shared across all workspaces)
  └── Workspace secrets (shared across repos in workspace)
        └── Repo secrets (specific to one repo)
              └── Task secrets (one-time, per-task)
```

An agent only receives secrets for its scope. A repo-scoped agent gets tenant + workspace + repo secrets, but not secrets from other repos.

### Secret Types

| Type | Example | Lifecycle |
|---|---|---|
| Static | `DATABASE_URL`, `SIEM_ENDPOINT` | Set by admin, persisted (encrypted at rest) |
| Ephemeral | Per-session DB credentials, short-lived API tokens | Generated at spawn, revoked at teardown |
| Rotated | OAuth tokens, Claude Max refresh tokens | Background job refreshes before expiry |
| Derived | Agent's own OIDC token, git credential | Generated from identity, scoped to session |

### Storage Backend

Secrets are stored encrypted in Gyre's database by default. Vault integration is optional:

- **Default (SOPS/database):** Secrets encrypted at rest with SOPS. Decrypted server-side, injected into agent env.
- **Vault:** Agent's OIDC token is used as Vault auth method. Vault returns scoped secrets. Gyre acts as intermediary (agent never talks to Vault directly).

### MCP Integration

Agents don't fetch secrets via MCP tools. Secrets are environment variables set before the agent process starts. The MCP server can report what secret names are available (not values):

| Resource | Purpose |
|---|---|
| `secrets://names` | List of secret names available to this agent (not values) |
| `secrets://status` | Expiry status of rotated secrets |

### Admin API

| Endpoint | Method | Purpose |
|---|---|---|
| `POST /api/v1/secrets` | POST | Create/update secret (admin only) |
| `GET /api/v1/secrets` | GET | List secret names by scope (not values) |
| `DELETE /api/v1/secrets/{id}` | DELETE | Delete secret |
| `POST /api/v1/secrets/{id}/rotate` | POST | Force rotation |

**CLI:** `gyre secret set --repo DATABASE_URL "postgres://..."`, `gyre secret list`, `gyre secret rotate`

**UI:** Secrets management in admin panel. Shows secret names, scopes, last rotated, expiry status. Never shows values.

---

## 8. Bootstrap & First-Run

### `gyre bootstrap` CLI Command

Automates the journey from nothing to a running platform:

```bash
gyre bootstrap \
  --tenant "Acme Corp" \
  --workspace "Platform Team" \
  --repo gyre \
  --repo-path /home/user/code/gyre \
  --admin-user jsell \
  --oidc-issuer https://keycloak.example.com/realms/acme
```

### What It Does

```
1. CREATE TENANT
   - Create tenant "Acme Corp" with default budget
   - Configure OIDC issuer (or skip for dev mode with static tokens)

2. CREATE ADMIN USER
   - Register admin user with Admin role
   - Generate API key for CLI authentication
   - Save config to ~/.gyre/config

3. CREATE WORKSPACE
   - Create workspace "Platform Team" under tenant
   - Set default budget (inherited from tenant)

4. ADD REPOSITORY
   - Initialize bare repo on disk (or link existing)
   - Register in Gyre database
   - Set up smart HTTP git endpoint

5. REGISTER BUILT-IN PERSONAS
   - workspace-orchestrator (pre-approved)
   - repo-orchestrator (pre-approved)
   - accountability (pre-approved)
   - security (pre-approved)

6. INITIALIZE SPEC REGISTRY
   - If repo contains specs/manifest.yaml, parse and register specs
   - If not, create a starter manifest with default policies

7. CONFIGURE DEFAULT GATES
   - TestCommand: cargo test (if Cargo.toml exists)
   - LintCommand: cargo clippy (if Cargo.toml exists)
   - Architecture lint: scripts/check-arch.sh (if exists)

8. SPAWN REPO ORCHESTRATOR
   - Spawn repo orchestrator agent with repo-orchestrator persona
   - Inject system prompt with protocol norms
   - Orchestrator reads specs/index.md, observes backlog, begins Ralph loop

9. PRINT SUMMARY
   - Tenant, workspace, repo IDs
   - Admin API key (show once)
   - Server URL, git clone URL
   - "Your repo orchestrator is running. Visit http://localhost:3000 for the dashboard."
```

### Dev Mode (No OIDC)

```bash
gyre bootstrap --dev
```

Skips OIDC configuration. Uses static auth tokens. Single tenant, single workspace. Good for local development and trying Gyre out.

### Starter Kit

`gyre bootstrap --starter-kit` creates a minimal repo structure:

```
specs/
  manifest.yaml         # Default manifest with design-principles spec
  index.md              # Auto-generated
  system/
    design-principles.md  # Template
AGENTS.md               # Entry point for agents
.prek.yaml              # Pre-commit hooks (cargo fmt, clippy, conventional commits)
```

### Protocol Injection

When any agent is spawned, the MCP server injects these system prompts:

1. **Persona prompt** - from the persona entity (versioned, approved)
2. **Protocol norms** - Ralph loop definition, MCP tool usage patterns, escalation rules, handoff protocol
3. **Context** - current task, spec refs, acceptance criteria, worktree path, budget limits
4. **Constraints** - "you are scoped to repo X, you cannot access repo Y, your budget is N tokens"

The agent doesn't need to read AGENTS.md to know how to behave. Everything is injected. AGENTS.md exists for human readability and for agents that connect outside of Gyre's spawn process.

---

## 9. CLI & UI Completeness

### CLI Command Tree

```
gyre
  ├── bootstrap [--dev] [--starter-kit]
  ├── tenant
  │     ├── create, list, show, set-budget
  │     └── users (list, invite, remove, set-role)
  ├── workspace
  │     ├── create, list, show, set-budget
  │     ├── add-repo, remove-repo
  │     └── team (add, remove, list)
  ├── repo
  │     ├── create, list, show, set-budget
  │     ├── status (main health, queue state)
  │     ├── queue (pause, resume, show)
  │     └── revert <MR-ID>
  ├── persona
  │     ├── create, list, show, edit, approve, delete
  │     └── versions <id>
  ├── spec
  │     ├── list, show, approve, revoke
  │     ├── pending (specs awaiting approval)
  │     └── drifted (specs with open drift tasks)
  ├── agent
  │     ├── list, show, spawn, stop
  │     └── logs <id>
  ├── task
  │     ├── list, create, show, take, update
  │     └── assign <task-id> <agent-id>
  ├── mr
  │     ├── create, list, show
  │     ├── add-dependency <mr-id> <depends-on-id>
  │     └── review (submit, comment)
  ├── budget
  │     ├── show [--workspace] [--tenant]
  │     └── set [--workspace] [--tenant] <limits>
  ├── secret
  │     ├── set, list, delete, rotate
  │     └── show-names
  ├── clone <workspace/repo>
  ├── push
  ├── status
  ├── health
  ├── ping
  └── tui
```

### UI Pages

> **Note:** Per `human-system-interface.md`, these pages are **contextual drill-downs** accessed by clicking entity references, not primary navigation items. The application has a stable 6-item sidebar (Inbox, Briefing, Explorer, Specs, Meta-specs, Admin). The pages below are rendered in the detail panel or as full-width pop-outs within those nav contexts.

| Page | Scope | Purpose |
|---|---|---|
| Tenant Dashboard | Tenant | Budget overview, workspace list, user management |
| Workspace Dashboard | Workspace | Repo list, cross-repo dependencies, budget per repo, active agents |
| Repo Detail | Repo | Branches, commits, MRs, tasks, agents, specs, budget, merge queue |
| Agent List | Repo | Active agents with status, task, budget usage, TTY attach |
| Task Board | Repo | Kanban board with status columns |
| MR Detail | Repo | Diff viewer, reviews, gate results, dependencies |
| Merge Queue | Repo | Ordered queue with dependency graph, gate status |
| Spec Registry | Repo | Specs with approval status, drift tasks, version history |
| Persona Management | Tenant/Workspace | Persona list, editor, approval workflow, usage report |
| Budget Dashboard | Any level | Usage charts, burn rate, projections, limit configuration |
| Secrets Management | Any level | Secret names, scopes, rotation status (never values) |
| Admin Panel | Tenant | Health, jobs, audit, SIEM, compute targets, snapshots |
| Activity Feed | Any level | Timeline of events with filters |
| Settings | Workspace | Server info, WebSocket status, configuration reference |

---

## Relationship to Existing Specs

This spec **supersedes or refines** the following:

- **Design Principles** (`design-principles.md`): hierarchy and scoping rules implement "security by default" and "server-side logic"
- **Identity & Security** (`identity-security.md`): OIDC tokens gain tenant_id, workspace_id, repo_id claims
- **Agent Runtime** (`agent-runtime.md`): MCP-based coordination replaces direct REST calls from agents
- **Agent Gates** (`agent-gates.md`): gate agents receive personas from the persona entity, not from markdown files
- **Spec Registry** (`spec-registry.md`): spec approval integrates with persona-based approver verification
- **Supply Chain** (`supply-chain.md`): stack_hash includes persona content_hash from the persona entity
- **Admin Panel** (`admin-panel.md`): admin panel gains tenant-level scope, persona management, budget dashboards
- **Spec Lifecycle** (`spec-lifecycle.md`): cross-repo spec escalation protocol defined here
- **Manager Agent** (`development/manager-agent.md`): split into workspace orchestrator + repo orchestrator
- **Workspace Orchestrator Persona** (`personas/workspace-orchestrator.md`): workspace orchestrator persona
- **Repo Orchestrator Persona** (`personas/repo-orchestrator.md`): repo orchestrator persona

This spec **depends on:**
- **Source Control** (`source-control.md`): repos, MRs, merge queue
- **Merge Dependencies** (`merge-dependencies.md`): cross-repo MR dependencies
- **Database Migrations** (`database-migrations.md`): tenant_id on all scoped tables (already specced)
