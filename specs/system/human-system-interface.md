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
| **Inbox** | Action queue across all workspaces | Action queue for this workspace | Action queue for this repo (filtered by `repo_id` on notifications). Workspace-scoped notifications with `repo_id: NULL` (e.g., trust suggestions, meta-spec drift) are only visible at workspace scope, not repo scope. |
| **Briefing** | Narrative across all workspaces (client-side aggregation: calls `GET /workspaces/:id/briefing` per workspace, merges sections) | Narrative for this workspace | Narrative for this repo |
| **Explorer** | At repo scope, the Explorer has two tabs in its control bar: **Architecture** (default — C4 graph) and **Code** (branches, commits, MRs, merge queue). The Code tab is part of the Explorer, not a separate nav item. At other scopes: Workspace cards with summary stats. This is a **card grid**, not a graph canvas — click a workspace card to enter the graph-based Explorer. Data sourced from `GET /api/v1/workspaces` (list) + `GET /api/v1/workspaces/:id/budget` (usage stats) — no new endpoint needed. Repo count and active agent count derived from existing list endpoints with workspace filter. | Realized architecture (C4 progressive drill-down per `system-explorer.md`) | Repo-level architecture detail |
| **Specs** | Spec registry across all workspaces | Specs across repos in workspace | Specs in this repo + implementation progress |
| **Meta-specs** | Persona/principle/standard catalog | Persona editor, preview loop, reconciliation progress | (redirects to workspace scope) |
| **Admin** | Users, compute, tenant budget, audit, **workspace creation** (+ New Workspace button) | Workspace settings, budget, trust level, teams, **Policies** (ABAC editor for Custom trust) | Repo settings, gates, policies |

**Meta-specs at workspace scope** is the primary location for the preview loop from `meta-spec-reconciliation.md`: edit a persona → select target specs → preview agents implement on throwaway branches → view diff → iterate → publish. Reconciliation progress tracking also lives here. At tenant scope, Meta-specs shows a catalog of all personas/principles/standards across workspaces. At repo scope, it redirects to the workspace scope (meta-specs are workspace-scoped, not repo-scoped).

**Workspace discovery:** The `GET /api/v1/workspaces` endpoint returns workspaces where the user is a member (filtered by `workspace-membership-required` ABAC policy). For workspace discovery (joining new workspaces), the tenant Admin view includes a "Manage Workspaces" section showing all workspaces in the tenant — Admin role bypasses the membership-required policy via `admin-all-operations`.

**Where old views live:** Task Board, Merge Queue, Agent List, MR Detail, Repo Detail, Persona Management, Activity Feed, and other entity views from `platform-model.md` are **contextual drill-downs**. The Activity Feed is accessible via the Explorer's Change View or Admin's Audit tab. **Agent discovery:** At workspace scope, the Explorer's Boundary View shows active agent count per repo on each node card. Clicking the agent count opens a filtered agent list in the detail panel. This ensures agents are always discoverable even when no Inbox items or Briefing sections reference them. Other entity views — accessed by clicking an entity reference anywhere in the UI (agent name → slide-in panel, MR link → detail view, etc.). They are not primary navigation items. The Code tab (branches, commits, MRs, merge queue) is accessed via the Explorer at repo scope, not as a separate nav item.

The content adapts. The sidebar doesn't.

A **status bar** at the bottom of the application shows trust level, budget usage, WebSocket status, and presence avatars for the current workspace. See `ui-layout.md` §1 for dimensions and layout. Presence updates are sent **immediately after WebSocket connection is established** (so the user appears present to others without waiting for the first timer tick), then on **both** a 30-second timer AND on view changes (sidebar nav click or scope transition), debounced to at most one update per 5 seconds. **Graceful disconnect:** The client sends a `UserPresence` with `view: "disconnected"` on `beforeunload` (browser tab close) so the server can evict the session immediately without waiting for the 60-second timeout. The server evicts entries after 60 seconds without an update. **Multi-tab:** Each browser tab opens its own WebSocket connection (connections are NOT shared across tabs). The presence map is keyed by `(user_id, session_id)` where `session_id` is a random UUID generated per browser tab. The server maps `session_id` to the specific WebSocket connection via the `Subscribe` message, enabling targeted `PresenceEvicted` delivery to the correct tab. The server caps at 5 sessions per user (oldest evicted first) to prevent flooding. Evicted sessions receive a `{"type": "PresenceEvicted", "session_id": "<evicted-uuid>"}` WebSocket message — the client checks if the session_id matches its own tab and stops heartbeating only for that tab. The server maps `session_id` to WebSocket connections by including `session_id` in the initial `Subscribe` message (amending `message-bus.md`'s `Subscribe` payload with a required `session_id` field — required for user connections that send `UserPresence`, optional for agent connections that don't use presence). Multiple tabs show the user as present multiple times. The UI collapses these into a single avatar with a badge count if the same user appears in multiple views.

### Deep Links

**Entrypoint:** First visit lands on Explorer at tenant scope (workspace cards). After workspace selection, redirects to Inbox at workspace scope — the default landing view. Subsequent visits restore the last-used workspace and land on the Inbox. See `ui-layout.md` §1 for full entrypoint flow.

**Last-seen tracking:** The server records `last_seen_at: u64` (epoch seconds) per user per workspace, updated on every authenticated request scoped to that workspace. **Mechanism:** an Axum middleware layer (placed after auth, before handlers) extracts `user_id` from the auth context and `workspace_id` from the route params or resolved entity. It fires an async upsert to `user_workspace_state` (debounced to at most once per 60 seconds per user-workspace pair to avoid write amplification). The Briefing's "since your last visit" default uses this timestamp. Stored in a new `user_workspace_state` table — per-workspace tracking requires its own table:
```sql
CREATE TABLE user_workspace_state (
    user_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,  -- globally unique, provides structural tenant isolation
    last_seen_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, workspace_id)
);
-- No tenant_id column: this table is never exposed via REST endpoint — it is
-- only accessed internally by the last_seen_at middleware and the briefing handler.
-- Since there is no API surface, there is no UUID-guessing attack vector, and
-- workspace_id global uniqueness provides structural tenant isolation.
-- This differs from the notifications table (which HAS tenant_id) because
-- notifications have REST endpoints where per-handler auth must verify tenant
-- ownership to prevent cross-tenant UUID guessing.
-- Added to check-tenant-filter.sh skip list alongside MessageRepository
```

The Briefing time range dropdown options: `Since last visit` (default), `Last 24h`, `Last 7d`, `Last 30d`, `Custom range`. The "Since last visit" option calls the briefing endpoint with no `?since=` parameter — the server uses the stored `last_seen_at` as the default when `since` is omitted. If no `user_workspace_state` row exists (first visit), the server falls back to 24 hours ago. Other options pass `?since=<epoch_seconds>` (epoch seconds, consistent with `last_seen_at` storage and domain entity timestamps per `hierarchy-enforcement.md` §1). No separate endpoint needed to read `last_seen_at` — the server handles it internally.

Every view state is URL-addressable:
- `/inbox` — tenant-scoped inbox
- `/workspaces/:id/inbox` — workspace-scoped inbox
- `/repos/:id/explorer` — repo-scoped explorer
- `/repos/:id/specs?path=system/vision.md` — specific spec in a repo (path as query param for clean URL structure; note: UI routes are not bound by `api-conventions.md` §4 which governs API endpoints, but the query param pattern is cleaner regardless)
- `/repos/:id/explorer?filters=Boundaries,Interfaces&lens=evaluative` — Explorer with active filter panel categories and lens as query params (filter panel state is URL-addressable for sharing)

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
- **Preset → Custom:** Server preserves the current preset's `trust:` policies as the starting point (they keep their `trust:` prefix so the user can see which were preset-managed). The user can then rename, edit, or delete them via the policy editor. Renaming a `trust:` policy to remove the prefix converts it to a user-created policy that is preserved on future transitions.
- **Custom → Preset:** Server deletes all `trust:` prefixed policies, then creates the preset's policies. Built-in policies (`builtin:` prefix) and user-created policies (no prefix) are preserved.

All trust level transitions (workspace `trust_level` field update AND policy delete+create) are performed in a **single database transaction** — if creating the new policies fails, the workspace field is not updated either. On rollback, the endpoint returns `409 Conflict` with `{"error": "Trust level transition failed — policies could not be created"}` so the UI can display an actionable error. The ABAC policy cache (`abac-policy-engine.md` §Performance) is invalidated **after** the transaction commits — not on each individual policy write — to prevent intermediate states from being visible to concurrent requests.

**Policy naming conventions and priorities:**
- `trust:` prefix — trust-preset-managed, deleted and recreated on trust level transitions. **Priority: 100-199** (below user-created policies, so user-created Allow policies can override trust Deny policies when intentional). **Reserved prefix:** the ABAC policy CRUD endpoint rejects user creation of policies with `trust:` or `builtin:` prefixes (400 error).
- `builtin:` prefix — immutable server-seeded policies, never deleted by trust transitions. Priority: per built-in policy table.
- No prefix — user-created custom policies, preserved across transitions. **Priority: 200-299** (above trust presets — user intent takes precedence over preset defaults). Note: user-created policies cannot override built-in policies (400-1000) because built-in policies have higher priority. This is intentional — built-ins enforce system invariants (tenant isolation, workspace membership, default deny) that should not be overridable by workspace-level custom policies.

