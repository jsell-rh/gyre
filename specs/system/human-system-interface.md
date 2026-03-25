# Human-System Interface

> This spec defines how humans interact with a fully autonomous software development system. It **supersedes `ui-journeys.md`** (which is deprecated — its navigation model, journeys, and keyboard shortcuts are replaced by this spec). It extends `system-explorer.md` with concrete interaction patterns for trust calibration, agent interrogation, scoped communication, and LLM-driven architectural exploration.

## The Novel Problem

No tool has addressed the interaction model for a system where agents write all code. GitHub, JIRA, Linear — every developer tool assumes humans write code and review each other's work. When agents do both, the human's role changes fundamentally:

- **You didn't write the code.** You can't reason about it from memory.
- **You didn't review the code.** Gates and agent reviewers did.
- **You don't track tasks.** The orchestrator decomposes specs into tasks automatically.
- **You don't manage branches.** The merge queue handles ordering and conflicts.

What remains: deciding what to build (specs), setting direction on how to build it (meta-specs), maintaining understanding of what exists (exploration), handling exceptions (escalation), and steering agents in-flight (communication).

Every surface in this spec exists to serve one of those five activities. If it doesn't, it shouldn't exist.

---

## 1. Navigation Model

### Stable Sidebar, Adaptive Content

The sidebar is **permanent and unchanging** regardless of scope. Users build muscle memory for nav item positions. The *content area* adapts to the current scope (tenant, workspace, repo), not the sidebar.

```
Sidebar (always visible, always these items):
  Inbox
  Briefing
  Explorer
  Specs
  Meta-specs
  Admin
```

Six items. That's it.

**Why not scope-dependent sidebar?** Changing the sidebar when the user navigates between scopes is disorienting — the same navigation item moves position or disappears. Notion, Backstage, and VS Code all keep the primary nav stable. The content area is where scope manifests.

### Scope Indicator (Breadcrumb)

A persistent breadcrumb in the topbar shows the current scope:

```
Acme Corp  >  Payments  >  payment-api
[tenant]      [workspace]   [repo]
```

Each segment is clickable — click "Payments" to zoom out to workspace scope. The breadcrumb is always present. Narrowing scope (clicking a workspace or repo in any view) updates the breadcrumb and the content area re-renders for that scope.

### What Each Nav Item Shows at Each Scope

| Nav Item | Tenant Scope | Workspace Scope | Repo Scope |
|---|---|---|---|
| **Inbox** | Action queue across all workspaces | Action queue for this workspace | Action queue for this repo |
| **Briefing** | Narrative across all workspaces | Narrative for this workspace | Narrative for this repo |
| **Explorer** | Workspace cards with summary stats. This is a **list view**, not a graph canvas — click a workspace to enter the graph-based Explorer. Data sourced from `GET /api/v1/workspaces` (list) + `GET /api/v1/workspaces/:id/budget` (usage stats) — no new endpoint needed. Repo count and active agent count derived from existing list endpoints with workspace filter. | Realized architecture (C4 progressive drill-down per `system-explorer.md`) | Repo-level architecture detail |
| **Specs** | Spec registry across all workspaces | Specs across repos in workspace | Specs in this repo + implementation progress |
| **Meta-specs** | Persona/principle/standard catalog | Persona editor, preview loop, reconciliation progress | (redirects to workspace scope) |
| **Admin** | Users, compute, tenant budget, audit | Workspace settings, budget, trust level, teams | Repo settings, gates, policies |

**Meta-specs at workspace scope** is the primary location for the preview loop from `meta-spec-reconciliation.md`: edit a persona → select target specs → preview agents implement on throwaway branches → view diff → iterate → publish. Reconciliation progress tracking also lives here. At tenant scope, Meta-specs shows a catalog of all personas/principles/standards across workspaces. At repo scope, it redirects to the workspace scope (meta-specs are workspace-scoped, not repo-scoped).

**Where old views live:** Task Board, Merge Queue, Agent List, MR Detail, Repo Detail, Persona Management, Activity Feed, and other entity views from `platform-model.md` are **contextual drill-downs**. The Activity Feed (chronological event timeline) is accessible as the Explorer's Change View at repo scope, or via the Briefing's "since your last visit" time range selector — there is no separate Activity nav item. For raw event debugging, the Admin view's Audit tab provides the unfiltered event stream. Other entity views — accessed by clicking an entity reference anywhere in the UI (agent name → slide-in panel, MR link → detail view, etc.). They are not primary navigation items. The Code tab (branches, commits, MRs, merge queue) is accessed via the Explorer at repo scope, not as a separate nav item.

The content adapts. The sidebar doesn't.

A **status bar** at the bottom of the application shows trust level, budget usage, WebSocket status, and presence avatars for the current workspace. See `ui-layout.md` §1 for dimensions and layout. Presence updates are sent on **both** a 30-second timer AND on every view change (sidebar nav click or scope transition). The server evicts entries after 60 seconds without an update.

### Deep Links

**Entrypoint:** First visit lands on Explorer at tenant scope (workspace cards). After workspace selection, redirects to Inbox at workspace scope — the default landing view. Subsequent visits restore the last-used workspace and land on the Inbox. See `ui-layout.md` §1 for full entrypoint flow.

Every view state is URL-addressable:
- `/inbox` — tenant-scoped inbox
- `/workspaces/:id/inbox` — workspace-scoped inbox
- `/repos/:id/explorer` — repo-scoped explorer
- `/repos/:id/specs?path=system/vision.md` — specific spec in a repo (path as query param for clean URL structure; note: UI routes are not bound by `api-conventions.md` §4 which governs API endpoints, but the query param pattern is cleaner regardless)

### Keyboard Navigation

| Shortcut | Action |
|---|---|
| `Cmd+K` | Global search (specs, types, concepts, agents). When Explorer canvas is focused, this opens global search, not canvas-local search. Canvas-local search uses `/` within the Explorer. This supersedes `system-explorer.md`'s `Cmd+K` which was canvas-scoped. |
| `Cmd+1` | Jump to Inbox |
| `Cmd+2` | Jump to Briefing |
| `Cmd+3` | Jump to Explorer |
| `Cmd+4` | Jump to Specs |
| `Cmd+5` | Jump to Meta-specs |
| `Cmd+6` | Jump to Admin |
| `Esc` | Close detail panel / go up one scope level |
| `/` | Focus search within current view (suppressed during text input) |
| `?` | Show keyboard shortcut reference (suppressed during text input) |

