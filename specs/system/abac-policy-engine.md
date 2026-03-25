# ABAC Policy Engine

## Problem

ABAC (Attribute-Based Access Control) is referenced throughout Gyre's specs but never defined in detail. The current implementation has basic role checks (Admin/Developer/Agent/ReadOnly) enforced per-endpoint. This is RBAC, not ABAC.

At enterprise scale, RBAC breaks down:
- "Developers can create MRs" is too coarse. Can they create MRs in ANY repo? Only repos they're assigned to?
- "Agents can push" doesn't account for scope. Can an agent push to a repo it wasn't spawned against?
- "Admins can approve specs" doesn't consider spec ownership. Should workspace Admin A be able to approve a spec in workspace B?
- Policy changes require code changes (adding/removing middleware). No runtime configurability.

ABAC evaluates **attributes** of the subject, resource, action, and environment to make access decisions. Policies are declarative, composable, and configurable at runtime.

## Core Concepts

### Attributes

Every access decision considers four categories of attributes:

**Subject attributes (who is asking):**

| Attribute | Source | Example |
|---|---|---|
| `subject.type` | Auth context | `user`, `agent`, `system` |
| `subject.id` | Auth context | `user:jsell`, `agent:worker-42` |
| `subject.tenant_id` | Auth context / OIDC | `tenant-acme` |
| `subject.global_role` | User entity | `TenantAdmin`, `Member` |
| `subject.workspace_role` | Membership | `Owner`, `Admin`, `Developer`, `Viewer` |
| `subject.workspace_ids` | Memberships | `["ws-platform", "ws-app"]` |
| `subject.team_ids` | Team memberships | `["team-platform"]` |
| `subject.persona` | Agent OIDC claim | `security`, `accountability` |
| `subject.stack_hash` | Agent OIDC claim | `sha256:abc123...` |
| `subject.attestation_level` | Agent OIDC claim | `1`, `2`, `3` |
| `subject.repo_scope` | Agent OIDC claim | `repo:gyre-server` |

**Resource attributes (what is being accessed):**

| Attribute | Source | Example |
|---|---|---|
| `resource.type` | Request context | `spec`, `task`, `mr`, `repo`, `agent`, `persona`, `explorer_view`, `message`, `conversation` |
| `resource.id` | Request path | `system/identity-security.md` |
| `resource.tenant_id` | Entity lookup | `tenant-acme` |
| `resource.workspace_id` | Entity lookup | `ws-platform` |
| `resource.repo_id` | Entity lookup | `repo-gyre-server` |
| `resource.owner` | Entity field | `user:jsell` |
| `resource.team` | Entity field | `team:platform` |
| `resource.approval_status` | Entity field | `pending`, `approved` |
| `resource.visibility` | Entity field | `private`, `workspace`, `tenant` |

**Action attributes (what operation):**

| Attribute | Example |
|---|---|
| `action` | `read`, `write`, `delete`, `approve`, `spawn`, `push`, `merge`, `escalate` |

**Environment attributes (context):**

| Attribute | Source | Example |
|---|---|---|
| `env.time` | System clock | Current timestamp |
| `env.ip` | Request | Client IP address |
| `env.budget_remaining` | Budget system | Tokens remaining in agent's budget |
| `env.main_health` | Forge state | `green`, `red` (merge queue paused?) |

## Policy Language

Policies are declarative rules evaluated in order. First match wins (deny takes precedence over allow within the same priority).

### Policy Entity

```rust
pub struct Policy {
    pub id: Id,
    pub name: String,
    pub description: String,
    pub scope: PolicyScope,
    pub scope_id: Option<Id>,    // tenant_id, workspace_id, or repo_id
    pub priority: u32,            // Higher = evaluated first
    pub effect: PolicyEffect,
    pub conditions: Vec<Condition>,
    pub actions: Vec<String>,     // Actions this policy applies to
    pub resource_types: Vec<String>, // Resource types this policy applies to
    pub enabled: bool,
    pub immutable: bool,         // Immutable Deny policies are evaluated before all others
                                 // and cannot be overridden by any Allow regardless of priority.
                                 // See human-system-interface.md §2.
    pub created_by: Id,
    pub created_at: u64,
    pub updated_at: u64,
}

pub enum PolicyScope {
    Tenant,      // Applies to all workspaces
    Workspace,   // Applies to all repos in workspace
    Repo,        // Applies to one repo
}

pub enum PolicyEffect {
    Allow,
    Deny,
}
```

### Conditions