### What Each Level Controls

| Aspect | Supervised | Guided | Autonomous |
|---|---|---|---|
| **MR merge** | Human approval required | Autonomous if all gates pass | Autonomous if all gates pass |
| **Spec approval** | Human approval required | Human approval required | Human approval required (always) |
| **Inbox shows** | Priorities 1-10 (everything in the priority table) | Priorities 1-9 (per row below) | Priorities 1-9 (per row below) |
| **Briefing detail** | Per-agent activity, per-MR status | Per-spec progress, exceptions | Spec-level summaries, exceptions only |
| **Notifications** | Every state change | Failures and approvals | Exceptions only |
| **Agent completion summaries** | Full decision log visible | Uncertainties highlighted | Only low-confidence decisions surfaced |
| **Inbox priorities shown** | 1-10 (everything) | 1-9 (excludes suggested links) | 1-9 (excludes suggested links) |

**Custom trust Inbox:** defaults to 1-10 (everything). The priority range is not separately configurable — Custom trust always shows all priorities. The differentiation is in the ABAC policies, not the Inbox filter.

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

**Guided** policy set (explicit):
```yaml
# No trust: policies created — Guided relies on built-in policies only.
# builtin:require-human-spec-approval (immutable, always present) handles spec approval.
# The merge processor is NOT blocked — no trust:require-human-mr-review policy exists.
# Gate failures surface in the Inbox via the default notification system.
# This is intentionally an empty trust: policy set — the delta from Supervised
# is the REMOVAL of trust:require-human-mr-review.
```

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

**Bootstrap exception:** The `gyre bootstrap` command seeds initial specs as pre-approved (it writes directly to the spec approval ledger with status `Approved`, bypassing the approval action entirely). This is not an ABAC `approve` action — it's database seeding, which runs before the ABAC engine is active. Post-bootstrap, all spec approvals require a human.

**Priority and override behavior:** The `require-human-spec-approval` policy uses `priority: 999` and is marked as `immutable: true` (a new boolean flag on `Policy` — requires amending `abac-policy-engine.md`). Immutable policies cannot be overridden regardless of priority — the ABAC evaluation engine processes immutable Deny policies FIRST, before any priority-based evaluation. This amends `abac-policy-engine.md` §"Policy Composition" rule 2 ("higher priority always wins") by adding a precondition: "immutable Deny policies are evaluated before all others and cannot be overridden by any Allow policy regardless of priority." The `system-full-access` policy operates at `priority: 1000` and applies only to `subject.id == "gyre-system-token"`. To carve out spec approval, a **separate** immutable deny policy is used (the `builtin:require-human-spec-approval` defined above at priority 999). Since immutable Deny policies are evaluated first (before any priority-based Allow), the `system-full-access` Allow at priority 1000 cannot override the immutable Deny at priority 999. No `EXCEPT` syntax needed — the carve-out is achieved through policy composition (two separate policies), not a single policy with exclusion syntax.

The Custom trust editor grays out immutable policies with tooltip: "This policy cannot be removed or overridden." Per `platform-model.md` §2, agents cannot approve specs that define their own behavior.

Budget warnings (priority 7 in the Inbox) remain visible at Autonomous trust because `platform-model.md` §5 defines budget exhaustion as requiring human action.

**Custom:** opens the ABAC policy editor within the **Admin** view at workspace scope (a new "Policies" tab alongside "Workspace settings, budget, trust level, teams"). The editor queries `GET /api/v1/policies?scope=Workspace&scope_id=<workspace_id>` to list policies for this workspace. The editor uses the Full-Width layout (per `ui-layout.md` §2) with three sections: (1) a policy list table grouped by prefix (`builtin:` / `trust:` / user-created), (2) a visual condition builder panel (form-based, not raw YAML) for creating/editing policies, (3) a dry-run simulator panel where users can test "would this request be allowed?" against the current policy set. Immutable policies are grayed out with tooltip. Layout details follow the standard Full-Width pattern — no custom layout needed.

### Trust Suggestions

The system can suggest increasing trust based on track record:

```
This workspace has had 0 gate failures and 0 reverted MRs in 30 days.
Consider increasing trust level to Autonomous.
[Increase Trust] [Dismiss]
```

This appears as an Inbox item (priority 8). The human decides.

**Mechanism:** A background job (`trust_suggestion_check`) runs daily per workspace. It queries gate results (from the gate evaluation records) and MR reverts (MRs with status `Reverted` — amend `platform-model.md` §6 to formally define `Reverted` as a variant in the MR status enum, set on the original MR when a revert commit is created by the recovery protocol. Manual revert commits are detected by the push hook checking `git log --grep="Revert"` on the default branch and matching the reverted commit to its source MR.) for the last 30 days. If both counts are 0 and the current trust level is not already Autonomous, and the user has not dismissed this suggestion in the last 30 days, it creates one `Notification` per workspace Admin member (each admin gets their own notification and can dismiss independently). The job is registered in the server's `JobRegistry` alongside existing jobs.

**Notification entity:** Notifications used throughout this spec (Inbox items) share a common schema:
```sql
CREATE TABLE notifications (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    user_id TEXT NOT NULL,          -- recipient
    notification_type TEXT NOT NULL, -- constrained to NotificationType enum values (see below)
    priority INTEGER NOT NULL,      -- 1-10 per §8 table
    title TEXT NOT NULL,
    body TEXT,                       -- JSON payload with type-specific data
    entity_ref TEXT,                 -- optional reference (spec_path, agent_id, mr_id)
    repo_id TEXT,                    -- optional, for repo-scope Inbox filtering
                                     -- populated by notification creators from the entity's owning repo
                                     -- (e.g., agent's scoped repo, MR's repository_id, spec's repo)
    resolved_at INTEGER,            -- epoch seconds, NULL if unresolved
    dismissed_at INTEGER,           -- epoch seconds, NULL if not dismissed
    created_at INTEGER NOT NULL,
    tenant_id TEXT NOT NULL
);
CREATE INDEX idx_notifications_user_ws ON notifications (user_id, workspace_id, resolved_at);
```
**NotificationType enum** (in `gyre-common`):
```rust
pub enum NotificationType {
    AgentNeedsClarification,    // priority 1
    SpecPendingApproval,        // priority 2
    GateFailure,                // priority 3
    CrossWorkspaceSpecChange,   // priority 4
    ConflictingInterpretations, // priority 5
    MetaSpecDrift,              // priority 6
    BudgetWarning,              // priority 7
    TrustSuggestion,            // priority 8
    SpecAssertionFailure,       // priority 9
    SuggestedSpecLink,          // priority 10
}
```

The `dismissed_at` field tracks user dismissals (used by trust suggestions to suppress re-creation for 30 days). The `resolved_at` field tracks resolution (action taken). The Inbox badge count is the count of notifications where `resolved_at IS NULL AND dismissed_at IS NULL`.

**Crate placement:** The `Notification` struct lives in `gyre-common` (shared wire type, like `Message` and `Id`). `NotificationRepository` lives in `gyre-ports`.

**Port trait** (in `gyre-ports`):
```rust
#[async_trait]
pub trait NotificationRepository: Send + Sync {
    async fn create(&self, notification: &Notification) -> Result<()>;
    async fn get(&self, id: &Id, user_id: &Id) -> Result<Option<Notification>>;
    async fn list_for_user(&self, user_id: &Id, workspace_id: Option<&Id>, min_priority: Option<u8>, max_priority: Option<u8>, limit: u32, offset: u32) -> Result<Vec<Notification>>;
    async fn dismiss(&self, id: &Id, user_id: &Id) -> Result<()>;
    async fn resolve(&self, id: &Id, user_id: &Id, action_taken: Option<&str>) -> Result<()>;
    async fn count_unresolved(&self, user_id: &Id, workspace_id: Option<&Id>) -> Result<u64>;
    // workspace_id=None counts across all workspaces (tenant-scope Inbox badge)
    async fn has_recent_dismissal(&self, workspace_id: &Id, user_id: &Id, notification_type: &str, days: u32) -> Result<bool>;
    // Used by trust_suggestion_check job: per-user check — each admin independently decides to dismiss
}
```

**Notification endpoints:**
- `GET /api/v1/users/me/notifications?workspace_id=&min_priority=&max_priority=` — list notifications for the authenticated user. When `workspace_id` is omitted, returns notifications across all workspaces the user belongs to (tenant-scope Inbox). No ABAC resource type needed — `/users/me/*` endpoints are implicitly scoped to the authenticated user's identity.
- `POST /api/v1/notifications/:id/dismiss` — set `dismissed_at` to now. Response: 204. (POST per `api-conventions.md` §2.2 — action endpoints are always POST.)
- `POST /api/v1/notifications/:id/resolve` — set `resolved_at` to now. Request body: `{action_taken: "approved"}` (optional, for audit). Response: 204.