Cmd-prefixed shortcuts avoid conflicts with text input (spec editing, chat, search). `/` and `?` are bare keys suppressed when a text input is focused.

---

## 2. Trust Gradient

### The Problem

When you first deploy Gyre, you don't trust the agents. You want to see everything — every MR, every agent decision, every gate result. Over time, trust builds. You stop reviewing MRs. You check the Briefing once a day. You only react to exceptions.

No current tool models this trust progression because no current tool has fully autonomous agents. The UI must adapt to the human's confidence level.

### Trust Levels

Trust is a **workspace-level setting**. Different workspaces can have different trust levels. A new experimental workspace starts at Supervised. The core platform workspace that's been running for months is at Autonomous.

```
Workspace Settings > Trust Level

  ○ Supervised    — I review everything before it merges
  ○ Guided        — Agents merge if gates pass, alert me on failures
  ● Autonomous    — Only interrupt me for exceptions
  ○ Custom        — Configure policies manually
```

One click. No ABAC knowledge required.

**Storage:** The trust level is a field on the `Workspace` entity: `trust_level: TrustLevel` (enum: `Supervised`, `Guided`, `Autonomous`, `Custom`). Changing trust level is a `PUT /api/v1/workspaces/:id` update (existing endpoint, `resource_type: "workspace"`, `action: "write"` — requires Admin workspace role). The ABAC policy replacement is a **server-internal side effect** of the workspace update:

- **Preset → Preset:** Server deletes trust-managed policies from the old preset, creates policies for the new preset.
- **Preset → Custom:** Server preserves the current preset's policies as the starting point. The user can then add/edit/delete via the policy editor.
- **Custom → Preset:** Server deletes all `trust:` prefixed policies, then creates the preset's policies. Built-in policies (`builtin:` prefix) and user-created policies (no prefix) are preserved.

**Policy naming conventions:**
- `trust:` prefix — trust-preset-managed, deleted and recreated on trust level transitions
- `builtin:` prefix — immutable server-seeded policies, never deleted by trust transitions
- No prefix — user-created custom policies, preserved across transitions

### What Each Level Controls

| Aspect | Supervised | Guided | Autonomous |
|---|---|---|---|
| **MR merge** | Human approval required | Autonomous if all gates pass | Autonomous if all gates pass |
| **Spec approval** | Human approval required | Human approval required | Human approval required (always) |
| **Inbox shows** | Every MR, every gate result, every agent decision | Priorities 1-9 (per row below) | Priorities 1-9 (per row below) |
| **Briefing detail** | Per-agent activity, per-MR status | Per-spec progress, exceptions | Spec-level summaries, exceptions only |
| **Notifications** | Every state change | Failures and approvals | Exceptions only |
| **Agent completion summaries** | Full decision log visible | Uncertainties highlighted | Only low-confidence decisions surfaced |
| **Inbox priorities shown** | 1-10 (everything) | 1-9 (excludes suggested links) | 1-9 (excludes suggested links) |

**Spec approval is always human.** This is a hard rule from `platform-model.md` — specs encode intent, and intent is a human decision. Trust level never bypasses spec approval.

### Mechanical Implementation

Each trust preset maps to a set of ABAC policies applied to the workspace:

**Supervised:**
```yaml
- name: "trust:require-human-mr-review"
  effect: deny
  actions: ["merge"]
  resource_types: ["mr"]
  conditions:
    - attribute: subject.type
      operator: equals
      value: "system"
  description: "trust: Block autonomous merge processor — require human MR approval first"
```