```rust
pub struct Condition {
    pub attribute: String,        // e.g., "subject.workspace_role"
    pub operator: ConditionOp,
    pub value: ConditionValue,
}

pub enum ConditionOp {
    Equals,
    NotEquals,
    In,
    NotIn,
    GreaterThan,
    LessThan,
    Contains,
    Exists,
}

pub enum ConditionValue {
    String(String),
    StringList(Vec<String>),
    Number(i64),
    Bool(bool),
}
```

### Policy Examples

**Tenant-level: agents can only access their scoped repo**
```yaml
- name: agent-repo-scope
  scope: tenant
  priority: 100
  effect: deny
  actions: ["*"]
  resource_types: ["*"]
  conditions:
    - attribute: subject.type
      operator: equals
      value: "agent"
    - attribute: subject.repo_scope
      operator: not_equals
      value: "$resource.repo_id"   # Dynamic reference
  description: "Agents can only access resources in the repo they were spawned against"
```

**Workspace-level: only Owner/Admin can manage personas**
```yaml
- name: persona-management
  scope: workspace
  priority: 90
  effect: deny
  actions: ["write", "delete"]
  resource_types: ["persona"]
  conditions:
    - attribute: subject.workspace_role
      operator: not_in
      value: ["Owner", "Admin"]
  description: "Only workspace Owner/Admin can create or modify personas"
```

**Workspace-level: viewers can't spawn agents**
```yaml
- name: viewer-no-spawn
  scope: workspace
  priority: 90
  effect: deny
  actions: ["spawn"]
  resource_types: ["agent"]
  conditions:
    - attribute: subject.workspace_role
      operator: equals
      value: "Viewer"
  description: "Viewers cannot spawn agents"
```

**Repo-level: only approved personas can be used as gate agents**
```yaml
- name: gate-approved-persona
  scope: repo
  priority: 80
  effect: deny
  actions: ["gate_review"]
  resource_types: ["mr"]
  conditions:
    - attribute: subject.type
      operator: equals
      value: "agent"
    - attribute: subject.attestation_level
      operator: less_than
      value: 3
  description: "Gate agents must have attestation level 3 (Gyre-managed runtime)"
```

**Tenant-level: deny all by default**
```yaml
- name: default-deny
  scope: tenant
  priority: 0    # Lowest priority, evaluated last
  effect: deny
  actions: ["*"]
  resource_types: ["*"]
  conditions: []  # No conditions = always matches
  description: "Deny everything not explicitly allowed"
```

## Evaluation Engine

### Evaluation Flow

```
Request arrives
  |
  v
Extract subject attributes (from auth context, OIDC token, memberships)
  |
  v
Extract resource attributes (from entity lookup)
  |
  v
Extract action (from request method + route)
  |
  v
Extract environment attributes (time, IP, system state)
  |
  v
Collect applicable policies:
  1. Repo-scoped policies (if resource is repo-scoped)
  2. Workspace-scoped policies
  3. Tenant-scoped policies
  |
  v
Check immutable Deny policies first:
  - Evaluate all policies where immutable == true && effect == Deny
  - If ANY immutable Deny matches → Deny (cannot be overridden)
  |
  v
Sort remaining policies by priority (highest first)
  |
  v
Evaluate each policy's conditions:
  - All conditions must match (AND)
  - First matching policy wins
  |
  v
Apply effect (Allow or Deny)
  |
  v
Log decision to audit trail
```

### Policy Composition

Policies at different scopes compose with these rules:

1. **More specific scope takes precedence** when priorities are equal (repo > workspace > tenant)
2. **Higher priority always wins** regardless of scope
3. **Deny overrides Allow** at the same priority and scope
4. **Default deny** at tenant level catches everything not explicitly allowed

This means:
- A tenant policy can set a baseline that workspaces can't override (by using high priority)
- A workspace policy can set defaults that repos can customize (by using moderate priority)
- A repo policy can grant or restrict access within its scope (by using repo-level scope)

### Built-In Policies

Gyre ships with a set of built-in tenant-level policies that enforce fundamental invariants:

| Policy | Effect | Purpose |
|---|---|---|
| `system-full-access` | Allow | Global `GYRE_AUTH_TOKEN` identity gets full access (matched by `subject.id == "gyre-system-token"`, not by `subject.type`) |
| `tenant-isolation` | Deny | Users/agents can't access other tenants |
| `agent-repo-scope` | Deny | Agents can't access resources outside their scoped repo |
| `workspace-membership-required` | Deny | Users must be workspace members to access workspace resources |
| `persona-human-approval` | Deny | Agents can't approve personas (human-only) |
| `default-deny` | Deny | Everything not explicitly allowed is denied |

Built-in policies cannot be deleted. Non-immutable built-in policies can be overridden by higher-priority custom policies (with audit logging). Immutable built-in policies (where `immutable: true`) cannot be overridden regardless of priority — immutable Deny policies are evaluated before all priority-based evaluation (see `human-system-interface.md` §2).

