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

### Tenant-Level User Onboarding

Before a user can join workspaces, they must exist in the tenant. Two modes:

**SSO mode (Keycloak/OIDC):**
- Users are auto-provisioned on first SSO login (existing behavior in `auth.rs`)
- SCIM handles bulk provisioning from the IdP
- Tenant admin can also manually invite by email

**Local mode (no external IdP):**
- Gyre manages users directly (no Keycloak dependency)
- Tenant admin invites users by email
- Invitee receives a magic link or sets a password
- Local user accounts are first-class, not second-class

This is critical for running Gyre locally, in air-gapped environments, or for teams that don't have Keycloak. The platform must work without any external identity system.

```rust
pub struct TenantInvitation {
    pub id: Id,
    pub tenant_id: Id,
    pub email: String,
    pub invited_by: Id,
    pub role: GlobalRole,         // TenantAdmin or Member
    pub workspace_ids: Vec<Id>,   // Optionally pre-assign workspaces
    pub workspace_roles: Vec<WorkspaceRole>, // Role per pre-assigned workspace
    pub status: InvitationStatus,
    pub token_hash: String,       // SHA-256 of invitation token (for magic link)
    pub expires_at: u64,
    pub created_at: u64,
    pub accepted_at: Option<u64>,
}

pub enum InvitationStatus {
    Pending,
    Accepted,
    Declined,
    Expired,
    Revoked,
}
```

**Tenant invitation flow:**

```
1. Tenant admin invites user@example.com
   - Optionally: pre-assign to workspaces with roles
   - Invitation token generated (cryptographically random)
   - Invitation expires after configurable period (default: 7 days)

2. Invitee receives email with magic link:
   https://gyre.example.com/invite/{token}

3. Invitee clicks link:
   - SSO mode: redirected to Keycloak login, then account linked
   - Local mode: prompted to set display name + password

4. Account created, workspace memberships activated

5. Invitee lands on their dashboard with pre-assigned workspaces
```

**Bulk invitation:**

Tenant admins can invite multiple users at once:

```json
POST /api/v1/tenant/invite/bulk
{
  "invitations": [
    {
      "email": "alice@example.com",
      "workspace_ids": ["ws-platform"],
      "workspace_roles": ["Developer"]
    },
    {
      "email": "bob@example.com",
      "workspace_ids": ["ws-platform", "ws-apps"],
      "workspace_roles": ["Admin", "Developer"]
    }
  ],
  "expires_in_days": 7
}
```

For SCIM-managed tenants, bulk invite is rarely needed (users auto-provision). For local-mode tenants, this is the primary onboarding mechanism.

### Workspace Invitation Flow

Once a user exists in the tenant, workspace access is granted through invitation:

1. Workspace Owner/Admin invites user by username (tenant-internal) or email
2. Invitation created with configurable expiry (default: 7 days)
3. Invitee receives notification (in-app always, email if configured)
4. Invitee accepts or declines
5. On accept: membership is active, user gains access to workspace repos
6. Pending invitations visible in workspace settings and admin panel
7. Expired invitations are automatically cleaned up by a background job

Users don't self-join workspaces. Access is always granted by an existing member with Owner/Admin role.

**Cross-tenant sharing:** Not supported. Tenants are hard isolation boundaries. A user in Tenant A cannot be invited to a workspace in Tenant B. If cross-org collaboration is needed, users must have accounts in both tenants.

### Invitation Expiry

All invitations (tenant and workspace) expire:

```rust
pub struct InvitationPolicy {
    pub tenant_invite_expiry_days: u32,     // Default: 7
    pub workspace_invite_expiry_days: u32,  // Default: 7
    pub max_pending_invitations: u32,       // Per workspace, default: 50
    pub allow_re_invite: bool,              // Re-invite after expiry, default: true
}
```

- Expired invitations are marked `Expired` by a background job (not deleted, for audit)
- Admins can revoke pending invitations at any time
- Re-inviting after expiry creates a new invitation with a new token

### Ownership Transfer & Reclamation

When a workspace Owner leaves (account deactivated, removed from tenant):

1. Tenant Admin receives a notification: "Workspace X has no active Owner"
2. Tenant Admin can reassign ownership via ABAC-permitted action:
   ```
   PUT /api/v1/workspaces/{id}/members/{new_owner_id}/role
   { "role": "Owner" }
   ```
3. This is an ABAC-enforced action: `subject.global_role == TenantAdmin` AND `action == transfer_ownership`
4. The action is audit-logged with the previous owner and reason
5. If no Tenant Admin acts within a configurable period (default: 30 days), the workspace is flagged in the admin dashboard as "orphaned"