All notification endpoints (`GET /users/me/notifications`, `POST /notifications/:id/dismiss`, `POST /notifications/:id/resolve`) use per-handler auth (the handler verifies the notification belongs to the authenticated user AND the user's tenant matches the notification's `tenant_id` — prevents cross-tenant manipulation via UUID guessing). These are added to the ABAC-exempt endpoint list in `hierarchy-enforcement.md` §4 alongside git HTTP and conversations.

---

## 3. The Explorer: Progressive Architecture Discovery

### The Zero-Knowledge Problem

The human didn't write the code. They can't reason about it from memory. Traditional tools assume familiarity — file trees, grep, code search. These are useless when you have zero knowledge of what exists.

The Explorer solves this with **progressive disclosure starting from boundaries** — the same technique architects use when onboarding onto a new system.

### Design Principle: The Built Software Is Always Alive

The Explorer shows the architecture of the software being built — not Gyre's own internals. The knowledge graph is static structure. But structure without behavior is dead. The human needs to see data flowing through the architecture to understand it — not just what the code *is*, but what it *does*.

The spreadsheet insight: Google Sheets is the best IDE because the program is always running. Change an input, see the output. The Explorer should be the same — you see the built software's architecture, and you see test data flowing through it. Edit a spec, see the ghost overlay predict structural changes. View an endpoint, see the actual request/response from the last gate run.

**Two layers of liveness on the canvas:**