### Performance

Policy evaluation happens on every request. It must be fast:

- **Cache policy sets per scope.** When policies change, invalidate the cache for that scope.
- **Precompute membership lookups.** User's workspace/team memberships are cached in the auth context, not looked up per-request.
- **Short-circuit on first match.** Don't evaluate remaining policies after a match.
- **Target: <1ms per evaluation.** With cached policies and precomputed attributes, this is achievable.

## Audit Integration

Every policy evaluation is logged:

```rust
pub struct PolicyDecision {
    pub request_id: String,
    pub subject_id: String,
    pub subject_type: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub decision: PolicyEffect,     // Allow or Deny
    pub matched_policy: Option<Id>, // Which policy matched
    pub evaluated_at: u64,
}
```

Denied requests are logged at `warn` level. Allowed requests at `debug` level. All decisions are queryable via the audit API.

For compliance: "show me every time someone was denied access to specs in workspace X" is a single query.

## API

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/policies` | GET | List policies (filtered by scope) |
| `POST /api/v1/policies` | POST | Create policy (Admin only) |
| `GET /api/v1/policies/{id}` | GET | Get policy details |
| `PUT /api/v1/policies/{id}` | PUT | Update policy |
| `DELETE /api/v1/policies/{id}` | DELETE | Delete policy (not built-ins) |
| `POST /api/v1/policies/evaluate` | POST | Dry-run: "would this request be allowed?" |
| `GET /api/v1/policies/decisions` | GET | Query audit log of policy decisions |
| `GET /api/v1/policies/effective?subject={id}&resource_type={type}` | GET | "What can this user/agent do on this resource type?" |

### Dry-Run Evaluation

Before deploying a new policy, admins can test it:

```json
POST /api/v1/policies/evaluate
{
  "subject": { "type": "agent", "id": "worker-42", "workspace_role": "Developer" },
  "action": "push",
  "resource": { "type": "repo", "id": "gyre-server" }
}

Response:
{
  "decision": "allow",
  "matched_policy": "developer-repo-access",
  "evaluated_policies": 7,
  "evaluation_ms": 0.3
}
```

## CLI

```bash
gyre policy list                                    # List all policies
gyre policy list --scope workspace --workspace-id ws-1  # Workspace policies
gyre policy create --file policy.yaml               # Create from YAML
gyre policy evaluate --subject user:jsell --action approve --resource spec:identity  # Dry-run
gyre policy decisions --subject agent:worker-42 --decision deny --since 1d  # Audit
gyre policy effective --subject user:jsell --resource-type spec  # "What can jsell do with specs?"
```

## UI

- **Policy management page** (tenant/workspace/repo scoped): list, create, edit, delete policies
- **Policy editor:** visual condition builder with attribute autocomplete
- **Dry-run simulator:** "what would happen if user X tried to do Y on resource Z?"
- **Decision audit log:** filterable table of all policy evaluations with Allow/Deny outcome
- **Effective permissions view:** select a user/agent, see what they can do across all resource types
- **Policy conflict detector:** highlight policies that could produce unexpected interactions

## MCP Integration

Agents don't interact with the policy engine directly. The MCP server enforces policies transparently on every tool call. However, agents can query their own permissions:

| Resource | Purpose |
|---|---|
| `permissions://me` | "What actions am I allowed to perform in my scope?" |
| `permissions://check?action={action}&resource={type}` | "Can I do this specific thing?" (pre-flight check) |

This lets agents avoid wasting tool calls on actions they'll be denied.

## Relationship to Existing Specs

- **Platform Model** (`platform-model.md`): the ownership hierarchy (tenant/workspace/repo) provides the scoping model for policies. Token claims provide subject attributes.
- **Identity & Security** (`identity-security.md`): OIDC claims are the primary source of subject attributes. ABAC replaces the current role-based middleware.
- **User Management** (`user-management.md`): workspace roles and team memberships are subject attributes used in policy evaluation.
- **Agent Gates** (`agent-gates.md`): gate agent attestation level is a subject attribute. Policies can enforce "only Level 3 agents can be gate reviewers."
- **Spec Registry** (`spec-registry.md`): spec approval permissions are ABAC policies, not hardcoded approver lists. The manifest declares who SHOULD approve; ABAC determines who CAN approve.
- **Observability** (`observability.md`): policy decisions feed the audit system. SIEM forwarding includes denied access events.
- **Supply Chain** (`supply-chain.md`): stack_hash in OIDC token is a subject attribute. Policies can enforce "only agents with this stack can approve specs."