Workspace Owners can also voluntarily transfer ownership:
- Must designate a new Owner before downgrading their own role
- Cannot remove themselves as Owner if they're the last Owner
- Transfer is audit-logged

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

### Tenant Invitations

| Endpoint | Method | Purpose |
|---|---|---|
| `POST /api/v1/tenant/invite` | POST | Invite user to tenant (TenantAdmin only) |
| `POST /api/v1/tenant/invite/bulk` | POST | Bulk invite (TenantAdmin only) |
| `GET /api/v1/tenant/invitations` | GET | List pending/expired/accepted invitations |
| `DELETE /api/v1/tenant/invitations/{id}` | DELETE | Revoke pending invitation |
| `POST /api/v1/invite/{token}/accept` | POST | Accept invitation via magic link token |

### User Management

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/users/me` | GET | Current user profile + preferences |
| `PUT /api/v1/users/me` | PUT | Update display name, timezone, locale, preferences |
| `GET /api/v1/users/{username}` | GET | User profile (tenant-public) |
| `GET /api/v1/users` | GET | List users (tenant admin only) |
| `PUT /api/v1/users/{id}/deactivate` | PUT | Deactivate user (TenantAdmin only) |
| `GET /api/v1/users/me/sessions` | GET | My active sessions |
| `DELETE /api/v1/users/me/sessions/{id}` | DELETE | Revoke a session |
| `POST /api/v1/users/me/sessions/revoke-all` | POST | Revoke all sessions |

### Workspace Membership

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/workspaces/{id}/members` | GET | List workspace members |
| `POST /api/v1/workspaces/{id}/invite` | POST | Invite user (Owner/Admin only) |
| `GET /api/v1/workspaces/{id}/invitations` | GET | List pending invitations |
| `POST /api/v1/workspaces/{id}/invite/accept` | POST | Accept invitation |
| `DELETE /api/v1/workspaces/{id}/invitations/{id}` | DELETE | Revoke invitation |
| `PUT /api/v1/workspaces/{id}/members/{user_id}/role` | PUT | Change member role (including ownership transfer) |
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
# Identity
gyre whoami                                  # Current user profile
gyre profile set --display-name "Jordan Sell" --timezone "America/New_York"
gyre sessions list                           # Active sessions
gyre sessions revoke <session-id>
gyre sessions revoke-all

# Tenant invitations (TenantAdmin only)
gyre tenant invite alice@example.com                              # Invite to tenant
gyre tenant invite alice@example.com --workspace platform --role Developer  # With workspace pre-assignment
gyre tenant invite --bulk users.csv                               # Bulk invite from CSV
gyre tenant invitations list                                      # Pending invitations
gyre tenant invitations revoke <invitation-id>

# Workspace membership
gyre workspace members list
gyre workspace invite @alice --role Developer                     # By username (already in tenant)
gyre workspace invite alice@example.com --role Developer          # By email (sends tenant invite if needed)
gyre workspace members set-role @asmith Admin
gyre workspace members set-role @asmith Owner --transfer          # Transfer ownership
gyre workspace members remove @asmith
gyre workspace invitations list
gyre workspace invitations revoke <invitation-id>

# Teams
gyre team create "Platform Team" --lead @jsell
gyre team add @asmith --team "Platform Team"
gyre team list
gyre team remove @asmith --team "Platform Team"

# Notifications
gyre notifications list                      # Recent notifications
gyre notifications list --unread --priority high
gyre notifications preferences set --email-digest hourly --email-min-priority high
gyre notifications preferences set --slack-url https://hooks.slack.com/...
```

## UI Pages

| Page | Purpose |
|---|---|
| My Dashboard | Landing page with my tasks, MRs, agents, approvals, notifications, pending invitations |
| User Profile (`/@username`) | Public profile with activity feed |
| User Settings | Edit display name, timezone, preferences, notification channels |
| Session Management | Active sessions with revoke |
| Tenant User Management | User list, invite, deactivate, role management (TenantAdmin only) |
| Tenant Invitation Management | Pending/expired/accepted invitations, bulk invite, revoke (TenantAdmin only) |
| Workspace Members | Member list, invite modal, role management, ownership transfer |
| Workspace Invitations | Pending invitations with expiry countdown, revoke |
| Pending Invitation Accept Page | `/invite/{token}` - accept/decline with workspace preview |
| Team Management | Create/edit teams, manage membership |
| Notification Drawer | Slide-out panel from bell icon |
| Notification Preferences | Channel configuration, priority filters |

---

## Remaining Gaps for Enterprise Readiness

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
