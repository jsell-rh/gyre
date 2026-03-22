# User Management & Notifications

## Problem

The current User model is auth-only: id, external_id, name, email, roles. This is insufficient for enterprise use:

- No profile (display name, avatar, timezone, preferences)
- No tenant/workspace membership model
- No team management or invitation system
- No notification system (humans can't know when agents need them)
- No session management (active sessions, revoke)
- No "my stuff" view (my agents, my tasks, my MRs, my approvals)
- No user-to-user relationships (who manages what workspace)
- No preference persistence (UI theme, default workspace, notification channels)

Without these, Gyre is a platform that agents can use but humans can't effectively operate.

## User Entity

```rust
pub struct User {
    pub id: Id,
    pub external_id: String,        // Keycloak subject (JWT sub claim)
    pub username: String,            // Unique, URL-safe, immutable after creation
    pub display_name: String,        // Human-readable, editable
    pub email: String,               // Derived from SSO, not user-editable
    pub avatar_url: Option<String>,  // From SSO or uploaded
    pub timezone: String,            // IANA timezone (e.g., "America/New_York")
    pub locale: String,              // i18n locale (e.g., "en-US")
    pub tenant_id: Id,
    pub global_role: GlobalRole,
    pub preferences: UserPreferences,
    pub last_login_at: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
}

pub enum GlobalRole {
    TenantAdmin,   // Full tenant management
    Member,        // Normal user, access determined by workspace membership
}
```

### Username vs Display Name

- **Username:** unique, URL-safe (`jsell`, `asmith`). Used in mentions (`@jsell`), URLs, git attribution, CLI. Derived from SSO `preferred_username` on first login. Immutable after creation.
- **Display Name:** human-readable (`Jordan Sell`, `Alice Smith`). Editable by user. Used in UI, activity feeds, reviews.
- **Email:** derived from SSO. Not user-editable. Used for notifications.

## Workspace Membership

Users access workspaces through explicit membership:

```rust
pub struct WorkspaceMembership {
    pub id: Id,
    pub user_id: Id,
    pub workspace_id: Id,
    pub role: WorkspaceRole,
    pub invited_by: Id,
    pub accepted_at: Option<u64>,
    pub created_at: u64,
}

pub enum WorkspaceRole {
    Owner,      // Full workspace control, budget management, can delete workspace
    Admin,      // Manage members, configure repos, manage personas
    Developer,  // Create/modify tasks, MRs, spawn agents, approve specs
    Viewer,     // Read-only access to all workspace data
}
```

### Invitation Flow

1. Workspace Owner/Admin invites user by email or username
2. Invitation created with `accepted_at: None`
3. Invitee receives notification (email + in-app)
4. Invitee accepts or declines
5. On accept: membership is active, user gains access to workspace repos
6. Pending invitations visible in admin panel

Users don't self-join workspaces. Access is always granted by an existing member with Owner/Admin role.

### Repo Access

Repo access is inherited from workspace membership by default:

| Workspace Role | Repo Access |
|---|---|
| Owner | Full (read, write, admin, manage gates, manage budget) |
| Admin | Full (read, write, admin, manage gates) |
| Developer | Read, write, create MRs, create tasks, spawn agents |
| Viewer | Read-only |

Per-repo overrides can restrict (but not expand) access beyond workspace role. A Developer can be restricted to Viewer on a specific repo, but a Viewer can't be elevated to Developer on a specific repo without changing their workspace role.

## Team Management

Within a workspace, users can be organized into teams:

```rust
pub struct Team {
    pub id: Id,
    pub workspace_id: Id,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub lead: Id,              // User responsible for the team
    pub created_at: u64,
}

pub struct TeamMembership {
    pub team_id: Id,
    pub user_id: Id,
    pub added_at: u64,
}
```

Teams are used for:
- Notification routing ("notify the platform team")
- Spec ownership ("this spec is owned by team:platform")
- Default reviewers (MRs in repos owned by team X auto-assign team X for review)
- Budget reporting (aggregate budget usage by team)

Teams don't grant additional permissions beyond workspace membership. They're organizational, not authorization.

## User Preferences

```rust
pub struct UserPreferences {
    pub theme: Theme,                       // Light, Dark, System
    pub default_workspace_id: Option<Id>,   // Landing workspace after login
    pub notification_channels: NotificationChannels,
    pub ui_density: UiDensity,              // Compact, Comfortable, Spacious
    pub code_font_size: u32,
    pub diff_view: DiffView,                // SideBySide, Unified
    pub activity_feed_scope: FeedScope,     // MyActivity, Workspace, All
}

pub enum Theme { Light, Dark, System }
pub enum UiDensity { Compact, Comfortable, Spacious }
pub enum DiffView { SideBySide, Unified }
pub enum FeedScope { MyActivity, Workspace, All }
```

Preferences stored server-side (not localStorage). Persist across devices and sessions.

## Session Management

```rust
pub struct UserSession {
    pub id: Id,
    pub user_id: Id,
    pub token_hash: String,     // SHA-256 of session token (never store plaintext)
    pub ip_address: String,
    pub user_agent: String,
    pub created_at: u64,
    pub last_active_at: u64,
    pub expires_at: u64,
    pub revoked: bool,
}
```

Users can:
- View all active sessions (device, IP, last active)
- Revoke individual sessions
- Revoke all sessions ("sign out everywhere")
- TenantAdmins can view and revoke any user's sessions

---

## Notifications

### The Problem

Gyre creates events that need human attention: gate failures, budget warnings, spec approvals pending, agent escalations, breaking change alerts. Without a notification system, these events sit in the activity feed and nobody sees them.

### Notification Entity

```rust
pub struct Notification {
    pub id: Id,
    pub user_id: Id,
    pub notification_type: NotificationType,
    pub title: String,
    pub body: String,
    pub source: NotificationSource,
    pub priority: NotificationPriority,
    pub action_url: Option<String>,  // Deep link to relevant page
    pub read: bool,
    pub read_at: Option<u64>,
    pub created_at: u64,
}

pub enum NotificationType {
    // Approvals
    SpecApprovalRequested,
    PersonaApprovalRequested,

    // Agent escalations
    AgentEscalation,
    AgentBudgetWarning,
    AgentBudgetExhausted,
    AgentFailed,

    // Gate results
    GateFailure,
    GatePassed,

    // Merge queue
    MrMerged,
    MrReverted,
    MergeQueuePaused,

    // Dependencies
    BreakingChangeDetected,
    SpecDriftDetected,
    DependencyStale,

    // Security
    SecurityFinding,

    // Workspace
    InvitationReceived,
    MembershipChanged,

    // System
    SystemAlert,
}

pub enum NotificationPriority {
    Low,       // Informational (MR merged, gate passed)
    Medium,    // Needs attention eventually (stale dependency, spec drift)
    High,      // Needs attention soon (budget warning, gate failure)
    Urgent,    // Needs attention now (security finding, agent escalation, MR reverted)
}

pub struct NotificationSource {
    pub entity_type: String,  // "agent", "mr", "spec", "gate", "budget"
    pub entity_id: String,
    pub workspace_id: Id,
    pub repo_id: Option<Id>,
}
```

### Delivery Channels

```rust
pub struct NotificationChannels {
    pub in_app: bool,           // Always true (can't disable)
    pub email: EmailConfig,
    pub webhook: Option<WebhookConfig>,
    pub slack: Option<SlackConfig>,
}

pub struct EmailConfig {
    pub enabled: bool,
    pub digest: DigestFrequency,   // Immediate, Hourly, Daily, Off
    pub min_priority: NotificationPriority,  // Only email for >= this priority
}

pub struct WebhookConfig {
    pub url: String,
    pub secret: String,            // HMAC-SHA256 signing secret
    pub min_priority: NotificationPriority,
}

pub struct SlackConfig {
    pub webhook_url: String,
    pub channel: Option<String>,   // Override channel (default: DM)
    pub min_priority: NotificationPriority,
}

pub enum DigestFrequency { Immediate, Hourly, Daily, Off }
```

### Who Gets Notified

| Event | Recipients |
|---|---|
| Spec approval requested | Spec's `approvers` from manifest |
| Persona approval requested | Persona's `owner` |
| Agent escalation to Overseer | Workspace Owner + repo's team lead |
| Agent budget warning | Agent's spawning user + workspace Admin |
| Agent budget exhausted | Agent's spawning user + workspace Owner |
| Gate failure | MR author (if human) + MR author's spawning user |
| MR merged | MR author (if human) |
| MR reverted | MR author + workspace Admin |
| Merge queue paused | All workspace Admins/Owners |
| Breaking change detected | Dependent repo team leads |
| Spec drift detected | Spec owner |
| Security finding (Critical/High) | Workspace Owner + tenant Admin |
| Invitation | Invited user |

### Notification Routing for Agent Escalations

When an agent escalates "to the Overseer," the notification system resolves the actual human:

1. Check the agent's `spawned_by` user - they're the primary recipient
2. If that user has no active sessions (offline), also notify the workspace Admins
3. For `Urgent` priority: always notify workspace Owners regardless

This solves the "how does the human know?" problem. Agent escalations become push notifications, not items in a feed that might go unseen.

### In-App Notification UI

- **Notification bell** in top nav with unread count badge
- **Notification drawer** slides out showing recent notifications grouped by priority
- **Deep links** - clicking a notification navigates to the relevant entity (spec, MR, agent, etc.)
- **Mark read/unread** individually or bulk
- **Filter** by type, priority, workspace, repo
- **Notification preferences** page in user settings

### Email Notifications

- Email templates per notification type
- Digest mode aggregates notifications into a single email at configured frequency
- Priority filter prevents low-priority email noise
- Unsubscribe link per notification type
- Rendered in the user's configured locale

---

## "My Stuff" Views

### My Dashboard (Landing Page After Login)

| Section | Content |
|---|---|
| My Tasks | Tasks assigned to me, grouped by status |
| My MRs | MRs I authored (or was spawned to create), with gate/merge status |
| My Agents | Agents I spawned, with status and budget usage |
| Pending Approvals | Specs and personas waiting for my approval |
| My Notifications | Unread notifications, prioritized |
| Recent Activity | My recent activity across all workspaces |

### User Profile Page (`/@{username}`)

Public within the tenant:
- Display name, username, avatar, timezone
- Workspace memberships and roles
- Team memberships
- Recent activity feed (public actions only)
- Stats: MRs reviewed, specs approved, agents spawned

---

## API

### User Management

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/users/me` | GET | Current user profile + preferences |
| `PUT /api/v1/users/me` | PUT | Update display name, timezone, locale, preferences |
| `GET /api/v1/users/{username}` | GET | User profile (tenant-public) |
| `GET /api/v1/users` | GET | List users (tenant admin only) |
| `GET /api/v1/users/me/sessions` | GET | My active sessions |
| `DELETE /api/v1/users/me/sessions/{id}` | DELETE | Revoke a session |
| `POST /api/v1/users/me/sessions/revoke-all` | POST | Revoke all sessions |

### Workspace Membership

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/workspaces/{id}/members` | GET | List workspace members |
| `POST /api/v1/workspaces/{id}/invite` | POST | Invite user (Owner/Admin only) |
| `POST /api/v1/workspaces/{id}/invite/accept` | POST | Accept invitation |
| `PUT /api/v1/workspaces/{id}/members/{user_id}/role` | PUT | Change member role |
| `DELETE /api/v1/workspaces/{id}/members/{user_id}` | DELETE | Remove member |

### Teams

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/workspaces/{id}/teams` | GET | List teams |
| `POST /api/v1/workspaces/{id}/teams` | POST | Create team |
| `PUT /api/v1/workspaces/{id}/teams/{team_id}` | PUT | Update team |
| `POST /api/v1/workspaces/{id}/teams/{team_id}/members` | POST | Add member to team |
| `DELETE /api/v1/workspaces/{id}/teams/{team_id}/members/{user_id}` | DELETE | Remove from team |

### Notifications

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/notifications` | GET | My notifications (paginated, filterable) |
| `GET /api/v1/notifications/unread-count` | GET | Unread count (for bell badge) |
| `PUT /api/v1/notifications/{id}/read` | PUT | Mark read |
| `POST /api/v1/notifications/mark-all-read` | POST | Mark all read |
| `GET /api/v1/notifications/preferences` | GET | Notification channel preferences |
| `PUT /api/v1/notifications/preferences` | PUT | Update preferences |
| `GET /api/v1/notifications/stream` | GET (SSE) | Real-time notification stream |

## CLI

```bash
gyre whoami                                  # Current user profile
gyre profile set --display-name "Jordan Sell" --timezone "America/New_York"
gyre sessions list                           # Active sessions
gyre sessions revoke <session-id>
gyre sessions revoke-all

gyre workspace members list
gyre workspace invite user@example.com --role Developer
gyre workspace members set-role @asmith Admin

gyre team create "Platform Team" --lead @jsell
gyre team add @asmith --team "Platform Team"
gyre team list

gyre notifications list                      # Recent notifications
gyre notifications list --unread --priority high
gyre notifications preferences set --email-digest hourly --email-min-priority high
gyre notifications preferences set --slack-url https://hooks.slack.com/...
```

## UI Pages

| Page | Purpose |
|---|---|
| My Dashboard | Landing page with my tasks, MRs, agents, approvals, notifications |
| User Profile (`/@username`) | Public profile with activity feed |
| User Settings | Edit display name, timezone, preferences, notification channels |
| Session Management | Active sessions with revoke |
| Workspace Members | Member list, invite, role management |
| Team Management | Create/edit teams, manage membership |
| Notification Drawer | Slide-out panel from bell icon |
| Notification Preferences | Channel configuration, priority filters |

---

## Remaining Gaps for Enterprise Readiness

Beyond this spec, the following gaps still need dedicated specs:

### Search (Not Yet Specced)

At scale, users need to find things across the tenant:
- Full-text search of specs, tasks, MR titles/descriptions, commit messages
- Faceted filtering (by workspace, repo, status, author, date range)
- Search results scoped by user's access (can't find things in workspaces you're not a member of)
- Agent-queryable (agents should be able to search too, via MCP)

### ABAC Policy Engine (Not Yet Specced)

ABAC is referenced throughout but never fully specified:
- What attributes are available for policy decisions?
- How are policies defined (declarative, in the manifest? in the admin panel?)
- How is the policy engine evaluated (middleware? per-handler?)
- How do policies compose (tenant policies + workspace policies + repo policies)?
- Audit logging of policy decisions

### External Integrations / Webhooks (Not Yet Specced)

Beyond notifications, the platform needs general outbound integration:
- Webhook endpoints for external systems to subscribe to Gyre events
- JIRA sync (tasks <-> JIRA issues)
- Slack bot (beyond notification webhooks - interactive slash commands)
- GitHub/GitLab mirroring triggers
- CI/CD pipeline triggers (ArgoCD, Tekton, etc.)

### Deployment / CD (Not Yet Specced)

The release process creates artifacts but doesn't deploy them:
- How does a Gyre release get deployed?
- How do applications built on Gyre get deployed?
- ArgoCD integration? Tekton? Kubernetes manifests?

## Relationship to Existing Specs

- **Platform Model** (`platform-model.md`): users belong to tenants, access workspaces through membership. This spec defines the membership model.
- **Identity & Security** (`identity-security.md`): SSO provides the external_id, email, and initial profile. This spec defines what Gyre does with that identity beyond auth.
- **Spec Registry** (`spec-registry.md`): spec approvers are users. Notifications route approval requests to the right humans.
- **Agent Gates** (`agent-gates.md`): gate failures notify the MR author's spawning user.
- **Resource Governance** (`platform-model.md` section 5): budget warnings/exhaustion notify workspace admins.
- **Admin Panel** (`admin-panel.md`): user management and session management are admin panel features.