**Layer 1 — SDLC activity (Gyre's work):** Ambient indicators of what agents are building:
- **Active agents:** nodes where agents are working pulse with a subtle glow (sourced from `RunStarted`/`RunFinished` Telemetry-tier messages).
- **Recent changes:** nodes modified in the last hour have a fading highlight ring (sourced from `ArchitecturalDelta` records).
- **Gate status:** edges show gate pass/fail as color (green/red) when the Evaluative lens is active.

**Layer 2 — Test-time traces (the built software's behavior):** Animated data flow showing real requests flowing through the architecture, captured during gate execution. This is the "always alive" layer — the software was alive during its last gate run, and you can see that aliveness replayed on the graph. See §3a (Test-Time Trace Capture) for the full design.

### Test-Time Trace Capture

#### The Problem

The knowledge graph shows what the software *is* — types, endpoints, dependencies. But it doesn't show what the software *does*. When an agent builds a payment retry endpoint, the Explorer shows the endpoint node, its connections, and its spec linkage. What it doesn't show: "when a retry request arrives, it calls the idempotency check, then the payment gateway, then records the result in the ledger." That data flow *through* the architecture is what makes the system legible.

Production observability (the Observable lens, §10) solves this eventually, but it requires a deployed system. **Test-time trace capture** solves it now — during gate execution, the test suite already exercises the software. We capture those execution traces and map them to the knowledge graph.

#### Design

**Gate-time OTel instrumentation:** A new gate type (`TraceCapture`) instruments the integration test run with OpenTelemetry. The gate runner starts an OTLP collector that receives spans from the application under test. Each span captures:
- The operation (HTTP request, function call, DB query)
- Input/output data (request body, response body, query params)
- Timing (start, duration)
- The call chain (parent span → child span)

**Gyre's internal OTLP receiver:** The forge includes a lightweight OTLP receiver (gRPC, per the OpenTelemetry Protocol specification) that ingests spans from gate runs. This is not a general-purpose observability backend — it is scoped to gate-time traces only, stored alongside gate results, and linked to a specific MR and commit SHA.

```rust
pub struct GateTrace {
    pub mr_id: Id,
    pub gate_run_id: Id,
    pub commit_sha: String,
    pub spans: Vec<TraceSpan>,
    pub captured_at: u64,
}

pub struct TraceSpan {
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub operation_name: String,        // e.g., "POST /payments/retry"
    pub service_name: String,          // e.g., "payment-api"
    pub kind: SpanKind,                // Server, Client, Internal
    pub start_time: u64,               // epoch microseconds
    pub duration_us: u64,
    pub attributes: HashMap<String, String>,  // http.method, http.url, db.statement, etc.
    pub input_summary: Option<String>,  // truncated request body
    pub output_summary: Option<String>, // truncated response body
    pub status: SpanStatus,            // Ok, Error
    pub graph_node_id: Option<Id>,     // linked to knowledge graph node (resolved post-capture)
}
```

**Graph node linkage:** After trace capture, the server maps spans to knowledge graph nodes:
- HTTP spans → `Endpoint` nodes (matched by path pattern)
- Function spans → `Function` nodes (matched by `qualified_name`)
- DB spans → adapter nodes (matched by module path)

This linkage is heuristic (name matching) and may not resolve every span. Unresolved spans are still stored and visible in the trace timeline but don't animate on the graph.

**Storage:** Traces are stored per-MR, capped at the most recent gate run per MR. Old traces are evicted when the MR merges (the merged trace is preserved on the `MergeAttestation` for provenance). This bounds storage: at most one trace per open MR.

**Input/output truncation:** `input_summary` and `output_summary` are truncated to 4KB each. The full payloads are stored as a separate blob (zstd-compressed, max 1MB per trace) retrievable via `GET /api/v1/merge-requests/:id/trace/spans/:span_id/payload`. This keeps the trace timeline lightweight while allowing drill-down into full payloads.

**Crate placement:** `GateTrace` and `TraceSpan` live in `gyre-common` (shared wire types). `TraceRepository` lives in `gyre-ports`. The OTLP receiver lives in `gyre-server` (it's an ingestion endpoint, not domain logic).

**Port trait** (in `gyre-ports`):
```rust
#[async_trait]
pub trait TraceRepository: Send + Sync {
    /// Store a gate trace (replaces any existing trace for the same MR).
    async fn store(&self, trace: &GateTrace) -> Result<()>;
    /// Get the most recent trace for an MR.
    async fn get_by_mr(&self, mr_id: &Id) -> Result<Option<GateTrace>>;
    /// Get a specific span's full payload (input/output bodies).
    async fn get_span_payload(&self, gate_run_id: &Id, span_id: &str) -> Result<Option<SpanPayload>>;
    /// Promote a trace to permanent storage (called on MR merge for attestation).
    async fn promote_to_attestation(&self, mr_id: &Id) -> Result<()>;
    /// Delete traces for an MR (called on MR close without merge).
    async fn delete_by_mr(&self, mr_id: &Id) -> Result<()>;
}

pub struct SpanPayload {
    pub input: Option<Vec<u8>>,   // full request body
    pub output: Option<Vec<u8>>,  // full response body
}
```

**Gate configuration:** The `TraceCapture` gate is configured per-repo in the gate manifest:
```yaml
gates:
  - name: trace-capture
    type: TraceCapture
    config:
      # The gate runner starts an OTLP receiver on this port before running tests
      otlp_port: 4317              # gRPC OTLP receiver port (default)
      # Env var injected into the test process so the app sends spans to the collector
      env:
        OTEL_EXPORTER_OTLP_ENDPOINT: "http://localhost:4317"
        OTEL_SERVICE_NAME: "{{repo_name}}"
      # Which test command to instrument (runs with OTel env vars set)
      test_command: "cargo test --features integration"
      # Max spans per trace (prevents unbounded storage from fuzz tests)
      max_spans: 10000
      # Whether to capture external dependency spans (requires real network access)
      capture_external: false      # default: false (use mocks)
```

The gate runner lifecycle:
1. Start OTLP gRPC receiver on `otlp_port`
2. Run `test_command` with the OTel env vars injected
3. Collect all received spans
4. Stop the receiver
5. Resolve span-to-graph-node linkage (post-capture)
6. Store the `GateTrace` via `TraceRepository::store`
7. Report gate pass/fail (the trace gate itself always passes — trace capture is observational, not a quality gate)

**OTLP receiver configuration** (server-level, `docs/server-config.md`):
```
GYRE_OTLP_ENABLED=true           # Enable the OTLP receiver (default: true)
GYRE_OTLP_GRPC_PORT=4317         # gRPC OTLP receiver port (default: 4317)
GYRE_OTLP_MAX_SPANS_PER_TRACE=10000  # Safety cap (default: 10000)
```

**REST endpoints:**

`GET /api/v1/merge-requests/:id/trace` — returns the `GateTrace` for an MR. ABAC: `RouteResourceMapping` with `resource_type: "mr"`, `id_param: "id"`, `action: "read"`. Response (200):
```json
{
  "mr_id": "...",
  "gate_run_id": "...",
  "commit_sha": "...",
  "captured_at": 1711324800,
  "span_count": 47,
  "root_spans": ["span-001", "span-012"],
  "spans": [
    {
      "span_id": "span-001",
      "parent_span_id": null,
      "operation_name": "POST /payments/retry",
      "service_name": "payment-api",
      "kind": "Server",
      "start_time": 1711324800000000,
      "duration_us": 300000,
      "attributes": {"http.method": "POST", "http.status_code": "200"},
      "input_summary": "{\"payment_id\": \"pay_123\", \"attempt\": 2}",
      "output_summary": "{\"status\": \"success\", \"retry_id\": \"ret_456\"}",
      "status": "Ok",
      "graph_node_id": "node-endpoint-retry"
    }
  ]
}
```
If no trace exists: 404. The `root_spans` array identifies top-level spans (entry points) for the flow animation.

`GET /api/v1/merge-requests/:id/trace/spans/:span_id/payload` — returns the full input/output for a specific span. ABAC: same as parent. Response (200): `{input: "<base64>", output: "<base64>"}`. 404 if the span has no stored payload.

#### Explorer Visualization: Animated Data Flow

**Prior art:** Netflix Vizceral (animated particles flowing through a service graph), Kiali (Istio traffic flow), Jaeger (trace timeline with time scrubbing).

The novel combination: **Vizceral-style animated particles mapped to a knowledge graph (not a service mesh), driven by test-time OTel traces (not production traffic), with Jaeger-style time scrubbing.**

When viewing an MR's trace in the Explorer:

```
┌──────────────────────────────────────────────────────────┐
│                                                          │
│   [payment-api]                                          │
│       │                                                  │
│       ●──→ POST /payments/retry                          │
│       │         │                                        │
│       │         ●──→ check_idempotency()                 │
│       │         │         │                              │
│       │         │         ●──→ SELECT * FROM payments    │
│       │         │                                        │
│       │         ●──→ PaymentGateway::charge()            │
│       │         │         │                              │
│       │         │         ●──→ [mock: stripe-rs]         │
│       │         │              {amount: 2500, ok: true}  │
│       │         │                                        │
│       │         ●──→ RecordResult::save()                │
│       │                                                  │
│  ─────┼──────────────────────────────────────────── time  │
│  0ms  50ms    120ms   180ms    250ms    300ms            │
│  [▶ Play] [⏸ Pause] [Speed: 1x ▾] [Scrub: ━━━●━━━━]   │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

- **Particles:** each test request is an animated dot that flows through the graph edges, following the span tree. Multiple test cases can play simultaneously with different colored particles.
- **Time scrubbing:** drag the scrubber to any point in the trace. The graph freezes at that moment. Click a span to see the input/output data at that point.
- **Speed control:** play at 1x (real-time relative to the trace), 10x, or step-through (span by span).
- **External deps:** mocked/stubbed dependencies show the mock response inline (e.g., `{amount: 2500, ok: true}` from the Stripe mock).
- **Hover:** hovering on any span during playback shows the input/output summary in a tooltip.

**Access:** The animated trace view is available via:
- The "Trace" tab on MR detail panels (alongside Diff, Gates, Attestation)
- The Explorer's Evaluative lens — when active, nodes that have trace data show a ▶ play icon. Click to start the animation.
- `GET /api/v1/merge-requests/:id/trace` REST endpoint (returns the `GateTrace` struct as JSON for CLI/MCP consumption per §11)

#### External Dependencies at Scale

External dependencies are mocked/stubbed during gate execution (standard integration test practice). The trace captures the mock interaction, not real external I/O. This is sufficient for understanding data flow — the human sees "this endpoint calls Stripe and expects a charge response" even though the actual Stripe API wasn't hit.

For workspaces that deploy to staging environments and run smoke tests against real deps, the `TraceCapture` gate can be configured to capture real external spans too. This is opt-in per workspace via gate configuration, not a default.

#### Relationship to Observable Lens (§10)

The Observable lens is about **production** telemetry — SLIs, error rates, latency from real traffic. Test-time traces are about **understanding** — seeing what the software does during its verified-correct execution. They serve different purposes:

| | Test-Time Traces (this section) | Observable Lens (§10, future) |
|---|---|---|
| **When** | Gate execution (pre-merge) | Production (post-deploy) |
| **Data** | Test requests, mock responses | Real user traffic, real deps |
| **Purpose** | Understanding what the code does | Monitoring how it performs |
| **Volume** | Bounded (test cases per MR) | Unbounded (production traffic) |
| **Storage** | Per-MR, evicted on merge | Time-series DB (external) |

**Architecture constraint:** The OTLP receiver, span-to-graph-node linkage, and animated flow visualization are designed to support **both** test-time and production traces. The same pipeline ingests spans regardless of source. The same Explorer visualization renders them. When the Observable lens ships, production traces flow through the identical path — the only difference is the volume (requiring sampling/aggregation for production) and the data source (real vs. mock external deps). Build once, use for both.

### Default Views (Automatic, No LLM)

#### Boundary View (C4 Progressive Drill-Down)

The default Explorer view. Each level answers "what is this made of?"

**Level 1 — Workspace scope (C4 "Container" level):** Repos and their external dependencies. We start at C4 Container (repos are containers within the workspace system) and skip C4 Context (the workspace boundary IS the context — the user already selected it).
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

What changed since last visit, at the structural level. Powered by architectural deltas. The "since last visit" timestamp is obtained from the `last_seen_at` middleware's response header `X-Gyre-Last-Seen: <epoch_seconds>` (set on every workspace-scoped response). The client caches this value and passes it to `GET /repos/:id/graph/timeline?since=<last_seen_at>` for the Change View query. This header is also the source for the Briefing's `since` field, ensuring consistency without requiring a separate endpoint or Briefing dependency. Note: the `graph/timeline` endpoint uses epoch seconds (consistent with domain entity timestamps), not epoch milliseconds (which is used only by the message bus).

```
Since your last visit:

  + Added: RetryPolicy (type in payment-domain)
    Spec: payment-retry.md | Agent: worker-12

  ~ Modified: ChargeService (3 new methods)
    Spec: charge-processing.md | Agent: worker-8

  - Removed: LegacyPaymentHandler
    Spec: (none — cleanup task)
```

#### System Trace View

What actually happened — the dynamic counterpart to the static architecture views. Inspired by trace-driven development: understanding comes from seeing real behavior through real structure, not from seeing structure alone.

```
MR #52: Payment retry endpoint
  Author: worker-12 | Spec: payment-retry.md

  TIMELINE
  ├─ 10:14  Spec lifecycle: task auto-created (TASK-42)
  ├─ 10:15  Orchestrator: assigned to worker-12
  ├─ 10:15  Agent spawned (persona: backend-dev v4, JWT: 2h)
  ├─ 10:16  Turn 1: Read spec, planned approach
  ├─ 10:18  Turn 3: Created RetryPolicy type
  ├─ 10:22  Turn 7: git push (3 files changed) → gates triggered
  │         ├─ cargo test: PASS (142 tests, 0.8s)
  │         ├─ cargo clippy: PASS
  │         └─ spec-binding: PASS (payment-retry.md@abc123)
  ├─ 10:23  Graph extraction: +RetryPolicy, +should_retry, ~PaymentPort
  ├─ 10:24  Turn 8: Completion summary (2 decisions, 1 uncertainty)
  ├─ 10:24  MR enqueued → merge queue position #3
  ├─ 10:25  Speculative merge: clean
  ├─ 10:26  Merged to main
  └─ 10:26  Post-merge gate: PASS

  [View Conversation] [View Diff] [View Attestation]
```

Each row links to the underlying data: clicking a gate result opens the gate detail, clicking a turn opens the conversation at that point, clicking the graph extraction shows the architectural delta. The trace is assembled from existing data — no new storage needed:

| Trace Event | Data Source |
|---|---|
| Spec lifecycle triggers | `SpecChanged` Event-tier messages |
| Agent spawn/complete | `AgentCreated`/`AgentCompleted` messages |
| Conversation turns | `TurnCommitLink` records (§5) |
| Gate execution | Gate result records (`agent-gates.md`) |
| Graph extraction | `ArchitecturalDelta` records (`realized-model.md`) |
| Merge queue events | `QueueUpdated` messages |
| Notifications created | `Notification` records |

**Access:** The System Trace view is a detail panel tab ("Trace") available on MR and agent entities. It can also be accessed from the Explorer's Change View by clicking any change entry. Data endpoint: `GET /api/v1/merge-requests/:id/timeline` — returns the assembled SDLC timeline (ABAC `resource_type: "mr"`, `id_param: "id"`, `action: "read"`). Response (200):
```json
{
  "mr_id": "...",
  "events": [
    {"timestamp": 1711324800, "type": "SpecLifecycleTrigger", "detail": {"spec_path": "...", "task_id": "..."}},
    {"timestamp": 1711324815, "type": "AgentSpawned", "detail": {"agent_id": "...", "persona": "..."}},
    {"timestamp": 1711324822, "type": "ConversationTurn", "detail": {"turn": 3, "summary": "Created RetryPolicy type"}},
    {"timestamp": 1711324860, "type": "GitPush", "detail": {"commit_sha": "...", "files_changed": 3}},
    {"timestamp": 1711324861, "type": "GateResult", "detail": {"gate": "cargo-test", "status": "pass"}},
    {"timestamp": 1711324862, "type": "GraphExtraction", "detail": {"nodes_added": 2, "nodes_modified": 1}},
    {"timestamp": 1711324870, "type": "MergeQueueEnqueued", "detail": {"position": 3}},
    {"timestamp": 1711324880, "type": "Merged", "detail": {}}
  ]
}
```
Note: this is the SDLC activity timeline (what Gyre did). For the built software's data flow traces (OTel spans), see `GET /api/v1/merge-requests/:id/trace` in §3a.

### Saved Views (Curated, Shared)

Views are serializable specs that can be saved to the workspace and shared:

```json
{
  "name": "API surface",
  "data": {
    "node_types": ["Endpoint"],
    "edge_types": ["RoutesTo"],
    "depth": 1
  },
  "layout": "list",
  "encoding": {
    "label": "qualified_name",
    "color": {"field": "node_type", "scale": "categorical"}
  }
}
```

Saved views use the **same view spec grammar** defined in `ui-layout.md` §4 (`data` + `layout` + `encoding` + optional `annotations` + `explanation`). The grammar is the single schema for all views — saved, generated, and built-in.

Saved views are stored as JSON documents keyed by workspace. The key format is `workspace_id:view_id` (UUID), ensuring workspace isolation (views from workspace A are not queryable by workspace B). If `KvJsonStore` is used, the namespace is `explorer_views` — note the single-tenant limitation flagged in `hierarchy-enforcement.md` §3. For multi-tenant deployments, saved views should migrate to a proper port trait with tenant-scoped adapter. API endpoints for view CRUD (each requires a `RouteResourceMapping` entry in the ABAC `ResourceResolver` with `resource_type: "explorer_view"` and `workspace_param: "workspace_id"`):
- `GET /api/v1/workspaces/:workspace_id/explorer-views` — list saved views
- `POST /api/v1/workspaces/:workspace_id/explorer-views` — create a view. Response (201): `{view_id: "<uuid>", name: "...", query: {...}, ...}`
- `GET /api/v1/workspaces/:workspace_id/explorer-views/:view_id` — load a specific saved view (for deep links and sharing)
- `PUT /api/v1/workspaces/:workspace_id/explorer-views/:view_id` — update a saved view
- `DELETE /api/v1/workspaces/:workspace_id/explorer-views/:view_id` — delete a view

`:view_id` is a UUID, not a slug — avoids name collision issues. The human-readable name is a display field, not a key.

**Ownership:** Saved views are editable and deletable only by the creator (`created_by` field) or workspace Admin members. All workspace members can read all saved views (shared by default). This is enforced by per-handler auth on the PUT and DELETE endpoints — the handler checks `created_by == caller_user_id OR caller has Admin workspace role`.

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
  "data": {
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

**Flow layout example — LLM-generated animated trace view:**
```
User: "Show me how a payment retry request flows through the system"

LLM generates:
{
  "name": "Payment retry flow",
  "data": {
    "concept": "retry",
    "node_types": ["Endpoint", "Function", "Type"],
    "depth": 3,
    "repo_id": "<payment-api-repo-id>",
    "trace_source": {"mr_id": "<latest-mr-touching-retry>"}
  },
  "layout": "flow",
  "encoding": {
    "particle_color": {"field": "status", "scale": {"ok": "#3b82f6", "error": "#ef4444"}},
    "node_badge": "latency_p99",
    "label": "name"
  },
  "explanation": "This shows the retry request path: POST /payments/retry → check_idempotency → PaymentGateway::charge → RecordResult::save. Particles represent test requests from the last gate run."
}
```

The LLM selects `"flow"` layout when the question implies data movement ("how does X flow", "what happens when Y calls Z", "show me the request path for W"). For structural questions ("what is X made of", "what depends on Y"), it uses `"graph"` or `"hierarchical"`.

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

**Notification creation paths by priority:**
| Priority | Type | Creation Path |
|---|---|---|
| 1 | Agent clarification | Synchronous in `agent.complete` handler (reliability-critical) |
| 2 | Spec pending approval | Synchronous in `specs/save` handler (human spec-edit MRs). For agent-authored specs: the `agent.complete` handler creates the notification when the agent's MR enters the merge queue with a pending spec approval gate (the gate evaluation detects the unapproved spec and creates the notification). Note: spec lifecycle's default-branch push handler creates *approval-invalidation* notifications, not *pending-approval* notifications. |
| 3 | Gate failure | Synchronous in gate evaluation handler |
| 4 | Cross-workspace spec change | Synchronous in spec lifecycle push handler (the push hook must query `spec_links` for inbound cross-workspace links and create notifications for dependent workspace members — amend `spec-lifecycle.md` to add this notification step alongside the existing task creation). One notification is created per dependent repo (the one that has the inbound link), so each repo-scope Inbox shows its own notification. The notification's `repo_id` is set to the dependent repo, not the changed repo. |
| 5 | Conflicting interpretations | Synchronous in post-extraction divergence check |
| 6 | Meta-spec drift | Via `MessageConsumer` consuming `ReconciliationCompleted` events |
| 7 | Budget warning | Synchronous in budget check middleware |
| 8 | Trust suggestion | Background job (`trust_suggestion_check`) |
| 9 | Spec assertion failure | Synchronous in post-extraction assertion check |
| 10 | Suggested spec link | Synchronous in post-extraction link suggestion |

Most notifications are created synchronously by the handler that detects the condition. The `MessageConsumer` path is used only when the notification source is an async event from another subsystem.
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

Clicking "Ask why" (disabled with tooltip "Conversation unavailable" when `conversation_sha` is null in the attestation) calls `POST /api/v1/agents/spawn` with a new `agent_type: "interrogation"` field and `conversation_sha` in the request body. The server: creates the agent record, mints a short-lived JWT (30 min), creates the scoped ABAC policies, retrieves the conversation via `ConversationRepository::get`, and **provides the conversation to the agent** via an MCP resource `conversation://context` (not injected into the system prompt — conversations can be up to 10MB which exceeds LLM context windows). The **server truncates** the conversation when populating the MCP resource at spawn time: it keeps the last N turns that fit within the configured model's context window (using the workspace's `llm_model` to determine the limit). The truncated conversation is what the MCP resource serves — the agent runtime receives a pre-truncated context, not the full blob. The UI opens an inline chat panel to this agent. The interrogation agent is spawned with:
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

- name: interrogation-allow-read-INTERROGATION_AGENT_UUID
  scope: tenant
  priority: 202
  effect: allow
  actions: ["read"]
  resource_types: ["conversation", "explorer_view", "spec", "mr", "repo", "task"]
  conditions:
    - attribute: subject.id
      operator: equals
      value: "agent:INTERROGATION_AGENT_UUID"
  # Read access to spec/mr/repo/task is required for the interrogation agent
  # to answer questions about the original agent's work context
```

The policies use `subject.id` conditions (not a new `Agent` scope variant) to target the specific interrogation agent, staying within `abac-policy-engine.md`'s existing `PolicyScope` enum (Tenant, Workspace, Repo). The agent's JWT `max_lifetime_secs` is set to 1800 (30 minutes).

**Policy cleanup:** Interrogation-specific ABAC policies are deleted on any of these paths:
1. Agent calls `agent.complete` → completion handler deletes policies by name pattern `interrogation-*-<agent-id>`
2. Admin kills agent → kill handler deletes policies by same pattern
3. JWT expires → stale agent detector (runs every 5 minutes) marks the agent Dead → the detector's cleanup logic deletes policies by pattern (the stale agent detector must be extended to check for `interrogation-*` prefixed policies on any Dead agent). Maximum orphan window: 35 minutes (30-min JWT + 5-min scan interval).

All three paths are deterministic. No orphaned policies possible as long as the stale agent detector runs (which it does as a background job).

The interrogation session is itself attested — the conversation is stored as a provenance artifact linked to the original MR. The interrogation agent's MCP tool access includes `conversation.upload` (same as regular agents) — the ABAC allow-write policy on `message` resource type is sufficient for MCP tool calls since MCP tools use the agent's JWT for auth, not ABAC middleware.

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
    async fn store(&self, agent_id: &Id, workspace_id: &Id, tenant_id: &Id, conversation: &[u8]) -> Result<String>;
    /// Retrieve a conversation by SHA. Returns decompressed bytes.
    /// The adapter handles decryption and decompression internally.
    /// Retrieve a conversation by SHA. Verifies tenant_id matches the caller's tenant.
    async fn get(&self, conversation_sha: &str, tenant_id: &Id) -> Result<Option<Vec<u8>>>;
    /// Record a turn-to-commit link (called from git push handler).
    async fn record_turn_link(&self, link: &TurnCommitLink) -> Result<()>;
    /// Get turn-commit links for a conversation (for "View conversation at this point").
    /// tenant_id ensures cross-tenant isolation on turn link queries.
    async fn get_turn_links(&self, conversation_sha: &str, tenant_id: &Id) -> Result<Vec<TurnCommitLink>>;
    /// Get metadata for a conversation without fetching the full blob (used by per-handler auth to resolve workspace_id).
    async fn get_metadata(&self, conversation_sha: &str, tenant_id: &Id) -> Result<Option<(Id, Id)>>; // returns (agent_id, workspace_id)
    /// List conversations for an agent (for Explorer detail panel provenance links).
    async fn list_by_agent(&self, agent_id: &Id, tenant_id: &Id) -> Result<Vec<String>>; // returns SHAs
}
```

**REST endpoint for retrieval:** `GET /api/v1/conversations/:sha` — returns the conversation binary blob (decompressed). **ABAC:** This endpoint uses a custom resource resolution strategy (not the standard `RouteResourceMapping` pattern, since `:sha` is not a UUID and workspace_id is not in the URL). The handler looks up `workspace_id` from `ConversationRepository` metadata, then calls the ABAC evaluator directly with the resolved context. This is one of the ABAC-exempt-from-middleware endpoints (like git HTTP), with authorization handled per-handler. The adapter stores conversations encrypted at rest; large conversations (>1MB) are stored as files on disk with the SHA as filename.

**DB schema** (new migration):
```sql
CREATE TABLE conversations (
    sha TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    blob BLOB,                  -- NULL if stored on disk (>1MB)
    file_path TEXT,             -- set if stored on disk
    created_at INTEGER NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE turn_commit_links (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    turn_number INTEGER NOT NULL,
    commit_sha TEXT NOT NULL,
    files_changed TEXT NOT NULL, -- JSON array
    conversation_sha TEXT,      -- NULL until back-filled at completion
    timestamp INTEGER NOT NULL,
    tenant_id TEXT NOT NULL
);
CREATE INDEX idx_turn_links_agent ON turn_commit_links (agent_id);
CREATE INDEX idx_turn_links_conversation ON turn_commit_links (conversation_sha);
```

The agent runtime captures the conversation via a new MCP tool `conversation.upload` (addition to `platform-model.md` §4 tool table, scope: `agent`). The conversation is transmitted as a base64-encoded zstd-compressed blob (base64 because MCP tools use JSON — binary is encoded as a string), max **10MB** before base64 encoding (configurable, `GYRE_MAX_CONVERSATION_SIZE`). The server decodes base64, computes SHA-256 on the raw compressed bytes and stores the blob encrypted at rest. If the caller-provided `conversation_sha` (if any) doesn't match the computed SHA, the server rejects with 400. The upload is called by the agent runtime just before `agent.complete`. If it fails, completion still succeeds but the conversation is marked as unavailable. The MCP server validates that the uploading agent's `sub` claim matches the `agent_id` in the request, and derives `workspace_id` and `tenant_id` from the agent JWT's claims (not from the request body).

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

The server resolves the composite path to a specific spec in a specific repo. Resolution is always scoped to the **caller's tenant** (the server uses the authenticated caller's `tenant_id` to scope the workspace slug lookup, preventing cross-tenant resolution). For agent-pushed manifests, the server performs the cross-workspace slug resolution using an internal service context (not the agent's repo-scoped JWT) since agents cannot read across workspaces. For UI navigation of cross-workspace links, the client resolves slugs via `GET /api/v1/workspaces?slug=<slug>` (returns the workspace matching the slug within the caller's tenant — the existing workspace list endpoint with a slug filter, no new endpoint needed). The first segment is the **workspace slug** (URL-safe identifier, unique per tenant, as defined in `platform-model.md`'s Workspace struct). Same-repo links use just the spec path (existing behavior). Cross-repo same-workspace links use `repo_name/spec_path`. Cross-workspace links use the full `workspace_slug/repo_name/spec_path`.

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

**`UserPresence` implementation:** Presence does NOT use the message bus `MessageKind` enum or tier system. It is a `WsMessage` variant used **bidirectionally**: clients send `UserPresence` to the server (heartbeat with current view), and the server rebroadcasts to other workspace subscribers. **The server derives `user_id` from the authenticated WebSocket connection** — it does NOT trust the payload's `user_id` field. **WebSocket identity:** User WebSocket connections authenticate via JWT (the same token used for REST — passed as a query parameter on the WebSocket handshake `?token=<jwt>`). This provides per-user identity required for presence. Shared-token connections (e.g., `GYRE_AUTH_TOKEN`) are excluded from presence tracking since they have no user identity. The client sends it for convenience but the server overwrites it with the verified identity. It is a WebSocket-only signal with its own handling path — the server receives it on the WebSocket, updates the in-memory presence map, and rebroadcasts to workspace subscribers. This avoids conflating presence with the message bus tier model (Telemetry tier's storage semantics don't fit presence).

The `WsMessage` enum gains a `UserPresence` variant (alongside `Subscribe`):
- **Payload schema:**
  | Field | Type | Required |
  |---|---|---|
  | `user_id` | Id | yes |
  | `session_id` | String | yes — random UUID per browser tab |
  | `workspace_id` | Id | yes |
  | `view` | String | yes — e.g., "inbox", "explorer", "specs" |
  | `timestamp` | u64 | yes — epoch ms |

`UserPresence` messages go through two paths:
1. **Real-time push:** through the broadcast channel to WebSocket subscribers (same as all Telemetry)
2. **Presence map:** the server maintains a lightweight in-memory map keyed by `(user_id, session_id)` → `{workspace_id, view, timestamp}`, updated on every `UserPresence` message. This is the source of truth for "who is active" — queryable synchronously, survives TelemetryBuffer eviction. The multi-session keying matches §1's per-tab session_id model (5-session cap, oldest evicted first).

`UserPresence` messages do NOT go into the `TelemetryBuffer` (not returned by `GET /activity`). They use a separate presence map.

**Presence query:** `GET /api/v1/workspaces/:workspace_id/presence` returns the current presence map for a workspace: `[{user_id, session_id, view, last_seen}]`. The `session_id` is included so clients can distinguish multi-tab sessions on reconnection (same data as the WebSocket `UserPresence` messages). This is a simple read from the in-memory map, not a message bus query. ABAC resource resolution: `resource_type: "workspace"`, `workspace_param: "workspace_id"` — uses the existing workspace resource type, no new type needed. On WebSocket reconnection, the client fetches this endpoint to populate the initial presence state.

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
| 2 | **Spec pending approval** | Spec registry | Approve / Reject (inline, read spec content). Reject calls `POST /api/v1/specs/:path/reject` (new endpoint — sets spec status to `Rejected` with a `reason` field, closes the associated MR if the spec came from a `spec-edit/*` branch). On rejection, the orchestrator receives a `SpecChanged` event and cancels in-flight tasks referencing that spec (tasks are moved to `Cancelled` status, active agents are sent a pause message). A priority-2 "Spec rejected" notification is created for all workspace Admin/Developer members (confirming the rejection and linking to the rejected spec for visibility). |
| 3 | **Gate failure** | Merge queue | View diff + output, Retry / Override / Close |
| 4 | **Cross-workspace spec change** | Spec link watcher | Review impact, Approve / Dismiss |
| 5 | **Conflicting spec interpretations** | Detected post-merge by the push-triggered graph extraction background job (M30b). After extraction completes, a **divergence check step** runs: it queries `ArchitecturalDelta` records for the merged MR's `spec_path`, comparing the latest delta against previous deltas from other agents for the same spec. A "conflicting node change" is defined as: two deltas **from different agents working on different tasks within the last 7 days, in the same repo,** for the same `spec_ref` (the `ArchitecturalDelta` field name per `realized-model.md`) that add nodes with the same `name` but different `node_type` or different `qualified_name` (indicating different structural placement). The comparison finds nodes that share a `name` across the two deltas' `nodes_added` lists (and optionally the `GraphNode` element from `nodes_modified` tuples) but differ in their other attributes — this is a same-name-different-structure match, not a set symmetric difference. For `nodes_modified`, a conflict is when both deltas modify the same node (matched by `name`) but with different `FieldChange` outcomes — compared on `(field, new_value)` pairs only (ignoring `old_value`, which naturally differs between deltas from different starting points). Cross-repo deltas are excluded — different repos naturally have different `qualified_name` prefixes. Deltas with `spec_ref: None` are also excluded — without a governing spec, there is no shared intent to conflict on. Deltas with `agent_id: None` (e.g., human-pushed code) are also excluded — divergence detection applies only to agent-produced work. Deltas produced by reconciliation agents (identified by the `persona_id` on the agent record matching the reconciliation persona) are also excluded — reconciliation is intentional re-implementation, not conflicting interpretation. The 7-day lookback window prevents old deltas from triggering false positives on iterative work. The comparison is on `(name, node_type, qualified_name)` tuples — all fields present on `GraphNode` in the `ArchitecturalDelta.nodes_added` vector (`Vec<GraphNode>`) and the first element of each tuple in `nodes_modified` (`Vec<(GraphNode, Vec<FieldChange>)>`). The `delta_json` DB column is the serialized form of these structured fields. The task context for each delta is derived from the `agent_id` field on `ArchitecturalDelta` → query the task where `assigned_to == agent_id` and `created_at <= delta.timestamp` and (`completed_at IS NULL OR completed_at >= delta.timestamp`) — using the task's `created_at` and `completed_at` timestamps as the time boundary. Note: agent records must be retained (not purged) for the 7-day divergence lookback window to function — the stale agent detector marks agents as Dead but does not delete their records. If the symmetric difference of these tuples across the two deltas exceeds the threshold, the server creates a `Notification` (not a message bus event) for workspace Admin/Developer members. The comparison uses `ArchitecturalDelta.delta_json` — no pre-merge snapshot needed. Threshold: `GYRE_DIVERGENCE_THRESHOLD` (default 3 conflicting node changes per spec_ref — the threshold applies independently to each spec_ref comparison, not globally across all spec_refs). One notification is created per spec_ref that exceeds the threshold — each notification includes the two conflicting MR/agent references so the human can act on each conflict independently. **Resolution mechanism:** "Pick A" creates a task to revert the non-selected MR's changes (standard `spec-implementation` task with the selected implementation as reference). "Pick B" does the same for the other. "Reconcile" creates a task assigned to the workspace orchestrator to produce a unified implementation that resolves the structural divergence. All three create a task via the existing task domain model — no new API endpoint needed. | Review both implementations, pick one or request reconciliation |
| 6 | **Meta-spec drift alert** | Reconciliation controller | Review results, adjust meta-spec |
| 7 | **Budget warning** | Budget enforcement | Increase limit / Pause work |
| 8 | **Trust level suggestion** | Track record analysis | Increase trust / Dismiss |
| 9 | **Spec assertion failure** | Knowledge graph + assertions | Fix code or update spec |
| 10 | **Suggested spec link** | Knowledge graph (low confidence) | Confirm / Dismiss linkage |

**Novel types not in any existing tool:**
- **Agent needs clarification** (#1) — the agent is stuck and explicitly says what it's uncertain about. This is the most important escalation.
- **Conflicting spec interpretations** (#5) — the system detects that two agents implemented the same spec differently and asks the human to arbitrate.
- **Trust level suggestion** (#8) — the system suggests the human can relax oversight based on track record.

Inbox filtering by trust level is defined in the Trust Level table (§2, "Inbox priorities shown" row) as the single source of truth. **Mechanism:** The Inbox query endpoint (`GET /api/v1/users/me/notifications`) accepts `?min_priority=1&max_priority=9` parameters. The UI reads the workspace's `trust_level` and maps it to the priority range before calling the endpoint. This is client-side logic, not ABAC — trust level controls what the UI *requests*, not what the server *allows*.

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
| In Progress | Task status + agent activity + in-flight uncertainties (queried from `Notification` table where `notification_type = 'AgentNeedsClarification'` and `resolved_at IS NULL` — these are created by the orchestrator from `Escalation` messages, so the Briefing reads notifications, not Directed messages directly) + completion summary uncertainties (for recently completed agents) |
| Cross-Workspace | Cross-workspace spec link watcher |
| Exceptions | Gate results + spec assertion failures + MR reverts (MRs with `Reverted` status) |
| Metrics | Budget usage + analytics |

### Briefing Q&A

Below the briefing, an inline chat grounded in the briefing data:

```
Ask about this briefing ▸ [                                        ]

Example: "Tell me more about the auth refactor"
         "Why did worker-12 choose exponential backoff?"
         "What changed in idempotent-api.md?"
```

**Briefing endpoint:** `GET /api/v1/workspaces/:workspace_id/briefing` — this spec is the **sole owner** of the response schema (supersedes `realized-model.md` §7's original narrative description). Response:
```json
{
  "since": 1711324800,
  "summary": "3 specs completed, 2 in progress, 1 gate failure. $23.40 spent.",
  "sections": {
    "completed": [{"spec_ref": "...", "summary": "...", "mr_count": 3, "agent_id": "...", "decisions": [...]}],
    "in_progress": [{"spec_ref": "...", "summary": "...", "progress": "3/5", "agents": [...], "uncertainties": [...]}],
    "cross_workspace": [{"source_workspace_slug": "...", "spec_path": "...", "summary": "..."}],
    "exceptions": [{"type": "gate_failure", "entity_id": "...", "summary": "...", "actions": [...]}],
    "metrics": {"mr_count": 12, "agent_runs": 47, "compute_cost_cents": 2340, "budget_pct": 67}
  }
}
```
The top-level `summary` is an LLM-synthesized narrative string (produced by the narrative generation pipeline in `realized-model.md` §6). Each section item also has a `summary` string field generated the same way. The structured `sections` object is the primary data; the `summary` strings provide human-readable context.

When `?since=` is omitted, the server uses `last_seen_at` from `user_workspace_state` as the default. If no row exists (first visit), falls back to 24 hours ago. **Zero activity:** If no activity occurred since `since`, the response returns empty arrays for all sections and zeroed metrics — always returns 200 with the full schema structure, never 204. ABAC: `resource_type: "workspace"`, `workspace_param: "workspace_id"`.

**Q&A endpoint:** `POST /api/v1/workspaces/:workspace_id/briefing/ask` — request: `{question: "...", history?: [{role: "user"|"assistant", content: "..."}]}`, response: `{answer: "...", sources: [{spec_path, agent_id, ...}]}`. The `history` array is capped at 20 entries (older entries are dropped by the client); the server rejects requests with more than 20 history entries (400). The optional `history` array enables follow-up questions — the client accumulates prior Q&A pairs and resends them with each request. The server is stateless; the client owns the conversation state. ABAC: `RouteResourceMapping` with `resource_type: "workspace"`, `workspace_param: "workspace_id"`, `action_override: "generate"`. Requires workspace membership.

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

## 11. CLI/MCP Parity Constraint

**Every data surface in the UI must be consumable outside the browser.** If understanding requires the Gyre platform UI to be running, we've created the same lock-in that killed Dark's proprietary editor. The knowledge graph, the briefing narrative, the notification inbox, and the system traces are valuable to developers working in terminals, CI pipelines, and LLM-powered editors.

**Principle:** The UI is a view layer over REST APIs. Every API that the UI consumes is available to the CLI and MCP clients. Specifically:

| UI Surface | CLI Equivalent | MCP Equivalent |
|---|---|---|
| Briefing | `gyre briefing [--workspace <slug>] [--since <epoch>]` | Resource: `briefing://` |
| Inbox | `gyre inbox [--workspace <slug>] [--priority <min>-<max>]` | Resource: `notifications://` |
| Explorer (concept search) | `gyre explore <concept> [--repo <name>]` | Tool: `graph.concept` |
| System Trace | `gyre trace <mr-id>` | Resource: `trace://<mr-id>` |
| Spec editing assist | `gyre spec assist <path> "<instruction>"` | Tool: `spec.assist` |
| Divergence alerts | `gyre divergence [--workspace <slug>]` | (via notifications) |

This table is not exhaustive — it establishes the pattern. The REST API is the single source of truth; the UI, CLI, and MCP are all consumers. No data surface should be UI-only.

**Why this matters:** When LLMs work through Cursor/VSCode/CLI (not Gyre's Explorer), the developer still needs the briefing, the traces, and the structural understanding. The system's value is in the data and the analysis, not in the rendering.

---

## Relationship to Existing Specs

**Upstream Amendments Applied** (all amendments from this spec have been applied to the listed specs in this PR):

| Spec | Amendment Needed |
|---|---|
| `system-explorer.md` §1 | `Cmd+K` → global search (not canvas-scoped). Canvas search uses `/`. Explorer "Sidebar" layout (Boundaries/Interfaces/Data/Specs subsections) becomes an in-view filter panel (200px, collapsible), not part of the app sidebar — update layout diagrams. §3 ghost overlays are **Phase 1 priority** (fast structural prediction via `graph/predict` ships before the thorough Editor Split preview). The Editor Split preview in `ui-layout.md` §9 is Phase 2 — complements ghost overlays with certainty after prediction builds intuition. |
| `hierarchy-enforcement.md` §4 | ABAC bypass must match by `subject.id == "gyre-system-token"`, not by `subject.type == "system"`. Internal services (merge processor) are `system` type but subject to ABAC. Add to ABAC-exempt endpoint list (per-handler auth, like git HTTP): `GET /api/v1/conversations/:sha`, `GET /api/v1/users/me/notifications`, `POST /api/v1/notifications/:id/dismiss`, `POST /api/v1/notifications/:id/resolve`. Add `user_workspace_state` to `check-tenant-filter.sh` skip list. |
| `api-conventions.md` §6 | Acknowledge per-handler auth as a third authorization mechanism alongside "no auth (public)" and "ABAC middleware." Per-handler auth is used for endpoints where the resource key is not a UUID (e.g., conversations by SHA) or where the resource is implicitly the authenticated user (e.g., `/users/me/*`). These endpoints are listed in the ABAC-exempt endpoint list in `hierarchy-enforcement.md` §4. |
| `message-bus.md` `MessageKind` | Add `AgentCompleted` (Event tier, server-only, payload schema defined in §4 of this spec). Add `ReconciliationCompleted` (Event tier, server-only — migrated from domain event in `meta-spec-reconciliation.md` §11; consumed by `MessageConsumer` to create priority-6 Inbox notifications). Extend `SpecChanged` payload with optional `dependent_workspace_id` and `source_workspace_slug` fields for cross-workspace notifications — the same kind reused with extra context, no new kind needed. |
| `abac-policy-engine.md` §"Resource attributes" | Add `explorer_view` (attributes: `workspace_id`, `created_by`), `message` (attributes: `workspace_id`, `to_agent_id`), and `conversation` (attributes: `workspace_id`, `agent_id`) to the resource type list. The `explorer-views/generate` endpoint uses `resource_type: "explorer_view"`. |
| `abac-policy-engine.md` §"Action attributes" | Add `generate` action (used by `explorer-views/generate` and `specs/assist` endpoints — distinct from `write` to allow policies that permit LLM generation while restricting CRUD). Applies to resource types: `explorer_view`, `spec`. |
| `abac-policy-engine.md` §"Built-In Policies" | Add `builtin:require-human-spec-approval` (immutable Deny, priority 999, denies `approve` on `spec` when `subject.type != "user"`) to the built-in policy table. This policy is defined in HSI §2 and seeded at server startup. |
| `abac-policy-engine.md` §CRUD API | Add `?scope_id=` query parameter to `GET /api/v1/policies` for filtering policies by their scope target (e.g., `?scope=Workspace&scope_id=<workspace_id>` returns only policies scoped to that workspace). Required for the Admin Policies tab. |
| `agent-gates.md` `MergeAttestation` | Add `conversation_sha: Option<String>` field. |
| `agent-gates.md` spec approval ledger | Add `Rejected` status with `rejected_at: Option<u64>`, `rejected_reason: Option<String>`, and `rejected_by: Option<Id>` fields to the `spec_approvals` table. Define formal `ApprovalStatus` enum: `Pending`, `Approved`, `Revoked`, `Rejected`. Mutual exclusivity: the handler clears all other timestamp columns when setting a new status (e.g., approving clears `rejected_at` and `revoked_at`). Only one timestamp column is non-null at any time — no precedence logic needed. **Rejected vs Revoked:** Rejected = human explicitly declines the spec before it takes effect (pre-merge). Revoked = a previously approved spec's approval is withdrawn after the fact (post-merge, e.g., spec content changed). A rejected spec can be re-submitted (new MR with revised content). The rejected `spec-edit/*` branch is deleted on rejection (same as MR-close branch cleanup). Add `POST /api/v1/specs/:path/reject` endpoint (request: `{reason}`, response: 204, per-handler auth: requires `subject.type == "user"` — agents cannot reject specs, same constraint as approval). Rejection closes the associated MR if the spec came from a `spec-edit/*` branch. |
| `spec-lifecycle.md` §"Spec Approval Interaction" | Add SHA-match check: when a spec modification is detected on the default branch, if the new SHA already has a valid approval in the ledger (e.g., pre-approved on a feature branch before merge), preserve the approval instead of invalidating it. |
| `spec-lifecycle.md` §"Automatic Task Creation" push handler | Add cross-workspace notification step: after detecting spec changes, query `spec_links` for inbound cross-workspace links and create priority-4 notifications for dependent workspace Admin/Developer members. |
| `platform-model.md` §4 MCP tools | Add `conversation.upload` (scope: agent), `message.send` (scope: workspace), `message.poll` (scope: agent), `message.ack` (scope: agent) — per `message-bus.md` MCP tools section. |
| `platform-model.md` §4 agent spawn | Add `agent_type: Option<String>` field to spawn request (values: `null` for normal agents, `"interrogation"` for interrogation agents per §4). |
| `platform-model.md` §4 MCP resources | Add `conversation://context` resource (scope: agent, read-only) — provides the original agent's conversation history to interrogation agents. The server populates this resource at spawn time from `ConversationRepository::get`. |
| `platform-model.md` §9 UI Pages | Note that standalone entity views (Task Board, Agent List, etc.) are contextual drill-downs, not primary navigation. |
| `spec-links.md` §target format | Cross-repo/cross-workspace targets use `@` prefix for disambiguation. Clarify that `{workspace}` segment uses **slug** (not name). Add `?repo_id=` query parameter to `GET /specs/:path/links` for path disambiguation (same pattern as `/history` and `/progress`). **Resolution:** The `@workspace_slug/repo_name/spec_path` composite is resolved at manifest parse time (push hook) — the server resolves slug→workspace_id→repo_id and stores the resolved `target_repo_id` UUID in the `spec_links` table. Add `target_display TEXT` column (preserves original composite path for human readability) and make `target_repo_id` nullable (`TEXT` instead of `TEXT NOT NULL`) for unresolved links. If a workspace slug is renamed, existing links remain functional (they use `target_repo_id`, not the slug) but `target_display` becomes stale — the staleness checker should also verify `target_display` matches the current slug and update it if not. If the target workspace or repo does not exist at push time, the push hook logs a warning and stores the link with `target_repo_id: NULL` — the link is marked as `unresolved` and the staleness checker resolves it when the target becomes available. |
| `vision.md` §"Relationship to Other Specs" | Replace `ui-journeys.md` references with `human-system-interface.md` in the principles governance table. |
| `message-bus.md` `WsMessage` enum | Add `UserPresence` variant (bidirectional, payload: user_id, session_id, workspace_id, view, timestamp) and `PresenceEvicted` variant (server→client, signals tab should stop heartbeating). |
| `platform-model.md` §1 `Workspace` struct | Add `trust_level: TrustLevel` field (enum: Supervised, Guided, Autonomous, Custom). Add `llm_model: Option<String>` field (per-workspace LLM model selection, defaults to `GYRE_LLM_MODEL` env var — see `ui-layout.md` §2). |
| `hierarchy-enforcement.md` §4 built-in policies | Rename `system-access` → `system-full-access`. Assign priorities to all built-in policies: `system-full-access` (1000), `admin-all-operations` (900), `developer-write-access` (800), `agent-scoped-access` (700), `readonly-get-only` (600), `tenant-isolation` (500), `workspace-membership-required` (400), `builtin:require-human-spec-approval` (999, immutable), `persona-human-approval` (500), `default-deny` (0). Add `immutable: bool` flag to `Policy` struct — immutable Deny policies evaluated before all others. `system-full-access` at priority 1000 must carve out spec approval. Define ABAC identity for internal server processes: each internal process (merge processor, stale agent detector, budget reset) creates an internal `SubjectContext` with `subject.type: "system"` and `subject.id: "<process-name>"` — no JWT needed, constructed in-process. Only the global `GYRE_AUTH_TOKEN` identity (`subject.id: "gyre-system-token"`) bypasses ABAC. |
| `platform-model.md` §1 Repository | Add unique constraint on `(workspace_id, name)` — repo names must be unique within a workspace for cross-workspace spec link resolution. |
| `platform-model.md` §9 or API endpoints | Add `?slug=` query parameter to `GET /api/v1/workspaces` for workspace slug lookup (used by cross-workspace link UI navigation). |
| `platform-model.md` §6 MR status | Formally define `Reverted` as a variant in the MR status enum (set on the original MR when a revert commit is created). Required by the trust suggestion job's MR revert query. |
| `platform-model.md` §5 `BudgetUsage` | Make `repo_id` and `agent_id` optional (`Option<Id>`) — user-initiated LLM queries (briefing/ask, explorer-views/generate, specs/assist) have no agent and may have no repo context. Add `usage_type: String` field (e.g., `"llm_query"`, `"agent_run"`) to distinguish cost entry sources. |
| `platform-model.md` §3 Task status | Add `Cancelled` as a formal variant in the task status enum. Reachable from `Backlog` or `InProgress` (any non-terminal status). Terminal — cancelled tasks cannot be re-opened (create a new task instead). Used when a spec is rejected and in-flight tasks referencing it are cancelled (per HSI §8 priority-2 rejection flow). |
| `realized-model.md` §3 `ArchitecturalDelta` | Define `FieldChange` struct: `{field: String, old_value: Option<String>, new_value: Option<String>}` — used in `nodes_modified: Vec<(GraphNode, Vec<FieldChange>)>`. |
| `message-bus.md` §Scoping Rules | Document that Users (not just Agents) can send Directed messages to agents — required for human→agent steering (Pause, inline chat). |
| `realized-model.md` `GraphNode` struct | Add `spec_confidence` field (already in DB schema but missing from Rust struct). Required by Explorer encoding layer. Define `SpecConfidence` enum variants: `None`, `Low`, `Medium`, `High` (maps to DB TEXT values `"none"`, `"low"`, `"medium"`, `"high"`). |
| `realized-model.md` predict endpoint | Reconcile HTTP method — changed to POST (sends draft content in body). |
| `realized-model.md` briefing endpoint | `GET /workspaces/:id/briefing` should use `last_seen_at` from `user_workspace_state` as default `since` when `?since=` parameter is omitted. |
| `realized-model.md` API | Add workspace-scoped concept search endpoint (`GET /workspaces/:id/graph/concept/:name`) to avoid full-graph download for workspace-scoped queries. Without this, workspace concept search falls back to downloading `GET /workspaces/:id/graph` and filtering client-side. Also verify `GraphNode` struct has `test_coverage` field (present in struct but missing from DB schema per realized-model.md §8). |
| `spec-lifecycle.md` §Configuration | Add `"specs/prompts/"` to `ignored_paths` — prompt templates should iterate quickly without formal spec approval (per `ui-layout.md` §2). |
| `realized-model.md` §7 briefing endpoint | `human-system-interface.md` §9 is the **sole owner** of the `GET /workspaces/:id/briefing` response schema. `realized-model.md` §7 no longer defines the response shape — it cross-references this spec. The knowledge graph narrative generation (§6) feeds the briefing's `summary` string fields. |
| `realized-model.md` §7 API table | Add workspace-scoped concept search: `GET /workspaces/:id/graph/concept/:name`. Add `?concept=` query param to `GET /repos/:id/graph` for substring-based concept filtering (distinct from manifest-based `/graph/concept/:name`). |
| `realized-model.md` §8 DB schema | Add `test_coverage REAL` column to `graph_nodes` table (present in Rust struct, was missing from DB schema). |

All amendments have been applied inline to the upstream specs in this PR. The table above serves as a cross-reference of what was changed and why.

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