The merge processor evaluates ABAC with `action: "merge"` (per `abac-policy-engine.md`'s action attribute table). **Important:** the merge processor must NOT use the global `GYRE_AUTH_TOKEN` (which bypasses ABAC per `hierarchy-enforcement.md` §4). Instead, it evaluates ABAC as an internal service. The mechanism: ABAC bypass is checked by identity (`subject.id == "gyre-system-token"`), not by type. The merge processor uses `subject.type: "system"`, `subject.id: "merge-processor"` — since its `subject.id` is not the system token identity, it does not bypass ABAC and is subject to the Supervised trust policy. This requires amending `hierarchy-enforcement.md` §4 to change the ABAC bypass condition from "system tokens bypass ABAC" to "the global `GYRE_AUTH_TOKEN` identity bypasses ABAC (matched by `subject.id`, not `subject.type`)." Under Supervised, this policy blocks the merge processor from autonomously merging. The human approves the MR via status transition in the UI (`action: "write"`, `subject.type: "user"` — not blocked). The merge processor then sees the approved status and proceeds. Administrative operations use the `system-full-access` built-in policy (higher priority), so they are not blocked.

**Guided** policy set:
- `require-human-spec-approval` (immutable, always present)
- Gate failures surface in the Inbox (no additional policy — this is the default behavior from `default-deny` + `developer-write-access`)
- `require-human-mr-review` is NOT present — merge processor proceeds autonomously when all gates pass

**Autonomous:** removes most notification policies. Keeps two policies:

```yaml
- name: "builtin:require-human-spec-approval"    # builtin: prefix, NOT trust: — immune to trust transitions
  scope: tenant
  priority: 999
  immutable: true        # ABAC engine skips priority override for immutable policies
  effect: deny
  actions: ["approve"]
  resource_types: ["spec"]
  conditions:
    - attribute: subject.type
      operator: not_equals
      value: "user"
  description: "Spec approval is always human, regardless of trust level"
```

This policy is **immutable** — it exists at every trust level, including Custom, and **independent of trust presets**. It is seeded at server startup as a built-in policy.

**Priority and override behavior:** The `require-human-spec-approval` policy uses `priority: 999` and is marked as `immutable: true` (a new boolean flag on `Policy` — requires amending `abac-policy-engine.md`). Immutable policies cannot be overridden regardless of priority — the ABAC evaluation engine processes immutable Deny policies FIRST, before any priority-based evaluation. This amends `abac-policy-engine.md` §"Policy Composition" rule 2 ("higher priority always wins") by adding a precondition: "immutable Deny policies are evaluated before all others and cannot be overridden by any Allow policy regardless of priority." The `system-full-access` policy (which enables server-internal operations) operates at `priority: 1000` but only applies to `subject.id == "gyre-system-token"`, not to spec approval — spec approval actions are carved out from `system-full-access` via an explicit condition: `actions: ["*"] EXCEPT approve ON spec`. This means even the superuser token cannot approve specs programmatically.

The Custom trust editor grays out immutable policies with tooltip: "This policy cannot be removed or overridden." Per `platform-model.md` §2, agents cannot approve specs that define their own behavior.

Budget warnings (priority 7 in the Inbox) remain visible at Autonomous trust because `platform-model.md` §5 defines budget exhaustion as requiring human action.

**Custom:** opens the ABAC policy editor for direct manipulation.

### Trust Suggestions

The system can suggest increasing trust based on track record:

```
This workspace has had 0 gate failures and 0 reverted MRs in 30 days.
Consider increasing trust level to Autonomous.
[Increase Trust] [Dismiss]
```

This appears as an Inbox item (priority 8). The human decides.

**Mechanism:** A background job (`trust_suggestion_check`) runs daily per workspace. It queries gate results and MR reverts for the last 30 days. If both counts are 0 and the current trust level is not already Autonomous, it creates a `Notification` for workspace Admin members. The job is registered in the server's `JobRegistry` alongside existing jobs (stale agent detector, budget reset, etc.).

---

## 3. The Explorer: Progressive Architecture Discovery

### The Zero-Knowledge Problem

The human didn't write the code. They can't reason about it from memory. Traditional tools assume familiarity — file trees, grep, code search. These are useless when you have zero knowledge of what exists.

The Explorer solves this with **progressive disclosure starting from boundaries** — the same technique architects use when onboarding onto a new system.

### Default Views (Automatic, No LLM)

#### Boundary View (C4 Progressive Drill-Down)

The default Explorer view. Each level answers "what is this made of?"

**Level 1 — Context (Workspace scope):** Repos and their external dependencies.
```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ payment-api  │────▸│ ledger-svc  │────▸│ billing-gw  │
│ 12 types     │     │ 8 types     │     │ 5 types     │
│ 3 endpoints  │     │ 4 endpoints │     │ 2 endpoints │
└─────────────┘     └─────────────┘     └─────────────┘
       │
       ▼
  [external: stripe-rs]
```

**Level 2 — Container (Repo scope):** Crates/packages within a repo.
```
payment-api
  ├── payment-common    (shared types, no deps)
  ├── payment-ports     (interfaces)
  ├── payment-domain    (business logic) ──▸ ports + common
  ├── payment-adapters  (DB, HTTP) ──▸ ports
  └── payment-server    (HTTP server) ──▸ domain + adapters
```

**Level 3 — Component (Crate scope):** Modules and their public interfaces.
```
payment-domain
  ├── mod retry         ──▸ pub struct RetryPolicy, pub fn should_retry()
  ├── mod charge        ──▸ pub struct Charge, pub enum ChargeStatus
  ├── mod refund        ──▸ pub struct Refund, pub trait RefundPort
  └── mod reconcile     ──▸ pub fn reconcile_ledger()
```

**Level 4 — Code (Module scope):** Types, functions, fields, methods.
```
retry::RetryPolicy
  ├── max_attempts: u32
  ├── backoff_ms: u64
  ├── fn should_retry(&self, attempt: u32) -> bool
  └── spec: specs/system/payment-retry.md (confidence: high)
      └── last modified by: worker-12, 2 hours ago, churn: 3/30d
```

Each level is generated automatically from the knowledge graph. No LLM needed. Click to drill down, breadcrumb to drill up.

#### Spec Realization View

Specs on the left, their linked graph nodes on the right. Shows "what implements this spec?"

```
┌─ Spec ────────────────────────┐  ┌─ Realization ─────────────────┐
│ payment-retry.md              │  │ RetryPolicy (type)            │
│ Status: Approved ✓            │  │ should_retry (function)       │
│ Implementation: 4/5 tasks     │  │ POST /payments/retry (endpoint)│
│                               │  │ retry_tests (module)          │
│ [Read Spec] [Edit Spec]       │  │                               │
└───────────────────────────────┘  └───────────────────────────────┘
```

#### Change View

What changed since last visit, at the structural level. Powered by architectural deltas.

```
Since your last visit:

  + Added: RetryPolicy (type in payment-domain)
    Spec: payment-retry.md | Agent: worker-12

  ~ Modified: ChargeService (3 new methods)
    Spec: charge-processing.md | Agent: worker-8

  - Removed: LegacyPaymentHandler
    Spec: (none — cleanup task)
```

### Saved Views (Curated, Shared)

Views are serializable specs that can be saved to the workspace and shared:

```json
{
  "name": "API surface",
  "query": {
    "node_types": ["Endpoint"],
    "include_edges": ["RoutesTo"],
    "depth": 1
  },
  "layout": "list",
  "columns": ["name", "qualified_name", "spec_path", "last_modified_by"]
}
```

Saved views are stored as JSON documents keyed by workspace. The key format is `workspace_id:view_id` (UUID), ensuring workspace isolation (views from workspace A are not queryable by workspace B). If `KvJsonStore` is used, the namespace is `explorer_views` — note the single-tenant limitation flagged in `hierarchy-enforcement.md` §3. For multi-tenant deployments, saved views should migrate to a proper port trait with tenant-scoped adapter. API endpoints for view CRUD (each requires a `RouteResourceMapping` entry in the ABAC `ResourceResolver` with `resource_type: "explorer_view"` and `workspace_param: "workspace_id"`):
- `GET /api/v1/workspaces/:workspace_id/explorer-views` — list saved views
- `POST /api/v1/workspaces/:workspace_id/explorer-views` — create a view. Response (201): `{view_id: "<uuid>", name: "...", query: {...}, ...}`
- `GET /api/v1/workspaces/:workspace_id/explorer-views/:view_id` — load a specific saved view (for deep links and sharing)
- `PUT /api/v1/workspaces/:workspace_id/explorer-views/:view_id` — update a saved view
- `DELETE /api/v1/workspaces/:workspace_id/explorer-views/:view_id` — delete a view

`:view_id` is a UUID, not a slug — avoids name collision issues. The human-readable name is a display field, not a key.

Built-in saved views shipped with every workspace:
- **API Surface** — all endpoints with their handlers
- **Domain Model** — types and interfaces in the domain crate
- **Security Boundary** — auth, ABAC, token validation paths
- **Test Coverage** — modules with/without test modules

### Generated Views (LLM-Powered, On-Demand)

The user asks a question in natural language. The LLM translates it to a graph query + layout, producing a focused view.

```
User: "How does authentication work?"

LLM generates view definition:
{
  "name": "How auth works",
  "query": {
    "concept": "auth",
    "node_types": ["Module", "Function", "Type", "Endpoint"],
    "depth": 2
  },
  "layout": "hierarchical",
  "highlight": {"spec_path": "specs/system/identity-security.md"},
  "explanation": "Authentication flows through require_auth_middleware (validates tokens),
    then AuthenticatedAgent extractor (resolves identity), with four token types..."
}
```

The view renders with the LLM's explanation as a sidebar annotation. The user can save the view for reuse or refine the question.

**Important constraint:** The LLM has access only to the knowledge graph API (read-only). It cannot modify code, create tasks, or trigger agent actions. It is a *query translator*, not an agent.

### Three Lenses

Every Explorer view can be overlaid with one of three lenses:

| Lens | What It Shows | Data Source |
|---|---|---|
| **Structural** (default) | Interfaces, boundaries, data shapes, dependencies | Knowledge graph nodes + edges |
| **Evaluative** | Test results, gate outcomes, spec assertion status | Gate results, spec approval ledger |
| **Observable** (future) | SLIs, error rates, latency per endpoint | Production telemetry (not yet implemented — design must not preclude it) |

The lenses compose — you can view the domain model (structural) with test coverage overlay (evaluative) to see "which types have tests?"

---

## 4. Agent Communication

### The Problem

The human needs to communicate with agents at three levels:
1. **Direction** — "build this" (specs, async, before agent starts)
2. **Steering** — "change approach" (in-flight, while agent is working)
3. **Interrogation** — "why did you do this?" (post-hoc, after agent completes)

Current tools have direction (JIRA tickets) but not steering or interrogation. In a fully autonomous system, all three are essential.

### Scoped Inline Chat

Chat is not a global interface. It is **scoped to a context** — you always know what you're talking to and where the message goes.

| Context | Recipient | Where It Appears | Use Case |
|---|---|---|---|
| Agent detail panel | Specific agent | Inline chat in agent's activity view | Steering: "try a different approach" |
| Workspace orchestrator | Workspace orchestrator agent | Inline chat in workspace overview | Priority: "pause search work, focus on payments" |
| Briefing view | LLM (read-only, grounded in briefing data) | Inline Q&A below the briefing | Follow-up: "tell me more about the auth refactor" |
| MR detail view | Author agent (or interrogation agent) | Inline chat in MR panel | Feedback: "naming convention is wrong" |

**Visual indicator:** The chat input shows the recipient: `Message to worker-12 ▸` or `Ask about this briefing ▸`. The user always knows where their message is going.

**Message delivery:** Human messages are Directed-tier messages via the message bus. They are signed, persisted, and attestable. The agent receives them via MCP `message.poll` tool.

### Hard Interrupt

The human can interrupt an agent immediately:

```
Agent Detail Panel:
  [Pause] [Stop] [Message]

  Pause: Sends a Directed-tier message with kind `StatusUpdate` and payload
         `{status: "pause_requested", summary: "Human requested pause"}` to the agent.
         Agent picks up on next `message.poll` and pauses after current tool call.
         No new API endpoint — uses existing message bus.

  Stop:  Calls existing `POST /api/v1/admin/agents/:id/kill` (per `api-reference.md`).
         Agent process terminated, work preserved in worktree and branch.
         Task marked as blocked with reason.

  Message: Opens inline chat. Sends Directed-tier message to agent's inbox.
           Agent picks up on next `message.poll` cycle.
```

**Pause** is the preferred interrupt — it's non-destructive. The agent finishes its current action, sees the human's message, and can adjust. **Stop** is for emergencies.

### Agent Completion Summaries

When an agent completes a task (`agent.complete`), it produces a structured summary as part of completion:

```json
{
  "task_id": "TASK-42",
  "spec_ref": "specs/system/payment-retry.md",
  "decisions": [
    {
      "what": "Used exponential backoff for retry",
      "why": "Spec mentions idempotency requirement",
      "confidence": "high",
      "alternatives_considered": ["fixed interval", "fibonacci backoff"]
    },
    {
      "what": "Split RetryPolicy into separate module",
      "why": "No explicit guidance in spec, followed hexagonal boundary pattern from meta-specs",
      "confidence": "medium"
    }
  ],
  "uncertainties": [
    "Spec doesn't cover timeout behavior for the 4th retry attempt — used 30s default"
  ],
  "conversation_sha": "<sha256 of full conversation history>"
}
```

**Delivery path:** The completion summary is submitted as part of the `agent.complete` MCP tool call (extends the existing tool with a `summary` field). The server:
1. Stores the summary in the MR attestation bundle
2. Emits an `AgentCompleted` message via the message bus. This kind must be added to `message-bus.md`'s `MessageKind` enum:

   **`AgentCompleted` specification:**
   - **Tier:** Event (signed, persisted with TTL)
   - **Destination:** `Workspace(agent's workspace_id)`
   - **server_only():** `true` (emitted by the server on `agent.complete`, not by agents)
   - **Payload schema:**
     | Field | Type | Required |
     |---|---|---|
     | `agent_id` | Id | yes |
     | `task_id` | Id | yes |
     | `spec_ref` | Option\<String\> | no |
     | `decisions` | `[{what: String, why: String, confidence: String, alternatives_considered: Option<[String]>}]` | yes |
     | `uncertainties` | `[String]` | yes |
     | `conversation_sha` | Option\<String\> | no |
3. The `agent.complete` handler directly creates Inbox notifications for **all workspace members with Admin or Developer workspace role** when the completion summary contains non-empty `uncertainties`. This is NOT routed through the `MessageConsumer` bounded channel (which can drop messages under backpressure). Priority-1 Inbox items are too critical to be fire-and-forget — they are created synchronously in the completion handler alongside the MR attestation write.
4. The Briefing consumes `AgentCompleted` messages for the "Completed" section

One LLM call at completion time, not continuous overhead. The summary is also used as seed context for interrogation agents.

### Interrogation Agents

When the human wants to understand *why* an agent made a decision, they can spawn an **interrogation agent** — a restricted agent that has the original agent's full context but can only communicate with the requesting human.

**UX:** In any agent-produced artifact (MR, code in Explorer, completion summary), a button: **"Ask why"**.

```
MR #52: Payment retry endpoint
  Author: worker-12 | Persona: backend-dev v4

  [Diff] [Gates] [Attestation] [Ask Why]
```

Clicking "Ask why" spawns an interrogation agent with:
- The original agent's conversation history (retrieved via `ConversationRepository::get(conversation_sha)` — the SHA is stored in the MR attestation bundle's `conversation_sha` field and in the `AgentCompleted` message payload)
- The original agent's persona
- The spec the task was implementing
- The MR diff

**MR attestation amendment:** The existing attestation bundle (`MergeAttestation` in `agent-gates.md`) gains a `conversation_sha: Option<String>` field, populated from the `AgentCompleted` message when the MR is merged.

**Restrictions on interrogation agents** are enforced via ABAC (the sole authorization layer per `hierarchy-enforcement.md` §4). When an interrogation agent is spawned, the server creates an `interrogation-only` ABAC policy scoped to that agent:

```yaml
- name: interrogation-restrict-<agent-id>
  scope: tenant                      # uses existing PolicyScope variants
  priority: 200                      # high priority, overrides workspace/repo policies
  effect: deny
  actions: ["write", "delete", "spawn", "approve", "merge"]
  resource_types: ["task", "mr", "repo", "agent", "spec", "persona", "worktree"]
  conditions:
    - attribute: subject.id
      operator: equals
      value: "agent:INTERROGATION_AGENT_UUID"  # actual UUID, no angle brackets
  description: "Interrogation agent is read-only + message to requesting human"

- name: interrogation-allow-message-INTERROGATION_AGENT_UUID
  scope: tenant
  priority: 201
  effect: allow
  actions: ["write"]
  resource_types: ["message"]
  conditions:
    - attribute: subject.id
      operator: equals
      value: "agent:INTERROGATION_AGENT_UUID"
```

The policies use `subject.id` conditions (not a new `Agent` scope variant) to target the specific interrogation agent, staying within `abac-policy-engine.md`'s existing `PolicyScope` enum (Tenant, Workspace, Repo). The agent's JWT `max_lifetime_secs` is set to 1800 (30 minutes).

**Policy cleanup:** Interrogation-specific ABAC policies are deleted on any of these paths:
1. Agent calls `agent.complete` → completion handler deletes policies by name pattern `interrogation-*-<agent-id>`
2. Admin kills agent → kill handler deletes policies by same pattern
3. JWT expires → stale agent detector marks the agent Dead → the detector's cleanup logic deletes policies by pattern (the stale agent detector must be extended to check for `interrogation-*` prefixed policies on any Dead agent)

All three paths are deterministic. No orphaned policies possible as long as the stale agent detector runs (which it does as a background job).

The interrogation session is itself attested — the conversation is stored as a provenance artifact linked to the original MR.

---

## 5. Conversation-to-Code Provenance

### The Problem

When an agent writes code, the reasoning behind each decision is locked in the agent's conversation history. Today, `git blame` tells you *who* wrote a line and *when*. In an autonomous system, you also need to know *why* — and the "why" is in the conversation.

### Design

Each agent's conversation with its LLM is hashed and stored as a provenance artifact.

**Crate placement:** `ConversationProvenance` and `TurnCommitLink` both live in `gyre-common` (shared wire types, like `Message` and `Id`). `ConversationRepository` lives in `gyre-ports`. The turn-to-commit linking is performed by the server layer (`gyre-server`) in the git push handler — it reads the current agent's conversation state from request context and records the link. This is server-layer orchestration, not domain logic.

Storage requires a new port trait:

```rust
// gyre-ports
#[async_trait]
pub trait ConversationRepository: Send + Sync {
    /// Store a conversation blob with metadata. Returns the SHA-256 hash.
    async fn store(&self, agent_id: &Id, workspace_id: &Id, conversation: &[u8]) -> Result<String>;
    /// Retrieve a conversation by SHA. Returns decompressed bytes.
    /// The adapter handles decryption and decompression internally.
    async fn get(&self, conversation_sha: &str) -> Result<Option<Vec<u8>>>;
    /// Record a turn-to-commit link (called from git push handler).
    async fn record_turn_link(&self, link: &TurnCommitLink) -> Result<()>;
    /// Get turn-commit links for a conversation (for "View conversation at this point").
    async fn get_turn_links(&self, conversation_sha: &str) -> Result<Vec<TurnCommitLink>>;
}
```

**REST endpoint for retrieval:** `GET /api/v1/conversations/:sha` — returns the conversation binary blob (decompressed). Authorization: the `ConversationRepository::store` method records `(sha, agent_id, workspace_id)` as metadata alongside the blob. The retrieval endpoint looks up `workspace_id` from this metadata and verifies the caller has workspace membership — no cross-repository join needed. The adapter stores conversations encrypted at rest; large conversations (>1MB) are stored as files on disk with the SHA as filename.

The agent runtime captures the conversation via a new MCP tool `conversation.upload` (addition to `platform-model.md` §4 tool table, scope: `agent`). The conversation is transmitted as a zstd-compressed binary blob, max **10MB** compressed (configurable, `GYRE_MAX_CONVERSATION_SIZE`). The server computes SHA-256 on receipt and stores the blob encrypted at rest. If the caller-provided `conversation_sha` (if any) doesn't match the computed SHA, the server rejects with 400. The upload is called by the agent runtime just before `agent.complete`. If it fails, completion still succeeds but the conversation is marked as unavailable. The MCP server validates that the uploading agent's `sub` claim matches the `agent_id` in the request.

```rust
pub struct ConversationProvenance {
    pub agent_id: Id,
    pub task_id: Id,
    pub conversation_sha: String,       // SHA-256 of the full conversation
    pub turn_index: Vec<TurnCommitLink>, // Maps conversation turns to commits
}

pub struct TurnCommitLink {
    pub turn_number: u32,               // Which conversation turn
    pub commit_sha: String,             // The commit produced during/after this turn
    pub files_changed: Vec<String>,     // Which files were modified
    pub timestamp: u64,
}
```

**How it works:**

1. The agent runtime includes a `X-Gyre-Conversation-Turn: <n>` header on every git push (extending the existing `X-Gyre-Model-Context` header from M13.2).
2. On push, the server reads the turn header and records a `TurnCommitLink`: "turn 7 of agent worker-12's conversation produced commit abc123 modifying `src/retry.rs`."
3. At completion, the agent runtime uploads the full conversation via `conversation.upload` MCP tool. The conversation is stored (encrypted at rest) and referenced by `conversation_sha` in the MR attestation.

The agent runtime (not the server) tracks the current conversation turn. It passes this to the server on every push via the header. The server records `TurnCommitLink` entries keyed by `(agent_id, turn_number, commit_sha)` — these are stored without a `conversation_sha` initially (it doesn't exist yet). When the conversation is uploaded at completion time, the server back-fills the `conversation_sha` on all `TurnCommitLink` records for that agent. This requires no server-side conversation state — the agent is the source of truth for its own turn counter.

**UI integration:**

In the Explorer, when viewing a specific code element (type, function, endpoint):
```
RetryPolicy (type)
  File: src/retry.rs:15-42
  Last modified: commit abc123 by worker-12
  Conversation turn: 7 of 23
  [View conversation at this point]
```

Clicking "View conversation at this point" opens the agent's conversation scrolled to turn 7, where the human can read the reasoning that led to this code. This is `git blame` extended to *reasoning blame*.

**Alternatively:** Clicking "Ask why" on this element spawns an interrogation agent loaded with the conversation up to turn 7, allowing interactive questioning.

---

## 6. Cross-Workspace Spec Dependencies

### The Problem

Specs live in repos. Repos belong to workspaces. But real systems have cross-workspace dependencies: Workspace A's payment-retry spec depends on Workspace B's idempotent-api spec. Today, spec links only work within a repo's manifest. Cross-workspace dependencies are invisible.

### Design

Spec links extend the existing `target` field from `spec-links.md` with a composite path format for cross-repo references:

```yaml
# In payment-api repo (Workspace: Payments)
specs:
  - path: system/payment-retry.md
    links:
      - type: depends_on
        target: "@platform-core/idempotent-service/system/idempotent-api.md"
        target_sha: "abc123..."    # SHA-pinned, per spec-links.md link format
        #        ^@workspace_slug/repo_name/spec_path
        # repo_name must be unique within a workspace (enforced by DB constraint)
```

The server resolves the composite path to a specific spec in a specific repo. The first segment is the **workspace slug** (URL-safe identifier, unique per tenant, as defined in `platform-model.md`'s Workspace struct). Same-repo links use just the spec path (existing behavior). Cross-repo same-workspace links use `repo_name/spec_path`. Cross-workspace links use the full `workspace_slug/repo_name/spec_path`.

Note: `spec-links.md` uses `{workspace}` in its format description without specifying whether this is name or slug. This spec clarifies: **always use slug** (unique, URL-safe). The server resolves slug → workspace ID internally.

**Path disambiguation:** To avoid ambiguity between same-repo spec paths and cross-repo references (e.g., a repo named `system` colliding with spec path `system/vision.md`), cross-workspace and cross-repo links use an explicit `@` prefix: `@workspace_slug/repo_name/spec_path` for cross-workspace, `@repo_name/spec_path` for cross-repo same-workspace. Paths without `@` are always same-repo. This is unambiguous and requires no heuristic resolution.

### What the System Does With Cross-Workspace Links

1. **Inbox notification:** The existing spec lifecycle system (`spec-lifecycle.md`) already detects spec file changes on push to the default branch. When a changed spec has inbound cross-workspace links (queried from the `spec_links` store), the server emits `SpecChanged` Event-tier messages into each dependent workspace and creates a `Notification` for the dependent workspace's Admin/Developer members: "idempotent-api.md changed in platform-core. Your payment-retry.md depends on it. Review impact."

2. **Briefing integration:** The Briefing surfaces cross-workspace activity: "platform-core updated idempotent-api.md. 3 specs in your workspace depend on it."

3. **Explorer visualization:** The workspace-level Explorer shows cross-workspace dependency edges as dashed lines crossing workspace boundaries.

4. **Orchestrator awareness:** Workspace orchestrators receive Event-tier messages when cross-linked specs change, enabling them to create coordination tasks automatically.

### Cross-Workspace Change Notification

When a spec with inbound cross-workspace links changes, the system notifies the dependent workspaces:

1. An Inbox item appears for the dependent workspace's human: "idempotent-api.md changed. Your payment-retry.md depends on it."
2. The workspace orchestrator receives an Event-tier `SpecChanged` message and can create coordination tasks.

**Merge blocking** for cross-workspace spec changes is handled by the existing spec approval flow (`agent-gates.md`), not by a new gate type. If the dependent workspace's spec policy requires `require_approved_spec`, and the upstream spec change invalidates the approval (per `spec-lifecycle.md` approval invalidation rules), the dependent MRs are blocked until the spec is re-approved. This leverages existing mechanisms rather than introducing a new `gate` field on spec links.

---

## 7. Multi-Human Collaboration

### Presence Awareness

Multiple humans may use Gyre simultaneously. The UI shows who else is active:

```
Workspace: Payments
  Active: jsell (Specs view), maria (Explorer), bot-deploy (Agent)
```

**`UserPresence` implementation:** Presence does NOT use the message bus `MessageKind` enum or tier system. It is a `WsMessage` variant used **bidirectionally**: clients send `UserPresence` to the server (heartbeat with current view), and the server rebroadcasts to other workspace subscribers. It is a WebSocket-only signal with its own handling path — the server receives it on the WebSocket, updates the in-memory presence map, and rebroadcasts to workspace subscribers. This avoids conflating presence with the message bus tier model (Telemetry tier's storage semantics don't fit presence).

The `WsMessage` enum gains a `UserPresence` variant (alongside `Subscribe`):
- **Payload schema:**
  | Field | Type | Required |
  |---|---|---|
  | `user_id` | Id | yes |
  | `workspace_id` | Id | yes |
  | `view` | String | yes — e.g., "inbox", "explorer", "specs" |
  | `timestamp` | u64 | yes — epoch ms |

`UserPresence` messages go through two paths:
1. **Real-time push:** through the broadcast channel to WebSocket subscribers (same as all Telemetry)
2. **Presence map:** the server maintains a lightweight in-memory map (`user_id → {workspace_id, view, timestamp}`) updated on every `UserPresence` message. This is the source of truth for "who is active" — queryable synchronously, survives TelemetryBuffer eviction.

`UserPresence` messages do NOT go into the `TelemetryBuffer` (not returned by `GET /activity`). They use a separate presence map.

**Presence query:** `GET /api/v1/workspaces/:workspace_id/presence` returns the current presence map for a workspace: `[{user_id, view, last_seen}]`. This is a simple read from the in-memory map, not a message bus query. ABAC resource resolution: `resource_type: "workspace"`, `workspace_param: "workspace_id"` — uses the existing workspace resource type, no new type needed. On WebSocket reconnection, the client fetches this endpoint to populate the initial presence state.

### Conflict Prevention

When two humans edit the same spec simultaneously:
1. The second editor sees a warning: "jsell is also editing this spec"
2. Edits are not merged automatically — the second save gets a conflict notification
3. The conflict appears in both users' Inboxes with a diff view

This is optimistic concurrency, not real-time co-editing (CRDT-based co-editing is future work). Specs are markdown in git — conflict resolution uses standard git merge semantics.

### Shared Views

Explorer views created by one human are visible to all workspace members. The saved view catalog is shared:

```
Saved Views:
  API Surface (created by jsell, 3 days ago)
  Domain Model (built-in)
  Auth Flow (created by maria, shared yesterday)
  [+ New View]
```

---

## 8. Inbox Detail

### Action Types (Priority Order)

| Priority | Action Type | Source | Inline Action |
|---|---|---|---|
| 1 | **Agent needs clarification** | Both in-flight and post-completion paths create the same `Notification` entity (via `NotificationRepository`) with `notification_type: AgentNeedsClarification`. In-flight: orchestrator creates it from `Escalation` message. Post-completion: `agent.complete` handler creates it from `AgentCompleted` uncertainties. Same schema, same query path, same Inbox rendering. | Respond inline or spawn interrogation |
| 2 | **Spec pending approval** | Spec registry | Approve / Reject (inline, read spec content) |
| 3 | **Gate failure** | Merge queue | View diff + output, Retry / Override / Close |
| 4 | **Cross-workspace spec change** | Spec link watcher | Review impact, Approve / Dismiss |
| 5 | **Conflicting spec interpretations** | Detected post-merge by the push-triggered graph extraction background job (M30b). After extraction completes, a **divergence check step** runs: it queries `ArchitecturalDelta` records for the merged MR's `spec_path`, comparing the latest delta against previous deltas from other agents for the same spec. A "conflicting node change" is defined as: two deltas for the same `spec_path` that add nodes with the same `name` but different `node_type`, different field sets (for types), or different method signatures (for interfaces). The comparison is on `(name, node_type, field_names_sorted)` tuples. If the symmetric difference of these tuples across the two deltas exceeds the threshold, the server creates a `Notification` (not a message bus event) for workspace Admin/Developer members. The comparison uses `ArchitecturalDelta.delta_json` — no pre-merge snapshot needed. Threshold: `GYRE_DIVERGENCE_THRESHOLD` (default 3 conflicting node changes). | Review both implementations, pick one or request reconciliation |
| 6 | **Meta-spec drift alert** | Reconciliation controller | Review results, adjust meta-spec |
| 7 | **Budget warning** | Budget enforcement | Increase limit / Pause work |
| 8 | **Trust level suggestion** | Track record analysis | Increase trust / Dismiss |
| 9 | **Spec assertion failure** | Knowledge graph + assertions | Fix code or update spec |
| 10 | **Suggested spec link** | Knowledge graph (low confidence) | Confirm / Dismiss linkage |

**Novel types not in any existing tool:**
- **Agent needs clarification** (#1) — the agent is stuck and explicitly says what it's uncertain about. This is the most important escalation.
- **Conflicting spec interpretations** (#5) — the system detects that two agents implemented the same spec differently and asks the human to arbitrate.
- **Trust level suggestion** (#8) — the system suggests the human can relax oversight based on track record.

Inbox filtering by trust level is defined in the Trust Level table (§2, "Inbox priorities shown" row) as the single source of truth.

---

## 9. Briefing Detail

### Structure

```
Since your last visit (14 hours ago):             [Workspace: Payments]

COMPLETED
  ✓ Payment retry logic (spec: payment-retry.md)
    3 MRs merged. All gates passed. Agent: worker-12.
    Decision: used exponential backoff (confidence: high)

  ✓ Persona reconciliation: backend-dev v3 → v4
    8 specs evaluated. 2 needed changes (merged). 6 compliant.

IN PROGRESS
  ◐ Auth refactor (spec: identity-security.md)
    3 of 5 sub-specs complete. 2 agents active.
    ⚠ worker-8 flagged uncertainty: "token refresh for offline clients not covered by spec"
    → [Respond to worker-8] [View spec]

CROSS-WORKSPACE
  ↔ platform-core updated idempotent-api.md
    Your payment-retry.md depends on it.
    → [Review changes] [Dismiss]

EXCEPTIONS
  ✗ Gate failure: billing-service MR #47
    cargo test failed (3 tests). Agent retried once, still failing.
    → [View Diff] [View Test Output] [Override] [Close MR]

METRICS
  12 MRs merged | 47 agent runs | $23.40 compute cost
  Budget: 67% of daily limit | Trust level: Guided
```

### Data Sources

| Section | Data Source |
|---|---|
| Completed | Spec registry + task rollup + agent completion summaries |
| In Progress | Task status + agent activity + completion summary uncertainties |
| Cross-Workspace | Cross-workspace spec link watcher |
| Exceptions | Gate results + spec assertion failures |
| Metrics | Budget usage + analytics |

### Briefing Q&A

Below the briefing, an inline chat grounded in the briefing data:

```
Ask about this briefing ▸ [                                        ]

Example: "Tell me more about the auth refactor"
         "Why did worker-12 choose exponential backoff?"
         "What changed in idempotent-api.md?"
```

The LLM answering this chat has read-only access to:
- The briefing data (specs, tasks, MRs, completion summaries, deltas)
- The knowledge graph (for structural context)
- The agent completion summaries (for decision reasoning)

It cannot modify anything or trigger actions.

---

## 10. Observable Lens (Future-Proofing)

The Observable lens (production telemetry overlaid on architecture) is not implemented in this milestone but the design must not preclude it.

### Design Constraints

1. **Graph nodes must support metadata extension.** The `GraphNode` struct should accept arbitrary key-value metadata so that production metrics (p99 latency, error rate, throughput) can be attached to Endpoint and Function nodes without schema changes.

2. **The Explorer's lens system must be pluggable.** Adding a new lens should not require modifying the Explorer core — it's a new data source that maps onto existing graph nodes.

3. **The Briefing should support external data sections.** A "Production Health" section can be added when production telemetry is available, using the same template/LLM-synthesized approach.

4. **The message bus should accept external event sources.** Production alerting systems should be able to push events into the bus as Event-tier messages with a `ProductionAlert` kind.

These are architectural constraints, not implementation work. They ensure we don't build ourselves into a corner.

---

## Relationship to Existing Specs

**Upstream Amendments Required** (these existing specs need updates to align with this spec):

| Spec | Amendment Needed |
|---|---|
| `system-explorer.md` §1 | `Cmd+K` → global search (not canvas-scoped). Canvas search uses `/`. Explorer "Sidebar" layout (Boundaries/Interfaces/Data/Specs subsections) becomes an in-view filter panel (200px, collapsible), not part of the app sidebar — update layout diagrams. §3 ghost overlays and structural prediction are deferred — the meta-specs preview loop (Editor Split in `ui-layout.md` §8) replaces inline ghost overlays with a dedicated preview workflow. |
| `hierarchy-enforcement.md` §4 | ABAC bypass must match by `subject.id == "gyre-system-token"`, not by `subject.type == "system"`. Internal services (merge processor) are `system` type but subject to ABAC. |
| `message-bus.md` `MessageKind` | Add `AgentCompleted` (Event tier, server-only, payload schema defined in §4 of this spec). |
| `abac-policy-engine.md` §"Resource attributes" | Add `explorer_view` (attributes: `workspace_id`, `created_by`), `message` (attributes: `workspace_id`, `to_agent_id`), and `conversation` (attributes: `workspace_id`, `agent_id`) to the resource type list. The `explorer-views/generate` endpoint uses `resource_type: "explorer_view"`. |
| `agent-gates.md` `MergeAttestation` | Add `conversation_sha: Option<String>` field. |
| `platform-model.md` §4 MCP tools | Add `conversation.upload` (scope: agent), `message.send` (scope: workspace), `message.poll` (scope: agent), `message.ack` (scope: agent) — per `message-bus.md` MCP tools section. |
| `platform-model.md` §9 UI Pages | Note that standalone entity views (Task Board, Agent List, etc.) are contextual drill-downs, not primary navigation. |
| `spec-links.md` §target format | Cross-repo/cross-workspace targets use `@` prefix for disambiguation. Clarify that `{workspace}` segment uses **slug** (not name). |
| `vision.md` §"Relationship to Other Specs" | Replace `ui-journeys.md` references with `human-system-interface.md` in the principles governance table. |
| `message-bus.md` `WsMessage` enum | Add `UserPresence` variant (bidirectional, payload: user_id, workspace_id, view, timestamp). |
| `platform-model.md` §1 `Workspace` struct | Add `trust_level: TrustLevel` field (enum: Supervised, Guided, Autonomous, Custom). |
| `hierarchy-enforcement.md` §4 built-in policies | Rename `system-access` → `system-full-access` (or vice versa). Add `immutable: bool` flag to `Policy` struct — immutable Deny policies evaluated before all others. `system-full-access` at priority 1000 must carve out spec approval. Define ABAC identity for internal server processes: each internal process (merge processor, stale agent detector, budget reset) creates an internal `SubjectContext` with `subject.type: "system"` and `subject.id: "<process-name>"` — no JWT needed, constructed in-process. Only the global `GYRE_AUTH_TOKEN` identity (`subject.id: "gyre-system-token"`) bypasses ABAC. |
| `platform-model.md` §1 Repository | Add unique constraint on `(workspace_id, name)` — repo names must be unique within a workspace for cross-workspace spec link resolution. |
| `message-bus.md` §Scoping Rules | Document that Users (not just Agents) can send Directed messages to agents — required for human→agent steering (Pause, inline chat). |
| `realized-model.md` `GraphNode` struct | Add `spec_confidence` field (already in DB schema but missing from Rust struct). Required by Explorer encoding layer. |
| `realized-model.md` API | Add workspace-scoped concept search endpoint (`GET /workspaces/:id/graph/concept/:name`) to avoid full-graph download for workspace-scoped queries. Without this, workspace concept search falls back to downloading `GET /workspaces/:id/graph` and filtering client-side. Also verify `GraphNode` struct has `test_coverage` field (present in struct but missing from DB schema per realized-model.md §8). |

These amendments are tracked here rather than applied inline because each upstream spec may have its own review cycle.

**Supersedes:**
- `ui-journeys.md` — this spec replaces it entirely with refined journeys, trust gradient, and communication model
- Sidebar navigation model in `docs/ui.md` — replaced by stable sidebar + adaptive content

**Extends (and partially supersedes):**
- `system-explorer.md` — adds progressive C4 drill-down, saved/generated views, three lenses. Supersedes `system-explorer.md`'s keyboard shortcuts (`Cmd+K` → global search, not canvas-scoped). The Explorer's left panel (Boundaries, Interfaces, Data, Specs subsections from `system-explorer.md`) is an **in-view panel inside the Explorer content area**, not part of the stable sidebar.
- `message-bus.md` — adds `UserPresence`, agent completion summaries, interrogation agent messages
- `platform-model.md` §3 — mechanizes cross-workspace spec escalation via spec links
- `abac-policy-engine.md` — trust level presets as ABAC policy bundles
- `agent-gates.md` — spec approval remains human-only regardless of trust level
- `spec-links.md` — cross-workspace spec link support

**Depends on:**
- `hierarchy-enforcement.md` — workspace scoping, ABAC middleware
- `message-bus.md` — all real-time communication flows through the bus
- `realized-model.md` — knowledge graph provides Explorer data
- `meta-spec-reconciliation.md` — preview loop for meta-spec editing
- `spec-registry.md` — spec approval flow and ledger

**New concepts introduced:**
- Trust gradient (workspace-level, backed by ABAC policies)
- Agent completion summaries (structured decision log)
- Interrogation agents (restricted ABAC, post-hoc reasoning exploration)
- Conversation-to-code provenance (turn-to-commit linking)
- Cross-workspace spec links (with optional approval gates)
- LLM-generated Explorer views (question → graph query → visualization)
- Scoped inline chat (per-agent, per-orchestrator, per-briefing)
- Presence awareness (multi-human collaboration)
