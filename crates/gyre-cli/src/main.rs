mod bootstrap;
mod client;
mod config;
mod tui;
mod ws;

use anyhow::Result;
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use gyre_common::WsMessage;
use tokio_tungstenite::tungstenite::Message;
use tracing::info;

const DEFAULT_SERVER: &str = "ws://localhost:3000/ws";
const DEFAULT_TOKEN: &str = "gyre-dev-token";

#[derive(Parser)]
#[command(name = "gyre", about = "Gyre platform CLI", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Connect to the Gyre server and stay connected
    Connect {
        #[arg(long, default_value = DEFAULT_SERVER)]
        server: String,
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
    /// Send a ping and print the round-trip time
    Ping {
        #[arg(long, default_value = DEFAULT_SERVER)]
        server: String,
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
    /// Check server health via HTTP
    Health {
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
    },
    /// Launch the TUI dashboard
    Tui {
        #[arg(long, default_value = DEFAULT_SERVER)]
        server: String,
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
    /// Register this CLI as a Gyre agent and save credentials to ~/.gyre/config
    Init {
        /// Gyre server base URL
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Agent name to register
        #[arg(long)]
        name: String,
        /// Use this token to authenticate the registration call (dev/system token)
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
    /// Bootstrap a fresh Gyre platform: tenant, workspace, repo, personas, gates,
    /// and a repo orchestrator, all in one run (platform-model.md §8)
    Bootstrap {
        /// Gyre server base URL
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Token used to authenticate bootstrap API calls. Defaults to
        /// $GYRE_AUTH_TOKEN when set, else the static dev token.
        #[arg(long)]
        token: Option<String>,
        /// Tenant display name (e.g. "Acme Corp")
        #[arg(long)]
        tenant: Option<String>,
        /// Workspace display name (e.g. "Platform Team")
        #[arg(long)]
        workspace: Option<String>,
        /// Repository name to register
        #[arg(long)]
        repo: Option<String>,
        /// Local path to the repo checkout (used for gate + spec detection,
        /// starter kit target)
        #[arg(long)]
        repo_path: Option<String>,
        /// Admin username to create (non-dev mode)
        #[arg(long)]
        admin_user: Option<String>,
        /// OIDC issuer URL for the tenant
        #[arg(long)]
        oidc_issuer: Option<String>,
        /// Dev mode: skip OIDC and admin user, use static tokens, tenant "dev",
        /// workspace "default"
        #[arg(long)]
        dev: bool,
        /// Create a starter spec structure in the repo path (or --repo-path)
        #[arg(long)]
        starter_kit: bool,
    },
    /// Clone a Gyre-hosted repository
    Clone {
        /// Repository in "project/repo" format, or a full Gyre git URL
        repo: String,
        /// Local directory to clone into (default: repo name)
        #[arg(long)]
        dir: Option<String>,
    },
    /// Push current branch to the Gyre server
    Push {
        /// Git remote name (default: origin)
        #[arg(long, default_value = "origin")]
        remote: String,
    },
    /// Merge request operations
    Mr {
        #[command(subcommand)]
        command: MrCommands,
    },
    /// Task operations
    Tasks {
        #[command(subcommand)]
        command: TaskCommands,
    },
    /// Show this agent's status and current task
    Status,
    /// Release automation: compute next version and generate changelog
    Release {
        #[command(subcommand)]
        command: ReleaseCommands,
    },
    /// Display workspace briefing narrative
    Briefing {
        /// Workspace slug (optional — if omitted, shows briefings for all accessible workspaces)
        #[arg(long)]
        workspace: Option<String>,
        /// Only show activity since this Unix epoch
        #[arg(long)]
        since: Option<u64>,
    },
    /// List and manage notifications (bare invocation lists all)
    Inbox {
        /// Workspace slug to filter by (applies to bare invocation)
        #[arg(long)]
        workspace: Option<String>,
        /// Priority range, e.g. "1-5" (applies to bare invocation)
        #[arg(long)]
        priority: Option<String>,
        #[command(subcommand)]
        command: Option<InboxCommands>,
    },
    /// Search the knowledge graph for a concept
    Explore {
        /// Concept name to search for
        concept: String,
        /// Repository name to scope the search (workspace inferred from git remote if --workspace omitted)
        #[arg(long)]
        repo: Option<String>,
        /// Workspace slug to scope the search
        #[arg(long)]
        workspace: Option<String>,
    },

    /// Full-text search across all entities (specs, tasks, MRs, commits, agents)
    Search {
        /// Search query — supports quoted phrases and facet:value syntax
        /// (e.g. "merge queue" type:spec status:approved)
        query: Option<String>,
        /// Filter by entity type (spec, task, mr, commit, agent)
        #[arg(long, short = 't')]
        r#type: Option<String>,
        /// Filter by status
        #[arg(long)]
        status: Option<String>,
        /// Filter by workspace slug
        #[arg(long, short = 'w')]
        workspace: Option<String>,
        /// Only results since this time (e.g., 7d, 2026-03-01)
        #[arg(long)]
        since: Option<String>,
        /// Autocomplete mode — return suggestions for the given prefix
        #[arg(long)]
        suggest: Option<String>,
        /// Maximum results to return
        #[arg(long, default_value = "20")]
        limit: usize,
    },

    /// Show system trace for a merge request
    Trace {
        /// Merge request ID
        mr_id: String,
    },
    /// Spec operations
    Spec {
        #[command(subcommand)]
        command: SpecCommands,
    },
    /// Show divergence (conflicting interpretation) alerts
    Divergence {
        /// Workspace slug to filter by
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Dependency graph operations
    Deps {
        #[command(subcommand)]
        command: DepsCommands,
    },
    /// Repository operations (status, revert, merge queue control)
    Repo {
        #[command(subcommand)]
        command: RepoCommands,
    },
    /// Budget governance: show usage and set limits
    ///
    /// The budget governance boundary is the workspace — repo scope (the
    /// default) resolves the current repo's owning workspace and operates on
    /// that workspace's budget (platform-model.md §5 CLI).
    Budget {
        #[command(subcommand)]
        command: BudgetCommands,
    },
    /// Secret management (platform-model.md §7 CLI)
    ///
    /// Admin-only server-side. Values are write-only: they are sent on
    /// set/rotate and never displayed or returned by list.
    Secret {
        #[command(subcommand)]
        command: SecretCommands,
    },
    /// Show the current user's profile (user-management.md §CLI > Identity)
    Whoami,
    /// Profile operations (user-management.md §CLI > Identity)
    Profile {
        #[command(subcommand)]
        command: ProfileCommands,
    },
    /// Session operations (user-management.md §CLI > Identity).
    ///
    /// Calls the §API > User Management session endpoints
    /// (`/api/v1/users/me/sessions[...`], owned server-side by task-111).
    Sessions {
        #[command(subcommand)]
        command: SessionsCommands,
    },
    /// Tenant invitation operations, TenantAdmin only
    /// (user-management.md §CLI > Tenant Invitations). Calls the §API >
    /// Tenant Invitations endpoints (`/api/v1/tenant/...`, owned
    /// server-side by task-110).
    Tenant {
        #[command(subcommand)]
        command: TenantCommands,
    },
    /// Workspace membership operations (user-management.md §CLI >
    /// Workspace Membership). The target workspace is `--workspace <slug>`
    /// or the one inferred from the git remote.
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommands,
    },
    /// Team operations (user-management.md §CLI > Teams). The target
    /// workspace is `--workspace <slug>` or the one inferred from the git
    /// remote; teams are referenced by slug or name.
    Team {
        #[command(subcommand)]
        command: TeamCommands,
    },
    /// Notification operations (user-management.md §CLI > Notifications)
    Notifications {
        #[command(subcommand)]
        command: NotificationCommands,
    },
}

/// Scope selector shared by every `gyre secret` subcommand.
///
/// Resolution rules (mirrors `gyre budget`):
/// - `--repo <name>` selects repo scope; the workspace is taken from
///   `--workspace-name` or inferred from the git remote, then the repo id
///   is resolved within it.
/// - `--scope tenant` selects tenant scope (the caller's own tenant).
/// - Otherwise workspace scope: `--workspace-name` or the git remote.
#[derive(Subcommand)]
enum SecretCommands {
    /// Create a secret, or update the value of an existing secret with the
    /// same name in the same scope (the spec's "create/update")
    Set {
        /// Secret name, e.g. DATABASE_URL
        name: String,
        /// Secret value (plaintext; sent to the server, never displayed)
        value: String,
        /// Scope level: workspace (default), repo, or tenant
        #[arg(long)]
        scope: Option<String>,
        /// Workspace slug to target (default: inferred from the git remote)
        #[arg(long)]
        workspace_name: Option<String>,
        /// Repository name whose scope to target (selects repo scope)
        #[arg(long)]
        repo: Option<String>,
        /// Lifecycle class: static (default), ephemeral, rotated, or derived
        #[arg(long)]
        secret_type: Option<String>,
        /// Expiry as Unix epoch seconds
        #[arg(long)]
        expires_at: Option<u64>,
    },
    /// List secret metadata for a scope (names and rotation/expiry state —
    /// the server never returns values)
    List {
        /// Scope level: workspace (default), repo, or tenant
        #[arg(long)]
        scope: Option<String>,
        /// Workspace slug to target (default: inferred from the git remote)
        #[arg(long)]
        workspace_name: Option<String>,
        /// Repository name whose scope to target (selects repo scope)
        #[arg(long)]
        repo: Option<String>,
    },
    /// Delete a secret by name in the target scope
    Delete {
        /// Secret name to delete
        name: String,
        /// Scope level: workspace (default), repo, or tenant
        #[arg(long)]
        scope: Option<String>,
        /// Workspace slug to target (default: inferred from the git remote)
        #[arg(long)]
        workspace_name: Option<String>,
        /// Repository name whose scope to target (selects repo scope)
        #[arg(long)]
        repo: Option<String>,
    },
    /// Replace a secret's value (force rotation)
    Rotate {
        /// Secret name to rotate
        name: String,
        /// New plaintext value
        new_value: String,
        /// Scope level: workspace (default), repo, or tenant
        #[arg(long)]
        scope: Option<String>,
        /// Workspace slug to target (default: inferred from the git remote)
        #[arg(long)]
        workspace_name: Option<String>,
        /// Repository name whose scope to target (selects repo scope)
        #[arg(long)]
        repo: Option<String>,
        /// New expiry as Unix epoch seconds (omit to keep the current expiry)
        #[arg(long)]
        expires_at: Option<u64>,
    },
}

/// `gyre profile` subcommands (user-management.md §CLI > Identity).
#[derive(Subcommand)]
enum ProfileCommands {
    /// Update your profile fields. Omitted fields keep their current
    /// values (server-side partial update).
    Set {
        /// Display name (human-readable, editable)
        #[arg(long)]
        display_name: Option<String>,
        /// IANA timezone, e.g. "America/New_York"
        #[arg(long)]
        timezone: Option<String>,
        /// i18n locale, e.g. "en-US"
        #[arg(long)]
        locale: Option<String>,
        /// Avatar URL
        #[arg(long)]
        avatar_url: Option<String>,
    },
}

/// `gyre sessions` subcommands (user-management.md §CLI > Identity).
#[derive(Subcommand)]
enum SessionsCommands {
    /// List your active sessions
    List,
    /// Revoke a single session
    Revoke {
        /// Session ID
        id: String,
    },
    /// Revoke all sessions ("sign out everywhere")
    RevokeAll,
}

/// `gyre tenant` subcommands (user-management.md §CLI > Tenant
/// Invitations; TenantAdmin only server-side).
#[derive(Subcommand)]
enum TenantCommands {
    /// Invite a user to the tenant by email, optionally pre-assigning a
    /// workspace and role. With `--bulk`, reads invitees from a CSV file
    /// (columns: email[,workspace][,role]) instead of the positional.
    Invite {
        /// Invitee email (omit when using --bulk)
        #[arg(value_name = "EMAIL")]
        email: Option<String>,
        /// Workspace slug to pre-assign the invitee to
        #[arg(long)]
        workspace: Option<String>,
        /// Workspace role for the pre-assignment (default Developer;
        /// requires --workspace)
        #[arg(long)]
        role: Option<String>,
        /// Bulk invite from a CSV file (columns: email[,workspace][,role];
        /// an "email" header row is recognized and skipped)
        #[arg(long)]
        bulk: Option<std::path::PathBuf>,
    },
    /// Invitation operations
    Invitations {
        #[command(subcommand)]
        command: TenantInvitationCommands,
    },
}

/// `gyre tenant invitations` subcommands.
#[derive(Subcommand)]
enum TenantInvitationCommands {
    /// List the tenant's invitations
    List,
    /// Revoke a pending invitation
    Revoke {
        /// Invitation ID
        id: String,
    },
}

/// `gyre workspace` subcommands (user-management.md §CLI > Workspace
/// Membership).
#[derive(Subcommand)]
enum WorkspaceCommands {
    /// Member operations
    Members {
        #[command(subcommand)]
        command: MemberCommands,
    },
    /// Invite a user to the workspace: `@username` (a tenant member) or an
    /// email address (sends a tenant invitation with this workspace and
    /// role pre-assigned).
    Invite {
        /// Invitee: @username or email address
        user: String,
        /// Workspace role to grant (default Developer)
        #[arg(long)]
        role: Option<String>,
        /// Workspace slug to target (default: inferred from the git remote)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Invitation operations
    Invitations {
        #[command(subcommand)]
        command: WorkspaceInvitationCommands,
    },
}

/// `gyre workspace members` subcommands.
#[derive(Subcommand)]
enum MemberCommands {
    /// List workspace members
    List {
        /// Workspace slug to target (default: inferred from the git remote)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Change a member's workspace role
    SetRole {
        /// Member: @username or user ID
        user: String,
        /// New role: Owner, Admin, Developer, or Viewer
        role: String,
        /// Explicit ownership-transfer acknowledgment (requires role
        /// Owner; the server audits the transfer)
        #[arg(long)]
        transfer: bool,
        /// Reason recorded in the audit trail
        #[arg(long)]
        reason: Option<String>,
        /// Workspace slug to target (default: inferred from the git remote)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Remove a member from the workspace
    Remove {
        /// Member: @username or user ID
        user: String,
        /// Workspace slug to target (default: inferred from the git remote)
        #[arg(long)]
        workspace: Option<String>,
    },
}

/// `gyre workspace invitations` subcommands.
#[derive(Subcommand)]
enum WorkspaceInvitationCommands {
    /// List the workspace's pending invitations
    List {
        /// Workspace slug to target (default: inferred from the git remote)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Revoke a pending invitation
    Revoke {
        /// Invitation ID
        id: String,
        /// Workspace slug to target (default: inferred from the git remote)
        #[arg(long)]
        workspace: Option<String>,
    },
}

/// `gyre team` subcommands (user-management.md §CLI > Teams).
#[derive(Subcommand)]
enum TeamCommands {
    /// Create a team
    Create {
        /// Team name
        name: String,
        /// Team lead: @username or user ID (must be a workspace member)
        #[arg(long)]
        lead: Option<String>,
        /// Team description
        #[arg(long)]
        description: Option<String>,
        /// Workspace slug to target (default: inferred from the git remote)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Add a user to a team
    Add {
        /// User: @username or user ID
        user: String,
        /// Team slug or name
        #[arg(long)]
        team: String,
        /// Workspace slug to target (default: inferred from the git remote)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// List the workspace's teams
    List {
        /// Workspace slug to target (default: inferred from the git remote)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Remove a user from a team
    Remove {
        /// User: @username or user ID
        user: String,
        /// Team slug or name
        #[arg(long)]
        team: String,
        /// Workspace slug to target (default: inferred from the git remote)
        #[arg(long)]
        workspace: Option<String>,
    },
}

/// `gyre notifications` subcommands (user-management.md §CLI >
/// Notifications).
#[derive(Subcommand)]
enum NotificationCommands {
    /// List your notifications
    List {
        /// Only unread notifications (client-side filter on the read flag)
        #[arg(long)]
        unread: bool,
        /// Four-level priority filter: low, medium, high, or urgent
        #[arg(long)]
        priority: Option<String>,
        /// Notification type filter (e.g. SpecApprovalRequested)
        #[arg(long, value_name = "TYPE")]
        r#type: Option<String>,
        /// Workspace slug to filter by
        #[arg(long)]
        workspace: Option<String>,
        /// Maximum results (server caps at 200)
        #[arg(long, default_value = "50")]
        limit: u32,
    },
    /// Preference operations
    Preferences {
        #[command(subcommand)]
        command: NotificationPreferencesCommands,
    },
}

/// `gyre notifications preferences` subcommands. `set` performs a
/// read-merge-write against GET/PUT /api/v1/notifications/preferences so
/// channels you do not touch keep their current configuration.
#[derive(Subcommand)]
enum NotificationPreferencesCommands {
    /// Update delivery-channel preferences. Omitted channels keep their
    /// current values.
    Set {
        /// Email digest frequency: immediate, hourly, daily, or off
        /// (enables the email channel)
        #[arg(long)]
        email_digest: Option<String>,
        /// Minimum priority for email delivery: low, medium, high, urgent
        /// (enables the email channel)
        #[arg(long)]
        email_min_priority: Option<String>,
        /// Slack incoming-webhook URL (configures the Slack channel)
        #[arg(long)]
        slack_url: Option<String>,
        /// Minimum priority for Slack delivery: low, medium, high, urgent
        #[arg(long)]
        slack_min_priority: Option<String>,
        /// Outbound webhook URL (configures the webhook channel)
        #[arg(long)]
        webhook_url: Option<String>,
        /// HMAC secret signing outbound webhook payloads
        #[arg(long)]
        webhook_secret: Option<String>,
        /// Minimum priority for webhook delivery: low, medium, high, urgent
        #[arg(long)]
        webhook_min_priority: Option<String>,
    },
}

#[derive(Subcommand)]
enum ReleaseCommands {
    /// Compute next semver version and generate changelog from conventional commits
    Prepare {
        /// Repository ID to analyze
        #[arg(long)]
        repo_id: String,
        /// Branch to analyze (default: repo's default branch)
        #[arg(long)]
        branch: Option<String>,
        /// Override "from" tag/ref for changelog range
        #[arg(long)]
        from: Option<String>,
        /// Create a release MR after computing the changelog
        #[arg(long)]
        create_mr: bool,
        /// Gyre server base URL
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Auth token
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
        /// Output changelog markdown to stdout instead of summary
        #[arg(long)]
        markdown: bool,
    },
}

#[derive(Subcommand)]
enum RepoCommands {
    /// Show repo status: main health (green/broken) and merge queue state
    Status {
        /// Repository ID
        #[arg(long)]
        repo_id: String,
        /// Gyre server base URL
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Auth token
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
    /// Manually revert a merged MR
    Revert {
        /// Merge request ID
        mr_id: String,
        /// Repository ID
        #[arg(long)]
        repo_id: String,
        /// Gyre server base URL
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Auth token
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
    /// Merge queue control (pause/resume)
    Queue {
        #[command(subcommand)]
        command: QueueCommands,
    },
}

#[derive(Subcommand)]
enum QueueCommands {
    /// Manually pause the merge queue
    Pause {
        /// Repository ID
        #[arg(long)]
        repo_id: String,
        /// Reason for the pause
        #[arg(long)]
        reason: Option<String>,
        /// Gyre server base URL
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Auth token
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
    /// Manually resume the merge queue
    Resume {
        /// Repository ID
        #[arg(long)]
        repo_id: String,
        /// Gyre server base URL
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Auth token
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
}

#[derive(Subcommand)]
enum MrCommands {
    /// Create a merge request for the current branch
    Create {
        /// MR title
        #[arg(long)]
        title: String,
        /// Target branch (default: main)
        #[arg(long, default_value = "main")]
        target: String,
        /// Repository ID (required)
        #[arg(long)]
        repo_id: String,
        /// Source branch (default: current git branch)
        #[arg(long)]
        source: Option<String>,
    },
}

#[derive(Subcommand)]
enum TaskCommands {
    /// List tasks
    List {
        /// Filter by status (backlog, in_progress, review, done, blocked)
        #[arg(long)]
        status: Option<String>,
        /// Only show tasks assigned to me
        #[arg(long)]
        mine: bool,
    },
    /// Assign a task to this agent and mark it in_progress
    Take {
        /// Task ID
        id: String,
    },
}

#[derive(Subcommand)]
enum InboxCommands {
    /// List notifications (same as bare `gyre inbox`)
    List {
        /// Workspace slug to filter by
        #[arg(long)]
        workspace: Option<String>,
        /// Priority range (e.g., "1-5")
        #[arg(long)]
        priority: Option<String>,
    },
    /// Dismiss a notification
    Dismiss {
        /// Notification ID
        id: String,
    },
    /// Resolve a notification
    Resolve {
        /// Notification ID
        id: String,
    },
}

#[derive(Subcommand)]
enum SpecCommands {
    /// Get LLM-suggested edits for a spec file
    Assist {
        /// Spec file path within the repository
        path: String,
        /// Instruction describing what to change
        instruction: String,
        /// Repository name (optional — inferred from git remote if omitted)
        #[arg(long)]
        repo: Option<String>,
        /// Workspace slug (optional — inferred from git remote if omitted)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Show all links for a spec (outbound and inbound)
    Links {
        /// Spec file path (e.g., system/identity-security.md)
        path: String,
    },
    /// Show specs that depend on the given spec
    Dependents {
        /// Spec file path (e.g., system/source-control.md)
        path: String,
    },
    /// Display the tenant-wide spec dependency graph
    Graph {
        /// Output format: "text" (default) or "dot" (Graphviz DOT)
        #[arg(long)]
        format: Option<String>,
    },
    /// List all stale links across the tenant
    StaleLinks,
    /// List all active conflicts
    Conflicts,
}

#[derive(Subcommand)]
enum DepsCommands {
    /// Show dependencies and dependents
    Show {
        /// Repository name (default: this repo, inferred from the git remote)
        repo: Option<String>,
        /// Show workspace-wide dependency graph
        #[arg(long)]
        workspace: bool,
        /// Show tenant-wide dependency graph
        #[arg(long)]
        tenant: bool,
    },
    /// Output dependency graph in DOT format
    Graph {
        /// Output format (dot)
        #[arg(long, default_value = "dot")]
        format: String,
    },
    /// Show blast radius (all transitive dependents) for a repo
    Impact {
        /// Repository name
        repo: String,
    },
    /// List all stale dependencies
    Stale,
    /// List unacknowledged breaking changes
    Breaking,
    /// Add a manual dependency from this repo to another
    Add {
        /// Target repository name
        #[arg(long)]
        target: String,
        /// Dependency type (code, spec, api, schema, manual)
        #[arg(long, rename_all = "snake_case")]
        r#type: String,
    },
    /// Acknowledge a breaking change
    Acknowledge {
        /// Breaking change ID
        id: String,
    },
}

#[derive(Subcommand)]
enum BudgetCommands {
    /// Show budget limits and real-time usage.
    ///
    /// With no flags, resolves the current repo's owning workspace from the
    /// git remote and shows that workspace's budget — there is no separate
    /// repo-level budget store; repo scope maps to the owning workspace.
    /// `--workspace` scopes to a workspace (named via --workspace-name, else
    /// inferred from the git remote). `--tenant` shows the tenant-wide
    /// summary (Admin only).
    Show {
        /// Show workspace-level usage (the named/current workspace)
        #[arg(long, conflicts_with = "tenant")]
        workspace: bool,
        /// Show tenant-level usage summary (Admin only)
        #[arg(long, conflicts_with = "workspace")]
        tenant: bool,
        /// Workspace slug to target (with --workspace; without it, the
        /// workspace is inferred from the git remote)
        #[arg(long)]
        workspace_name: Option<String>,
        /// Repository name whose owning workspace to target (validated
        /// against the resolved workspace; requires --workspace-name or a
        /// gyre git remote)
        #[arg(long)]
        repo: Option<String>,
    },
    /// Set budget limits (Admin only).
    ///
    /// With no flags, sets limits on the current repo's owning workspace
    /// (resolved from the git remote — repo scope maps to the owning
    /// workspace; there is no repo-level budget store). `--workspace`
    /// targets a workspace named via --workspace-name (or inferred from the
    /// git remote). Omitted limits keep their current values. Setting
    /// tenant-level limits is not supported by the server API.
    Set {
        /// Set limits on a workspace (the named/current one)
        #[arg(long, conflicts_with = "tenant")]
        workspace: bool,
        /// Tenant scope (not supported for set — no tenant-level set endpoint)
        #[arg(long, conflicts_with = "workspace")]
        tenant: bool,
        /// Max LLM tokens per day
        #[arg(long)]
        llm_tokens: Option<u64>,
        /// Max LLM cost per day (USD)
        #[arg(long)]
        llm_cost: Option<f64>,
        /// Max concurrent agents
        #[arg(long)]
        max_agents: Option<u32>,
        /// Max agent lifetime in seconds
        #[arg(long)]
        max_agent_lifetime_secs: Option<u64>,
        /// Workspace slug to target (with --workspace; without it, the
        /// workspace is inferred from the git remote)
        #[arg(long)]
        workspace_name: Option<String>,
        /// Repository name whose owning workspace to target (validated
        /// against the resolved workspace; requires --workspace-name or a
        /// gyre git remote)
        #[arg(long)]
        repo: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Connect { server, token } => {
            info!("Connecting to {server}");
            let client = ws::WsClient::new(server.clone(), token);
            let mut ws = client.connect_and_auth().await?;
            println!("Connected to {server}. Listening for messages (Ctrl-C to quit)...");
            while let Some(frame) = ws.next().await {
                match frame? {
                    Message::Text(text) => {
                        let msg: Result<WsMessage, _> = serde_json::from_str(&text);
                        match msg {
                            Ok(m) => println!("{m:?}"),
                            Err(_) => println!("Raw: {text}"),
                        }
                    }
                    Message::Close(_) => {
                        println!("Server closed connection");
                        break;
                    }
                    _ => {}
                }
            }
        }

        Commands::Ping { server, token } => {
            info!("Pinging {server}");
            let client = ws::WsClient::new(server.clone(), token);
            let mut ws = client.connect_and_auth().await?;
            let rtt = client.ping(&mut ws).await?;
            println!("Pong from {server}: RTT {rtt}ms");
        }

        Commands::Health { server } => {
            let url = if server.starts_with("ws://") {
                server.replacen("ws://", "http://", 1)
            } else if server.starts_with("wss://") {
                server.replacen("wss://", "https://", 1)
            } else {
                server
            };
            let health_url = format!("{url}/health");
            info!("Checking health at {health_url}");
            let resp = reqwest::get(&health_url)
                .await
                .map_err(|e| anyhow::anyhow!("HTTP request failed: {e}"))?;
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            println!("HTTP {status}: {body}");
        }

        Commands::Tui { server, token } => {
            tui::run(server, token).await?;
        }

        Commands::Init {
            server,
            name,
            token,
        } => {
            let api = client::GyreClient::new(server.clone(), token);
            println!("Registering agent '{name}' with {server}...");
            let resp = api.register_agent(&name).await?;
            let cfg = config::Config {
                server,
                token: Some(resp.auth_token.clone()),
                agent_id: Some(resp.id.clone()),
                agent_name: Some(resp.name.clone()),
            };
            cfg.save()?;
            let path = config::Config::path();
            println!("Agent registered!");
            println!("  ID:     {}", resp.id);
            println!("  Name:   {}", resp.name);
            println!("  Status: {}", resp.status);
            println!("Config saved to {}", path.display());
        }

        Commands::Bootstrap {
            server,
            token,
            tenant,
            workspace,
            repo,
            repo_path,
            admin_user,
            oidc_issuer,
            dev,
            starter_kit,
        } => {
            // GYRE_AUTH_TOKEN pattern (server-config.md): explicit flag >
            // env > static dev token.
            let token = token
                .or_else(|| std::env::var("GYRE_AUTH_TOKEN").ok())
                .unwrap_or_else(|| DEFAULT_TOKEN.to_string());
            run_bootstrap(BootstrapArgs {
                server,
                token,
                tenant,
                workspace,
                repo,
                repo_path,
                admin_user,
                oidc_issuer,
                dev,
                starter_kit,
            })
            .await?;
        }

        Commands::Clone { repo, dir } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;

            // Build git URL: if "project/repo", construct from server; otherwise use as-is
            let git_url = if repo.starts_with("http://") || repo.starts_with("https://") {
                repo.clone()
            } else {
                // Expect "project/repo" or "project/repo.git"
                let normalized = if repo.ends_with(".git") {
                    repo.clone()
                } else {
                    format!("{repo}.git")
                };
                format!("{}/git/{normalized}", cfg.server)
            };

            // Local directory: use last path segment without .git
            let local_dir = dir.unwrap_or_else(|| {
                git_url
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("repo")
                    .trim_end_matches(".git")
                    .to_string()
            });

            println!("Cloning {git_url} into {local_dir}/");
            let status = std::process::Command::new("git")
                .args([
                    "-c",
                    &format!("http.extraHeader=Authorization: Bearer {token}"),
                    "clone",
                    &git_url,
                    &local_dir,
                ])
                .status()
                .map_err(|e| anyhow::anyhow!("failed to run git: {e}"))?;
            if !status.success() {
                anyhow::bail!("git clone failed");
            }
        }

        Commands::Push { remote } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;

            let status = std::process::Command::new("git")
                .args([
                    "-c",
                    &format!("http.extraHeader=Authorization: Bearer {token}"),
                    "push",
                    &remote,
                ])
                .status()
                .map_err(|e| anyhow::anyhow!("failed to run git: {e}"))?;
            if !status.success() {
                anyhow::bail!("git push failed");
            }
        }

        Commands::Mr {
            command:
                MrCommands::Create {
                    title,
                    target,
                    repo_id,
                    source,
                },
        } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let agent_id = cfg.agent_id.as_deref();

            // Detect current branch if --source not given
            let source_branch = match source {
                Some(b) => b,
                None => {
                    let out = std::process::Command::new("git")
                        .args(["rev-parse", "--abbrev-ref", "HEAD"])
                        .output()
                        .map_err(|e| anyhow::anyhow!("failed to run git: {e}"))?;
                    if !out.status.success() {
                        anyhow::bail!("could not detect current branch; use --source");
                    }
                    String::from_utf8_lossy(&out.stdout).trim().to_string()
                }
            };

            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());
            println!("Creating MR: '{title}' ({source_branch} → {target})");
            let mr = api
                .create_mr(&repo_id, &title, &source_branch, &target, agent_id)
                .await?;
            println!("MR created!");
            println!("  ID:     {}", mr.id);
            println!("  Title:  {}", mr.title);
            println!("  Branch: {} → {}", mr.source_branch, mr.target_branch);
            println!("  Status: {}", mr.status);
        }

        Commands::Tasks {
            command: TaskCommands::List { status, mine },
        } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            let assigned_to = if mine { cfg.agent_id.as_deref() } else { None };
            let tasks = api.list_tasks(status.as_deref(), assigned_to).await?;

            if tasks.is_empty() {
                println!("No tasks found.");
            } else {
                println!("{:<20} {:<12} {:<10} TITLE", "ID", "STATUS", "PRIORITY");
                println!("{}", "-".repeat(70));
                for t in &tasks {
                    println!(
                        "{:<20} {:<12} {:<10} {}",
                        t.id, t.status, t.priority, t.title
                    );
                }
            }
        }

        Commands::Tasks {
            command: TaskCommands::Take { id },
        } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let agent_id = cfg.require_agent_id()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            let task = api.assign_task(&id, agent_id).await?;
            println!("Task assigned to you: {}", task.id);

            // Also transition to in_progress (best-effort; task may already be in_progress)
            match api.transition_task_status(&id, "in_progress").await {
                Ok(t) => println!("Status: {} → {}", task.status, t.status),
                Err(e) => println!("Note: could not transition status: {e}"),
            }
        }

        Commands::Status => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let agent_id = cfg.require_agent_id()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            let agent = api.get_agent(agent_id).await?;
            println!("Agent Status");
            println!("  ID:           {}", agent.id);
            println!("  Name:         {}", agent.name);
            println!("  Status:       {}", agent.status);
            if let Some(task_id) = &agent.current_task_id {
                println!("  Current Task: {task_id}");
            } else {
                println!("  Current Task: (none)");
            }
            if let Some(hb) = agent.last_heartbeat {
                println!("  Last Heartbeat: {hb}");
            }
        }

        Commands::Release {
            command:
                ReleaseCommands::Prepare {
                    repo_id,
                    branch,
                    from,
                    create_mr,
                    server,
                    token,
                    markdown: show_markdown,
                },
        } => {
            let api = client::GyreClient::new(server.clone(), token.clone());
            let result = api
                .release_prepare(&repo_id, branch.as_deref(), from.as_deref(), create_mr)
                .await?;

            if show_markdown {
                if let Some(md) = result["changelog"].as_str() {
                    print!("{md}");
                }
            } else {
                let current = result["current_tag"].as_str().unwrap_or("(none)");
                let next = result["next_version"].as_str().unwrap_or("unknown");
                let bump = result["bump_type"].as_str().unwrap_or("none");
                let count = result["commit_count"].as_u64().unwrap_or(0);
                let has_release = result["has_release"].as_bool().unwrap_or(false);

                println!("Release Preparation");
                println!("  Current tag:   {current}");
                println!("  Next version:  {next}");
                println!("  Bump type:     {bump}");
                println!("  Commits since: {count}");
                println!("  Has release:   {has_release}");
                if let Some(mr_id) = result["mr_id"].as_str() {
                    println!("  Release MR:    {mr_id}");
                }
                println!();

                if let Some(sections) = result["sections"].as_array() {
                    if sections.is_empty() {
                        println!("No releasable changes found.");
                    } else {
                        for section in sections {
                            let title = section["title"].as_str().unwrap_or("Other");
                            println!("--- {title} ---");
                            if let Some(entries) = section["entries"].as_array() {
                                for e in entries {
                                    let desc = e["description"].as_str().unwrap_or("");
                                    let scope = e["scope"]
                                        .as_str()
                                        .map(|s| format!("({s}) "))
                                        .unwrap_or_default();
                                    let agent = e["agent_name"]
                                        .as_str()
                                        .or_else(|| e["agent_id"].as_str())
                                        .unwrap_or("");
                                    let task = e["task_id"].as_str().unwrap_or("");
                                    let mut attrs = Vec::new();
                                    if !agent.is_empty() {
                                        attrs.push(agent.to_string());
                                    }
                                    if !task.is_empty() {
                                        attrs.push(task.to_string());
                                    }
                                    let attr_str = if attrs.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" [{}]", attrs.join(", "))
                                    };
                                    println!("  - {scope}{desc}{attr_str}");
                                }
                            }
                            println!();
                        }
                    }
                }
                println!("Run with --markdown to output full changelog markdown.");
            }
        }

        Commands::Briefing { workspace, since } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            // If --workspace is given, show that workspace's briefing.
            // Otherwise, list all accessible workspaces and show briefings for each.
            let workspace_ids: Vec<(String, String)> = if let Some(slug) = &workspace {
                let wid = api.resolve_workspace_slug(slug).await?;
                vec![(slug.clone(), wid)]
            } else {
                let workspaces = api.list_workspaces().await?;
                workspaces
                    .iter()
                    .filter_map(|w| {
                        let id = w["id"].as_str()?;
                        let slug = w["slug"].as_str().or_else(|| w["name"].as_str())?;
                        Some((slug.to_string(), id.to_string()))
                    })
                    .collect()
            };

            if workspace_ids.is_empty() {
                println!("No accessible workspaces found.");
            }

            for (i, (slug, wid)) in workspace_ids.iter().enumerate() {
                if i > 0 {
                    println!();
                    println!("{}", "=".repeat(80));
                    println!();
                }

                let briefing = api.get_briefing(wid, since).await?;

                println!("Workspace Briefing: {slug}");
                println!();

                print_briefing(&briefing);
            }
        }

        Commands::Inbox {
            workspace,
            priority,
            command,
        } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            match command {
                None => {
                    // Bare `gyre inbox` — list notifications using top-level flags
                    print_notifications(&api, workspace.as_deref(), priority.as_deref()).await?;
                }
                Some(InboxCommands::List {
                    workspace: sub_ws,
                    priority: sub_pri,
                }) => {
                    // `gyre inbox list` — subcommand flags take precedence
                    let ws = sub_ws.as_deref().or(workspace.as_deref());
                    let pri = sub_pri.as_deref().or(priority.as_deref());
                    print_notifications(&api, ws, pri).await?;
                }
                Some(InboxCommands::Dismiss { id }) => {
                    api.dismiss_notification(&id).await?;
                    println!("Notification {id} dismissed.");
                }
                Some(InboxCommands::Resolve { id }) => {
                    api.resolve_notification(&id).await?;
                    println!("Notification {id} resolved.");
                }
            }
        }

        Commands::Explore {
            concept,
            repo,
            workspace,
        } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            // Determine which concept search endpoints to call.
            // If --repo is given (with --workspace), search that single repo.
            // If --repo is given without --workspace, infer workspace from git remote.
            // If --workspace is given (without --repo), search across all repos in the workspace.
            // If neither is given, search across all accessible workspaces.
            let results: Vec<serde_json::Value> = match (&workspace, &repo) {
                (Some(slug), Some(repo_name)) => {
                    let wid = api.resolve_workspace_slug(slug).await?;
                    let rid = api.resolve_repo_name(&wid, repo_name).await?;
                    vec![api.get_graph_concept(&concept, Some(&rid), None).await?]
                }
                (Some(slug), None) => {
                    let wid = api.resolve_workspace_slug(slug).await?;
                    vec![api.get_graph_concept(&concept, None, Some(&wid)).await?]
                }
                (None, Some(repo_name)) => {
                    // Infer workspace from git remote, then resolve repo name
                    let (ws_slug, _) = infer_repo_from_git_remote().ok_or_else(|| {
                        anyhow::anyhow!(
                            "could not infer workspace from git remote. \
                             Use --workspace <slug> --repo <name> to specify."
                        )
                    })?;
                    let wid = api.resolve_workspace_slug(&ws_slug).await?;
                    let rid = api.resolve_repo_name(&wid, repo_name).await?;
                    vec![api.get_graph_concept(&concept, Some(&rid), None).await?]
                }
                (None, None) => {
                    // Search across all accessible workspaces
                    let workspaces = api.list_workspaces().await?;
                    let mut all = Vec::new();
                    for ws in &workspaces {
                        if let Some(wid) = ws["id"].as_str() {
                            match api.get_graph_concept(&concept, None, Some(wid)).await {
                                Ok(r) => all.push(r),
                                Err(_) => continue, // skip inaccessible workspaces
                            }
                        }
                    }
                    all
                }
            };

            // Collect all nodes from all results
            let all_nodes: Vec<&serde_json::Value> = results
                .iter()
                .filter_map(|r| r["nodes"].as_array())
                .flatten()
                .collect();

            if all_nodes.is_empty() {
                println!("No matching graph nodes found for '{concept}'.");
            } else {
                println!(
                    "{:<12} {:<30} {:<50} {:<10} SPEC",
                    "TYPE", "NAME", "QUALIFIED_NAME", "CONFIDENCE"
                );
                println!("{}", "-".repeat(120));
                for n in &all_nodes {
                    let ntype = n["node_type"].as_str().unwrap_or("");
                    let name = n["name"].as_str().unwrap_or("");
                    let qname = n["qualified_name"].as_str().unwrap_or("");
                    let confidence = n["spec_confidence"].as_str().unwrap_or("None");
                    let spec = n["spec_path"].as_str().unwrap_or("-");
                    println!("{ntype:<12} {name:<30} {qname:<50} {confidence:<10} {spec}");
                }
            }
        }

        Commands::Search {
            query,
            r#type,
            status,
            workspace,
            since,
            suggest,
            limit,
        } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            let status_filter = status.as_deref().map(str::trim).filter(|s| !s.is_empty());
            let since_cutoff = match since.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(s) => Some(parse_since(s).ok_or_else(|| {
                    anyhow::anyhow!(
                        "invalid --since value '{s}' (expected <N><s|m|h|d|w> such as 7d, or a date like 2026-03-01)"
                    )
                })?),
                None => None,
            };

            // Autocomplete: the dedicated /search/suggest endpoint
            // (search.md §API, task-153) is not implemented yet; fall back
            // to a regular search over the prefix and keep title-prefix
            // matches (see `collect_suggestions`).
            let q = match suggest.as_deref() {
                Some(p) if !p.trim().is_empty() => p.trim().to_string(),
                _ => query.as_deref().unwrap_or_default().trim().to_string(),
            };

            if q.is_empty() {
                println!("No search query given. Usage: gyre search <query> [--type spec] [--status approved] [--workspace slug] [--since 7d] [--suggest prefix] [--limit N]");
            } else {
                let workspace_id = match &workspace {
                    Some(slug) => Some(api.resolve_workspace_slug(slug).await?),
                    None => None,
                };
                // `--type`/`--workspace` are enforced server-side
                // (entity_type / workspace_id params on GET /api/v1/search).
                // `--status`/`--since` filter client-side against live
                // entity state, so over-fetch candidates before filtering;
                // the server caps any limit at 100, which bounds how many
                // candidates a filtered search can scan.
                let filtering = status_filter.is_some() || since_cutoff.is_some();
                let fetch_limit = if filtering { limit.max(100) } else { limit };
                let entity_type = r#type.as_deref().map(str::trim).filter(|t| !t.is_empty());
                let mut response = api
                    .search(&q, entity_type, workspace_id.as_deref(), fetch_limit)
                    .await?;

                if filtering {
                    // Search index facets freeze at create time (the index
                    // write sites run only on entity creation), so resolve
                    // live status/recency per result before filtering.
                    let mut kept: Vec<client::SearchResult> = Vec::new();
                    let mut unresolvable = 0usize;
                    for r in &response.results {
                        let live = live_entity_state(&api, r).await;
                        if live.unavailable {
                            unresolvable += 1;
                        }
                        if result_matches_filters(status_filter, since_cutoff, &live) {
                            kept.push(r.clone());
                        }
                    }
                    if unresolvable > 0 {
                        eprintln!(
                            "note: {unresolvable} result(s) excluded — live status/recency \
                             could not be resolved for their entity type (spec/commit have \
                             no detail endpoint yet, or the record was deleted)"
                        );
                    }
                    response.total = kept.len();
                    kept.truncate(limit);
                    response.results = kept;
                }

                if let Some(prefix) = suggest.as_deref() {
                    print_search_suggestions(prefix, &response);
                } else {
                    print_search_results(&response);
                }
            }
        }

        Commands::Trace { mr_id } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            let result = api.get_mr_trace(&mr_id).await?;

            let commit_sha = result["commit_sha"].as_str().unwrap_or("unknown");
            let gate_run_id = result["gate_run_id"].as_str().unwrap_or("unknown");
            let span_count = result["span_count"].as_u64().unwrap_or(0);
            let captured_at = result["captured_at"].as_u64().unwrap_or(0);

            println!("Trace for MR {mr_id}");
            println!("  commit:     {commit_sha}");
            println!("  gate_run:   {gate_run_id}");
            println!("  captured:   {}", format_timestamp(captured_at));
            println!("  spans:      {span_count}");
            println!();

            let root_spans = result["root_spans"].as_array();
            if let Some(roots) = root_spans {
                let root_ids: Vec<&str> = roots.iter().filter_map(|v| v.as_str()).collect();
                println!("Root spans: {}", root_ids.join(", "));
                println!();
            }

            let spans = result["spans"].as_array();
            match spans {
                Some(items) if !items.is_empty() => {
                    println!(
                        "{:<20} {:<20} {:<30} {:>10} {:<8}",
                        "SPAN_ID", "SERVICE", "OPERATION", "DURATION", "STATUS"
                    );
                    println!("{}", "-".repeat(92));
                    for span in items {
                        let span_id = span["span_id"].as_str().unwrap_or("");
                        let service = span["service_name"].as_str().unwrap_or("");
                        let operation = span["operation_name"].as_str().unwrap_or("");
                        let duration_us = span["duration_us"].as_u64().unwrap_or(0);
                        let status = span["status"].as_str().unwrap_or("");
                        let duration_str = if duration_us >= 1_000_000 {
                            format!("{:.1}s", duration_us as f64 / 1_000_000.0)
                        } else if duration_us >= 1_000 {
                            format!("{:.1}ms", duration_us as f64 / 1_000.0)
                        } else {
                            format!("{duration_us}us")
                        };
                        println!(
                            "{:<20} {:<20} {:<30} {:>10} {:<8}",
                            span_id, service, operation, duration_str, status
                        );
                    }
                }
                _ => println!("No trace spans."),
            }
        }

        Commands::Spec { command } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            match command {
                SpecCommands::Assist {
                    path,
                    instruction,
                    repo,
                    workspace,
                } => {
                    // Resolve repo ID: from explicit flags, or infer from git remote
                    let (ws_slug, repo_name) = match (workspace, repo) {
                        (Some(ws), Some(r)) => (ws, r),
                        (Some(ws), None) => {
                            let (_, rn) = infer_repo_from_git_remote().ok_or_else(|| {
                                anyhow::anyhow!(
                                    "could not infer repository from git remote. \
                                     Use --repo <name> to specify."
                                )
                            })?;
                            (ws, rn)
                        }
                        (None, Some(repo_name)) => {
                            let (ws, _) = infer_repo_from_git_remote().ok_or_else(|| {
                                anyhow::anyhow!(
                                    "could not infer workspace from git remote. \
                                     Use --workspace <slug> --repo <name> to specify."
                                )
                            })?;
                            (ws, repo_name)
                        }
                        (None, None) => infer_repo_from_git_remote().ok_or_else(|| {
                            anyhow::anyhow!(
                                "could not infer repository from git remote. \
                                 Use --workspace <slug> --repo <name> to specify, \
                                 or run from a gyre-cloned repository."
                            )
                        })?,
                    };

                    let workspace_id = api.resolve_workspace_slug(&ws_slug).await?;
                    let repo_id = api.resolve_repo_name(&workspace_id, &repo_name).await?;

                    println!("Requesting spec assist for {path}...");
                    let ops = api.spec_assist(&repo_id, &path, &instruction).await?;

                    if ops.is_empty() {
                        println!("No suggestions returned.");
                    } else {
                        for op in &ops {
                            if let Some(error_msg) = op["error"].as_str() {
                                eprintln!("Error: {error_msg}");
                                continue;
                            }
                            if let Some(explanation) = op["explanation"].as_str() {
                                println!();
                                println!("Explanation: {explanation}");
                            }
                            if let Some(diff) = op["diff"].as_array() {
                                println!();
                                for d in diff {
                                    let op_type = d["op"].as_str().unwrap_or("unknown");
                                    let path = d["path"].as_str().unwrap_or("");
                                    let content = d["content"].as_str().unwrap_or("");
                                    println!("  [{op_type}] {path}");
                                    for line in content.lines() {
                                        println!("    {line}");
                                    }
                                }
                            }
                        }
                    }
                }

                SpecCommands::Links { path } => {
                    let links = api.get_spec_links(&path).await?;
                    if links.is_empty() {
                        println!("No links found for {path}.");
                    } else {
                        println!("Links for {path}");
                        println!();
                        print_spec_links_table(&links);
                    }
                }

                SpecCommands::Dependents { path } => {
                    let deps = api.get_spec_dependents(&path).await?;
                    if deps.is_empty() {
                        println!("No specs depend on {path}.");
                    } else {
                        println!("Specs that depend on {path}");
                        println!();
                        print_spec_links_table(&deps);
                    }
                }

                SpecCommands::Graph { format } => {
                    let graph = api.get_spec_graph().await?;
                    let fmt = format.as_deref().unwrap_or("text");
                    match fmt {
                        "dot" => print_spec_dot_graph(&graph),
                        "text" => print_spec_graph_text(&graph),
                        other => {
                            anyhow::bail!("unsupported format '{other}': use 'text' or 'dot'");
                        }
                    }
                }

                SpecCommands::StaleLinks => {
                    let links = api.get_stale_spec_links().await?;
                    if links.is_empty() {
                        println!("No stale spec links.");
                    } else {
                        print_spec_links_table(&links);
                    }
                }

                SpecCommands::Conflicts => {
                    let links = api.get_spec_conflicts().await?;
                    if links.is_empty() {
                        println!("No active conflicts.");
                    } else {
                        print_spec_links_table(&links);
                    }
                }
            }
        }

        Commands::Deps { command } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            match command {
                DepsCommands::Show {
                    repo,
                    workspace,
                    tenant,
                } => {
                    if tenant {
                        // Tenant-wide graph
                        let graph = api.get_dependency_graph().await?;
                        print_dependency_graph(&graph);
                    } else if workspace {
                        // Workspace-scoped graph — infer workspace from git remote
                        let (ws_slug, _) = infer_repo_from_git_remote().ok_or_else(|| {
                            anyhow::anyhow!(
                                "could not infer workspace from git remote. \
                                 Run from a gyre-cloned repository."
                            )
                        })?;
                        let ws_id = api.resolve_workspace_slug(&ws_slug).await?;
                        let repos = api.list_workspace_repos(&ws_id).await?;
                        let ws_repo_ids: std::collections::HashSet<String> = repos
                            .iter()
                            .filter_map(|r| r["id"].as_str().map(|s| s.to_string()))
                            .collect();

                        let graph = api.get_dependency_graph().await?;
                        print_dependency_graph_filtered(&graph, &ws_repo_ids);
                    } else {
                        // This repo's (or the named repo's) dependencies and
                        // dependents. The workspace is inferred from the git
                        // remote; the repo defaults to the remote's repo but can
                        // be named positionally (dependency-graph.md §CLI
                        // `gyre deps show [repo]`).
                        let (ws_slug, remote_repo) =
                            infer_repo_from_git_remote().ok_or_else(|| {
                                anyhow::anyhow!(
                                    "could not infer workspace from git remote. \
                                     Run from a gyre-cloned repository."
                                )
                            })?;
                        let repo_name = repo.unwrap_or(remote_repo);
                        let ws_id = api.resolve_workspace_slug(&ws_slug).await?;
                        let repo_id = api.resolve_repo_name(&ws_id, &repo_name).await?;

                        // Repo name lookup for readable tables (fall back to ID).
                        let ws_repos = api.list_workspace_repos(&ws_id).await?;
                        let name_by_id: std::collections::HashMap<String, String> = ws_repos
                            .iter()
                            .filter_map(|r| {
                                r["id"].as_str().map(|id| {
                                    (id.to_string(), r["name"].as_str().unwrap_or(id).to_string())
                                })
                            })
                            .collect();

                        let deps = api.list_dependencies(&repo_id).await?;
                        let dependents = api.list_dependents(&repo_id).await?;

                        println!("Dependencies for {repo_name}");
                        println!();
                        if deps.is_empty() {
                            println!("No outgoing dependencies.");
                        } else {
                            println!(
                                "{:<24} {:<10} {:<12} {:<12} {:>5} {:<10} {:<10}",
                                "TARGET", "TYPE", "PINNED", "CURRENT", "DRIFT", "STATUS", "METHOD"
                            );
                            println!("{}", "-".repeat(95));
                            for d in &deps {
                                let target_id = d["target_repo_id"].as_str().unwrap_or("");
                                let target = name_by_id
                                    .get(target_id)
                                    .map(String::as_str)
                                    .unwrap_or(target_id);
                                let dtype = d["dependency_type"].as_str().unwrap_or("");
                                let pinned = d["version_pinned"].as_str().unwrap_or("-");
                                let current = d["target_version_current"].as_str().unwrap_or("-");
                                let drift = d["version_drift"]
                                    .as_u64()
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "-".to_string());
                                let status = d["status"].as_str().unwrap_or("");
                                let method = d["detection_method"].as_str().unwrap_or("");
                                println!(
                                    "{:<24} {:<10} {:<12} {:<12} {:>5} {:<10} {:<10}",
                                    target, dtype, pinned, current, drift, status, method
                                );
                            }
                        }

                        println!();
                        if dependents.is_empty() {
                            println!("No incoming dependents.");
                        } else {
                            println!("Dependents (repos that depend on {repo_name}):");
                            println!(
                                "{:<24} {:<10} {:<12} {:<12} {:>5} {:<10} {:<10}",
                                "SOURCE", "TYPE", "PINNED", "CURRENT", "DRIFT", "STATUS", "METHOD"
                            );
                            println!("{}", "-".repeat(95));
                            for d in &dependents {
                                let source_id = d["source_repo_id"].as_str().unwrap_or("");
                                let source = name_by_id
                                    .get(source_id)
                                    .map(String::as_str)
                                    .unwrap_or(source_id);
                                let dtype = d["dependency_type"].as_str().unwrap_or("");
                                let pinned = d["version_pinned"].as_str().unwrap_or("-");
                                let current = d["target_version_current"].as_str().unwrap_or("-");
                                let drift = d["version_drift"]
                                    .as_u64()
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "-".to_string());
                                let status = d["status"].as_str().unwrap_or("");
                                let method = d["detection_method"].as_str().unwrap_or("");
                                println!(
                                    "{:<24} {:<10} {:<12} {:<12} {:>5} {:<10} {:<10}",
                                    source, dtype, pinned, current, drift, status, method
                                );
                            }
                        }
                    }
                }

                DepsCommands::Graph { format } => {
                    if format != "dot" {
                        anyhow::bail!("unsupported format '{format}': only 'dot' is supported");
                    }
                    let graph = api.get_dependency_graph().await?;
                    print_dot_graph(&graph);
                }

                DepsCommands::Impact { repo } => {
                    // Resolve repo name to ID via git remote context
                    let (ws_slug, _) = infer_repo_from_git_remote().ok_or_else(|| {
                        anyhow::anyhow!(
                            "could not infer workspace from git remote. \
                             Run from a gyre-cloned repository."
                        )
                    })?;
                    let ws_id = api.resolve_workspace_slug(&ws_slug).await?;
                    let repo_id = api.resolve_repo_name(&ws_id, &repo).await?;

                    let result = api.get_blast_radius(&repo_id).await?;

                    let direct = result["direct_dependents"].as_array();
                    let transitive = result["transitive_dependents"].as_array();
                    let total = result["total"].as_u64().unwrap_or(0);

                    println!("Blast radius for {repo}");
                    println!("  Total dependents: {total}");
                    println!();

                    if let Some(items) = direct {
                        if !items.is_empty() {
                            println!("Direct dependents:");
                            for item in items {
                                let rid = item.as_str().unwrap_or("");
                                println!("  ├── {rid}");
                            }
                        }
                    }

                    if let Some(items) = transitive {
                        if !items.is_empty() {
                            println!("Transitive dependents:");
                            for item in items {
                                let rid = item.as_str().unwrap_or("");
                                println!("  └── {rid}");
                            }
                        }
                    }

                    if total == 0 {
                        println!("No dependents found.");
                    }
                }

                DepsCommands::Stale => {
                    let stale = api.list_stale_dependencies(None).await?;
                    if stale.is_empty() {
                        println!("No stale dependencies.");
                    } else {
                        println!(
                            "{:<36} {:<36} {:<12} {:>6} PINNED → CURRENT",
                            "SOURCE", "TARGET", "TYPE", "DRIFT"
                        );
                        println!("{}", "-".repeat(110));
                        for d in &stale {
                            let source = d["source_repo_id"].as_str().unwrap_or("");
                            let target = d["target_repo_id"].as_str().unwrap_or("");
                            let dtype = d["dependency_type"].as_str().unwrap_or("");
                            let drift = d["version_drift"].as_u64().unwrap_or(0);
                            let pinned = d["version_pinned"].as_str().unwrap_or("-");
                            let current = d["target_version_current"].as_str().unwrap_or("-");
                            println!(
                                "{:<36} {:<36} {:<12} {:>6} {} → {}",
                                source, target, dtype, drift, pinned, current
                            );
                        }
                    }
                }

                DepsCommands::Breaking => {
                    let changes = api.list_breaking_changes().await?;
                    if changes.is_empty() {
                        println!("No unacknowledged breaking changes.");
                    } else {
                        println!(
                            "{:<36} {:<36} {:<12} {:<20} DESCRIPTION",
                            "ID", "SOURCE_REPO", "COMMIT", "DETECTED"
                        );
                        println!("{}", "-".repeat(120));
                        for bc in &changes {
                            let id = bc["id"].as_str().unwrap_or("");
                            let source = bc["source_repo_id"].as_str().unwrap_or("");
                            // dependency_edge_id: internal reference, not user-facing
                            // acknowledged / acknowledged_by / acknowledged_at: always false/None for unacknowledged list
                            let sha = bc["commit_sha"].as_str().unwrap_or("");
                            let short_sha: String = sha.chars().take(10).collect();
                            let detected = bc["detected_at"].as_u64().unwrap_or(0);
                            let desc = bc["description"].as_str().unwrap_or("");
                            println!(
                                "{:<36} {:<36} {:<12} {:<20} {}",
                                id,
                                source,
                                short_sha,
                                format_timestamp(detected),
                                desc
                            );
                        }
                    }
                }

                DepsCommands::Add { target, r#type } => {
                    let (ws_slug, repo_name) = infer_repo_from_git_remote().ok_or_else(|| {
                        anyhow::anyhow!(
                            "could not infer repository from git remote. \
                             Run from a gyre-cloned repository."
                        )
                    })?;
                    let ws_id = api.resolve_workspace_slug(&ws_slug).await?;
                    let source_repo_id = api.resolve_repo_name(&ws_id, &repo_name).await?;
                    let target_repo_id = api.resolve_repo_name(&ws_id, &target).await?;

                    let result = api
                        .add_dependency(&source_repo_id, &target_repo_id, &r#type)
                        .await?;

                    let id = result["id"].as_str().unwrap_or("unknown");
                    println!(
                        "Dependency added: {repo_name} → {target} (type: {})",
                        r#type
                    );
                    println!("  ID: {id}");
                }

                DepsCommands::Acknowledge { id } => {
                    api.acknowledge_breaking_change(&id).await?;
                    println!("Breaking change {id} acknowledged.");
                }
            }
        }

        Commands::Divergence { workspace } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            let workspace_id = if let Some(slug) = &workspace {
                Some(api.resolve_workspace_slug(slug).await?)
            } else {
                None
            };

            let result = api
                .get_notifications(
                    workspace_id.as_deref(),
                    None,
                    None,
                    Some("ConflictingInterpretations"),
                )
                .await?;

            let notifications = result["notifications"].as_array();
            match notifications {
                Some(items) if !items.is_empty() => {
                    println!("Divergence Alerts");
                    println!("{}", "-".repeat(80));
                    for n in items {
                        let id = n["id"].as_str().unwrap_or("");
                        let title = n["title"].as_str().unwrap_or("");
                        let desc = n["body"].as_str().unwrap_or("");
                        let pri = n["priority"].as_u64().unwrap_or(0);
                        let entity_ref = n["entity_ref"].as_str().unwrap_or("");
                        println!("[P{pri}] {title}");
                        println!("  ID: {id}");
                        if !entity_ref.is_empty() {
                            println!("  Spec: {entity_ref}");
                        }
                        if !desc.is_empty() {
                            println!("  {desc}");
                        }
                        println!();
                    }
                }
                _ => println!("No divergence alerts."),
            }
        }

        Commands::Repo { command } => match command {
            RepoCommands::Status {
                repo_id,
                server,
                token,
            } => {
                let api = client::GyreClient::new(server, token);
                let status = api.repo_status(&repo_id).await?;

                let paused = status["queue_paused"].as_bool().unwrap_or(false);
                let main_green = status["main_green"].as_bool();
                let gates = status["post_merge_gates"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();

                println!("Repo {repo_id}");
                println!();
                match main_green {
                    Some(true) => println!("Main: green"),
                    Some(false) => println!("Main: broken (post-merge validation failing)"),
                    None => println!("Main: unknown (no post-merge gates configured)"),
                }
                if paused {
                    let reason = status["pause_reason"].as_str().unwrap_or("unknown");
                    println!("Merge queue: paused ({reason})");
                } else {
                    println!("Merge queue: running");
                }
                if gates.is_empty() {
                    println!("Post-merge gates: none configured");
                } else {
                    println!("Post-merge gates:");
                    for g in &gates {
                        let name = g["name"].as_str().unwrap_or("?");
                        let cmd = g["command"].as_str().unwrap_or("?");
                        let required = g["required"].as_bool().unwrap_or(true);
                        println!("  {name}: {cmd} (required: {required})");
                    }
                }
            }
            RepoCommands::Revert {
                mr_id,
                repo_id,
                server,
                token,
            } => {
                let api = client::GyreClient::new(server, token);
                let result = api.revert_mr(&repo_id, &mr_id).await?;
                let sha = result["revert_commit_sha"].as_str().unwrap_or("?");
                println!("Reverted MR {mr_id} (revert commit: {sha})");
            }
            RepoCommands::Queue { command } => match command {
                QueueCommands::Pause {
                    repo_id,
                    reason,
                    server,
                    token,
                } => {
                    let api = client::GyreClient::new(server, token);
                    api.pause_queue(&repo_id, reason.as_deref()).await?;
                    println!("Merge queue for repo {repo_id} paused.");
                }
                QueueCommands::Resume {
                    repo_id,
                    server,
                    token,
                } => {
                    let api = client::GyreClient::new(server, token);
                    api.resume_queue(&repo_id).await?;
                    println!("Merge queue for repo {repo_id} resumed.");
                }
            },
        },

        Commands::Budget { command } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            match command {
                BudgetCommands::Show {
                    workspace: _,
                    tenant,
                    workspace_name,
                    repo,
                } => {
                    if tenant {
                        let summary = api.budget_summary().await?;
                        print_tenant_budget_summary(&summary);
                    } else {
                        // Repo scope maps to the owning workspace: resolve the
                        // workspace from --workspace-name or the git remote
                        // (same helpers `gyre explore`/`gyre deps` use).
                        let ws_id = resolve_budget_workspace(
                            &api,
                            workspace_name.as_deref(),
                            repo.as_deref(),
                        )
                        .await?;
                        let budget = api.get_workspace_budget(&ws_id).await?;
                        print_workspace_budget(&budget);
                    }
                }

                BudgetCommands::Set {
                    workspace: _,
                    tenant,
                    llm_tokens,
                    llm_cost,
                    max_agents,
                    max_agent_lifetime_secs,
                    workspace_name,
                    repo,
                } => {
                    if tenant {
                        anyhow::bail!(
                            "tenant-level budget limits cannot be set: the server API only \
                             supports setting workspace budgets (PUT /api/v1/workspaces/:id/budget)"
                        );
                    }
                    // run_budget_set validates --llm-cost before any network
                    // I/O, then resolves the workspace and fetch-merge-puts.
                    let updated = run_budget_set(
                        &api,
                        workspace_name.as_deref(),
                        repo.as_deref(),
                        llm_tokens,
                        llm_cost,
                        max_agents,
                        max_agent_lifetime_secs,
                    )
                    .await?;
                    print_workspace_budget(&updated);
                }
            }
        }

        Commands::Secret { command } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            match command {
                SecretCommands::Set {
                    name,
                    value,
                    scope,
                    workspace_name,
                    repo,
                    secret_type,
                    expires_at,
                } => {
                    let target = resolve_secret_scope(
                        &api,
                        scope.as_deref(),
                        workspace_name.as_deref(),
                        repo.as_deref(),
                    )
                    .await?;
                    let req = client::CreateSecretRequest {
                        name,
                        scope: target.scope.clone(),
                        scope_id: target.scope_id.clone(),
                        secret_type: secret_type.unwrap_or_else(|| "static".to_string()),
                        value,
                        expires_at,
                    };
                    let created = api.create_secret(&req).await?;
                    println!("Secret '{}' set in {}", created.name, target.describe());
                    print_secret(&created);
                }

                SecretCommands::List {
                    scope,
                    workspace_name,
                    repo,
                } => {
                    let target = resolve_secret_scope(
                        &api,
                        scope.as_deref(),
                        workspace_name.as_deref(),
                        repo.as_deref(),
                    )
                    .await?;
                    let secrets = api
                        .list_secrets(&target.scope, target.scope_id.as_deref())
                        .await?;
                    println!("Secrets in {}:", target.describe());
                    println!();
                    print_secret_table(&secrets);
                }

                SecretCommands::Delete {
                    name,
                    scope,
                    workspace_name,
                    repo,
                } => {
                    let target = resolve_secret_scope(
                        &api,
                        scope.as_deref(),
                        workspace_name.as_deref(),
                        repo.as_deref(),
                    )
                    .await?;
                    let id = find_secret_id_by_name(&api, &target, &name).await?;
                    api.delete_secret(&id).await?;
                    println!("Secret '{name}' deleted from {}", target.describe());
                }

                SecretCommands::Rotate {
                    name,
                    new_value,
                    scope,
                    workspace_name,
                    repo,
                    expires_at,
                } => {
                    let target = resolve_secret_scope(
                        &api,
                        scope.as_deref(),
                        workspace_name.as_deref(),
                        repo.as_deref(),
                    )
                    .await?;
                    let id = find_secret_id_by_name(&api, &target, &name).await?;
                    let rotated = api.rotate_secret(&id, &new_value, expires_at).await?;
                    println!("Secret '{}' rotated in {}", rotated.name, target.describe());
                    print_secret(&rotated);
                }
            }
        }

        // ── User management (user-management.md §CLI) ────────────────────
        Commands::Whoami => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());
            let me = api.get_my_profile().await?;
            print_my_profile(&me);
        }

        Commands::Profile { command } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());
            match command {
                ProfileCommands::Set {
                    display_name,
                    timezone,
                    locale,
                    avatar_url,
                } => {
                    run_profile_set(&api, display_name, timezone, locale, avatar_url).await?;
                }
            }
        }

        Commands::Sessions { command } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());
            match command {
                SessionsCommands::List => {
                    let sessions = api.list_sessions().await?;
                    print_session_list(&sessions);
                }
                SessionsCommands::Revoke { id } => {
                    api.revoke_session(&id).await?;
                    println!("Session {id} revoked.");
                }
                SessionsCommands::RevokeAll => {
                    let result = api.revoke_all_sessions().await?;
                    let revoked = result["revoked"].as_u64().unwrap_or(0);
                    println!("Revoked {revoked} session(s).");
                }
            }
        }

        Commands::Tenant { command } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());
            match command {
                TenantCommands::Invite {
                    email,
                    workspace,
                    role,
                    bulk,
                } => {
                    run_tenant_invite(&api, email, workspace, role, bulk).await?;
                }
                TenantCommands::Invitations { command } => match command {
                    TenantInvitationCommands::List => {
                        let result = api.list_tenant_invitations().await?;
                        print_tenant_invitation_list(&result);
                    }
                    TenantInvitationCommands::Revoke { id } => {
                        api.revoke_tenant_invitation(&id).await?;
                        println!("Invitation {id} revoked.");
                    }
                },
            }
        }

        Commands::Workspace { command } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());
            match command {
                WorkspaceCommands::Members { command } => match command {
                    MemberCommands::List { workspace } => {
                        let ws_id = resolve_workspace_target(&api, workspace.as_deref()).await?;
                        let result = api.list_workspace_members(&ws_id).await?;
                        print_member_list(&result);
                    }
                    MemberCommands::SetRole {
                        user,
                        role,
                        transfer,
                        reason,
                        workspace,
                    } => {
                        run_member_set_role(
                            &api,
                            workspace.as_deref(),
                            &user,
                            &role,
                            transfer,
                            reason.as_deref(),
                        )
                        .await?;
                    }
                    MemberCommands::Remove { user, workspace } => {
                        let ws_id = resolve_workspace_target(&api, workspace.as_deref()).await?;
                        let user_id = resolve_user_ref(&api, &user).await?;
                        api.remove_workspace_member(&ws_id, &user_id).await?;
                        println!("Member {user} removed from workspace.");
                    }
                },
                WorkspaceCommands::Invite {
                    user,
                    role,
                    workspace,
                } => {
                    run_workspace_invite(&api, workspace.as_deref(), &user, role.as_deref())
                        .await?;
                }
                WorkspaceCommands::Invitations { command } => match command {
                    WorkspaceInvitationCommands::List { workspace } => {
                        let ws_id = resolve_workspace_target(&api, workspace.as_deref()).await?;
                        let result = api.list_workspace_invitations(&ws_id).await?;
                        print_invitation_list(&result);
                    }
                    WorkspaceInvitationCommands::Revoke { id, workspace } => {
                        let ws_id = resolve_workspace_target(&api, workspace.as_deref()).await?;
                        api.revoke_workspace_invitation(&ws_id, &id).await?;
                        println!("Invitation {id} revoked.");
                    }
                },
            }
        }

        Commands::Team { command } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());
            match command {
                TeamCommands::Create {
                    name,
                    lead,
                    description,
                    workspace,
                } => {
                    run_team_create(
                        &api,
                        workspace.as_deref(),
                        &name,
                        lead.as_deref(),
                        description.as_deref(),
                    )
                    .await?;
                }
                TeamCommands::Add {
                    user,
                    team,
                    workspace,
                } => {
                    run_team_membership(&api, workspace.as_deref(), &user, &team, false).await?;
                }
                TeamCommands::List { workspace } => {
                    let ws_id = resolve_workspace_target(&api, workspace.as_deref()).await?;
                    let result = api.list_workspace_teams(&ws_id).await?;
                    print_team_list(&result);
                }
                TeamCommands::Remove {
                    user,
                    team,
                    workspace,
                } => {
                    run_team_membership(&api, workspace.as_deref(), &user, &team, true).await?;
                }
            }
        }

        Commands::Notifications { command } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());
            match command {
                NotificationCommands::List {
                    unread,
                    priority,
                    r#type,
                    workspace,
                    limit,
                } => {
                    run_notifications_list(
                        &api,
                        unread,
                        priority.as_deref(),
                        r#type.as_deref(),
                        workspace.as_deref(),
                        limit,
                    )
                    .await?;
                }
                NotificationCommands::Preferences { command } => match command {
                    NotificationPreferencesCommands::Set {
                        email_digest,
                        email_min_priority,
                        slack_url,
                        slack_min_priority,
                        webhook_url,
                        webhook_secret,
                        webhook_min_priority,
                    } => {
                        run_notification_preferences_set(
                            &api,
                            ChannelPrefsUpdate {
                                email_digest: email_digest.as_deref(),
                                email_min_priority: email_min_priority.as_deref(),
                                slack_url: slack_url.as_deref(),
                                slack_min_priority: slack_min_priority.as_deref(),
                                webhook_url: webhook_url.as_deref(),
                                webhook_secret: webhook_secret.as_deref(),
                                webhook_min_priority: webhook_min_priority.as_deref(),
                            },
                        )
                        .await?;
                    }
                },
            }
        }
    }

    Ok(())
}

// ── Bootstrap (platform-model.md §8) ──────────────────────────────────────────

struct BootstrapArgs {
    server: String,
    token: String,
    tenant: Option<String>,
    workspace: Option<String>,
    repo: Option<String>,
    repo_path: Option<String>,
    admin_user: Option<String>,
    oidc_issuer: Option<String>,
    dev: bool,
    starter_kit: bool,
}

/// One bootstrap step, named for the failure report (spec §8: "on failure,
/// report which steps succeeded, which failed, how to resume").
struct StepTracker {
    done: Vec<&'static str>,
    current: &'static str,
}

impl StepTracker {
    fn new(current: &'static str) -> Self {
        Self {
            done: Vec::new(),
            current,
        }
    }

    fn advance(&mut self, next: &'static str) {
        self.done.push(self.current);
        self.current = next;
    }

    fn fail(&self, err: anyhow::Error) -> anyhow::Error {
        let succeeded = if self.done.is_empty() {
            "none".to_string()
        } else {
            self.done.join(", ")
        };
        anyhow::anyhow!(
            "bootstrap failed at step '{}' (completed: {succeeded}): {err}\n\
             Resume hint: re-run `gyre bootstrap` after fixing the issue; \
             already-created resources keep their IDs and slugs, so \
             re-running skips or fails gracefully on duplicates.",
            self.current
        )
    }
}

async fn run_bootstrap(args: BootstrapArgs) -> Result<()> {
    // ── Flag validation ──
    if args.dev {
        if args.tenant.is_some() {
            anyhow::bail!("--dev cannot be combined with --tenant (dev mode uses tenant \"dev\")");
        }
        if args.workspace.is_some() {
            anyhow::bail!(
                "--dev cannot be combined with --workspace (dev mode uses workspace \"default\")"
            );
        }
        if args.admin_user.is_some() {
            anyhow::bail!(
                "--dev cannot be combined with --admin-user (dev mode uses static tokens)"
            );
        }
        if args.oidc_issuer.is_some() {
            anyhow::bail!("--dev cannot be combined with --oidc-issuer (dev mode skips OIDC)");
        }
    } else if args.tenant.is_none() {
        anyhow::bail!("--tenant is needed to name the tenant (or pass --dev for defaults)");
    }

    let repo_name = args.repo.clone().unwrap_or_else(|| "main".to_string());
    let tenant_name = args.tenant.clone().unwrap_or_else(|| "dev".to_string());
    let workspace_name = args
        .workspace
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let repo_path = args.repo_path.as_deref().map(std::path::PathBuf::from);

    let api = client::GyreClient::new(args.server.clone(), args.token.clone());
    let mut summary = bootstrap::BootstrapSummary {
        server_url: args.server.clone(),
        repo_name: repo_name.clone(),
        tenant_name: tenant_name.clone(),
        workspace_name: workspace_name.clone(),
        ..Default::default()
    };

    // ── Step 0: health check ──
    println!("Checking server health at {}...", args.server);
    let health = api.health().await?;
    if health["status"].as_str() != Some("ok") {
        anyhow::bail!("server health check returned unexpected status: {health}");
    }
    println!(
        "  Server healthy (version {}).",
        health["version"].as_str().unwrap_or("?")
    );

    let mut step = StepTracker::new("create tenant");

    // ── Step 1: create tenant (resume: reuse existing by slug) ──
    let tenant_slug = bootstrap::derive_slug(&tenant_name);
    let tenant = match api
        .create_tenant(&tenant_name, &tenant_slug, args.oidc_issuer.as_deref())
        .await
    {
        Ok(t) => {
            println!("  Tenant '{}' created ({})", t.name, t.id);
            t
        }
        Err(create_err) => match api.find_tenant_by_slug(&tenant_slug).await {
            Ok(Some(existing)) => {
                println!(
                    "  Tenant '{}' already exists ({}) - reusing",
                    existing.name, existing.id
                );
                existing
            }
            _ => return Err(step.fail(create_err)),
        },
    };
    summary.tenant_id = tenant.id.clone();

    // ── Step 2: create admin user + API key (skipped in dev mode) ──
    step.advance("create admin user");
    let mut client_api = api;
    if !args.dev {
        let username = args.admin_user.as_deref().unwrap_or("admin");
        // F1: bind the admin user to the tenant created in step 1 — the
        // server rejects an unbound user, and the auth extractor fail-closes
        // on one at API-key auth time.
        match client_api.create_user(username, &summary.tenant_id).await {
            Ok(created) => {
                summary.admin_username = Some(created.user.username.clone());
                summary.api_key = Some(created.api_key.key.clone());
                // Save credentials to ~/.gyre/config for subsequent CLI calls.
                let cfg = config::Config {
                    server: args.server.clone(),
                    token: Some(created.api_key.key.clone()),
                    agent_id: Some(created.user.id.clone()),
                    agent_name: Some(created.user.username.clone()),
                };
                cfg.save()?;
                println!(
                    "  Admin user '{}' created; credentials saved to {}",
                    created.user.username,
                    config::Config::path().display()
                );
                // Continue authenticating as the new admin via its API key.
                client_api =
                    client::GyreClient::new(args.server.clone(), created.api_key.key.clone());
            }
            Err(e) => {
                // Resume path — but ONLY for a real duplicate: a 409 from the
                // server means the user already exists. Any other failure
                // (403 from a non-admin token, validation, network) must
                // fail the run: silently adopting whatever token sits in
                // ~/.gyre/config would continue as the wrong principal
                // (the config is also written by `gyre register` with
                // agent tokens, and a matching server URL says nothing
                // about which user the token belongs to).
                let duplicate_detail = match &e {
                    client::CreateUserError::Duplicate(detail) => detail.clone(),
                    client::CreateUserError::Other(_) => return Err(step.fail(e.into())),
                };
                // The API key minted on the first run was saved to
                // ~/.gyre/config — VERIFY it authenticates as the
                // requested user before adopting it.
                let saved = config::Config::load().ok();
                let token = saved
                    .filter(|c| c.server == args.server)
                    .and_then(|c| c.token)
                    .filter(|t| !t.is_empty());
                let resume_client = match token {
                    Some(token) => client::GyreClient::new(args.server.clone(), token),
                    None => {
                        return Err(step.fail(anyhow::anyhow!(
                            "admin user '{username}' already exists ({duplicate_detail}) \
                             but ~/.gyre/config holds no credentials for {} — remove \
                             the user server-side or resume with a different --admin-user",
                            args.server
                        )))
                    }
                };
                let me = match resume_client.me().await {
                    Ok(me) => me,
                    Err(err) => {
                        return Err(step.fail(anyhow::anyhow!(
                            "admin user '{username}' already exists but the saved \
                             credential no longer authenticates ({err}) — mint a new \
                             key server-side or resume with a different --admin-user"
                        )))
                    }
                };
                if me.username != username {
                    return Err(step.fail(anyhow::anyhow!(
                        "saved credentials authenticate as '{}', not '{username}' — \
                         ~/.gyre/config belongs to a different user; remove it or pass \
                         the matching token via --token",
                        me.username
                    )));
                }
                if me.tenant_id.as_deref() != Some(summary.tenant_id.as_str()) {
                    return Err(step.fail(anyhow::anyhow!(
                        "admin user '{username}' is bound to tenant {}, but this bootstrap \
                         targets tenant {} — the saved credential cannot administer it; \
                         resume with a different --admin-user",
                        me.tenant_id.as_deref().unwrap_or("(none)"),
                        summary.tenant_id
                    )));
                }
                println!("  Admin user '{username}' already exists - reusing saved credentials");
                summary.admin_username = Some(username.to_string());
                client_api = resume_client;
            }
        }
    }

    // ── Step 3: create workspace (resume: reuse existing by name) ──
    step.advance("create workspace");
    let workspace = match client_api
        .create_workspace(&summary.tenant_id, &workspace_name)
        .await
    {
        Ok(ws) => {
            println!("  Workspace '{}' created ({})", ws.name, ws.id);
            ws
        }
        Err(create_err) => {
            match client_api
                .find_workspace_by_name(&summary.tenant_id, &workspace_name)
                .await
            {
                Ok(Some(existing)) => {
                    println!(
                        "  Workspace '{}' already exists ({}) - reusing",
                        existing.name, existing.id
                    );
                    existing
                }
                _ => return Err(step.fail(create_err)),
            }
        }
    };
    summary.workspace_id = workspace.id.clone();
    step.advance("register repo");

    // ── Step 4: add repo (resume: reuse existing by name) ──
    let repo = match client_api
        .create_repo(&summary.workspace_id, &repo_name)
        .await
    {
        Ok(r) => {
            println!("  Repo '{}' created ({})", r.name, r.id);
            r
        }
        Err(create_err) => match client_api
            .find_repo_by_name(&summary.workspace_id, &repo_name)
            .await
        {
            Ok(Some(existing)) => {
                println!(
                    "  Repo '{}' already exists ({}) - reusing",
                    existing.name, existing.id
                );
                existing
            }
            _ => return Err(step.fail(create_err)),
        },
    };
    summary.repo_id = repo.id.clone();
    summary.clone_url = repo.clone_url.clone();
    step.advance("register personas");

    // ── Step 5: register built-in personas (pre-approved; resume: skip existing) ──
    // The resume lookup is scoped to THIS tenant: the unscoped persona list
    // spans all tenants, which made a second tenant's bootstrap skip
    // registration and die at spawn with a persona 404 (round-3 finding 3).
    for persona in bootstrap::BUILTIN_PERSONAS {
        match client_api
            .find_persona_by_slug_in_tenant(persona.slug, &summary.tenant_id)
            .await?
        {
            Some(existing) => {
                if existing.approval_status == "Approved" {
                    println!("  Persona '{}' already registered - skipping", persona.slug);
                } else {
                    // A run interrupted between create_persona and
                    // approve_persona leaves a Pending persona: spawn would
                    // 409 on it (persona Pending, NOT "orchestrator already
                    // live"). Approve it here so a resume converges instead
                    // of exiting 0 with zero orchestrators.
                    println!(
                        "  Persona '{}' registered but {} - approving",
                        persona.slug, existing.approval_status
                    );
                    client_api
                        .approve_persona(&existing.id)
                        .await
                        .map_err(|e| step.fail(e))?;
                }
            }
            None => {
                let created = client_api
                    .create_persona(
                        persona.name,
                        persona.slug,
                        &summary.tenant_id,
                        persona.system_prompt,
                        persona.capabilities,
                        persona.protocols,
                    )
                    .await
                    .map_err(|e| step.fail(e))?;
                client_api
                    .approve_persona(&created.id)
                    .await
                    .map_err(|e| step.fail(e))?;
            }
        }
        summary.personas_registered.push(persona.slug.to_string());
    }
    println!(
        "  {} personas registered and pre-approved",
        bootstrap::BUILTIN_PERSONAS.len()
    );
    step.advance("init spec registry");

    // ── Step 6: initialize spec registry (platform-model.md §8 step 6) ──
    // With a manifest present (pre-existing or just written by the starter
    // kit), push the local tree to the server repo and run the server-side
    // ledger sync so the registry is populated at first-run time. The
    // post-receive hook keeps it in sync on every later push.
    if args.starter_kit {
        // F6: a bare `repo_name` here would default to a RELATIVE path
        // resolved against the process cwd -- the starter kit must land at
        // an explicit, operator-visible location instead.
        let target = repo_path.clone().ok_or_else(|| {
            step.fail(anyhow::anyhow!(
                "--starter-kit requires --repo-path: pass the directory to write the kit into"
            ))
        })?;
        bootstrap::write_starter_kit(&target)?;
        println!("  Starter kit written to {}", target.display());
    }
    match &repo_path {
        Some(path) if path.join("specs").join("manifest.yaml").exists() => {
            let clone_url = repo
                .clone_url
                .clone()
                .unwrap_or_else(|| format!("{}/git/{}/{}", args.server, workspace.slug, repo_name));
            match bootstrap::push_and_sync_specs(&client_api, path, &summary.repo_id, &clone_url)
                .await
            {
                Ok(registered) => {
                    summary.specs_registered = registered;
                    println!(
                        "  Spec manifest registered - {registered} spec(s) in the platform registry"
                    );
                }
                Err(e) => {
                    // Non-fatal: the specs register on the next push to the
                    // default branch (post-receive hook). Report why.
                    println!(
                        "  Spec manifest found but not registered yet ({e}); \
                         it will register on your next push to the default branch"
                    );
                }
            }
        }
        Some(path) => {
            println!(
                "  No spec manifest found under {} - spec registry stays empty \
                 until specs are pushed",
                path.display()
            );
        }
        None => {
            println!("  No --repo-path given - spec registry stays empty until specs are pushed")
        }
    }
    step.advance("configure gates");

    // ── Step 7: configure default gates ──
    if let Some(path) = &repo_path {
        let detected = bootstrap::detect_default_gates(path);
        if detected.is_empty() {
            println!("  No default gates detected (no Cargo.toml or check-arch.sh in repo path)");
        } else {
            // Resume: skip gates already configured for this repo. The
            // server also rejects duplicates (409), but skipping here keeps
            // a re-run from duplicating the gate set — every subsequent MR
            // runs each gate once per bootstrap run otherwise.
            let existing: std::collections::HashSet<String> = client_api
                .list_gates(&summary.repo_id)
                .await
                .map_err(|e| step.fail(e))?
                .into_iter()
                .map(|g| g.name)
                .collect();
            for gate in detected {
                if existing.contains(gate.name) {
                    println!("  Gate '{}' already configured - skipping", gate.name);
                    continue;
                }
                client_api
                    .create_gate(&summary.repo_id, gate.name, gate.gate_type, gate.command)
                    .await
                    .map_err(|e| step.fail(e))?;
                summary.gates_configured.push(gate.name.to_string());
            }
            if summary.gates_configured.is_empty() {
                println!("  All default gates already configured - nothing to do");
            } else {
                println!(
                    "  Gates configured: {}",
                    summary.gates_configured.join(", ")
                );
            }
        }
    }
    step.advance("spawn repo orchestrator");

    // ── Step 8: spawn repo orchestrator ──
    // Task-093 endpoint: repo-tier orchestrator with exactly-one-live
    // semantics, repo-scoped JWT, and restart-on-failure — no synthetic task.
    //
    // Agent names are unique server-wide. A stopped predecessor holding the
    // default name is released server-side (orchestrator.rs), but a LIVE
    // agent elsewhere — e.g. another tenant's bootstrap of a same-named repo
    // — still owns it; retry with a suffix instead of failing the run.
    let base_orchestrator_name = format!("{repo_name}-orchestrator");
    let mut orchestrator_name = base_orchestrator_name.clone();
    let spawn_result = loop {
        match client_api
            .spawn_repo_orchestrator(&summary.repo_id, Some(&orchestrator_name))
            .await
        {
            Ok(client::SpawnRepoOrchestratorOutcome::NameTaken) => {
                let attempt = orchestrator_name
                    .rsplit('-')
                    .next()
                    .and_then(|n| n.parse::<u32>().ok());
                let next = attempt.map(|n| n + 1).unwrap_or(2);
                // Strip any previous numeric suffix before appending the new one.
                let stem = match attempt {
                    Some(_) => orchestrator_name
                        .rsplit_once('-')
                        .map(|(s, _)| s.to_string())
                        .unwrap_or(base_orchestrator_name.clone()),
                    None => orchestrator_name.clone(),
                };
                if next > 20 {
                    break Err(anyhow::anyhow!(
                        "could not find a free orchestrator name (tried up to {orchestrator_name})"
                    ));
                }
                println!(
                    "  Name '{orchestrator_name}' is taken by a live agent elsewhere - \
                     retrying as {stem}-{next}"
                );
                orchestrator_name = format!("{stem}-{next}");
            }
            other => break other,
        }
    };
    match spawn_result {
        Ok(client::SpawnRepoOrchestratorOutcome::Spawned(spawned)) => {
            summary.orchestrator_agent_id = Some(spawned.agent.id.clone());
            // F3: record the truthful process-launch outcome so the summary
            // cannot claim "running" for a row whose process never started.
            summary.orchestrator_launch_status = Some(spawned.launch_status.clone());
            summary.orchestrator_launch_detail = spawned.launch_detail.clone();
            if spawned.launch_status == "launch_failed" {
                println!(
                    "  WARNING: orchestrator process failed to launch{} - \
                     auto-restart will retry",
                    spawned
                        .launch_detail
                        .as_deref()
                        .map(|d| format!(": {d}"))
                        .unwrap_or_default()
                );
            }
        }
        Ok(client::SpawnRepoOrchestratorOutcome::AlreadyLive) => {
            println!("  A repo orchestrator is already active for this repo - keeping it");
        }
        Ok(client::SpawnRepoOrchestratorOutcome::PersonaPending(detail)) => {
            // The persona resolves but is not Approved — no orchestrator
            // exists. Failing truthfully beats the previous false "already
            // active" success. Step 5's resume approval fixes the tenant-
            // scope case; a persona created outside bootstrap (repo or
            // workspace scope, still Pending) lands here for the operator
            // to resolve.
            return Err(step.fail(anyhow::anyhow!(
                "repo-orchestrator persona is not approved — no orchestrator was \
                 spawned. Approve the persona (or re-run bootstrap) and try again. \
                 Server detail: {detail}"
            )));
        }
        // Unreachable: the retry loop above only breaks on a non-NameTaken
        // outcome (or an error). Kept explicit so a future refactor of the
        // loop cannot silently drop the case.
        Ok(client::SpawnRepoOrchestratorOutcome::NameTaken) => {
            return Err(step.fail(anyhow::anyhow!("orchestrator name still taken")));
        }
        Err(e) => return Err(step.fail(e)),
    }
    step.advance("done");

    // ── Step 9: summary ──
    println!();
    print!("{}", summary.render());
    Ok(())
}

/// Infer workspace slug and repo name from the git remote URL.
/// Gyre git URLs have the form: {server}/git/{workspace_slug}/{repo_name}.git
fn infer_repo_from_git_remote() -> Option<(String, String)> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // Parse: .../git/{workspace_slug}/{repo_name}.git
    let parts: Vec<&str> = url.trim_end_matches(".git").rsplit('/').collect();
    if parts.len() >= 2 {
        let repo_name = parts[0].to_string();
        let workspace_slug = parts[1].to_string();
        // Verify this looks like a gyre git URL (has /git/ in it)
        if url.contains("/git/") {
            return Some((workspace_slug, repo_name));
        }
    }
    None
}

/// Resolve the workspace whose budget a `gyre budget` command operates on.
///
/// Repo scope maps to the owning workspace (the budget governance boundary
/// is the workspace; there is no repo-keyed budget store). Resolution reuses
/// the exact helpers `gyre explore`/`gyre deps` use:
/// - `--workspace-name <slug>` → `resolve_workspace_slug`
/// - `--repo <name>` (with a slug or a gyre git remote) → the repo is resolved
///   inside the workspace to validate it owns the budget target
/// - no flags → workspace slug inferred from the git remote
async fn resolve_budget_workspace(
    api: &client::GyreClient,
    workspace_name: Option<&str>,
    repo: Option<&str>,
) -> Result<String> {
    match (workspace_name, repo) {
        (Some(slug), repo_name) => {
            let wid = api.resolve_workspace_slug(slug).await?;
            if let Some(name) = repo_name {
                // Validate the repo belongs to the named workspace.
                api.resolve_repo_name(&wid, name).await?;
            }
            Ok(wid)
        }
        (None, Some(name)) => {
            // Same as `gyre explore --repo`: infer the workspace from the git
            // remote, then resolve the repo name within it.
            let (ws_slug, _) = infer_repo_from_git_remote().ok_or_else(|| {
                anyhow::anyhow!(
                    "could not infer workspace from git remote. \
                     Use --workspace-name <slug> --repo <name> to specify."
                )
            })?;
            let wid = api.resolve_workspace_slug(&ws_slug).await?;
            api.resolve_repo_name(&wid, name).await?;
            Ok(wid)
        }
        (None, None) => {
            let (ws_slug, _) = infer_repo_from_git_remote().ok_or_else(|| {
                anyhow::anyhow!(
                    "could not infer workspace from git remote. \
                     Run from a gyre-cloned repository, or pass --workspace-name <slug>."
                )
            })?;
            api.resolve_workspace_slug(&ws_slug).await
        }
    }
}

/// The target scope of a `gyre secret` subcommand, resolved from the
/// `--scope`/`--workspace-name`/`--repo` flags.
struct SecretScopeTarget {
    /// "tenant" | "workspace" | "repo" — sent as the API `scope` value.
    scope: String,
    /// The scoped entity's id (tenant scope has none server-side; the
    /// caller's own tenant is used).
    scope_id: Option<String>,
}

impl SecretScopeTarget {
    /// Human-readable description for error messages and output.
    fn describe(&self) -> String {
        match &self.scope_id {
            Some(id) => format!("{} scope ({id})", self.scope),
            None => format!("{} scope", self.scope),
        }
    }
}

/// Resolve the (scope, scope_id) a `gyre secret` command operates on.
///
/// Precedence:
/// - `--scope repo` (or a bare `--repo <name>`) → repo scope; the repo id
///   is resolved inside the workspace named by `--workspace-name` or
///   inferred from the git remote.
/// - `--scope tenant` → the caller's own tenant (no scope_id needed).
/// - `--scope workspace` / default → workspace scope via
///   `--workspace-name` or the git remote; a `--repo` alongside it only
///   validates that the repo belongs to the resolved workspace.
async fn resolve_secret_scope(
    api: &client::GyreClient,
    scope: Option<&str>,
    workspace_name: Option<&str>,
    repo: Option<&str>,
) -> Result<SecretScopeTarget> {
    let scope = scope.unwrap_or(if repo.is_some() { "repo" } else { "workspace" });
    match scope {
        "tenant" => Ok(SecretScopeTarget {
            scope: "tenant".to_string(),
            scope_id: None,
        }),
        "workspace" => {
            let ws_id = resolve_budget_workspace(api, workspace_name, repo).await?;
            Ok(SecretScopeTarget {
                scope: "workspace".to_string(),
                scope_id: Some(ws_id),
            })
        }
        "repo" => {
            let repo_name = repo.ok_or_else(|| {
                anyhow::anyhow!(
                    "repo scope needs --repo <name> (or use --scope workspace / --scope tenant)"
                )
            })?;
            let ws_id = resolve_budget_workspace(api, workspace_name, None).await?;
            let repo_id = api.resolve_repo_name(&ws_id, repo_name).await?;
            Ok(SecretScopeTarget {
                scope: "repo".to_string(),
                scope_id: Some(repo_id),
            })
        }
        other => anyhow::bail!("unknown scope '{other}' (expected workspace, repo, or tenant)"),
    }
}

/// Find the id of the secret named `name` in the target scope. The Admin
/// API is id-keyed, while the CLI is name-keyed (the spec's
/// `gyre secret delete <name>` / `gyre secret rotate <name> ...`).
async fn find_secret_id_by_name(
    api: &client::GyreClient,
    target: &SecretScopeTarget,
    name: &str,
) -> Result<String> {
    let secrets = api
        .list_secrets(&target.scope, target.scope_id.as_deref())
        .await?;
    secrets
        .iter()
        .find(|s| s.name == name)
        .map(|s| s.id.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "secret '{name}' not found in {} — check `gyre secret list` for names",
                target.describe()
            )
        })
}

/// Format a Unix epoch (seconds) as a local datetime string, or "-".
fn fmt_secret_epoch(secs: Option<u64>) -> String {
    match secs {
        Some(s) => {
            // No chrono dependency in the CLI: render as epoch seconds with
            // a stable, testable label. Users needing precision can convert.
            format!("epoch {s}")
        }
        None => "-".to_string(),
    }
}

/// Print one secret's metadata (`gyre secret set`/`rotate` result).
/// Never prints a value — the client type cannot even represent one.
fn print_secret(secret: &client::SecretResponse) {
    println!("Secret '{}' ({})", secret.name, secret.secret_type);
    println!("  ID:              {}", secret.id);
    println!("  Scope:           {} ({})", secret.scope, secret.scope_id);
    println!(
        "  Created:         {}",
        fmt_secret_epoch(Some(secret.created_at))
    );
    println!("  Expires:         {}", fmt_secret_epoch(secret.expires_at));
    println!(
        "  Last rotated:    {}",
        fmt_secret_epoch(secret.last_rotated_at)
    );
}

/// Print a scope's secrets as a table (`gyre secret list`).
fn print_secret_table(secrets: &[client::SecretResponse]) {
    if secrets.is_empty() {
        println!("No secrets in this scope.");
        return;
    }
    let name_w = secrets
        .iter()
        .map(|s| s.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let type_w = secrets
        .iter()
        .map(|s| s.secret_type.len())
        .max()
        .unwrap_or(4)
        .max(4);
    println!(
        "{:<name_w$}  {:<type_w$}  {:<12}  {:<16}  {:<16}  CREATED",
        "NAME", "TYPE", "SCOPE", "EXPIRES", "LAST ROTATED"
    );
    for s in secrets {
        println!(
            "{:<name_w$}  {:<type_w$}  {:<12}  {:<16}  {:<16}  {}",
            s.name,
            s.secret_type,
            s.scope,
            fmt_secret_epoch(s.expires_at),
            fmt_secret_epoch(s.last_rotated_at),
            fmt_secret_epoch(Some(s.created_at))
        );
    }
}

/// Merge `gyre budget set` updates into the current config so unmentioned
/// limits keep their values (the server PUT replaces the whole config).
fn merge_budget_updates(
    current: &client::BudgetConfig,
    updates: &client::SetBudgetRequest,
) -> client::SetBudgetRequest {
    client::SetBudgetRequest {
        max_tokens_per_day: updates.max_tokens_per_day.or(current.max_tokens_per_day),
        max_cost_per_day: updates.max_cost_per_day.or(current.max_cost_per_day),
        max_concurrent_agents: updates
            .max_concurrent_agents
            .or(current.max_concurrent_agents),
        max_agent_lifetime_secs: updates
            .max_agent_lifetime_secs
            .or(current.max_agent_lifetime_secs),
    }
}

/// Validate the `--llm-cost` input of `gyre budget set` before any request
/// is sent. Non-finite values (NaN/Infinity) must be rejected because
/// serde_json serializes them as `null`, which the server would store as
/// "no limit" — silently clearing a financial governance cap instead of
/// setting one. Negative amounts are rejected because a cap below zero
/// can never be exceeded by non-negative usage. A finite ZERO cap is
/// VALID: it explicitly disables all spending for the day — the server
/// stores it, the over-budget check blocks any usage above it, and the
/// CLI display renders it as an explicit `(zero cap)` rather than `none`.
fn validate_llm_cost(cost: f64) -> anyhow::Result<()> {
    if !cost.is_finite() {
        anyhow::bail!(
            "invalid --llm-cost {cost}: the cost limit must be a finite amount in USD \
             (NaN or Infinity would be serialized as null and silently clear the limit)"
        );
    }
    if cost < 0.0 {
        anyhow::bail!(
            "invalid --llm-cost {cost}: the cost limit must be a non-negative amount in USD \
             (zero is valid and disables spending)"
        );
    }
    Ok(())
}

/// Execute `gyre budget set`: validate the requested updates, resolve the
/// target workspace, then fetch-merge-put (the server PUT replaces the
/// whole config, so unmentioned limits keep their current values).
/// Validation runs before any network I/O, so rejected --llm-cost input
/// can never reach the server — in particular it can never produce the
/// PUT that mutates the budget.
async fn run_budget_set(
    api: &client::GyreClient,
    workspace_name: Option<&str>,
    repo: Option<&str>,
    llm_tokens: Option<u64>,
    llm_cost: Option<f64>,
    max_agents: Option<u32>,
    max_agent_lifetime_secs: Option<u64>,
) -> anyhow::Result<client::BudgetResponse> {
    if let Some(cost) = llm_cost {
        validate_llm_cost(cost)?;
    }
    let updates = client::SetBudgetRequest {
        max_tokens_per_day: llm_tokens,
        max_cost_per_day: llm_cost,
        max_concurrent_agents: max_agents,
        max_agent_lifetime_secs,
    };
    if updates == client::SetBudgetRequest::default() {
        anyhow::bail!(
            "no limits given: use --llm-tokens, --llm-cost, --max-agents \
             or --max-agent-lifetime-secs"
        );
    }
    let ws_id = resolve_budget_workspace(api, workspace_name, repo).await?;
    let current = api.get_workspace_budget(&ws_id).await?;
    let merged = merge_budget_updates(&current.config, &updates);
    api.set_workspace_budget(&ws_id, &merged).await
}

// ── User management (user-management.md §CLI) ─────────────────────────────────

/// Resolve the workspace *slug* a command targets: `--workspace <slug>`
/// wins; otherwise the slug is inferred from the gyre git remote. Used
/// where an API keys on the slug (tenant-invite workspace pre-assignment)
/// rather than the id.
fn resolve_workspace_slug_target(workspace: Option<&str>) -> Result<String> {
    match workspace {
        Some(slug) => Ok(slug.to_string()),
        None => infer_repo_from_git_remote()
            .map(|(ws, _)| ws)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "could not infer workspace from git remote; pass --workspace <slug>"
                )
            }),
    }
}

/// Resolve the workspace a `gyre workspace`/`gyre team` command operates
/// on: `--workspace <slug>` wins; otherwise the slug is inferred from the
/// gyre git remote (the same inference `gyre budget`/`gyre explore` use).
/// Returns the workspace id the membership/team APIs key on.
async fn resolve_workspace_target(
    api: &client::GyreClient,
    workspace: Option<&str>,
) -> Result<String> {
    let slug = match workspace {
        Some(slug) => slug.to_string(),
        None => infer_repo_from_git_remote()
            .map(|(ws, _)| ws)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "could not infer workspace from git remote; pass --workspace <slug>"
                )
            })?,
    };
    api.resolve_workspace_slug(&slug).await
}

/// Whether `s` is an email address rather than a username. Usernames are
/// URL-safe `[a-z0-9_-]` (user-management.md §Username vs Display Name),
/// so any `@` outside a leading handle marker means an email.
fn looks_like_email(s: &str) -> bool {
    !s.starts_with('@') && s.contains('@')
}

/// Resolve a user reference (`@username`, bare username, or raw user id)
/// to the user id the membership/team APIs key on, via the tenant-public
/// profile endpoint (GET /api/v1/users/:username). A `@handle` or bare
/// username that does not resolve is a 404 from the server; a raw id that
/// does not resolve likewise fails here with the server's error.
async fn resolve_user_ref(api: &client::GyreClient, user: &str) -> Result<String> {
    if looks_like_email(user) {
        anyhow::bail!(
            "'{user}' is an email address; invite email users with `gyre workspace invite` \
             (email path) or `gyre tenant invite`"
        );
    }
    let username = user.strip_prefix('@').unwrap_or(user);
    let profile = api.get_public_profile(username).await?;
    Ok(profile.id)
}

/// Validate a workspace role name against the server's accepted set
/// (WorkspaceRole::parse_role: Owner/Admin/Developer/Viewer) before any
/// request is sent.
fn validate_workspace_role(role: &str) -> Result<()> {
    const ROLES: [&str; 4] = ["Owner", "Admin", "Developer", "Viewer"];
    if ROLES.iter().any(|r| r.eq_ignore_ascii_case(role)) {
        Ok(())
    } else {
        anyhow::bail!("unknown role '{role}': expected one of Owner, Admin, Developer, Viewer");
    }
}

/// Normalize a digest frequency flag value to the wire representation
/// (Immediate/Hourly/Daily/Off — DigestFrequency variant names).
fn normalize_digest(value: &str) -> Result<String> {
    const DIGESTS: [&str; 4] = ["Immediate", "Hourly", "Daily", "Off"];
    DIGESTS
        .iter()
        .find(|d| d.eq_ignore_ascii_case(value))
        .map(|d| d.to_string())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "invalid email digest '{value}': expected one of immediate, hourly, daily, off"
            )
        })
}

/// Normalize a priority level flag value to the wire representation
/// (Low/Medium/High/Urgent — NotificationPriority variant names).
fn normalize_priority_level(value: &str) -> Result<String> {
    const LEVELS: [&str; 4] = ["Low", "Medium", "High", "Urgent"];
    LEVELS
        .iter()
        .find(|l| l.eq_ignore_ascii_case(value))
        .map(|l| l.to_string())
        .ok_or_else(|| {
            anyhow::anyhow!("invalid priority '{value}': expected one of low, medium, high, urgent")
        })
}

/// One parsed row of a bulk-invite CSV.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BulkInviteRow {
    email: String,
    workspace: Option<String>,
    role: Option<String>,
}

/// Strip whitespace and surrounding double quotes from a CSV field.
fn unquote(field: &str) -> String {
    let f = field.trim();
    f.strip_prefix('"')
        .and_then(|f| f.strip_suffix('"'))
        .unwrap_or(f)
        .to_string()
}

/// Parse bulk-invite CSV content (user-management.md §CLI: `gyre tenant
/// invite --bulk users.csv`). Rows are `email[,workspace][,role]`; an
/// optional leading `email` header row is recognized and skipped, fields
/// are trimmed, and surrounding double quotes are stripped. Every row
/// must carry a non-empty email.
fn parse_bulk_invitation_csv(content: &str) -> Result<Vec<BulkInviteRow>> {
    let mut rows = Vec::new();
    let mut lines = content.lines().filter(|l| !l.trim().is_empty());
    // Skip a header row whose first field is "email".
    if let Some(first) = lines.next() {
        let mut fields = first.split(',');
        if !unquote(fields.next().unwrap_or("")).eq_ignore_ascii_case("email") {
            let row = parse_bulk_line(first)?;
            rows.push(row);
        }
    }
    for line in lines {
        rows.push(parse_bulk_line(line)?);
    }
    if rows.is_empty() {
        anyhow::bail!("bulk invite file contains no invitation rows");
    }
    Ok(rows)
}

fn parse_bulk_line(line: &str) -> Result<BulkInviteRow> {
    let mut fields = line.split(',').map(unquote);
    let email = fields.next().unwrap_or_default();
    if email.is_empty() {
        anyhow::bail!("bulk invite row '{line}' has no email address");
    }
    if !email.contains('@') {
        anyhow::bail!("bulk invite row '{line}': '{email}' is not an email address");
    }
    Ok(BulkInviteRow {
        email,
        workspace: fields.next().filter(|w| !w.is_empty()),
        role: fields.next().filter(|r| !r.is_empty()),
    })
}

/// `gyre profile set` — partial update of the caller's profile via
/// PUT /api/v1/users/me. At least one field is required; omitted fields
/// keep their stored values.
async fn run_profile_set(
    api: &client::GyreClient,
    display_name: Option<String>,
    timezone: Option<String>,
    locale: Option<String>,
    avatar_url: Option<String>,
) -> Result<()> {
    let mut body = serde_json::Map::new();
    if let Some(dn) = &display_name {
        if dn.trim().is_empty() {
            anyhow::bail!("--display-name must not be empty");
        }
        body.insert("display_name".into(), serde_json::json!(dn));
    }
    if let Some(tz) = &timezone {
        if tz.trim().is_empty() {
            anyhow::bail!("--timezone must not be empty");
        }
        body.insert("timezone".into(), serde_json::json!(tz));
    }
    if let Some(l) = &locale {
        if l.trim().is_empty() {
            anyhow::bail!("--locale must not be empty");
        }
        body.insert("locale".into(), serde_json::json!(l));
    }
    if let Some(a) = &avatar_url {
        body.insert("avatar_url".into(), serde_json::json!(a));
    }
    if body.is_empty() {
        anyhow::bail!("no fields given: use --display-name, --timezone, --locale, or --avatar-url");
    }
    let updated = api
        .update_my_profile(&serde_json::Value::Object(body))
        .await?;
    println!("Profile updated.");
    print_my_profile(&updated);
    Ok(())
}

/// `gyre tenant invite` — single email invite (optionally with workspace
/// pre-assignment) or `--bulk` CSV invite.
async fn run_tenant_invite(
    api: &client::GyreClient,
    email: Option<String>,
    workspace: Option<String>,
    role: Option<String>,
    bulk: Option<std::path::PathBuf>,
) -> Result<()> {
    if let Some(path) = bulk {
        if email.is_some() {
            anyhow::bail!("--bulk and a positional email are mutually exclusive");
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        let rows = parse_bulk_invitation_csv(&content)?;
        let count = rows.len();
        let invitations: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "email": r.email,
                    "workspace": r.workspace,
                    "role": r.role,
                })
            })
            .collect();
        let result = api
            .tenant_invite_bulk(&serde_json::json!({ "invitations": invitations }))
            .await?;
        let invited = result["invited"]
            .as_u64()
            .or_else(|| result["created"].as_u64())
            .unwrap_or(count as u64);
        println!("Invited {invited} user(s) from {}.", path.display());
        return Ok(());
    }

    let email = email
        .ok_or_else(|| anyhow::anyhow!("no email given: pass an email or use --bulk <file.csv>"))?;
    if !email.contains('@') {
        anyhow::bail!("'{email}' is not an email address");
    }
    if role.is_some() && workspace.is_none() {
        anyhow::bail!("--role pre-assigns a workspace role and requires --workspace");
    }
    let mut body = serde_json::Map::new();
    body.insert("email".into(), serde_json::json!(email));
    if let Some(ws) = &workspace {
        body.insert("workspace".into(), serde_json::json!(ws));
        body.insert(
            "role".into(),
            serde_json::json!(role.as_deref().unwrap_or("Developer")),
        );
    }
    api.tenant_invite(&serde_json::Value::Object(body)).await?;
    match workspace {
        Some(ws) => println!("Invitation sent to {email} (workspace {ws} pre-assigned)."),
        None => println!("Invitation sent to {email}."),
    }
    Ok(())
}

/// `gyre workspace invite` — `@username` (or bare username / user id)
/// invites an existing tenant member via POST /workspaces/:id/invite; an
/// email address takes the tenant-invitation path with this workspace and
/// role pre-assigned (user-management.md §CLI: "By email (sends tenant
/// invite if needed)").
async fn run_workspace_invite(
    api: &client::GyreClient,
    workspace: Option<&str>,
    user: &str,
    role: Option<&str>,
) -> Result<()> {
    let ws_id = resolve_workspace_target(api, workspace).await?;
    let role = role.unwrap_or("Developer");
    validate_workspace_role(role)?;

    if looks_like_email(user) {
        // Email invite: the invitee may not be a tenant user yet, so the
        // server sends a tenant invitation pre-assigned to this workspace
        // (spec: "sends tenant invite if needed"). The pre-assignment
        // names the workspace by the slug the caller targeted (explicit
        // --workspace, or the one inferred from the git remote).
        let slug = resolve_workspace_slug_target(workspace)?;
        let body = serde_json::json!({
            "email": user,
            "workspace": slug,
            "role": role,
        });
        api.tenant_invite(&body).await?;
        println!("Tenant invitation sent to {user} (workspace pre-assigned, role {role}).");
        return Ok(());
    }

    let username = user.strip_prefix('@').unwrap_or(user);
    let profile = api.get_public_profile(username).await?;
    api.invite_workspace_member(&ws_id, &profile.id, role)
        .await?;
    let who = if profile.display_name.is_empty() {
        format!("@{}", profile.username)
    } else {
        format!("@{} ({})", profile.username, profile.display_name)
    };
    println!("Invited {who} to the workspace with role {role}.");
    Ok(())
}

/// `gyre workspace members set-role` — change a member's role via
/// PUT /workspaces/:id/members/:user_id/role. `--transfer` is the explicit
/// ownership-transfer acknowledgment: it requires the Owner role (the
/// server performs and audits the actual transfer).
async fn run_member_set_role(
    api: &client::GyreClient,
    workspace: Option<&str>,
    user: &str,
    role: &str,
    transfer: bool,
    reason: Option<&str>,
) -> Result<()> {
    validate_workspace_role(role)?;
    if transfer && !role.eq_ignore_ascii_case("Owner") {
        anyhow::bail!("--transfer is only valid when setting the role to Owner");
    }
    let reason = match (transfer, reason) {
        (_, Some(r)) => Some(r.to_string()),
        (true, None) => Some("ownership transfer via CLI".to_string()),
        (false, None) => None,
    };
    let ws_id = resolve_workspace_target(api, workspace).await?;
    let user_id = resolve_user_ref(api, user).await?;
    api.set_workspace_member_role(&ws_id, &user_id, role, reason.as_deref())
        .await?;
    if transfer {
        println!("Ownership transferred to {user}.");
    } else {
        println!("Role of {user} set to {role}.");
    }
    Ok(())
}

/// `gyre team create` — resolve the lead handle, create the team via
/// POST /workspaces/:id/teams.
async fn run_team_create(
    api: &client::GyreClient,
    workspace: Option<&str>,
    name: &str,
    lead: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    let ws_id = resolve_workspace_target(api, workspace).await?;
    // Resolve the lead before creating anything: a lead that is not a
    // tenant user should fail before the team exists, not after
    // (the server additionally requires the lead to be a workspace
    // member).
    let lead_id = match lead {
        Some(l) => resolve_user_ref(api, l).await?,
        None => anyhow::bail!("--lead is required: pass --lead @username"),
    };
    let team = api
        .create_workspace_team(&ws_id, name, &lead_id, description)
        .await?;
    let slug = team["slug"].as_str().unwrap_or("");
    let id = team["id"].as_str().unwrap_or("");
    println!("Team '{}' created.", team["name"].as_str().unwrap_or(name));
    println!("  ID:    {id}");
    if !slug.is_empty() {
        println!("  Slug:  {slug}");
    }
    Ok(())
}

/// `gyre team add` / `gyre team remove` — resolve the user handle and the
/// team (by slug or name) within the target workspace, then add/remove
/// the membership.
async fn run_team_membership(
    api: &client::GyreClient,
    workspace: Option<&str>,
    user: &str,
    team_ref: &str,
    remove: bool,
) -> Result<()> {
    let ws_id = resolve_workspace_target(api, workspace).await?;
    let user_id = resolve_user_ref(api, user).await?;
    let team_id = resolve_team_ref(api, &ws_id, team_ref).await?;
    if remove {
        api.remove_team_member(&ws_id, &team_id, &user_id).await?;
        println!("{user} removed from team '{team_ref}'.");
    } else {
        api.add_team_member(&ws_id, &team_id, &user_id).await?;
        println!("{user} added to team '{team_ref}'.");
    }
    Ok(())
}

/// Resolve a team reference (slug or name) to the team id by listing the
/// workspace's teams and matching slug first, then name.
async fn resolve_team_ref(api: &client::GyreClient, ws_id: &str, team_ref: &str) -> Result<String> {
    let teams = api.list_workspace_teams(ws_id).await?;
    let items = teams["teams"].as_array().cloned().unwrap_or_default();
    let by_slug = items.iter().find(|t| {
        t["slug"]
            .as_str()
            .map(|s| s.eq_ignore_ascii_case(team_ref))
            .unwrap_or(false)
    });
    let by_name = items.iter().find(|t| {
        t["name"]
            .as_str()
            .map(|s| s.eq_ignore_ascii_case(team_ref))
            .unwrap_or(false)
    });
    let team = by_slug
        .or(by_name)
        .ok_or_else(|| anyhow::anyhow!("no team '{team_ref}' in this workspace"))?;
    Ok(team["id"].as_str().unwrap_or_default().to_string())
}

/// `gyre notifications list` — spec-path listing with filters. `--unread`
/// filters on the response's read flag (the server's filter surface has
/// no unread parameter); the fetch uses the server's maximum limit so the
/// filter sees the same window the unfiltered list would.
async fn run_notifications_list(
    api: &client::GyreClient,
    unread: bool,
    priority: Option<&str>,
    notification_type: Option<&str>,
    workspace: Option<&str>,
    limit: u32,
) -> Result<()> {
    if let Some(p) = priority {
        normalize_priority_level(p)?;
    }
    let workspace_id = match workspace {
        Some(slug) => Some(api.resolve_workspace_slug(slug).await?),
        None => None,
    };
    let fetch_limit = if unread { 200 } else { limit };
    let result = api
        .list_notifications_filtered(
            workspace_id.as_deref(),
            priority,
            notification_type,
            Some(fetch_limit),
        )
        .await?;
    let items = result["notifications"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let items: Vec<serde_json::Value> = if unread {
        items
            .into_iter()
            .filter(|n| !n["read"].as_bool().unwrap_or(false))
            .collect()
    } else {
        items
    };
    let items: Vec<serde_json::Value> = items.into_iter().take(limit as usize).collect();
    print_notification_items(&items);
    Ok(())
}

/// Requested delivery-channel updates for `gyre notifications preferences
/// set`. Omitted fields keep their current values.
#[derive(Default)]
struct ChannelPrefsUpdate<'a> {
    email_digest: Option<&'a str>,
    email_min_priority: Option<&'a str>,
    slack_url: Option<&'a str>,
    slack_min_priority: Option<&'a str>,
    webhook_url: Option<&'a str>,
    webhook_secret: Option<&'a str>,
    webhook_min_priority: Option<&'a str>,
}

/// `gyre notifications preferences set` — read-merge-write against
/// GET/PUT /api/v1/notifications/preferences so untouched channels keep
/// their current configuration.
async fn run_notification_preferences_set(
    api: &client::GyreClient,
    update: ChannelPrefsUpdate<'_>,
) -> Result<()> {
    let current = api.get_notification_preferences().await?;
    let merged = merge_channel_updates(current.get("channels"), &update)?;
    api.update_notification_preferences(&serde_json::json!({ "channels": merged }))
        .await?;
    println!("Notification preferences updated.");
    Ok(())
}

/// Pure merge of requested channel updates over the current channel
/// configuration (user-management.md §Delivery Channels). Values are
/// validated and normalized before merging; a missing/null current
/// configuration starts from the default channel shape (in_app always
/// true, email disabled). Any `--email-*` flag enables the email channel
/// (setting a digest or threshold on a disabled channel would silently
/// deliver nothing).
fn merge_channel_updates(
    current: Option<&serde_json::Value>,
    update: &ChannelPrefsUpdate<'_>,
) -> Result<serde_json::Value> {
    let digest = update.email_digest.map(normalize_digest).transpose()?;
    let email_min = update
        .email_min_priority
        .map(normalize_priority_level)
        .transpose()?;
    let slack_min = update
        .slack_min_priority
        .map(normalize_priority_level)
        .transpose()?;
    let webhook_min = update
        .webhook_min_priority
        .map(normalize_priority_level)
        .transpose()?;

    let mut channels = match current {
        Some(v) if v.is_object() => v.clone(),
        _ => serde_json::json!({
            "in_app": true,
            "email": { "enabled": false, "digest": "Off", "min_priority": "Medium" }
        }),
    };
    // in_app is always true ("can't disable", §Delivery Channels) — a
    // stored configuration claiming otherwise would fail the PUT.
    channels["in_app"] = serde_json::json!(true);

    if digest.is_some() || email_min.is_some() {
        let email = channels.get("email").cloned().unwrap_or_else(
            || serde_json::json!({"enabled": false, "digest": "Off", "min_priority": "Medium"}),
        );
        channels["email"] = email;
        if let Some(d) = digest {
            channels["email"]["digest"] = serde_json::json!(d);
        }
        if let Some(p) = email_min {
            channels["email"]["min_priority"] = serde_json::json!(p);
        }
        channels["email"]["enabled"] = serde_json::json!(true);
    }

    if update.slack_url.is_some() || slack_min.is_some() {
        let slack = channels
            .get("slack")
            .cloned()
            .filter(|s| s.is_object())
            .unwrap_or_else(|| serde_json::json!({"webhook_url": "", "channel": null, "min_priority": "Medium"}));
        channels["slack"] = slack;
        if let Some(u) = update.slack_url {
            channels["slack"]["webhook_url"] = serde_json::json!(u);
        }
        if let Some(p) = slack_min {
            channels["slack"]["min_priority"] = serde_json::json!(p);
        }
    }

    if update.webhook_url.is_some() || update.webhook_secret.is_some() || webhook_min.is_some() {
        let webhook = channels
            .get("webhook")
            .cloned()
            .filter(|w| w.is_object())
            .unwrap_or_else(
                || serde_json::json!({"url": "", "secret": "", "min_priority": "Medium"}),
            );
        channels["webhook"] = webhook;
        if let Some(u) = update.webhook_url {
            channels["webhook"]["url"] = serde_json::json!(u);
        }
        if let Some(s) = update.webhook_secret {
            channels["webhook"]["secret"] = serde_json::json!(s);
        }
        if let Some(p) = webhook_min {
            channels["webhook"]["min_priority"] = serde_json::json!(p);
        }
    }

    Ok(channels)
}

/// Print a profile block (`gyre whoami`, `gyre profile set`).
fn print_my_profile(me: &client::MyProfile) {
    println!("User Profile");
    println!("  ID:           {}", me.id);
    println!("  Username:     {}", me.username);
    println!("  Display Name: {}", me.display_name);
    if let Some(email) = &me.email {
        println!("  Email:        {email}");
    }
    if let Some(avatar) = &me.avatar_url {
        println!("  Avatar:       {avatar}");
    }
    println!("  Timezone:     {}", me.timezone);
    println!("  Locale:       {}", me.locale);
    println!("  Global Role:  {}", me.global_role);
    if !me.roles.is_empty() {
        println!("  Roles:        {}", me.roles.join(", "));
    }
    if let Some(login) = me.last_login_at {
        println!("  Last Login:   {}", format_timestamp(login));
    }
    println!("  Created:      {}", format_timestamp(me.created_at));
    println!("  Updated:      {}", format_timestamp(me.updated_at));
}

/// Print a session list (GET /users/me/sessions). Tolerant of the field
/// set the session endpoint exposes: id plus any of device/user_agent,
/// ip_address, created_at, last_active_at.
fn print_session_list(result: &serde_json::Value) {
    let items = result["sessions"].as_array();
    match items {
        Some(items) if !items.is_empty() => {
            println!(
                "{:<38} {:<18} {:<15} {:<20} {:<20}",
                "ID", "DEVICE", "IP", "CREATED", "LAST ACTIVE"
            );
            println!("{}", "-".repeat(115));
            for s in items {
                let id = s["id"].as_str().unwrap_or("");
                let device = s["device"]
                    .as_str()
                    .or_else(|| s["user_agent"].as_str())
                    .unwrap_or("");
                let device = truncate_for_table(device, 18);
                let ip = s["ip_address"].as_str().unwrap_or("");
                let created = s["created_at"]
                    .as_u64()
                    .map(format_timestamp)
                    .unwrap_or_default();
                let active = s["last_active_at"]
                    .as_u64()
                    .map(format_timestamp)
                    .unwrap_or_default();
                println!("{id:<38} {device:<18} {ip:<15} {created:<20} {active:<20}");
            }
        }
        _ => println!("No active sessions."),
    }
}

/// Truncate a string for a fixed-width table column, respecting char
/// boundaries (never slice at a fixed byte index).
fn truncate_for_table(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(width.saturating_sub(1)).collect();
        format!("{truncated}\u{2026}")
    }
}

/// Print a member list (GET /workspaces/:id/members).
fn print_member_list(result: &serde_json::Value) {
    let items = result["members"].as_array();
    match items {
        Some(items) if !items.is_empty() => {
            println!(
                "{:<38} {:<10} {:<9} {:<20} SINCE",
                "USER ID", "ROLE", "ACCEPTED", "INVITED"
            );
            println!("{}", "-".repeat(100));
            for m in items {
                let id = m["user_id"].as_str().unwrap_or("");
                let role = m["role"].as_str().unwrap_or("");
                let accepted = if m["accepted"].as_bool().unwrap_or(false) {
                    "yes"
                } else {
                    "no"
                };
                let invited = m["id"].as_str().unwrap_or("");
                let since = m["created_at"]
                    .as_u64()
                    .map(format_timestamp)
                    .unwrap_or_default();
                println!("{id:<38} {role:<10} {accepted:<9} {invited:<20} {since}");
            }
        }
        _ => println!("No members."),
    }
}

/// Print a pending-invitation list (GET /workspaces/:id/invitations).
fn print_invitation_list(result: &serde_json::Value) {
    let items = result["invitations"].as_array();
    match items {
        Some(items) if !items.is_empty() => {
            println!(
                "{:<38} {:<38} {:<10} {:<20} CREATED",
                "INVITATION ID", "USER ID", "ROLE", "INVITED"
            );
            println!("{}", "-".repeat(130));
            for inv in items {
                let id = inv["id"].as_str().unwrap_or("");
                let user = inv["user_id"].as_str().unwrap_or("");
                let role = inv["role"].as_str().unwrap_or("");
                let invited_by = inv["id"].as_str().unwrap_or("");
                let created = inv["created_at"]
                    .as_u64()
                    .map(format_timestamp)
                    .unwrap_or_default();
                println!("{id:<38} {user:<38} {role:<10} {invited_by:<20} {created}");
            }
        }
        _ => println!("No pending invitations."),
    }
}

/// Print a tenant invitation list (GET /api/v1/tenant/invitations).
/// Tolerant of the field set the tenant-invitation endpoint exposes.
fn print_tenant_invitation_list(result: &serde_json::Value) {
    let items = result["invitations"]
        .as_array()
        .or_else(|| result.as_array());
    match items {
        Some(items) if !items.is_empty() => {
            println!(
                "{:<38} {:<30} {:<14} {:<10} CREATED",
                "ID", "EMAIL", "ROLE", "STATUS"
            );
            println!("{}", "-".repeat(100));
            for inv in items {
                let id = inv["id"].as_str().unwrap_or("");
                let email = inv["email"].as_str().unwrap_or("");
                let role = inv["role"].as_str().unwrap_or("");
                let status = inv["status"].as_str().unwrap_or("");
                let created = inv["created_at"]
                    .as_u64()
                    .map(format_timestamp)
                    .unwrap_or_default();
                println!("{id:<38} {email:<30} {role:<14} {status:<10} {created}");
            }
        }
        _ => println!("No invitations."),
    }
}

/// Print a team list (GET /workspaces/:id/teams).
fn print_team_list(result: &serde_json::Value) {
    let items = result["teams"].as_array();
    match items {
        Some(items) if !items.is_empty() => {
            println!(
                "{:<38} {:<24} {:<24} {:<8} LEAD",
                "ID", "SLUG", "NAME", "MEMBERS"
            );
            println!("{}", "-".repeat(120));
            for t in items {
                let id = t["id"].as_str().unwrap_or("");
                let slug = t["slug"].as_str().unwrap_or("");
                let name = t["name"].as_str().unwrap_or("");
                let members = t["member_ids"].as_array().map(|m| m.len()).unwrap_or(0);
                let lead = t["lead"].as_str().unwrap_or("");
                println!("{id:<38} {slug:<24} {name:<24} {members:<8} {lead}");
            }
        }
        _ => println!("No teams."),
    }
}

/// Print notification rows in the same table shape `gyre inbox` uses,
/// extended with the read state (the §CLI surface filters on it).
fn print_notification_items(items: &[serde_json::Value]) {
    if items.is_empty() {
        println!("No notifications.");
        return;
    }
    println!(
        "{:<8} {:<38} {:<5} {:<7} {:<28} TITLE",
        "PRI", "ID", "TYPE", "READ", "AGE"
    );
    println!("{}", "-".repeat(110));
    for n in items {
        let id = n["id"].as_str().unwrap_or("");
        let pri = n["priority"].as_u64().unwrap_or(0);
        let ntype = n["notification_type"].as_str().unwrap_or("");
        let ntype = truncate_for_table(ntype, 28);
        let read = if n["read"].as_bool().unwrap_or(false) {
            "read"
        } else {
            "unread"
        };
        let title = n["title"].as_str().unwrap_or("");
        let created = n["created_at"].as_u64().unwrap_or(0);
        let age = format_age(created);
        println!("P{pri:<7} {id:<38} {ntype:<5} {read:<7} {age:<28} {title}");
    }
}

/// "123456 / 500000 (24.7%)" vs "123456 / none" — usage vs limit with
/// utilization when a limit is set. A zero cap has no utilization ratio to
/// report (0/0), so it is displayed explicitly instead of as NaN: a zero
/// cap blocks all usage, so any usage above it is over cap.
fn budget_usage_line(used: u64, limit: Option<u64>) -> String {
    match limit {
        Some(0) => {
            if used > 0 {
                format!("{used} / 0 (over cap)")
            } else {
                format!("{used} / 0 (zero cap)")
            }
        }
        Some(max) => format!("{used} / {max} ({:.1}%)", used as f64 / max as f64 * 100.0),
        None => format!("{used} / none"),
    }
}

fn budget_cost_line(used: f64, limit: Option<f64>) -> String {
    match limit {
        Some(0.0) => {
            if used > 0.0 {
                format!("${used:.2} / $0.00 (over cap)")
            } else {
                format!("${used:.2} / $0.00 (zero cap)")
            }
        }
        Some(max) => format!("${used:.2} / ${max:.2} ({:.1}%)", used / max * 100.0),
        None => format!("${used:.2} / none"),
    }
}

fn budget_limit_line<T: std::fmt::Display>(limit: Option<T>) -> String {
    match limit {
        Some(v) => v.to_string(),
        None => "none".to_string(),
    }
}

fn budget_cost_limit_line(limit: Option<f64>) -> String {
    match limit {
        Some(v) => format!("${v:.2}"),
        None => "none".to_string(),
    }
}

/// Print a workspace budget: limits + real-time usage
/// (`gyre budget show`, `gyre budget set` result).
fn print_workspace_budget(budget: &client::BudgetResponse) {
    println!("Budget for {} {}", budget.entity_type, budget.entity_id);
    println!();
    println!("Limits:");
    println!(
        "  max tokens per day:     {}",
        budget_limit_line(budget.config.max_tokens_per_day)
    );
    println!(
        "  max cost per day:       {}",
        budget_cost_limit_line(budget.config.max_cost_per_day)
    );
    println!(
        "  max concurrent agents:  {}",
        budget_limit_line(budget.config.max_concurrent_agents)
    );
    println!(
        "  max agent lifetime:     {}",
        budget_limit_line(
            budget
                .config
                .max_agent_lifetime_secs
                .map(|s| format!("{s}s"))
        )
    );
    println!("Usage today (since {}):", budget.usage.period_start);
    println!(
        "  tokens used:   {}",
        budget_usage_line(
            budget.usage.tokens_used_today,
            budget.config.max_tokens_per_day
        )
    );
    println!(
        "  cost today:    {}",
        budget_cost_line(budget.usage.cost_today, budget.config.max_cost_per_day)
    );
    println!(
        "  active agents: {}",
        budget_usage_line(
            budget.usage.active_agents as u64,
            budget.config.max_concurrent_agents.map(|a| a as u64),
        )
    );
}

/// Print the tenant-wide budget summary: tenant totals plus a per-workspace
/// table (`gyre budget show --tenant`).
fn print_tenant_budget_summary(summary: &client::TenantBudgetSummary) {
    println!("Tenant budget");
    println!();
    println!("Limits:");
    println!(
        "  max tokens per day:     {}",
        budget_limit_line(summary.tenant_config.max_tokens_per_day)
    );
    println!(
        "  max cost per day:       {}",
        budget_cost_limit_line(summary.tenant_config.max_cost_per_day)
    );
    println!(
        "  max concurrent agents:  {}",
        budget_limit_line(summary.tenant_config.max_concurrent_agents)
    );
    println!(
        "  max agent lifetime:     {}",
        budget_limit_line(
            summary
                .tenant_config
                .max_agent_lifetime_secs
                .map(|s| format!("{s}s"))
        )
    );
    println!("Usage today (since {}):", summary.tenant_usage.period_start);
    println!(
        "  tokens used:   {}",
        budget_usage_line(
            summary.tenant_usage.tokens_used_today,
            summary.tenant_config.max_tokens_per_day
        )
    );
    println!(
        "  cost today:    {}",
        budget_cost_line(
            summary.tenant_usage.cost_today,
            summary.tenant_config.max_cost_per_day
        )
    );
    println!(
        "  active agents: {}",
        budget_usage_line(
            summary.tenant_usage.active_agents as u64,
            summary
                .tenant_config
                .max_concurrent_agents
                .map(|a| a as u64),
        )
    );
    println!();
    if summary.workspaces.is_empty() {
        println!("No workspace budgets configured.");
        return;
    }
    println!("Workspaces:");
    println!(
        "{:<38} {:>12} {:>18} {:>14} {:>10}",
        "ID", "TOKENS", "TOKENS LIMIT", "COST TODAY", "AGENTS"
    );
    println!("{}", "-".repeat(96));
    for ws in &summary.workspaces {
        println!(
            "{:<38} {:>12} {:>18} {:>14} {:>10}",
            ws.entity_id,
            ws.usage.tokens_used_today,
            budget_limit_line(ws.config.max_tokens_per_day),
            format!("${:.2}", ws.usage.cost_today),
            ws.usage.active_agents
        );
    }
}

/// List notifications, resolving workspace slug and parsing priority range.
async fn print_notifications(
    api: &client::GyreClient,
    workspace_slug: Option<&str>,
    priority: Option<&str>,
) -> Result<()> {
    let workspace_id = if let Some(slug) = workspace_slug {
        Some(api.resolve_workspace_slug(slug).await?)
    } else {
        None
    };

    let (min_pri, max_pri) = parse_priority_range(priority)?;

    let result = api
        .get_notifications(workspace_id.as_deref(), min_pri, max_pri, None)
        .await?;

    let notifications = result["notifications"].as_array();
    match notifications {
        Some(items) if !items.is_empty() => {
            println!(
                "{:<8} {:<36} {:<5} {:<28} TITLE",
                "PRI", "ID", "TYPE", "AGE"
            );
            println!("{}", "-".repeat(100));
            for n in items {
                let id = n["id"].as_str().unwrap_or("");
                let pri = n["priority"].as_u64().unwrap_or(0);
                let ntype = n["notification_type"].as_str().unwrap_or("");
                let title = n["title"].as_str().unwrap_or("");
                let created = n["created_at"].as_u64().unwrap_or(0);
                let age = format_age(created);
                println!("P{pri:<7} {id:<36} {ntype:<5} {age:<28} {title}");
            }
        }
        _ => println!("No notifications."),
    }
    Ok(())
}

/// Print a briefing JSON response in human-readable format.
fn print_briefing(briefing: &serde_json::Value) {
    if let Some(summary) = briefing["summary"].as_str() {
        if !summary.is_empty() {
            println!("{summary}");
            println!();
        }
    }

    // Completed
    if let Some(items) = briefing["completed"].as_array() {
        if !items.is_empty() {
            println!("--- Completed ---");
            for item in items {
                print_briefing_item(item, "  - ");
            }
            println!();
        }
    }

    // Completed Agents (HSI §9 — agent decisions and uncertainties)
    if let Some(agents) = briefing["completed_agents"].as_array() {
        if !agents.is_empty() {
            println!("--- Completed Agents ---");
            for agent in agents {
                let agent_id = agent["agent_id"].as_str().unwrap_or("unknown");
                let spec_ref = agent["spec_ref"].as_str().unwrap_or("");
                if spec_ref.is_empty() {
                    println!("  Agent: {agent_id}");
                } else {
                    println!("  Agent: {agent_id} (spec: {spec_ref})");
                }
                if let Some(decisions) = agent["decisions"].as_array() {
                    for decision in decisions {
                        if let Some(text) = decision.as_str() {
                            println!("    Decision: {text}");
                        } else if let Some(obj) = decision.as_object() {
                            let reasoning =
                                obj.get("reasoning").and_then(|v| v.as_str()).unwrap_or("");
                            let confidence =
                                obj.get("confidence").and_then(|v| v.as_str()).unwrap_or("");
                            if !reasoning.is_empty() && !confidence.is_empty() {
                                println!("    Decision: {reasoning} (confidence: {confidence})");
                            } else if !reasoning.is_empty() {
                                println!("    Decision: {reasoning}");
                            }
                        }
                    }
                }
                if let Some(uncertainties) = agent["uncertainties"].as_array() {
                    for u in uncertainties {
                        if let Some(text) = u.as_str() {
                            println!("    Uncertainty: {text}");
                        }
                    }
                }
                if let Some(sha) = agent["conversation_sha"].as_str() {
                    if !sha.is_empty() {
                        println!("    Conversation: {sha}");
                    }
                }
                if let Some(completed_at) = agent["completed_at"].as_u64() {
                    println!("    Completed at: {}", format_timestamp(completed_at));
                }
            }
            println!();
        }
    }

    // In Progress
    if let Some(items) = briefing["in_progress"].as_array() {
        if !items.is_empty() {
            println!("--- In Progress ---");
            for item in items {
                print_briefing_item(item, "  - ");
            }
            println!();
        }
    }

    // Cross-Workspace
    if let Some(items) = briefing["cross_workspace"].as_array() {
        if !items.is_empty() {
            println!("--- Cross-Workspace ---");
            for item in items {
                print_briefing_item(item, "  ↔ ");
            }
            println!();
        }
    }

    // Exceptions
    if let Some(items) = briefing["exceptions"].as_array() {
        if !items.is_empty() {
            println!("--- Exceptions ---");
            for item in items {
                print_briefing_item(item, "  ! ");
            }
            println!();
        }
    }

    // Metrics
    if let Some(metrics) = briefing.get("metrics") {
        let mrs = metrics["mrs_merged"].as_u64().unwrap_or(0);
        let gates = metrics["gate_runs"].as_u64().unwrap_or(0);
        let budget = metrics["budget_spent_usd"].as_f64().unwrap_or(0.0);
        let pct = metrics["budget_pct"].as_u64().unwrap_or(0);
        println!("--- Metrics ---");
        println!("  MRs merged:    {mrs}");
        println!("  Gate runs:     {gates}");
        println!("  Budget spent:  ${budget:.2} ({pct}%)");
    }
}

/// Print a single briefing item with its details (description, spec_path, entity_type, entity_id, timestamp).
fn print_briefing_item(item: &serde_json::Value, prefix: &str) {
    let title = item["title"].as_str().unwrap_or("");
    println!("{prefix}{title}");
    if let Some(desc) = item["description"].as_str() {
        if !desc.is_empty() {
            println!("      {desc}");
        }
    }
    if let Some(spec) = item["spec_path"].as_str() {
        if !spec.is_empty() {
            println!("      (spec: {spec})");
        }
    }
    if let Some(etype) = item["entity_type"].as_str() {
        if !etype.is_empty() {
            let eid = item["entity_id"].as_str().unwrap_or("");
            if !eid.is_empty() {
                println!("      [{etype}: {eid}]");
            } else {
                println!("      [{etype}]");
            }
        }
    }
    if let Some(ts) = item["timestamp"].as_u64() {
        if ts > 0 {
            println!("      ({})", format_timestamp(ts));
        }
    }
    if let Some(ws_slug) = item["source_workspace_slug"].as_str() {
        if !ws_slug.is_empty() {
            println!("      (workspace: {ws_slug})");
        }
    }
    if let Some(actions) = item["actions"].as_array() {
        if !actions.is_empty() {
            let labels: Vec<&str> = actions.iter().filter_map(|a| a.as_str()).collect();
            println!("      Actions: {}", labels.join(" | "));
        }
    }
}

/// Print a dependency graph as a table (nodes + edges).
fn print_dependency_graph(graph: &serde_json::Value) {
    let nodes = graph["nodes"].as_array();
    let edges = graph["edges"].as_array();

    if let Some(nodes) = nodes {
        if nodes.is_empty() {
            println!("No dependencies in the graph.");
            return;
        }
        println!("Repos ({} nodes):", nodes.len());
        for n in nodes {
            let repo_id = n["repo_id"].as_str().unwrap_or("");
            let name = n["name"].as_str().unwrap_or("");
            println!("  {name} ({repo_id})");
        }
    }

    println!();

    if let Some(edges) = edges {
        if edges.is_empty() {
            println!("No dependency edges.");
        } else {
            println!("{:<30} {:<30} {:<10} STATUS", "SOURCE", "TARGET", "TYPE");
            println!("{}", "-".repeat(80));
            for e in edges {
                let source = e["source"].as_str().unwrap_or("");
                let target = e["target"].as_str().unwrap_or("");
                let etype = e["type"].as_str().unwrap_or("");
                let status = e["status"].as_str().unwrap_or("");
                println!("{:<30} {:<30} {:<10} {}", source, target, etype, status);
            }
        }
    }
}

/// Print a dependency graph filtered to a set of repo IDs.
fn print_dependency_graph_filtered(
    graph: &serde_json::Value,
    repo_ids: &std::collections::HashSet<String>,
) {
    let nodes = graph["nodes"].as_array();
    let edges = graph["edges"].as_array();

    let filtered_nodes: Vec<&serde_json::Value> = nodes
        .map(|ns| {
            ns.iter()
                .filter(|n| {
                    n["repo_id"]
                        .as_str()
                        .map(|id| repo_ids.contains(id))
                        .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default();

    if filtered_nodes.is_empty() {
        println!("No dependencies in this workspace.");
        return;
    }

    println!("Repos ({} nodes):", filtered_nodes.len());
    for n in &filtered_nodes {
        let repo_id = n["repo_id"].as_str().unwrap_or("");
        let name = n["name"].as_str().unwrap_or("");
        println!("  {name} ({repo_id})");
    }
    println!();

    let filtered_edges: Vec<&serde_json::Value> = edges
        .map(|es| {
            es.iter()
                .filter(|e| {
                    let src = e["source"].as_str().unwrap_or("");
                    let tgt = e["target"].as_str().unwrap_or("");
                    repo_ids.contains(src) || repo_ids.contains(tgt)
                })
                .collect()
        })
        .unwrap_or_default();

    if filtered_edges.is_empty() {
        println!("No dependency edges in this workspace.");
    } else {
        println!("{:<30} {:<30} {:<10} STATUS", "SOURCE", "TARGET", "TYPE");
        println!("{}", "-".repeat(80));
        for e in &filtered_edges {
            let source = e["source"].as_str().unwrap_or("");
            let target = e["target"].as_str().unwrap_or("");
            let etype = e["type"].as_str().unwrap_or("");
            let status = e["status"].as_str().unwrap_or("");
            println!("{:<30} {:<30} {:<10} {}", source, target, etype, status);
        }
    }
}

/// Render a dependency graph in Graphviz DOT format.
fn print_dot_graph(graph: &serde_json::Value) {
    let mut stdout = std::io::stdout();
    write_dot_graph(graph, &mut stdout).expect("failed to write DOT output");
}

fn write_dot_graph(graph: &serde_json::Value, w: &mut dyn std::io::Write) -> std::io::Result<()> {
    writeln!(w, "digraph dependencies {{")?;
    writeln!(w, "  rankdir=LR;")?;
    writeln!(w, "  node [shape=box, style=filled, fillcolor=lightblue];")?;

    if let Some(nodes) = graph["nodes"].as_array() {
        for n in nodes {
            let repo_id = n["repo_id"].as_str().unwrap_or("");
            let name = n["name"].as_str().unwrap_or(repo_id);
            let safe_name = name.replace('"', "\\\"");
            writeln!(w, "  \"{}\" [label=\"{}\"];", repo_id, safe_name)?;
        }
    }

    if let Some(edges) = graph["edges"].as_array() {
        for e in edges {
            let source = e["source"].as_str().unwrap_or("");
            let target = e["target"].as_str().unwrap_or("");
            let etype = e["type"].as_str().unwrap_or("manual");
            let color = match etype {
                "code" => "blue",
                "spec" => "green",
                "api" => "orange",
                "schema" => "purple",
                _ => "gray",
            };
            writeln!(
                w,
                "  \"{}\" -> \"{}\" [color={}, label=\"{}\"];",
                source, target, color, etype
            )?;
        }
    }

    writeln!(w, "}}")?;
    Ok(())
}

/// Display a list of SpecLinkResponse items as a table.
fn print_spec_links_table(links: &[serde_json::Value]) {
    println!(
        "{:<14} {:<40} {:<40} {:<8} STALE SINCE",
        "TYPE", "SOURCE", "TARGET", "STATUS"
    );
    println!("{}", "-".repeat(120));
    for link in links {
        let link_type = link["link_type"].as_str().unwrap_or("");
        let source = link["source_path"].as_str().unwrap_or("");
        let target = link["target_path"].as_str().unwrap_or("");
        let target_display = link["target_display"].as_str().unwrap_or(target);
        let status = link["status"].as_str().unwrap_or("");
        // id: internal UUID, not user-facing
        // target_repo_id: internal UUID, not user-facing
        // target_sha: internal hash, not user-facing
        // reason: shown inline below if present
        // created_at: creation time less relevant than staleness
        let stale_since = link["stale_since"]
            .as_u64()
            .map(format_timestamp)
            .unwrap_or_else(|| "-".to_string());
        let display_target = if target_display != target {
            target_display
        } else {
            target
        };
        println!(
            "{:<14} {:<40} {:<40} {:<8} {}",
            link_type, source, display_target, status, stale_since
        );
        if let Some(reason) = link["reason"].as_str() {
            if !reason.is_empty() {
                println!("             reason: {reason}");
            }
        }
    }
}

/// Render search results as a readable table (search.md §CLI):
/// one line per result with entity type, title, id, and snippet.
fn print_search_results(results: &client::SearchResponse) {
    if results.results.is_empty() {
        println!("No results for '{}'.", results.query);
        return;
    }
    println!(
        "Search results for '{}' ({} shown of {} total):",
        results.query,
        results.results.len(),
        results.total
    );
    println!("{}", "-".repeat(80));
    for r in &results.results {
        let snippet = r.snippet.replace(['\n', '\r'], " ");
        println!("[{}] {} ({})", r.entity_type, r.title, r.entity_id);
        if !snippet.is_empty() {
            println!("      {snippet}");
        }
    }
}

/// Current UNIX time in seconds.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Days from civil date to days since the UNIX epoch (Howard Hinnant's
/// algorithm — no chrono dependency needed for one date parse).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64; // [0, 399]
    let mp = (m + 9) % 12; // [0, 11]
    let doy = (153 * mp as u64 + 2) / 5 + d as u64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe as i64 - 719468
}

/// Parse a relative `--since` duration (`7d`, `12h`, `30m`, `2w`, `45s`)
/// against `now` (UNIX seconds), returning the cutoff. `now` is a
/// parameter so tests are deterministic.
fn parse_since_relative(s: &str, now: u64) -> Option<u64> {
    let s = s.trim();
    let mut chars = s.chars();
    // Relative: <count><unit> where unit ∈ {s, m, h, d, w}. Split on the
    // last char, not a byte index — a multibyte unit must not panic.
    let unit = chars.next_back()?;
    let digits = chars.as_str();
    let count: u64 = digits.parse().ok()?;
    let secs = match unit {
        's' => count,
        'm' => count.checked_mul(60)?,
        'h' => count.checked_mul(3600)?,
        'd' => count.checked_mul(86_400)?,
        'w' => count.checked_mul(604_800)?,
        _ => return None,
    };
    now.checked_sub(secs)
}

/// Parse a `--since` argument (search.md §Query Language `since:` facet):
/// either a relative duration or an ISO date, against `now`.
fn parse_since_at(s: &str, now: u64) -> Option<u64> {
    parse_since_relative(s, now).or_else(|| parse_since_date(s))
}

/// ISO date branch of `--since` (e.g. `2026-03-01` → UTC midnight).
/// Kept separate from the relative parser so each stays testable in
/// isolation; `parse_since_at` dispatches on shape.
fn parse_since_date(s: &str) -> Option<u64> {
    let s = s.trim();
    let mut parts = s.splitn(3, '-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    // Strictly zero-padded ISO 8601 calendar date: YYYY-MM-DD. Variable
    // widths (2026-3-1) are rejected rather than guessed at.
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    if !y.bytes().all(|b| b.is_ascii_digit())
        || !m.bytes().all(|b| b.is_ascii_digit())
        || !d.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let y: i64 = y.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    let d: u32 = d.parse().ok()?;
    if !is_valid_civil_date(y, m, d) {
        return None;
    }
    let days = days_from_civil(y, m, d);
    if days < 0 {
        return None;
    }
    Some(days as u64 * 86_400)
}

/// True when (y, m, d) is a real calendar date (rejects Feb 30, Apr 31,
/// month 13, ...). Years before 1970 are out of domain for `--since`.
fn is_valid_civil_date(y: i64, m: u32, d: u32) -> bool {
    if y < 1970 || !(1..=12).contains(&m) {
        return false;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let max_d = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            // February.
            if leap {
                29
            } else {
                28
            }
        }
    };
    (1..=max_d).contains(&d)
}

/// Parse a `--since` value against the current clock: relative (`7d`)
/// or ISO date (`2026-03-01`).
fn parse_since(s: &str) -> Option<u64> {
    parse_since_at(s, now_secs())
}

/// Live status/recency of a search result, resolved from the entity's
/// detail endpoint. The search index is written only at entity creation,
/// so its `status` facet freezes at the create-time value; `--status` and
/// `--since` must consult the live record.
#[derive(Debug, Clone, PartialEq)]
struct LiveEntityState {
    /// Current status string (task/mr/agent states; empty when unknown).
    status: String,
    /// Most recent activity timestamp (UNIX secs; 0 when unknown).
    updated_at: u64,
    /// True when neither field could be resolved for this entity type.
    unavailable: bool,
}

/// Fetch the live state behind one search result. Entity types the CLI
/// cannot query (spec/commit — no detail endpoint on the current API)
/// return `unavailable`, and the filter treats them conservatively
/// (see `result_matches_filters`).
async fn live_entity_state(api: &client::GyreClient, r: &client::SearchResult) -> LiveEntityState {
    match r.entity_type.as_str() {
        "task" => match api.get_task(&r.entity_id).await {
            Ok(t) => LiveEntityState {
                status: t.status,
                updated_at: t.updated_at,
                unavailable: false,
            },
            // The entity may have been deleted after indexing; a missing
            // record can no longer match a status/recency filter.
            Err(_) => LiveEntityState {
                status: String::new(),
                updated_at: 0,
                unavailable: true,
            },
        },
        "mr" => match api.get_mr(&r.entity_id).await {
            Ok(mr) => LiveEntityState {
                status: mr.status,
                updated_at: mr.updated_at,
                unavailable: false,
            },
            Err(_) => LiveEntityState {
                status: String::new(),
                updated_at: 0,
                unavailable: true,
            },
        },
        "agent" => match api.get_agent(&r.entity_id).await {
            Ok(a) => {
                // Agents carry no updated_at; recency is the later of
                // spawn time and last heartbeat.
                let updated_at = a.spawned_at.max(a.last_heartbeat.unwrap_or(0));
                LiveEntityState {
                    status: a.status,
                    updated_at,
                    unavailable: false,
                }
            }
            Err(_) => LiveEntityState {
                status: String::new(),
                updated_at: 0,
                unavailable: true,
            },
        },
        // spec/commit: no detail endpoint to resolve live state from.
        _ => LiveEntityState {
            status: String::new(),
            updated_at: 0,
            unavailable: true,
        },
    }
}

/// Decide whether one search result passes the `--status`/`--since`
/// filters, given its live state.
///
/// Semantics:
/// - `--status S`: keep results whose live status equals `S`
///   (case-insensitive). Results whose live status cannot be resolved
///   (entity type has no detail endpoint, or the record is gone) are
///   dropped — an unknown status is not a match.
/// - `--since T`: keep results with a known last-activity timestamp
///   at or after `T`. Unknown timestamps drop only when a timestamp
///   filter is active.
fn result_matches_filters(
    status_filter: Option<&str>,
    since_cutoff: Option<u64>,
    live: &LiveEntityState,
) -> bool {
    // Unresolvable live state (no detail endpoint for the entity type, or
    // the record is gone) can never satisfy a status or recency claim.
    if (status_filter.is_some() || since_cutoff.is_some()) && live.unavailable {
        return false;
    }
    if let Some(want) = status_filter {
        if live.status.to_lowercase() != want.to_lowercase() {
            return false;
        }
    }
    if let Some(cutoff) = since_cutoff {
        if live.updated_at < cutoff {
            return false;
        }
    }
    true
}

/// Collect autocomplete suggestions for `prefix`: results whose title starts
/// with the prefix (case-insensitive). The server's search endpoint matches
/// by substring anywhere in the title or body, so the prefix filter for
/// autocomplete is applied client-side.
fn collect_suggestions<'a>(
    prefix: &str,
    results: &'a client::SearchResponse,
) -> Vec<(&'a str, &'a str, &'a str)> {
    let p = prefix.to_lowercase();
    results
        .results
        .iter()
        .filter(|r| r.title.to_lowercase().starts_with(&p))
        .map(|r| {
            (
                r.entity_type.as_str(),
                r.title.as_str(),
                r.entity_id.as_str(),
            )
        })
        .collect()
}

/// Render autocomplete suggestions for `prefix` in `type  title  (id)` form.
fn print_search_suggestions(prefix: &str, results: &client::SearchResponse) {
    let matches = collect_suggestions(prefix, results);
    if matches.is_empty() {
        println!("No suggestions for '{}'.", prefix);
        return;
    }
    for (entity_type, title, entity_id) in &matches {
        println!("{entity_type}    {title}    ({entity_id})");
    }
}

/// Print a SpecGraphResponse as a text summary.
fn print_spec_graph_text(graph: &serde_json::Value) {
    let nodes = graph["nodes"].as_array();
    let edges = graph["edges"].as_array();

    if let Some(nodes) = nodes {
        if nodes.is_empty() {
            println!("No specs in the graph.");
            return;
        }
        println!("Specs ({} nodes):", nodes.len());
        for n in nodes {
            let path = n["path"].as_str().unwrap_or("");
            let title = n["title"].as_str().unwrap_or("");
            let approval = n["approval_status"].as_str().unwrap_or("");
            if title.is_empty() {
                println!("  {path} [{approval}]");
            } else {
                println!("  {path} — {title} [{approval}]");
            }
        }
    }

    println!();

    if let Some(edges) = edges {
        if edges.is_empty() {
            println!("No spec links.");
        } else {
            println!("{:<40} {:<14} {:<40} STATUS", "SOURCE", "TYPE", "TARGET");
            println!("{}", "-".repeat(100));
            for e in edges {
                let source = e["source"].as_str().unwrap_or("");
                let target = e["target"].as_str().unwrap_or("");
                let link_type = e["link_type"].as_str().unwrap_or("");
                let status = e["status"].as_str().unwrap_or("");
                // reason: omitted in summary view
                println!("{:<40} {:<14} {:<40} {}", source, link_type, target, status);
            }
        }
    }
}

/// Print a SpecGraphResponse as Graphviz DOT format.
fn print_spec_dot_graph(graph: &serde_json::Value) {
    let mut stdout = std::io::stdout();
    write_spec_dot_graph(graph, &mut stdout).expect("failed to write DOT output");
}

/// Write a SpecGraphResponse as Graphviz DOT format.
fn write_spec_dot_graph(
    graph: &serde_json::Value,
    w: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    writeln!(w, "digraph specs {{")?;
    writeln!(w, "  rankdir=LR;")?;
    writeln!(w, "  node [shape=box, style=filled];")?;

    if let Some(nodes) = graph["nodes"].as_array() {
        for n in nodes {
            let path = n["path"].as_str().unwrap_or("");
            let title = n["title"].as_str().unwrap_or(path);
            let approval = n["approval_status"].as_str().unwrap_or("");
            let fillcolor = match approval {
                "approved" | "Approved" => "palegreen",
                "rejected" | "Rejected" => "lightcoral",
                "pending" | "Pending" => "lightyellow",
                _ => "lightblue",
            };
            let safe_title = title.replace('"', "\\\"");
            let safe_path = path.replace('"', "\\\"");
            writeln!(
                w,
                "  \"{}\" [label=\"{}\\n[{}]\", fillcolor={}];",
                safe_path, safe_title, approval, fillcolor
            )?;
        }
    }

    if let Some(edges) = graph["edges"].as_array() {
        for e in edges {
            let source = e["source"].as_str().unwrap_or("");
            let target = e["target"].as_str().unwrap_or("");
            let link_type = e["link_type"].as_str().unwrap_or("");
            let status = e["status"].as_str().unwrap_or("");
            // reason: not shown in DOT graph (visual medium uses color/style instead)
            let (color, style) = match link_type {
                "implements" => ("blue", "solid"),
                "depends_on" => ("green", "solid"),
                "supersedes" => ("gray", "solid"),
                "conflicts_with" => ("red", "solid"),
                "extends" => ("orange", "solid"),
                "references" => ("gray", "dotted"),
                _ => ("gray", "solid"),
            };
            let penwidth = if status == "stale" { "2.0" } else { "1.0" };
            let edge_color = if status == "stale" { "gold" } else { color };
            let safe_source = source.replace('"', "\\\"");
            let safe_target = target.replace('"', "\\\"");
            writeln!(
                w,
                "  \"{}\" -> \"{}\" [color={}, style={}, penwidth={}, label=\"{}\"];",
                safe_source, safe_target, edge_color, style, penwidth, link_type
            )?;
        }
    }

    writeln!(w, "}}")?;
    Ok(())
}

/// Parse a priority range string like "1-5" into (min, max).
fn parse_priority_range(range: Option<&str>) -> Result<(Option<u8>, Option<u8>)> {
    match range {
        None => Ok((None, None)),
        Some(s) => {
            let parts: Vec<&str> = s.split('-').collect();
            if parts.len() != 2 {
                anyhow::bail!(
                    "invalid priority range '{s}': expected format 'MIN-MAX' (e.g., '1-5')"
                );
            }
            let min: u8 = parts[0]
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid min priority '{}'", parts[0]))?;
            let max: u8 = parts[1]
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid max priority '{}'", parts[1]))?;
            Ok((Some(min), Some(max)))
        }
    }
}

/// Format a Unix epoch timestamp as a human-readable age string.
fn format_age(epoch_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if epoch_secs == 0 || epoch_secs > now {
        return "just now".to_string();
    }
    let diff = now - epoch_secs;
    if diff < 60 {
        format!("{diff}s ago")
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

/// Format a Unix epoch timestamp as ISO-ish string.
fn format_timestamp(epoch_secs: u64) -> String {
    // Simple UTC formatting without chrono dependency
    let secs = epoch_secs;
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Approximate date from days since epoch (1970-01-01)
    // Good enough for display — not calendar-precise for leap seconds
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from Howard Hinnant's chrono-compatible date library
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_connect_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "connect",
            "--server",
            "ws://host:3000/ws",
            "--token",
            "tok",
        ]);
        assert!(args.is_ok());
        let cli = args.unwrap();
        if let Commands::Connect { server, token } = cli.command {
            assert_eq!(server, "ws://host:3000/ws");
            assert_eq!(token, "tok");
        } else {
            panic!("Expected Connect");
        }
    }

    #[test]
    fn cli_ping_parses() {
        let args = Cli::try_parse_from(["gyre", "ping"]);
        assert!(args.is_ok());
        if let Commands::Ping { server, token } = args.unwrap().command {
            assert_eq!(server, DEFAULT_SERVER);
            assert_eq!(token, DEFAULT_TOKEN);
        } else {
            panic!("Expected Ping");
        }
    }

    #[test]
    fn cli_health_parses() {
        let args = Cli::try_parse_from(["gyre", "health", "--server", "http://myhost:8080"]);
        assert!(args.is_ok());
        if let Commands::Health { server } = args.unwrap().command {
            assert_eq!(server, "http://myhost:8080");
        } else {
            panic!("Expected Health");
        }
    }

    #[test]
    fn cli_tui_parses() {
        let args = Cli::try_parse_from(["gyre", "tui"]);
        assert!(args.is_ok());
    }

    #[test]
    fn cli_init_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "init",
            "--server",
            "http://localhost:3333",
            "--name",
            "ralph",
        ]);
        assert!(args.is_ok());
        if let Commands::Init {
            server,
            name,
            token,
        } = args.unwrap().command
        {
            assert_eq!(server, "http://localhost:3333");
            assert_eq!(name, "ralph");
            assert_eq!(token, DEFAULT_TOKEN);
        } else {
            panic!("Expected Init");
        }
    }

    #[test]
    fn cli_bootstrap_parses_with_defaults() {
        let args = Cli::try_parse_from(["gyre", "bootstrap", "--tenant", "Acme Corp"]);
        assert!(args.is_ok());
        if let Commands::Bootstrap {
            server,
            token,
            tenant,
            workspace,
            repo,
            repo_path,
            admin_user,
            oidc_issuer,
            dev,
            starter_kit,
        } = args.unwrap().command
        {
            assert_eq!(token.as_deref(), None);
            assert_eq!(server, "http://localhost:3000");
            assert_eq!(tenant.as_deref(), Some("Acme Corp"));
            assert!(workspace.is_none());
            assert!(repo.is_none());
            assert!(repo_path.is_none());
            assert!(admin_user.is_none());
            assert!(oidc_issuer.is_none());
            assert!(!dev);
            assert!(!starter_kit);
        } else {
            panic!("Expected Bootstrap");
        }
    }

    #[test]
    fn cli_bootstrap_parses_all_flags() {
        let args = Cli::try_parse_from([
            "gyre",
            "bootstrap",
            "--server",
            "http://boothost:9100",
            "--token",
            "boot-tok",
            "--tenant",
            "Acme Corp",
            "--workspace",
            "Platform Team",
            "--repo",
            "gyre",
            "--repo-path",
            "/tmp/checkout",
            "--admin-user",
            "jsell",
            "--oidc-issuer",
            "https://sso.acme.test",
            "--starter-kit",
        ]);
        assert!(args.is_ok());
        if let Commands::Bootstrap {
            server,
            token,
            tenant,
            workspace,
            repo,
            repo_path,
            admin_user,
            oidc_issuer,
            dev,
            starter_kit,
        } = args.unwrap().command
        {
            assert_eq!(server, "http://boothost:9100");
            assert_eq!(token.as_deref(), Some("boot-tok"));
            assert_eq!(tenant.as_deref(), Some("Acme Corp"));
            assert_eq!(workspace.as_deref(), Some("Platform Team"));
            assert_eq!(repo.as_deref(), Some("gyre"));
            assert_eq!(repo_path.as_deref(), Some("/tmp/checkout"));
            assert_eq!(admin_user.as_deref(), Some("jsell"));
            assert_eq!(oidc_issuer.as_deref(), Some("https://sso.acme.test"));
            assert!(!dev);
            assert!(starter_kit);
        } else {
            panic!("Expected Bootstrap");
        }
    }

    #[test]
    fn cli_bootstrap_dev_mode_parses() {
        let args = Cli::try_parse_from(["gyre", "bootstrap", "--dev"]);
        assert!(args.is_ok());
        if let Commands::Bootstrap { dev, .. } = args.unwrap().command {
            assert!(dev);
        } else {
            panic!("Expected Bootstrap");
        }
    }

    #[test]
    fn cli_clone_parses() {
        let args = Cli::try_parse_from(["gyre", "clone", "myproject/myrepo"]);
        assert!(args.is_ok());
        if let Commands::Clone { repo, dir } = args.unwrap().command {
            assert_eq!(repo, "myproject/myrepo");
            assert!(dir.is_none());
        } else {
            panic!("Expected Clone");
        }
    }

    #[test]
    fn cli_clone_with_dir_parses() {
        let args = Cli::try_parse_from(["gyre", "clone", "proj/repo", "--dir", "/tmp/myrepo"]);
        assert!(args.is_ok());
    }

    #[test]
    fn cli_push_parses() {
        let args = Cli::try_parse_from(["gyre", "push"]);
        assert!(args.is_ok());
        if let Commands::Push { remote } = args.unwrap().command {
            assert_eq!(remote, "origin");
        } else {
            panic!("Expected Push");
        }
    }

    #[test]
    fn cli_push_custom_remote_parses() {
        let args = Cli::try_parse_from(["gyre", "push", "--remote", "gyre"]);
        assert!(args.is_ok());
    }

    #[test]
    fn cli_mr_create_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "mr",
            "create",
            "--title",
            "My PR",
            "--repo-id",
            "repo-123",
        ]);
        assert!(args.is_ok());
        if let Commands::Mr {
            command:
                MrCommands::Create {
                    title,
                    target,
                    repo_id,
                    source,
                },
        } = args.unwrap().command
        {
            assert_eq!(title, "My PR");
            assert_eq!(target, "main");
            assert_eq!(repo_id, "repo-123");
            assert!(source.is_none());
        } else {
            panic!("Expected Mr Create");
        }
    }

    #[test]
    fn cli_tasks_list_parses() {
        let args = Cli::try_parse_from(["gyre", "tasks", "list"]);
        assert!(args.is_ok());
    }

    #[test]
    fn cli_tasks_list_with_filter_parses() {
        let args =
            Cli::try_parse_from(["gyre", "tasks", "list", "--status", "in_progress", "--mine"]);
        assert!(args.is_ok());
        if let Commands::Tasks {
            command: TaskCommands::List { status, mine },
        } = args.unwrap().command
        {
            assert_eq!(status.as_deref(), Some("in_progress"));
            assert!(mine);
        } else {
            panic!("Expected Tasks List");
        }
    }

    #[test]
    fn cli_tasks_take_parses() {
        let args = Cli::try_parse_from(["gyre", "tasks", "take", "task-001"]);
        assert!(args.is_ok());
        if let Commands::Tasks {
            command: TaskCommands::Take { id },
        } = args.unwrap().command
        {
            assert_eq!(id, "task-001");
        } else {
            panic!("Expected Tasks Take");
        }
    }

    #[test]
    fn cli_status_parses() {
        let args = Cli::try_parse_from(["gyre", "status"]);
        assert!(args.is_ok());
    }

    #[test]
    fn cli_verify() {
        Cli::command().debug_assert();
    }

    #[test]
    fn cli_release_prepare_parses() {
        let args = Cli::try_parse_from(["gyre", "release", "prepare", "--repo-id", "repo-123"]);
        assert!(args.is_ok());
        if let Commands::Release {
            command:
                ReleaseCommands::Prepare {
                    repo_id,
                    branch,
                    from,
                    create_mr,
                    markdown,
                    ..
                },
        } = args.unwrap().command
        {
            assert_eq!(repo_id, "repo-123");
            assert!(branch.is_none());
            assert!(from.is_none());
            assert!(!create_mr);
            assert!(!markdown);
        } else {
            panic!("Expected Release Prepare");
        }
    }

    #[test]
    fn cli_release_prepare_with_options_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "release",
            "prepare",
            "--repo-id",
            "repo-456",
            "--branch",
            "main",
            "--from",
            "v1.2.3",
            "--create-mr",
            "--markdown",
        ]);
        assert!(args.is_ok());
        if let Commands::Release {
            command:
                ReleaseCommands::Prepare {
                    repo_id,
                    branch,
                    from,
                    create_mr,
                    markdown,
                    ..
                },
        } = args.unwrap().command
        {
            assert_eq!(repo_id, "repo-456");
            assert_eq!(branch.as_deref(), Some("main"));
            assert_eq!(from.as_deref(), Some("v1.2.3"));
            assert!(create_mr);
            assert!(markdown);
        } else {
            panic!("Expected Release Prepare with options");
        }
    }

    // ── Briefing command tests ───────────────────────────────────────────────

    #[test]
    fn cli_briefing_parses() {
        let args = Cli::try_parse_from(["gyre", "briefing", "--workspace", "platform"]);
        assert!(args.is_ok());
        if let Commands::Briefing { workspace, since } = args.unwrap().command {
            assert_eq!(workspace.as_deref(), Some("platform"));
            assert!(since.is_none());
        } else {
            panic!("Expected Briefing");
        }
    }

    #[test]
    fn cli_briefing_with_since_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "briefing",
            "--workspace",
            "platform",
            "--since",
            "1700000000",
        ]);
        assert!(args.is_ok());
        if let Commands::Briefing { workspace, since } = args.unwrap().command {
            assert_eq!(workspace.as_deref(), Some("platform"));
            assert_eq!(since, Some(1700000000));
        } else {
            panic!("Expected Briefing");
        }
    }

    #[test]
    fn cli_briefing_without_workspace_parses() {
        let args = Cli::try_parse_from(["gyre", "briefing"]);
        assert!(args.is_ok());
        if let Commands::Briefing { workspace, since } = args.unwrap().command {
            assert!(workspace.is_none());
            assert!(since.is_none());
        } else {
            panic!("Expected Briefing");
        }
    }

    // ── Inbox command tests ─────────────────────────────────────────────────

    #[test]
    fn cli_inbox_bare_parses() {
        // Bare `gyre inbox` should parse (defaults to list behavior)
        let args = Cli::try_parse_from(["gyre", "inbox"]);
        assert!(args.is_ok());
        if let Commands::Inbox {
            workspace,
            priority,
            command,
        } = args.unwrap().command
        {
            assert!(workspace.is_none());
            assert!(priority.is_none());
            assert!(command.is_none());
        } else {
            panic!("Expected Inbox");
        }
    }

    #[test]
    fn cli_inbox_bare_with_filters_parses() {
        let args =
            Cli::try_parse_from(["gyre", "inbox", "--workspace", "myws", "--priority", "1-5"]);
        assert!(args.is_ok());
        if let Commands::Inbox {
            workspace,
            priority,
            command,
        } = args.unwrap().command
        {
            assert_eq!(workspace.as_deref(), Some("myws"));
            assert_eq!(priority.as_deref(), Some("1-5"));
            assert!(command.is_none());
        } else {
            panic!("Expected Inbox");
        }
    }

    #[test]
    fn cli_inbox_list_subcommand_parses() {
        let args = Cli::try_parse_from(["gyre", "inbox", "list"]);
        assert!(args.is_ok());
        if let Commands::Inbox { command, .. } = args.unwrap().command {
            assert!(matches!(command, Some(InboxCommands::List { .. })));
        } else {
            panic!("Expected Inbox");
        }
    }

    #[test]
    fn cli_inbox_list_with_filters_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "inbox",
            "list",
            "--workspace",
            "myws",
            "--priority",
            "1-5",
        ]);
        assert!(args.is_ok());
        if let Commands::Inbox { command, .. } = args.unwrap().command {
            if let Some(InboxCommands::List {
                workspace,
                priority,
            }) = command
            {
                assert_eq!(workspace.as_deref(), Some("myws"));
                assert_eq!(priority.as_deref(), Some("1-5"));
            } else {
                panic!("Expected Inbox List subcommand");
            }
        } else {
            panic!("Expected Inbox");
        }
    }

    #[test]
    fn cli_inbox_dismiss_parses() {
        let args = Cli::try_parse_from(["gyre", "inbox", "dismiss", "notif-123"]);
        assert!(args.is_ok());
        if let Commands::Inbox { command, .. } = args.unwrap().command {
            if let Some(InboxCommands::Dismiss { id }) = command {
                assert_eq!(id, "notif-123");
            } else {
                panic!("Expected Inbox Dismiss");
            }
        } else {
            panic!("Expected Inbox");
        }
    }

    #[test]
    fn cli_inbox_resolve_parses() {
        let args = Cli::try_parse_from(["gyre", "inbox", "resolve", "notif-456"]);
        assert!(args.is_ok());
        if let Commands::Inbox { command, .. } = args.unwrap().command {
            if let Some(InboxCommands::Resolve { id }) = command {
                assert_eq!(id, "notif-456");
            } else {
                panic!("Expected Inbox Resolve");
            }
        } else {
            panic!("Expected Inbox");
        }
    }

    // ── Explore command tests ───────────────────────────────────────────────

    #[test]
    fn cli_explore_parses() {
        let args = Cli::try_parse_from(["gyre", "explore", "UserRepository"]);
        assert!(args.is_ok());
        if let Commands::Explore {
            concept,
            repo,
            workspace,
        } = args.unwrap().command
        {
            assert_eq!(concept, "UserRepository");
            assert!(repo.is_none());
            assert!(workspace.is_none());
        } else {
            panic!("Expected Explore");
        }
    }

    #[test]
    fn cli_explore_with_repo_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "explore",
            "AuthMiddleware",
            "--repo",
            "my-service",
            "--workspace",
            "platform",
        ]);
        assert!(args.is_ok());
        if let Commands::Explore {
            concept,
            repo,
            workspace,
        } = args.unwrap().command
        {
            assert_eq!(concept, "AuthMiddleware");
            assert_eq!(repo.as_deref(), Some("my-service"));
            assert_eq!(workspace.as_deref(), Some("platform"));
        } else {
            panic!("Expected Explore");
        }
    }

    #[test]
    fn cli_explore_repo_without_workspace_parses() {
        // --repo without --workspace is valid: workspace is inferred from git remote at runtime
        let args =
            Cli::try_parse_from(["gyre", "explore", "AuthMiddleware", "--repo", "my-service"]);
        assert!(args.is_ok());
        if let Commands::Explore {
            concept,
            repo,
            workspace,
        } = args.unwrap().command
        {
            assert_eq!(concept, "AuthMiddleware");
            assert_eq!(repo.as_deref(), Some("my-service"));
            assert!(workspace.is_none());
        } else {
            panic!("Expected Explore");
        }
    }

    #[test]
    fn cli_explore_with_workspace_parses() {
        let args =
            Cli::try_parse_from(["gyre", "explore", "HttpServer", "--workspace", "platform"]);
        assert!(args.is_ok());
        if let Commands::Explore {
            concept,
            repo,
            workspace,
        } = args.unwrap().command
        {
            assert_eq!(concept, "HttpServer");
            assert!(repo.is_none());
            assert_eq!(workspace.as_deref(), Some("platform"));
        } else {
            panic!("Expected Explore");
        }
    }

    // ── Search command tests ────────────────────────────────────────────────

    #[test]
    fn cli_search_parses() {
        let args = Cli::try_parse_from(["gyre", "search", "identity security"]);
        assert!(args.is_ok());
        if let Commands::Search {
            query,
            r#type,
            status,
            workspace,
            since,
            suggest,
            limit,
        } = args.unwrap().command
        {
            assert_eq!(query.as_deref(), Some("identity security"));
            assert!(r#type.is_none());
            assert!(status.is_none());
            assert!(workspace.is_none());
            assert!(since.is_none());
            assert!(suggest.is_none());
            assert_eq!(limit, 20);
        } else {
            panic!("Expected Search");
        }
    }

    #[test]
    fn cli_search_faceted_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "search",
            "--type",
            "spec",
            "--status",
            "approved",
            "--workspace",
            "platform-team",
            "--since",
            "7d",
            "--limit",
            "5",
            "ABAC",
        ]);
        assert!(args.is_ok());
        if let Commands::Search {
            query,
            r#type,
            status,
            workspace,
            since,
            suggest,
            limit,
        } = args.unwrap().command
        {
            assert_eq!(query.as_deref(), Some("ABAC"));
            assert_eq!(r#type.as_deref(), Some("spec"));
            assert_eq!(status.as_deref(), Some("approved"));
            assert_eq!(workspace.as_deref(), Some("platform-team"));
            assert_eq!(since.as_deref(), Some("7d"));
            assert!(suggest.is_none());
            assert_eq!(limit, 5);
        } else {
            panic!("Expected Search");
        }
    }

    #[test]
    fn cli_search_suggest_parses() {
        let args = Cli::try_parse_from(["gyre", "search", "--suggest", "iden"]);
        if let Commands::Search { query, suggest, .. } = args.unwrap().command {
            assert!(query.is_none());
            assert_eq!(suggest.as_deref(), Some("iden"));
        } else {
            panic!("Expected Search");
        }
    }

    #[test]
    fn parse_since_relative_units() {
        // Fixed `now` keeps the arithmetic observable.
        let now = 1_000_000_000u64;
        assert_eq!(parse_since_at("45s", now), Some(now - 45));
        assert_eq!(parse_since_at("30m", now), Some(now - 1_800));
        assert_eq!(parse_since_at("12h", now), Some(now - 43_200));
        assert_eq!(parse_since_at("7d", now), Some(now - 604_800));
        assert_eq!(parse_since_at("2w", now), Some(now - 1_209_600));
    }

    #[test]
    fn parse_since_relative_rejects_junk() {
        let now = 1_000_000_000u64;
        // Unknown unit, empty, bare unit, negative, float, and overflow
        // (a count whose seconds exceed `now` itself) all fail.
        assert_eq!(parse_since_at("7x", now), None);
        assert_eq!(parse_since_at("", now), None);
        assert_eq!(parse_since_at("d", now), None);
        assert_eq!(parse_since_at("-3d", now), None);
        assert_eq!(parse_since_at("1.5d", now), None);
        assert_eq!(parse_since_at("999999999999999999999d", now), None);
    }

    #[test]
    fn parse_since_multibyte_unit_does_not_panic() {
        // Regression: a byte-index split panicked on multibyte input
        // (`--since é`). It must be a clean parse error, not a crash.
        let now = 1_000_000_000u64;
        assert_eq!(parse_since_at("é", now), None);
        assert_eq!(parse_since_at("7é", now), None);
        assert_eq!(parse_since_at("café", now), None);
    }

    #[test]
    fn parse_since_relative_before_epoch_fails() {
        // A duration longer than elapsed time since the epoch cannot be
        // represented as a cutoff; it must be an error, not a wraparound.
        assert_eq!(parse_since_at("999999w", 1_000_000_000), None);
    }

    #[test]
    fn parse_since_iso_date() {
        // 2026-03-01T00:00:00Z == 1772323200 (verified independently).
        assert_eq!(parse_since_at("2026-03-01", 0), Some(1_772_323_200));
        // Leap-year day is a valid date.
        assert_eq!(parse_since_at("2024-02-29", 0), Some(1_709_164_800));
    }

    #[test]
    fn parse_since_iso_date_rejects_invalid() {
        assert_eq!(parse_since_at("2026-02-30", 0), None); // Feb has 28 days
        assert_eq!(parse_since_at("2026-13-01", 0), None); // month 13
        assert_eq!(parse_since_at("2026-04-31", 0), None); // Apr has 30 days
        assert_eq!(parse_since_at("2023-02-29", 0), None); // not a leap year
        assert_eq!(parse_since_at("1969-12-31", 0), None); // before epoch
        assert_eq!(parse_since_at("2026-3-1", 0), None); // not zero-padded
    }

    #[test]
    fn result_matches_status_filter() {
        let live = LiveEntityState {
            status: "in_progress".into(),
            updated_at: 1_000,
            unavailable: false,
        };
        // Exact and case-insensitive matches pass; other statuses fail.
        assert!(result_matches_filters(Some("in_progress"), None, &live));
        assert!(result_matches_filters(Some("In_Progress"), None, &live));
        assert!(!result_matches_filters(Some("done"), None, &live));
        // No filter passes everything.
        assert!(result_matches_filters(None, None, &live));
    }

    #[test]
    fn result_matches_since_filter() {
        let live = LiveEntityState {
            status: String::new(),
            updated_at: 1_000,
            unavailable: false,
        };
        assert!(result_matches_filters(None, Some(1_000), &live)); // boundary: at cutoff
        assert!(result_matches_filters(None, Some(999), &live));
        assert!(!result_matches_filters(None, Some(1_001), &live));
    }

    #[test]
    fn result_matches_filters_compose_with_and() {
        let live = LiveEntityState {
            status: "approved".into(),
            updated_at: 5_000,
            unavailable: false,
        };
        assert!(result_matches_filters(Some("approved"), Some(4_000), &live));
        // Status matches but recency does not → dropped.
        assert!(!result_matches_filters(
            Some("approved"),
            Some(6_000),
            &live
        ));
        // Recency matches but status does not → dropped.
        assert!(!result_matches_filters(Some("open"), Some(4_000), &live));
    }
    #[test]
    fn result_matches_filters_drop_unavailable_state() {
        // Unresolvable live state (no detail endpoint / deleted entity)
        // can never satisfy a status or recency claim.
        let live = LiveEntityState {
            status: String::new(),
            updated_at: 0,
            unavailable: true,
        };
        assert!(!result_matches_filters(Some("approved"), None, &live));
        assert!(!result_matches_filters(None, Some(1), &live));
        // Without filters, nothing consults live state.
        assert!(result_matches_filters(None, None, &live));
    }
    #[test]
    fn suggest_filters_to_title_prefix() {
        let results = client::SearchResponse {
            query: "iden".into(),
            total: 3,
            results: vec![
                client::SearchResult {
                    entity_type: "spec".into(),
                    entity_id: "system/identity-security.md".into(),
                    title: "Identity & Security".into(),
                    snippet: String::new(),
                    score: 3.0,
                    facets: Default::default(),
                },
                client::SearchResult {
                    entity_type: "task".into(),
                    entity_id: "task-042".into(),
                    title: "Coincidental iden-tifier cleanup".into(),
                    snippet: String::new(),
                    score: 1.0,
                    facets: Default::default(),
                },
                client::SearchResult {
                    entity_type: "spec".into(),
                    entity_id: "system/identity.md".into(),
                    title: "identity model".into(),
                    snippet: String::new(),
                    score: 2.0,
                    facets: Default::default(),
                },
            ],
        };
        let suggestions = collect_suggestions("iden", &results);
        assert_eq!(
            suggestions,
            vec![
                ("spec", "Identity & Security", "system/identity-security.md"),
                ("spec", "identity model", "system/identity.md"),
            ]
        );
    }

    #[test]
    fn suggest_prefix_is_case_insensitive() {
        let results = client::SearchResponse {
            query: "IDEN".into(),
            total: 1,
            results: vec![client::SearchResult {
                entity_type: "spec".into(),
                entity_id: "system/identity.md".into(),
                title: "identity model".into(),
                snippet: String::new(),
                score: 2.0,
                facets: Default::default(),
            }],
        };
        assert_eq!(
            collect_suggestions("IDEN", &results),
            vec![("spec", "identity model", "system/identity.md")]
        );
    }

    #[test]
    fn suggest_no_prefix_match_reports_none() {
        let results = client::SearchResponse {
            query: "xyz".into(),
            total: 1,
            results: vec![client::SearchResult {
                entity_type: "task".into(),
                entity_id: "task-001".into(),
                title: "unrelated".into(),
                snippet: String::new(),
                score: 1.0,
                facets: Default::default(),
            }],
        };
        assert!(collect_suggestions("xyz", &results).is_empty());
    }

    // ── Trace command tests ─────────────────────────────────────────────────

    #[test]
    fn cli_trace_parses() {
        let args = Cli::try_parse_from(["gyre", "trace", "mr-789"]);
        assert!(args.is_ok());
        if let Commands::Trace { mr_id } = args.unwrap().command {
            assert_eq!(mr_id, "mr-789");
        } else {
            panic!("Expected Trace");
        }
    }

    // ── Spec assist command tests ───────────────────────────────────────────

    #[test]
    fn cli_spec_assist_parses() {
        // Minimal: just path and instruction (repo inferred from git remote)
        let args = Cli::try_parse_from([
            "gyre",
            "spec",
            "assist",
            "specs/auth.md",
            "add RBAC section",
        ]);
        assert!(args.is_ok());
        if let Commands::Spec {
            command:
                SpecCommands::Assist {
                    path,
                    instruction,
                    repo,
                    workspace,
                },
        } = args.unwrap().command
        {
            assert_eq!(path, "specs/auth.md");
            assert_eq!(instruction, "add RBAC section");
            assert!(repo.is_none());
            assert!(workspace.is_none());
        } else {
            panic!("Expected Spec Assist");
        }
    }

    #[test]
    fn cli_spec_assist_with_explicit_repo_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "spec",
            "assist",
            "specs/auth.md",
            "add RBAC section",
            "--repo",
            "my-service",
            "--workspace",
            "platform",
        ]);
        assert!(args.is_ok());
        if let Commands::Spec {
            command:
                SpecCommands::Assist {
                    path,
                    instruction,
                    repo,
                    workspace,
                },
        } = args.unwrap().command
        {
            assert_eq!(path, "specs/auth.md");
            assert_eq!(instruction, "add RBAC section");
            assert_eq!(repo.as_deref(), Some("my-service"));
            assert_eq!(workspace.as_deref(), Some("platform"));
        } else {
            panic!("Expected Spec Assist");
        }
    }

    // ── Divergence command tests ────────────────────────────────────────────

    #[test]
    fn cli_divergence_parses() {
        let args = Cli::try_parse_from(["gyre", "divergence"]);
        assert!(args.is_ok());
        if let Commands::Divergence { workspace } = args.unwrap().command {
            assert!(workspace.is_none());
        } else {
            panic!("Expected Divergence");
        }
    }

    #[test]
    fn cli_divergence_with_workspace_parses() {
        let args = Cli::try_parse_from(["gyre", "divergence", "--workspace", "platform"]);
        assert!(args.is_ok());
        if let Commands::Divergence { workspace } = args.unwrap().command {
            assert_eq!(workspace.as_deref(), Some("platform"));
        } else {
            panic!("Expected Divergence");
        }
    }

    // ── Deps command tests ───────────────────────────────────────────────────

    #[test]
    fn cli_deps_show_bare_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "show"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command:
                DepsCommands::Show {
                    repo,
                    workspace,
                    tenant,
                },
        } = args.unwrap().command
        {
            assert_eq!(repo.as_deref(), None);
            assert!(!workspace);
            assert!(!tenant);
        } else {
            panic!("Expected Deps Show");
        }
    }

    #[test]
    fn cli_deps_show_workspace_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "show", "--workspace"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command:
                DepsCommands::Show {
                    repo,
                    workspace,
                    tenant,
                },
        } = args.unwrap().command
        {
            assert_eq!(repo.as_deref(), None);
            assert!(workspace);
            assert!(!tenant);
        } else {
            panic!("Expected Deps Show with --workspace");
        }
    }

    #[test]
    fn cli_deps_show_tenant_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "show", "--tenant"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command:
                DepsCommands::Show {
                    repo,
                    workspace,
                    tenant,
                },
        } = args.unwrap().command
        {
            assert_eq!(repo.as_deref(), None);
            assert!(!workspace);
            assert!(tenant);
        } else {
            panic!("Expected Deps Show with --tenant");
        }
    }

    #[test]
    fn cli_deps_show_named_repo_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "show", "repo-b"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command:
                DepsCommands::Show {
                    repo,
                    workspace,
                    tenant,
                },
        } = args.unwrap().command
        {
            assert_eq!(repo.as_deref(), Some("repo-b"));
            assert!(!workspace);
            assert!(!tenant);
        } else {
            panic!("Expected Deps Show with positional repo");
        }
    }

    #[test]
    fn cli_deps_graph_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "graph"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command: DepsCommands::Graph { format },
        } = args.unwrap().command
        {
            assert_eq!(format, "dot");
        } else {
            panic!("Expected Deps Graph");
        }
    }

    #[test]
    fn cli_deps_graph_custom_format_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "graph", "--format", "dot"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command: DepsCommands::Graph { format },
        } = args.unwrap().command
        {
            assert_eq!(format, "dot");
        } else {
            panic!("Expected Deps Graph with --format dot");
        }
    }

    #[test]
    fn cli_deps_impact_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "impact", "repo-b"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command: DepsCommands::Impact { repo },
        } = args.unwrap().command
        {
            assert_eq!(repo, "repo-b");
        } else {
            panic!("Expected Deps Impact");
        }
    }

    #[test]
    fn cli_deps_stale_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "stale"]);
        assert!(args.is_ok());
        assert!(matches!(
            args.unwrap().command,
            Commands::Deps {
                command: DepsCommands::Stale
            }
        ));
    }

    #[test]
    fn cli_deps_breaking_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "breaking"]);
        assert!(args.is_ok());
        assert!(matches!(
            args.unwrap().command,
            Commands::Deps {
                command: DepsCommands::Breaking
            }
        ));
    }

    #[test]
    fn cli_deps_add_parses() {
        let args =
            Cli::try_parse_from(["gyre", "deps", "add", "--target", "repo-b", "--type", "api"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command: DepsCommands::Add { target, r#type },
        } = args.unwrap().command
        {
            assert_eq!(target, "repo-b");
            assert_eq!(r#type, "api");
        } else {
            panic!("Expected Deps Add");
        }
    }

    #[test]
    fn cli_deps_acknowledge_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "acknowledge", "breaking-123"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command: DepsCommands::Acknowledge { id },
        } = args.unwrap().command
        {
            assert_eq!(id, "breaking-123");
        } else {
            panic!("Expected Deps Acknowledge");
        }
    }

    // ── Budget command tests ─────────────────────────────────────────────────

    #[test]
    fn cli_budget_show_bare_parses() {
        // `gyre budget show` — default (repo) scope, resolved to the owning
        // workspace at runtime.
        let args = Cli::try_parse_from(["gyre", "budget", "show"]);
        assert!(args.is_ok());
        if let Commands::Budget {
            command:
                BudgetCommands::Show {
                    workspace,
                    tenant,
                    workspace_name,
                    repo,
                },
        } = args.unwrap().command
        {
            assert!(!workspace);
            assert!(!tenant);
            assert_eq!(workspace_name, None);
            assert_eq!(repo, None);
        } else {
            panic!("Expected Budget Show");
        }
    }

    #[test]
    fn cli_budget_show_workspace_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "budget",
            "show",
            "--workspace",
            "--workspace-name",
            "platform",
        ]);
        assert!(args.is_ok());
        if let Commands::Budget {
            command:
                BudgetCommands::Show {
                    workspace,
                    tenant,
                    workspace_name,
                    repo: _,
                },
        } = args.unwrap().command
        {
            assert!(workspace);
            assert!(!tenant);
            assert_eq!(workspace_name.as_deref(), Some("platform"));
        } else {
            panic!("Expected Budget Show with --workspace");
        }
    }

    #[test]
    fn cli_budget_show_tenant_parses() {
        let args = Cli::try_parse_from(["gyre", "budget", "show", "--tenant"]);
        assert!(args.is_ok());
        if let Commands::Budget {
            command:
                BudgetCommands::Show {
                    workspace,
                    tenant,
                    workspace_name: _,
                    repo: _,
                },
        } = args.unwrap().command
        {
            assert!(!workspace);
            assert!(tenant);
        } else {
            panic!("Expected Budget Show with --tenant");
        }
    }

    #[test]
    fn cli_budget_show_workspace_and_tenant_conflict() {
        // --workspace and --tenant are mutually exclusive scope selectors.
        let args = Cli::try_parse_from(["gyre", "budget", "show", "--workspace", "--tenant"]);
        assert!(args.is_err());
    }

    #[test]
    fn cli_budget_show_repo_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "budget",
            "show",
            "--workspace-name",
            "platform",
            "--repo",
            "gyre",
        ]);
        assert!(args.is_ok());
        if let Commands::Budget {
            command:
                BudgetCommands::Show {
                    workspace: _,
                    tenant: _,
                    workspace_name,
                    repo,
                },
        } = args.unwrap().command
        {
            assert_eq!(workspace_name.as_deref(), Some("platform"));
            assert_eq!(repo.as_deref(), Some("gyre"));
        } else {
            panic!("Expected Budget Show with --workspace-name --repo");
        }
    }

    #[test]
    fn cli_budget_set_tokens_parses() {
        // Spec: `gyre budget set --llm-tokens 500000`
        let args = Cli::try_parse_from(["gyre", "budget", "set", "--llm-tokens", "500000"]);
        assert!(args.is_ok());
        if let Commands::Budget {
            command:
                BudgetCommands::Set {
                    tenant,
                    llm_tokens,
                    llm_cost,
                    max_agents,
                    max_agent_lifetime_secs,
                    workspace_name: _,
                    repo: _,
                    workspace: _,
                },
        } = args.unwrap().command
        {
            assert!(!tenant);
            assert_eq!(llm_tokens, Some(500000));
            assert_eq!(llm_cost, None);
            assert_eq!(max_agents, None);
            assert_eq!(max_agent_lifetime_secs, None);
        } else {
            panic!("Expected Budget Set with --llm-tokens");
        }
    }

    #[test]
    fn cli_budget_set_workspace_cost_parses() {
        // Spec: `gyre budget set --workspace --llm-cost 100.00`
        let args = Cli::try_parse_from([
            "gyre",
            "budget",
            "set",
            "--workspace",
            "--llm-cost",
            "100.00",
        ]);
        assert!(args.is_ok());
        if let Commands::Budget {
            command:
                BudgetCommands::Set {
                    workspace,
                    tenant,
                    llm_cost,
                    ..
                },
        } = args.unwrap().command
        {
            assert!(workspace);
            assert!(!tenant);
            assert_eq!(llm_cost, Some(100.0));
        } else {
            panic!("Expected Budget Set with --workspace --llm-cost");
        }
    }

    #[test]
    fn cli_budget_set_all_limits_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "budget",
            "set",
            "--llm-tokens",
            "500000",
            "--llm-cost",
            "100.00",
            "--max-agents",
            "10",
            "--max-agent-lifetime-secs",
            "3600",
        ]);
        assert!(args.is_ok());
        if let Commands::Budget {
            command:
                BudgetCommands::Set {
                    llm_tokens,
                    llm_cost,
                    max_agents,
                    max_agent_lifetime_secs,
                    ..
                },
        } = args.unwrap().command
        {
            assert_eq!(llm_tokens, Some(500000));
            assert_eq!(llm_cost, Some(100.0));
            assert_eq!(max_agents, Some(10));
            assert_eq!(max_agent_lifetime_secs, Some(3600));
        } else {
            panic!("Expected Budget Set with all limits");
        }
    }

    // ── `gyre secret` command parsing (platform-model.md §7 CLI) ──────────

    #[test]
    fn cli_secret_set_repo_scope_parses() {
        // Spec shape: `gyre secret set --repo gyre DATABASE_URL "postgres://..."`
        let args = Cli::try_parse_from([
            "gyre",
            "secret",
            "set",
            "--repo",
            "gyre",
            "DATABASE_URL",
            "postgres://user:pass@host/db",
        ]);
        assert!(args.is_ok());
        if let Commands::Secret {
            command:
                SecretCommands::Set {
                    name,
                    value,
                    scope,
                    workspace_name,
                    repo,
                    secret_type,
                    expires_at,
                },
        } = args.unwrap().command
        {
            assert_eq!(name, "DATABASE_URL");
            assert_eq!(value, "postgres://user:pass@host/db");
            assert_eq!(scope, None); // --repo selects repo scope by default
            assert_eq!(repo.as_deref(), Some("gyre"));
            assert_eq!(workspace_name, None);
            assert_eq!(secret_type, None);
            assert_eq!(expires_at, None);
        } else {
            panic!("Expected Secret Set with --repo");
        }
    }

    #[test]
    fn cli_secret_set_tenant_with_type_and_expiry_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "secret",
            "set",
            "--scope",
            "tenant",
            "--secret-type",
            "rotated",
            "--expires-at",
            "1893456000",
            "API_KEY",
            "secret-value",
        ]);
        assert!(args.is_ok());
        if let Commands::Secret {
            command:
                SecretCommands::Set {
                    name,
                    value,
                    scope,
                    secret_type,
                    expires_at,
                    ..
                },
        } = args.unwrap().command
        {
            assert_eq!(name, "API_KEY");
            assert_eq!(value, "secret-value");
            assert_eq!(scope.as_deref(), Some("tenant"));
            assert_eq!(secret_type.as_deref(), Some("rotated"));
            assert_eq!(expires_at, Some(1_893_456_000));
        } else {
            panic!("Expected Secret Set with tenant scope");
        }
    }

    #[test]
    fn cli_secret_set_requires_value() {
        // A missing value positional must fail to parse, not silently create
        // an empty secret.
        let args = Cli::try_parse_from(["gyre", "secret", "set", "API_KEY"]);
        assert!(args.is_err());
    }

    #[test]
    fn cli_secret_list_workspace_parses() {
        let args = Cli::try_parse_from(["gyre", "secret", "list", "--workspace-name", "platform"]);
        assert!(args.is_ok());
        if let Commands::Secret {
            command:
                SecretCommands::List {
                    scope,
                    workspace_name,
                    repo,
                },
        } = args.unwrap().command
        {
            assert_eq!(scope, None);
            assert_eq!(workspace_name.as_deref(), Some("platform"));
            assert_eq!(repo, None);
        } else {
            panic!("Expected Secret List");
        }
    }

    #[test]
    fn cli_secret_delete_and_rotate_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "secret",
            "rotate",
            "--scope",
            "workspace",
            "--workspace-name",
            "platform",
            "API_KEY",
            "new-value",
        ]);
        assert!(args.is_ok());
        if let Commands::Secret {
            command:
                SecretCommands::Rotate {
                    name,
                    new_value,
                    scope,
                    workspace_name,
                    expires_at,
                    ..
                },
        } = args.unwrap().command
        {
            assert_eq!(name, "API_KEY");
            assert_eq!(new_value, "new-value");
            assert_eq!(scope.as_deref(), Some("workspace"));
            assert_eq!(workspace_name.as_deref(), Some("platform"));
            assert_eq!(expires_at, None);
        } else {
            panic!("Expected Secret Rotate");
        }

        let args =
            Cli::try_parse_from(["gyre", "secret", "delete", "--scope", "tenant", "API_KEY"]);
        assert!(args.is_ok());
        if let Commands::Secret {
            command: SecretCommands::Delete { name, scope, .. },
        } = args.unwrap().command
        {
            assert_eq!(name, "API_KEY");
            assert_eq!(scope.as_deref(), Some("tenant"));
        } else {
            panic!("Expected Secret Delete");
        }
    }

    #[test]
    fn cli_secret_set_rejects_unknown_scope() {
        let args =
            Cli::try_parse_from(["gyre", "secret", "set", "--scope", "galaxy", "API_KEY", "v"]);
        // Parsing succeeds (the value is a free-form string); the scope is
        // validated at resolution time. This pins that behavior so the error
        // message stays a runtime diagnostic, not a clap panic.
        assert!(args.is_ok());
    }

    #[test]
    fn merge_budget_updates_preserves_unmentioned_limits() {
        // The server PUT replaces the whole config, so `gyre budget set`
        // fetch-merges: provided limits override, others keep current values.
        let current = client::BudgetConfig {
            max_tokens_per_day: Some(100000),
            max_cost_per_day: Some(50.0),
            max_concurrent_agents: Some(5),
            max_agent_lifetime_secs: Some(1800),
        };
        let updates = client::SetBudgetRequest {
            max_tokens_per_day: Some(500000),
            max_cost_per_day: None,
            max_concurrent_agents: None,
            max_agent_lifetime_secs: None,
        };
        let merged = merge_budget_updates(&current, &updates);
        assert_eq!(merged.max_tokens_per_day, Some(500000)); // overridden
        assert_eq!(merged.max_cost_per_day, Some(50.0)); // preserved
        assert_eq!(merged.max_concurrent_agents, Some(5)); // preserved
        assert_eq!(merged.max_agent_lifetime_secs, Some(1800)); // preserved
    }

    // ── `gyre budget set` input-validation regressions (review repair) ─────
    //
    // Non-finite --llm-cost must be rejected before any network I/O:
    // serde_json serializes NaN/Infinity as null, so a PUT carrying it
    // would silently CLEAR the cost limit instead of setting one. The
    // command-path tests below drive run_budget_set (the exact flow the
    // `budget set` handler calls) against a real loopback HTTP server and
    // assert that rejected input produces no requests at all — in
    // particular no budget-mutating PUT.

    const BUDGET_SET_WORKSPACES_JSON: &str =
        r#"[{"id":"ws-123","name":"platform","slug":"platform"}]"#;
    const BUDGET_SET_RESPONSE_JSON: &str = r#"{
        "entity_type": "workspace",
        "entity_id": "ws-123",
        "config": {
            "max_tokens_per_day": 500000,
            "max_cost_per_day": 100.0,
            "max_concurrent_agents": 10,
            "max_agent_lifetime_secs": 3600
        },
        "usage": {
            "entity_type": "workspace",
            "entity_id": "ws-123",
            "tokens_used_today": 123456,
            "cost_today": 12.5,
            "active_agents": 3,
            "period_start": 1790000000
        }
    }"#;

    /// Mock server scripting the full `budget set --workspace-name platform`
    /// request sequence: slug resolution, budget GET (fetch-merge-put),
    /// budget PUT. Captures every request that actually arrives.
    fn spawn_budget_set_server() -> (
        String,
        std::sync::Arc<std::sync::Mutex<Vec<client::test_http::CapturedRequest>>>,
    ) {
        client::test_http::spawn_mock_http_server_sequence(vec![
            ("200 OK".to_string(), BUDGET_SET_WORKSPACES_JSON.to_string()),
            ("200 OK".to_string(), BUDGET_SET_RESPONSE_JSON.to_string()),
            ("200 OK".to_string(), BUDGET_SET_RESPONSE_JSON.to_string()),
        ])
    }

    // ── Bootstrap resume flow tests (task-099 repair) ──────────────────
    //
    // Scripted full-bootstrap runs against a mock HTTP server. These pin
    // the resume semantics the review found broken: the saved-credential
    // fallback must verify the token actually authenticates as the
    // requested admin (wrong-identity continuation), a Pending persona
    // from an interrupted run must be approved (not skipped — spawn 409s
    // on it), and step 7 must not duplicate the gate set on re-run.

    /// Serialize tests that touch $HOME (the resume path reads
    /// ~/.gyre/config via Config::load). A tokio mutex so the guard can be
    /// held across the awaited bootstrap run.
    static HOME_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct HomeRestore(Option<String>);
    impl Drop for HomeRestore {
        fn drop(&mut self) {
            match &self.0 {
                Some(h) => std::env::set_var("HOME", h),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    /// Point $HOME at `dir` until the returned guard drops. Caller must
    /// hold HOME_GUARD.
    fn set_home(dir: &std::path::Path) -> HomeRestore {
        let previous = std::env::var("HOME").ok();
        std::env::set_var("HOME", dir);
        HomeRestore(previous)
    }

    /// An isolated $HOME holding a saved ~/.gyre/config for `server` with
    /// token `token` (what a prior bootstrap or `gyre register` left
    /// behind).
    fn isolated_home(server: &str, token: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".gyre")).unwrap();
        let cfg = config::Config {
            server: server.to_string(),
            token: token.map(|t| t.to_string()),
            agent_id: None,
            agent_name: None,
        };
        std::fs::write(
            dir.path().join(".gyre").join("config"),
            serde_json::to_string(&cfg).unwrap(),
        )
        .unwrap();
        dir
    }

    fn persona_list(slug: &str, status: &str) -> String {
        format!(r#"[{{"id":"p-{slug}","slug":"{slug}","approval_status":"{status}"}}]"#)
    }

    const BOOTSTRAP_SPAWN_JSON: &str = r#"{
        "agent": {"id":"a-1","name":"main-orchestrator","status":"active"},
        "token": "jwt",
        "launch_status": "running"
    }"#;

    fn bootstrap_args(server: String, repo_path: Option<String>) -> BootstrapArgs {
        BootstrapArgs {
            server,
            token: "boot-tok".to_string(),
            tenant: Some("Acme Corp".to_string()),
            workspace: None,
            repo: None,
            repo_path,
            admin_user: Some("jsell".to_string()),
            oidc_issuer: None,
            dev: false,
            starter_kit: false,
        }
    }

    #[tokio::test]
    async fn bootstrap_resume_approves_pending_persona_and_skips_existing_gates() {
        let _guard = HOME_GUARD.lock().await;

        let repo_dir = tempfile::tempdir().unwrap();
        std::fs::write(repo_dir.path().join("Cargo.toml"), "[package]\n").unwrap();

        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![
            (
                "200 OK".to_string(),
                r#"{"status":"ok","version":"t"}"#.to_string(),
            ),
            (
                "201 Created".to_string(),
                r#"{"id":"t-1","name":"Acme Corp","slug":"acme-corp"}"#.to_string(),
            ),
            // create_user -> 409 duplicate: the resume path begins.
            (
                "409 Conflict".to_string(),
                "user with external id local:jsell already exists (id u-1)".to_string(),
            ),
            // whoami with the saved token: verifies it IS jsell of t-1.
            (
                "200 OK".to_string(),
                r#"{"username":"jsell","tenant_id":"t-1"}"#.to_string(),
            ),
            (
                "201 Created".to_string(),
                r#"{"id":"ws-1","name":"default","slug":"default"}"#.to_string(),
            ),
            (
                "201 Created".to_string(),
                r#"{"id":"r-1","name":"main"}"#.to_string(),
            ),
            // Persona resume: approved, PENDING (approve follows), approved, approved.
            (
                "200 OK".to_string(),
                persona_list("workspace-orchestrator", "Approved"),
            ),
            (
                "200 OK".to_string(),
                persona_list("repo-orchestrator", "Pending"),
            ),
            ("200 OK".to_string(), "{}".to_string()), // approve
            (
                "200 OK".to_string(),
                persona_list("accountability", "Approved"),
            ),
            ("200 OK".to_string(), persona_list("security", "Approved")),
            // Gate resume: cargo-test already configured -> only clippy is created.
            (
                "200 OK".to_string(),
                r#"[{"name":"cargo-test"}]"#.to_string(),
            ),
            (
                "201 Created".to_string(),
                r#"{"name":"cargo-clippy"}"#.to_string(),
            ),
            ("200 OK".to_string(), BOOTSTRAP_SPAWN_JSON.to_string()),
        ]);

        let home = isolated_home(&base, Some("saved-tok"));
        let _home_reset = set_home(home.path());

        let result = run_bootstrap(bootstrap_args(
            base.clone(),
            Some(repo_dir.path().to_string_lossy().to_string()),
        ))
        .await;
        let reqs = captured.lock().unwrap().clone();
        drop(_home_reset);

        result.expect("verified resume bootstrap must succeed");
        assert_eq!(
            reqs.len(),
            14,
            "unexpected request count: {:?}",
            reqs.iter()
                .map(|r| r.request_line.clone())
                .collect::<Vec<_>>()
        );
        // The saved credential is verified before adoption, then used for
        // every subsequent call.
        assert_eq!(reqs[3].request_line, "GET /api/v1/users/me HTTP/1.1");
        assert!(reqs[3]
            .headers
            .to_ascii_lowercase()
            .contains("authorization: bearer saved-tok"));
        assert!(reqs[4]
            .headers
            .to_ascii_lowercase()
            .contains("authorization: bearer saved-tok"));
        // A Pending persona from an interrupted run is approved, not skipped.
        assert_eq!(
            reqs[8].request_line, "POST /api/v1/personas/p-repo-orchestrator/approve HTTP/1.1",
            "Pending persona must be approved on resume"
        );
        // Gates: exactly one create (the missing one), for the right name.
        let gate_posts: Vec<&client::test_http::CapturedRequest> = reqs
            .iter()
            .filter(|r| r.request_line == "POST /api/v1/repos/r-1/gates HTTP/1.1")
            .collect();
        assert_eq!(
            gate_posts.len(),
            1,
            "re-run must not duplicate the gate set"
        );
        assert!(
            gate_posts[0].body.contains("cargo-clippy")
                && !gate_posts[0].body.contains("cargo-test"),
            "only the unconfigured gate is created; body: {}",
            gate_posts[0].body
        );
        assert_eq!(
            reqs[13].request_line,
            "POST /api/v1/repos/r-1/orchestrator/spawn HTTP/1.1"
        );
    }

    #[tokio::test]
    async fn bootstrap_resume_rejects_saved_token_for_a_different_user() {
        let _guard = HOME_GUARD.lock().await;

        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![
            (
                "200 OK".to_string(),
                r#"{"status":"ok","version":"t"}"#.to_string(),
            ),
            (
                "201 Created".to_string(),
                r#"{"id":"t-1","name":"Acme Corp","slug":"acme-corp"}"#.to_string(),
            ),
            (
                "409 Conflict".to_string(),
                "user with external id local:jsell already exists (id u-1)".to_string(),
            ),
            // The saved token belongs to a DIFFERENT user.
            (
                "200 OK".to_string(),
                r#"{"username":"someone-else","tenant_id":"t-1"}"#.to_string(),
            ),
        ]);

        let home = isolated_home(&base, Some("saved-tok"));
        let _home_reset = set_home(home.path());

        let err = run_bootstrap(bootstrap_args(base, None))
            .await
            .expect_err("wrong-identity saved credential must fail the run");
        let reqs = captured.lock().unwrap().clone();
        drop(_home_reset);

        let msg = format!("{err:#}");
        assert!(
            msg.contains("someone-else") && msg.contains("jsell"),
            "error must name the mismatch: {msg}"
        );
        assert_eq!(
            reqs.len(),
            4,
            "no request may follow the failed identity verification: {:?}",
            reqs.iter()
                .map(|r| r.request_line.clone())
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn bootstrap_resume_rejects_admin_bound_to_a_different_tenant() {
        let _guard = HOME_GUARD.lock().await;

        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![
            (
                "200 OK".to_string(),
                r#"{"status":"ok","version":"t"}"#.to_string(),
            ),
            (
                "201 Created".to_string(),
                r#"{"id":"t-1","name":"Acme Corp","slug":"acme-corp"}"#.to_string(),
            ),
            (
                "409 Conflict".to_string(),
                "user with external id local:jsell already exists (id u-1)".to_string(),
            ),
            // Same username, but bound to another tenant: continuing would
            // let it administer a tenant it was never scoped to.
            (
                "200 OK".to_string(),
                r#"{"username":"jsell","tenant_id":"t-other"}"#.to_string(),
            ),
        ]);

        let home = isolated_home(&base, Some("saved-tok"));
        let _home_reset = set_home(home.path());

        let err = run_bootstrap(bootstrap_args(base, None))
            .await
            .expect_err("cross-tenant saved credential must fail the run");
        let reqs = captured.lock().unwrap().clone();
        drop(_home_reset);

        let msg = format!("{err:#}");
        assert!(
            msg.contains("tenant"),
            "error must name the tenant mismatch: {msg}"
        );
        assert_eq!(
            reqs.len(),
            4,
            "no request may follow the failed tenant check"
        );
    }

    #[tokio::test]
    async fn bootstrap_create_user_403_fails_without_adopting_saved_credentials() {
        let _guard = HOME_GUARD.lock().await;

        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![
            (
                "200 OK".to_string(),
                r#"{"status":"ok","version":"t"}"#.to_string(),
            ),
            (
                "201 Created".to_string(),
                r#"{"id":"t-1","name":"Acme Corp","slug":"acme-corp"}"#.to_string(),
            ),
            // 403 is NOT "already exists": a non-admin bootstrap token
            // must fail the run, never adopt the saved credential.
            (
                "403 Forbidden".to_string(),
                "only Admin role may create users".to_string(),
            ),
        ]);

        let home = isolated_home(&base, Some("saved-tok"));
        let _home_reset = set_home(home.path());

        let err = run_bootstrap(bootstrap_args(base, None))
            .await
            .expect_err("non-duplicate create_user error must fail the run");
        let reqs = captured.lock().unwrap().clone();
        drop(_home_reset);

        let msg = format!("{err:#}");
        assert!(
            msg.contains("create admin user") && msg.contains("only Admin role"),
            "error should name the failed step and server detail: {msg}"
        );
        assert_eq!(
            reqs.len(),
            3,
            "the saved credential must never be consulted for a non-duplicate error: {:?}",
            reqs.iter()
                .map(|r| r.request_line.clone())
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn budget_set_rejects_nonfinite_llm_cost_before_any_request() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let (base, captured) = spawn_budget_set_server();
            let api = client::GyreClient::new(base, "tok".to_string());
            let err = run_budget_set(&api, Some("platform"), None, None, Some(bad), None, None)
                .await
                .expect_err("non-finite --llm-cost must be rejected");
            let msg = format!("{err:#}");
            assert!(
                msg.contains("--llm-cost") && msg.contains("finite"),
                "error should name the flag and finiteness: {msg}"
            );
            // No request may reach the server — rejected input must never
            // produce a PUT (or even the workspace-resolution GET).
            assert!(
                captured.lock().unwrap().is_empty(),
                "non-finite --llm-cost produced HTTP requests"
            );
        }
    }

    #[tokio::test]
    async fn budget_set_rejects_negative_llm_cost_before_any_request() {
        // A negative cost cap can never be exceeded by non-negative usage
        // and is rejected before any network I/O. Zero is NOT rejected here:
        // a finite zero cap is a valid way to disable spending (see
        // budget_set_zero_cost_disables_spending_and_preserves_limits).
        for bad in [-5.0] {
            let (base, captured) = spawn_budget_set_server();
            let api = client::GyreClient::new(base, "tok".to_string());
            let err = run_budget_set(&api, Some("platform"), None, None, Some(bad), None, None)
                .await
                .expect_err("negative --llm-cost must be rejected");
            let msg = format!("{err:#}");
            assert!(
                msg.contains("--llm-cost") && msg.contains("negative"),
                "error should name the flag and negativity: {msg}"
            );
            assert!(
                captured.lock().unwrap().is_empty(),
                "negative --llm-cost produced HTTP requests"
            );
        }
    }

    #[tokio::test]
    async fn budget_set_valid_cost_puts_merged_body_and_preserves_limits() {
        let (base, captured) = spawn_budget_set_server();
        let api = client::GyreClient::new(base, "tok".to_string());
        let updated = run_budget_set(&api, Some("platform"), None, None, Some(75.0), None, None)
            .await
            .expect("finite --llm-cost must be accepted");
        assert_eq!(updated.entity_id, "ws-123");

        let reqs = captured.lock().unwrap();
        assert_eq!(
            reqs.len(),
            3,
            "expected slug-resolve GET, budget GET, budget PUT"
        );
        assert_eq!(
            reqs[0].request_line,
            "GET /api/v1/workspaces?slug=platform HTTP/1.1"
        );
        assert_eq!(
            reqs[1].request_line,
            "GET /api/v1/workspaces/ws-123/budget HTTP/1.1"
        );
        assert_eq!(
            reqs[2].request_line,
            "PUT /api/v1/workspaces/ws-123/budget HTTP/1.1"
        );
        // Fetch-merge-put: the new cost is sent, every unmentioned limit
        // keeps the value from the GET (server replaces the whole config).
        assert_eq!(
            reqs[2].body,
            "{\"max_tokens_per_day\":500000,\"max_cost_per_day\":75.0,\"max_concurrent_agents\":10,\"max_agent_lifetime_secs\":3600}"
        );
    }

    #[tokio::test]
    async fn budget_set_zero_cost_disables_spending_and_preserves_limits() {
        // A finite ZERO cost cap is valid (review repair 02b87ba4): it
        // explicitly disables all spending for the day. The command must
        // PUT max_cost_per_day=0 — not reject the input, and not let
        // Option/or-merge turn Some(0.0) into the previous value or None —
        // while every unmentioned limit keeps its GET value.
        let (base, captured) = spawn_budget_set_server();
        let api = client::GyreClient::new(base, "tok".to_string());
        let updated = run_budget_set(&api, Some("platform"), None, None, Some(0.0), None, None)
            .await
            .expect("finite zero --llm-cost must be accepted");
        assert_eq!(updated.entity_id, "ws-123");

        let reqs = captured.lock().unwrap();
        assert_eq!(
            reqs.len(),
            3,
            "expected slug-resolve GET, budget GET, budget PUT"
        );
        assert_eq!(
            reqs[2].request_line,
            "PUT /api/v1/workspaces/ws-123/budget HTTP/1.1"
        );
        // The zero cap must be serialized as 0.0 (a null here would
        // silently clear the limit instead of disabling spending), and the
        // other limits must be preserved from the fetch-merge-put GET.
        assert_eq!(
            reqs[2].body,
            "{\"max_tokens_per_day\":500000,\"max_cost_per_day\":0.0,\"max_concurrent_agents\":10,\"max_agent_lifetime_secs\":3600}"
        );
    }

    // ── zero-cap display regressions (review repair) ─────────────────────
    //
    // A zero cap has no utilization ratio (0/0); it must not render as
    // NaN%. A zero cap blocks all usage, so usage above it is over cap.

    #[test]
    fn budget_usage_line_zero_limit_is_bounded_not_nan() {
        let line = budget_usage_line(0, Some(0));
        assert!(!line.contains("NaN"), "zero cap rendered NaN: {line}");
        assert!(
            line.contains("zero cap"),
            "expected explicit zero-cap display: {line}"
        );
        let over = budget_usage_line(5, Some(0));
        assert!(!over.contains("NaN"), "zero cap rendered NaN: {over}");
        assert!(
            over.contains("over cap"),
            "usage above zero cap must say over cap: {over}"
        );
        // Non-zero paths are unchanged.
        assert_eq!(
            budget_usage_line(123456, Some(500000)),
            "123456 / 500000 (24.7%)"
        );
        assert_eq!(budget_usage_line(7, None), "7 / none");
    }

    #[test]
    fn budget_cost_line_zero_limit_is_bounded_not_nan() {
        let line = budget_cost_line(0.0, Some(0.0));
        assert!(!line.contains("NaN"), "zero cap rendered NaN: {line}");
        assert!(
            line.contains("zero cap"),
            "expected explicit zero-cap display: {line}"
        );
        let over = budget_cost_line(1.5, Some(0.0));
        assert!(!over.contains("NaN"), "zero cap rendered NaN: {over}");
        assert!(
            over.contains("over cap"),
            "usage above zero cap must say over cap: {over}"
        );
        // Non-zero paths are unchanged.
        assert_eq!(
            budget_cost_line(12.5, Some(100.0)),
            "$12.50 / $100.00 (12.5%)"
        );
        assert_eq!(budget_cost_line(3.0, None), "$3.00 / none");
    }

    #[test]
    fn dot_output_produces_valid_syntax() {
        let graph = serde_json::json!({
            "nodes": [
                {"repo_id": "repo-a", "name": "service-a"},
                {"repo_id": "repo-b", "name": "service-b"},
            ],
            "edges": [
                {"source": "repo-a", "target": "repo-b", "type": "code", "status": "active"},
            ]
        });

        let mut buf = Vec::new();
        write_dot_graph(&graph, &mut buf).unwrap();
        let dot = String::from_utf8(buf).unwrap();

        assert!(dot.starts_with("digraph dependencies {"));
        assert!(dot.trim_end().ends_with('}'));
        assert!(dot.contains("\"repo-a\" -> \"repo-b\""));
        assert!(dot.contains("color=blue"));
        assert!(dot.contains("label=\"service-a\""));
        assert!(dot.contains("label=\"service-b\""));
        assert!(dot.contains("rankdir=LR;"));
        assert!(dot.contains("node [shape=box, style=filled, fillcolor=lightblue];"));
    }

    #[test]
    fn dot_output_edge_colors() {
        let graph = serde_json::json!({
            "nodes": [
                {"repo_id": "a", "name": "a"},
                {"repo_id": "b", "name": "b"},
            ],
            "edges": [
                {"source": "a", "target": "b", "type": "code", "status": "active"},
                {"source": "a", "target": "b", "type": "spec", "status": "active"},
                {"source": "a", "target": "b", "type": "api", "status": "active"},
                {"source": "a", "target": "b", "type": "schema", "status": "active"},
                {"source": "a", "target": "b", "type": "manual", "status": "active"},
            ]
        });

        let mut buf = Vec::new();
        write_dot_graph(&graph, &mut buf).unwrap();
        let dot = String::from_utf8(buf).unwrap();

        assert!(dot.contains("color=blue"), "code type should be blue");
        assert!(dot.contains("color=green"), "spec type should be green");
        assert!(dot.contains("color=orange"), "api type should be orange");
        assert!(dot.contains("color=purple"), "schema type should be purple");
        assert!(dot.contains("color=gray"), "manual type should be gray");
    }

    // ── Helper function tests ───────────────────────────────────────────────

    #[test]
    fn parse_priority_range_none() {
        let (min, max) = parse_priority_range(None).unwrap();
        assert!(min.is_none());
        assert!(max.is_none());
    }

    #[test]
    fn parse_priority_range_valid() {
        let (min, max) = parse_priority_range(Some("1-5")).unwrap();
        assert_eq!(min, Some(1));
        assert_eq!(max, Some(5));
    }

    #[test]
    fn parse_priority_range_invalid_format() {
        assert!(parse_priority_range(Some("invalid")).is_err());
    }

    #[test]
    fn parse_priority_range_invalid_number() {
        assert!(parse_priority_range(Some("abc-5")).is_err());
    }

    #[test]
    fn format_age_zero_is_just_now() {
        assert_eq!(format_age(0), "just now");
    }

    #[test]
    fn format_age_future_is_just_now() {
        assert_eq!(format_age(u64::MAX), "just now");
    }

    #[test]
    fn format_timestamp_epoch() {
        assert_eq!(format_timestamp(0), "1970-01-01 00:00:00Z");
    }

    #[test]
    fn format_timestamp_known_date() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        assert_eq!(format_timestamp(1704067200), "2024-01-01 00:00:00Z");
    }

    #[test]
    fn days_to_ymd_epoch() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2024-01-01 is day 19723 since epoch
        assert_eq!(days_to_ymd(19723), (2024, 1, 1));
    }

    // ── Spec link CLI tests ──────────────────────────────────────────────────

    #[test]
    fn cli_spec_links_parses() {
        let args = Cli::try_parse_from(["gyre", "spec", "links", "system/identity-security.md"]);
        assert!(args.is_ok());
        if let Commands::Spec {
            command: SpecCommands::Links { path },
        } = args.unwrap().command
        {
            assert_eq!(path, "system/identity-security.md");
        } else {
            panic!("Expected Spec Links");
        }
    }

    #[test]
    fn cli_spec_dependents_parses() {
        let args = Cli::try_parse_from(["gyre", "spec", "dependents", "system/source-control.md"]);
        assert!(args.is_ok());
        if let Commands::Spec {
            command: SpecCommands::Dependents { path },
        } = args.unwrap().command
        {
            assert_eq!(path, "system/source-control.md");
        } else {
            panic!("Expected Spec Dependents");
        }
    }

    #[test]
    fn cli_spec_graph_text_default() {
        let args = Cli::try_parse_from(["gyre", "spec", "graph"]);
        assert!(args.is_ok());
        if let Commands::Spec {
            command: SpecCommands::Graph { format },
        } = args.unwrap().command
        {
            assert_eq!(format, None);
        } else {
            panic!("Expected Spec Graph");
        }
    }

    #[test]
    fn cli_spec_graph_dot_format() {
        let args = Cli::try_parse_from(["gyre", "spec", "graph", "--format", "dot"]);
        assert!(args.is_ok());
        if let Commands::Spec {
            command: SpecCommands::Graph { format },
        } = args.unwrap().command
        {
            assert_eq!(format.as_deref(), Some("dot"));
        } else {
            panic!("Expected Spec Graph with dot format");
        }
    }

    #[test]
    fn cli_spec_stale_links_parses() {
        let args = Cli::try_parse_from(["gyre", "spec", "stale-links"]);
        assert!(args.is_ok());
        assert!(matches!(
            args.unwrap().command,
            Commands::Spec {
                command: SpecCommands::StaleLinks
            }
        ));
    }

    #[test]
    fn cli_spec_conflicts_parses() {
        let args = Cli::try_parse_from(["gyre", "spec", "conflicts"]);
        assert!(args.is_ok());
        assert!(matches!(
            args.unwrap().command,
            Commands::Spec {
                command: SpecCommands::Conflicts
            }
        ));
    }

    #[test]
    fn write_spec_dot_graph_basic() {
        let graph = serde_json::json!({
            "nodes": [
                {"path": "system/auth.md", "title": "Authentication", "approval_status": "approved"},
                {"path": "system/api.md", "title": "API Layer", "approval_status": "pending"}
            ],
            "edges": [
                {"source": "system/api.md", "target": "system/auth.md", "link_type": "depends_on", "status": "active"},
                {"source": "system/api.md", "target": "system/auth.md", "link_type": "references", "status": "stale"}
            ]
        });

        let mut buf = Vec::new();
        write_spec_dot_graph(&graph, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("digraph specs {"));
        assert!(output.contains("rankdir=LR;"));
        // Nodes with approval-based coloring
        assert!(output.contains("system/auth.md"));
        assert!(output.contains("Authentication"));
        assert!(output.contains("fillcolor=palegreen"));
        assert!(output.contains("fillcolor=lightyellow"));
        // Edges with type-based coloring
        assert!(output.contains("color=green"));
        assert!(output.contains("style=solid"));
        assert!(output.contains("label=\"depends_on\""));
        // Stale link highlighted in gold
        assert!(output.contains("color=gold"));
        assert!(output.contains("style=dotted"));
        assert!(output.contains("label=\"references\""));
        assert!(output.ends_with("}\n"));
    }

    #[test]
    fn write_spec_dot_graph_empty() {
        let graph = serde_json::json!({"nodes": [], "edges": []});
        let mut buf = Vec::new();
        write_spec_dot_graph(&graph, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("digraph specs {"));
        assert!(output.ends_with("}\n"));
    }

    #[test]
    fn write_spec_dot_graph_all_link_types() {
        let graph = serde_json::json!({
            "nodes": [
                {"path": "a.md", "title": "A", "approval_status": "approved"},
                {"path": "b.md", "title": "B", "approval_status": "rejected"},
                {"path": "c.md", "title": "C", "approval_status": "other"}
            ],
            "edges": [
                {"source": "a.md", "target": "b.md", "link_type": "implements", "status": "active"},
                {"source": "a.md", "target": "b.md", "link_type": "depends_on", "status": "active"},
                {"source": "a.md", "target": "b.md", "link_type": "supersedes", "status": "active"},
                {"source": "a.md", "target": "b.md", "link_type": "conflicts_with", "status": "active"},
                {"source": "a.md", "target": "b.md", "link_type": "extends", "status": "active"},
                {"source": "a.md", "target": "b.md", "link_type": "references", "status": "active"}
            ]
        });

        let mut buf = Vec::new();
        write_spec_dot_graph(&graph, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        // Node colors
        assert!(output.contains("fillcolor=palegreen"));
        assert!(output.contains("fillcolor=lightcoral"));
        assert!(output.contains("fillcolor=lightblue"));

        // Edge colors per link type
        assert!(output.contains("color=blue")); // implements
        assert!(output.contains("color=green")); // depends_on
        assert!(output.contains("color=gray")); // supersedes
        assert!(output.contains("color=red")); // conflicts_with
        assert!(output.contains("color=orange")); // extends
                                                  // references: gray + dotted
        let has_dotted = output
            .lines()
            .any(|l| l.contains("references") && l.contains("dotted"));
        assert!(has_dotted, "references edge should use dotted style");
    }

    #[test]
    fn write_spec_dot_graph_escapes_quotes() {
        let graph = serde_json::json!({
            "nodes": [
                {"path": "a\"b.md", "title": "Test \"quotes\"", "approval_status": "pending"}
            ],
            "edges": []
        });

        let mut buf = Vec::new();
        write_spec_dot_graph(&graph, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains(r#"a\"b.md"#));
        assert!(output.contains(r#"Test \"quotes\""#));
    }

    // ── User management CLI (user-management.md §CLI, task-126) ──────────

    // Parse tests: every command line in the spec's §CLI block must parse
    // into the intended command shape.

    #[test]
    fn cli_whoami_parses() {
        let args = Cli::try_parse_from(["gyre", "whoami"]);
        assert!(args.is_ok());
        assert!(matches!(args.unwrap().command, Commands::Whoami));
    }

    #[test]
    fn cli_profile_set_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "profile",
            "set",
            "--display-name",
            "Jordan Sell",
            "--timezone",
            "America/New_York",
        ]);
        assert!(args.is_ok());
        if let Commands::Profile {
            command:
                ProfileCommands::Set {
                    display_name,
                    timezone,
                    locale,
                    avatar_url,
                },
        } = args.unwrap().command
        {
            assert_eq!(display_name.as_deref(), Some("Jordan Sell"));
            assert_eq!(timezone.as_deref(), Some("America/New_York"));
            assert_eq!(locale, None);
            assert_eq!(avatar_url, None);
        } else {
            panic!("Expected Profile Set");
        }
    }

    #[test]
    fn cli_sessions_subcommands_parse() {
        assert!(matches!(
            Cli::try_parse_from(["gyre", "sessions", "list"])
                .unwrap()
                .command,
            Commands::Sessions {
                command: SessionsCommands::List
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["gyre", "sessions", "revoke", "s-1"]).unwrap().command,
            Commands::Sessions { command: SessionsCommands::Revoke { id } } if id == "s-1"
        ));
        assert!(matches!(
            Cli::try_parse_from(["gyre", "sessions", "revoke-all"])
                .unwrap()
                .command,
            Commands::Sessions {
                command: SessionsCommands::RevokeAll
            }
        ));
    }

    #[test]
    fn cli_tenant_invite_parses() {
        // Bare email
        let args = Cli::try_parse_from(["gyre", "tenant", "invite", "alice@example.com"]);
        assert!(matches!(
            args.unwrap().command,
            Commands::Tenant { command: TenantCommands::Invite { ref email, workspace: None, role: None, bulk: None } }
                if email.as_deref() == Some("alice@example.com")
        ));
        // With workspace pre-assignment + role
        let args = Cli::try_parse_from([
            "gyre",
            "tenant",
            "invite",
            "alice@example.com",
            "--workspace",
            "platform",
            "--role",
            "Developer",
        ]);
        assert!(matches!(
            args.unwrap().command,
            Commands::Tenant { command: TenantCommands::Invite { ref email, ref workspace, ref role, bulk: None } }
                if email.as_deref() == Some("alice@example.com")
                    && workspace.as_deref() == Some("platform")
                    && role.as_deref() == Some("Developer")
        ));
        // Bulk from CSV
        let args = Cli::try_parse_from(["gyre", "tenant", "invite", "--bulk", "users.csv"]);
        assert!(matches!(
            args.unwrap().command,
            Commands::Tenant { command: TenantCommands::Invite { email: None, bulk: Some(ref path), .. } }
                if path == &std::path::PathBuf::from("users.csv")
        ));
    }

    #[test]
    fn cli_tenant_invitations_subcommands_parse() {
        assert!(matches!(
            Cli::try_parse_from(["gyre", "tenant", "invitations", "list"])
                .unwrap()
                .command,
            Commands::Tenant {
                command: TenantCommands::Invitations {
                    command: TenantInvitationCommands::List
                }
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["gyre", "tenant", "invitations", "revoke", "inv-1"]).unwrap().command,
            Commands::Tenant { command: TenantCommands::Invitations { command: TenantInvitationCommands::Revoke { id } } } if id == "inv-1"
        ));
    }

    #[test]
    fn cli_workspace_subcommands_parse() {
        assert!(matches!(
            Cli::try_parse_from(["gyre", "workspace", "members", "list"])
                .unwrap()
                .command,
            Commands::Workspace {
                command: WorkspaceCommands::Members {
                    command: MemberCommands::List { workspace: None }
                }
            }
        ));
        // Invite by handle and by email, with a role
        assert!(matches!(
            Cli::try_parse_from([
                "gyre", "workspace", "invite", "@alice", "--role", "Developer",
            ])
            .unwrap()
            .command,
            Commands::Workspace { command: WorkspaceCommands::Invite { ref user, ref role, workspace: None } }
                if user == "@alice" && role.as_deref() == Some("Developer")
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "gyre", "workspace", "invite", "alice@example.com", "--role", "Developer",
            ])
            .unwrap()
            .command,
            Commands::Workspace { command: WorkspaceCommands::Invite { ref user, .. } } if user == "alice@example.com"
        ));
        // set-role, with and without --transfer
        assert!(matches!(
            Cli::try_parse_from(["gyre", "workspace", "members", "set-role", "@asmith", "Admin"])
                .unwrap()
                .command,
            Commands::Workspace { command: WorkspaceCommands::Members { command: MemberCommands::SetRole { ref user, ref role, transfer: false, .. } } }
                if user == "@asmith" && role == "Admin"
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "gyre", "workspace", "members", "set-role", "@asmith", "Owner", "--transfer",
            ])
            .unwrap()
            .command,
            Commands::Workspace { command: WorkspaceCommands::Members { command: MemberCommands::SetRole { ref user, ref role, transfer: true, .. } } }
                if user == "@asmith" && role == "Owner"
        ));
        assert!(matches!(
            Cli::try_parse_from(["gyre", "workspace", "members", "remove", "@asmith"]).unwrap().command,
            Commands::Workspace { command: WorkspaceCommands::Members { command: MemberCommands::Remove { ref user, .. } } } if user == "@asmith"
        ));
        assert!(matches!(
            Cli::try_parse_from(["gyre", "workspace", "invitations", "list"])
                .unwrap()
                .command,
            Commands::Workspace {
                command: WorkspaceCommands::Invitations {
                    command: WorkspaceInvitationCommands::List { workspace: None }
                }
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["gyre", "workspace", "invitations", "revoke", "inv-9"]).unwrap().command,
            Commands::Workspace { command: WorkspaceCommands::Invitations { command: WorkspaceInvitationCommands::Revoke { ref id, .. } } } if id == "inv-9"
        ));
    }

    #[test]
    fn cli_team_subcommands_parse() {
        assert!(matches!(
            Cli::try_parse_from(["gyre", "team", "create", "Platform Team", "--lead", "@jsell"])
                .unwrap()
                .command,
            Commands::Team { command: TeamCommands::Create { ref name, ref lead, .. } }
                if name == "Platform Team" && lead.as_deref() == Some("@jsell")
        ));
        assert!(matches!(
            Cli::try_parse_from(["gyre", "team", "add", "@asmith", "--team", "Platform Team"])
                .unwrap()
                .command,
            Commands::Team { command: TeamCommands::Add { ref user, ref team, .. } }
                if user == "@asmith" && team == "Platform Team"
        ));
        assert!(matches!(
            Cli::try_parse_from(["gyre", "team", "list"])
                .unwrap()
                .command,
            Commands::Team {
                command: TeamCommands::List { workspace: None }
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["gyre", "team", "remove", "@asmith", "--team", "Platform Team"])
                .unwrap()
                .command,
            Commands::Team { command: TeamCommands::Remove { ref user, ref team, .. } }
                if user == "@asmith" && team == "Platform Team"
        ));
    }

    #[test]
    fn cli_notifications_subcommands_parse() {
        assert!(matches!(
            Cli::try_parse_from(["gyre", "notifications", "list"])
                .unwrap()
                .command,
            Commands::Notifications {
                command: NotificationCommands::List {
                    unread: false,
                    priority: None,
                    r#type: None,
                    workspace: None,
                    limit: 50
                }
            }
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "gyre", "notifications", "list", "--unread", "--priority", "high",
            ])
            .unwrap()
            .command,
            Commands::Notifications { command: NotificationCommands::List { unread: true, ref priority, .. } }
                if priority.as_deref() == Some("high")
        ));
        let args = Cli::try_parse_from([
            "gyre",
            "notifications",
            "preferences",
            "set",
            "--email-digest",
            "hourly",
            "--email-min-priority",
            "high",
        ]);
        assert!(matches!(
            args.unwrap().command,
            Commands::Notifications { command: NotificationCommands::Preferences { command: NotificationPreferencesCommands::Set { ref email_digest, ref email_min_priority, slack_url: None, .. } } }
                if email_digest.as_deref() == Some("hourly")
                    && email_min_priority.as_deref() == Some("high")
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "gyre", "notifications", "preferences", "set", "--slack-url",
                "https://hooks.slack.com/services/T000/B000/XYZ",
            ])
            .unwrap()
            .command,
            Commands::Notifications { command: NotificationCommands::Preferences { command: NotificationPreferencesCommands::Set { ref slack_url, email_digest: None, .. } } }
                if slack_url.as_deref() == Some("https://hooks.slack.com/services/T000/B000/XYZ")
        ));
    }

    // Pure-logic tests.

    #[test]
    fn looks_like_email_distinguishes_handles_and_addresses() {
        assert!(looks_like_email("alice@example.com"));
        assert!(!looks_like_email("@alice"));
        assert!(!looks_like_email("alice"));
        assert!(!looks_like_email("@alice@example.com")); // handle marker wins
    }

    #[test]
    fn normalize_digest_accepts_case_insensitive_names() {
        assert_eq!(normalize_digest("hourly").unwrap(), "Hourly");
        assert_eq!(normalize_digest("HOURLY").unwrap(), "Hourly");
        assert_eq!(normalize_digest("immediate").unwrap(), "Immediate");
        assert_eq!(normalize_digest("daily").unwrap(), "Daily");
        assert_eq!(normalize_digest("off").unwrap(), "Off");
        assert!(normalize_digest("weekly").is_err());
    }

    #[test]
    fn normalize_priority_level_accepts_case_insensitive_names() {
        assert_eq!(normalize_priority_level("high").unwrap(), "High");
        assert_eq!(normalize_priority_level("URGENT").unwrap(), "Urgent");
        assert_eq!(normalize_priority_level("low").unwrap(), "Low");
        assert_eq!(normalize_priority_level("medium").unwrap(), "Medium");
        assert!(normalize_priority_level("critical").is_err());
    }

    #[test]
    fn validate_workspace_role_matches_server_role_set() {
        for role in ["Owner", "Admin", "Developer", "Viewer"] {
            assert!(
                validate_workspace_role(role).is_ok(),
                "{role} should be valid"
            );
            assert!(validate_workspace_role(&role.to_uppercase()).is_ok());
        }
        assert!(validate_workspace_role("SuperAdmin").is_err());
    }

    #[test]
    fn parse_bulk_csv_without_header() {
        let rows =
            parse_bulk_invitation_csv("alice@example.com\nbob@example.com,platform,Admin\n\n")
                .unwrap();
        assert_eq!(
            rows,
            vec![
                BulkInviteRow {
                    email: "alice@example.com".into(),
                    workspace: None,
                    role: None
                },
                BulkInviteRow {
                    email: "bob@example.com".into(),
                    workspace: Some("platform".into()),
                    role: Some("Admin".into()),
                },
            ]
        );
    }

    #[test]
    fn parse_bulk_csv_with_header_and_quotes() {
        let rows = parse_bulk_invitation_csv(
            "email,workspace,role\n\"carol@example.com\", \"platform\" ,Developer\n",
        )
        .unwrap();
        assert_eq!(
            rows,
            vec![BulkInviteRow {
                email: "carol@example.com".into(),
                workspace: Some("platform".into()),
                role: Some("Developer".into()),
            }]
        );
    }

    #[test]
    fn parse_bulk_csv_rejects_bad_rows() {
        // Row without an email
        let err = parse_bulk_invitation_csv(",platform,Developer").unwrap_err();
        assert!(err.to_string().contains("no email"));
        // Email column is not an address
        let err = parse_bulk_invitation_csv("not-an-address").unwrap_err();
        assert!(err.to_string().contains("not an email"));
        // No rows at all
        let err = parse_bulk_invitation_csv("email,workspace,role\n").unwrap_err();
        assert!(err.to_string().contains("no invitation rows"));
    }

    #[test]
    fn merge_channel_updates_enables_email_and_normalizes() {
        let current = serde_json::json!({
            "in_app": true,
            "email": { "enabled": false, "digest": "Off", "min_priority": "Low" }
        });
        let merged = merge_channel_updates(
            Some(&current),
            &ChannelPrefsUpdate {
                email_digest: Some("hourly"),
                email_min_priority: Some("high"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(merged["email"]["enabled"], serde_json::json!(true));
        assert_eq!(merged["email"]["digest"], serde_json::json!("Hourly"));
        assert_eq!(merged["email"]["min_priority"], serde_json::json!("High"));
        assert_eq!(merged["in_app"], serde_json::json!(true));
    }

    #[test]
    fn merge_channel_updates_preserves_untouched_channels() {
        // Setting only email flags must not drop a configured Slack channel
        // (read-merge-write, not replace).
        let current = serde_json::json!({
            "in_app": true,
            "email": { "enabled": false, "digest": "Off", "min_priority": "Medium" },
            "slack": { "webhook_url": "https://hooks.slack.com/x", "channel": null, "min_priority": "Low" }
        });
        let merged = merge_channel_updates(
            Some(&current),
            &ChannelPrefsUpdate {
                email_digest: Some("daily"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            merged["slack"]["webhook_url"],
            serde_json::json!("https://hooks.slack.com/x")
        );
        assert_eq!(merged["email"]["digest"], serde_json::json!("Daily"));
    }

    #[test]
    fn merge_channel_updates_configures_slack_from_scratch() {
        let merged = merge_channel_updates(
            None,
            &ChannelPrefsUpdate {
                slack_url: Some("https://hooks.slack.com/services/T/B/X"),
                slack_min_priority: Some("high"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            merged["slack"]["webhook_url"],
            serde_json::json!("https://hooks.slack.com/services/T/B/X")
        );
        assert_eq!(merged["slack"]["min_priority"], serde_json::json!("High"));
        // Default email shape is present and untouched.
        assert_eq!(merged["email"]["enabled"], serde_json::json!(false));
    }

    #[test]
    fn merge_channel_updates_rejects_invalid_values_before_any_request() {
        let err = merge_channel_updates(
            None,
            &ChannelPrefsUpdate {
                email_digest: Some("weekly"),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("email digest"));
        let err = merge_channel_updates(
            None,
            &ChannelPrefsUpdate {
                slack_min_priority: Some("critical"),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("priority"));
    }

    #[test]
    fn merge_channel_updates_forces_in_app_true() {
        // A stored configuration claiming in_app=false would fail the
        // server's PUT validation; the merge must repair it.
        let current = serde_json::json!({ "in_app": false });
        let merged = merge_channel_updates(
            Some(&current),
            &ChannelPrefsUpdate {
                email_digest: Some("off"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(merged["in_app"], serde_json::json!(true));
    }

    // Command-flow tests against scripted HTTP servers: the exact request
    // sequences and bodies the new commands emit.

    /// Script GET /api/v1/workspaces?slug=platform → workspace ws-1.
    const WORKSPACE_SLUG_JSON: &str = r#"[{"id":"ws-1","slug":"platform","name":"Platform"}]"#;

    #[tokio::test]
    async fn profile_set_puts_only_requested_fields() {
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![
            (
                "200 OK".to_string(),
                r#"{"id":"u-1","username":"jsell","display_name":"Jordan Sell","timezone":"America/New_York","locale":"en-US","global_role":"Member","created_at":1,"updated_at":2}"#.to_string(),
            ),
        ]);
        let api = client::GyreClient::new(base, "tok".to_string());
        run_profile_set(
            &api,
            Some("Jordan Sell".to_string()),
            Some("America/New_York".to_string()),
            None,
            None,
        )
        .await
        .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let req = &requests[0];
        assert!(req.request_line.starts_with("PUT /api/v1/users/me"));
        let body: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        // Only the two requested fields — no locale/avatar_url keys, so the
        // server keeps their stored values.
        assert_eq!(
            body,
            serde_json::json!({
                "display_name": "Jordan Sell",
                "timezone": "America/New_York",
            })
        );
    }

    #[tokio::test]
    async fn profile_set_requires_a_field_before_any_request() {
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![]);
        let api = client::GyreClient::new(base, "tok".to_string());
        let err = run_profile_set(&api, None, None, None, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no fields given"));
        assert!(captured.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn member_set_role_transfer_requires_owner_role() {
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![]);
        let api = client::GyreClient::new(base, "tok".to_string());
        let err = run_member_set_role(
            &api,
            Some("platform"),
            "@asmith",
            "Admin",
            true, // --transfer with a non-Owner role
            None,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("--transfer"));
        assert!(captured.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn member_set_role_validates_role_before_any_request() {
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![]);
        let api = client::GyreClient::new(base, "tok".to_string());
        let err = run_member_set_role(&api, Some("platform"), "@asmith", "SuperAdmin", false, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown role"));
        assert!(captured.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn member_set_role_sends_role_and_transfer_reason() {
        // Sequence: slug resolve → user lookup → role PUT.
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![
            ("200 OK".to_string(), WORKSPACE_SLUG_JSON.to_string()),
            (
                "200 OK".to_string(),
                r#"{"id":"u-9","username":"asmith","display_name":"Alice Smith"}"#.to_string(),
            ),
            ("204 No Content".to_string(), String::new()),
        ]);
        let api = client::GyreClient::new(base, "tok".to_string());
        run_member_set_role(
            &api,
            Some("platform"),
            "@asmith",
            "Owner",
            true,
            Some("handover"),
        )
        .await
        .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[0]
            .request_line
            .starts_with("GET /api/v1/workspaces?"));
        assert!(requests[1]
            .request_line
            .starts_with("GET /api/v1/users/asmith"));
        assert!(requests[2]
            .request_line
            .starts_with("PUT /api/v1/workspaces/ws-1/members/u-9/role"));
        let body: serde_json::Value = serde_json::from_str(&requests[2].body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "role": "Owner", "reason": "handover" })
        );
    }

    #[tokio::test]
    async fn workspace_invite_by_handle_resolves_user_then_posts_invite() {
        // Sequence: slug resolve → user lookup → invite POST.
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![
            ("200 OK".to_string(), WORKSPACE_SLUG_JSON.to_string()),
            (
                "200 OK".to_string(),
                r#"{"id":"u-2","username":"alice","display_name":"Alice Smith"}"#.to_string(),
            ),
            (
                "201 Created".to_string(),
                r#"{"id":"m-1","user_id":"u-2","workspace_id":"ws-1","role":"Developer","accepted":false,"created_at":1}"#.to_string(),
            ),
        ]);
        let api = client::GyreClient::new(base, "tok".to_string());
        run_workspace_invite(&api, Some("platform"), "@alice", Some("Developer"))
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[2]
            .request_line
            .starts_with("POST /api/v1/workspaces/ws-1/invite"));
        let body: serde_json::Value = serde_json::from_str(&requests[2].body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "user_id": "u-2", "role": "Developer" })
        );
    }

    #[tokio::test]
    async fn workspace_invite_defaults_to_developer_role() {
        // Sequence: slug resolve → user lookup → invite POST.
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![
            ("200 OK".to_string(), WORKSPACE_SLUG_JSON.to_string()),
            (
                "200 OK".to_string(),
                r#"{"id":"u-2","username":"alice","display_name":""}"#.to_string(),
            ),
            (
                "201 Created".to_string(),
                r#"{"id":"m-1","user_id":"u-2","workspace_id":"ws-1","role":"Developer","accepted":false,"created_at":1}"#.to_string(),
            ),
        ]);
        let api = client::GyreClient::new(base, "tok".to_string());
        run_workspace_invite(&api, Some("platform"), "alice", None)
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        let body: serde_json::Value = serde_json::from_str(&requests[2].body).unwrap();
        assert_eq!(body["role"], serde_json::json!("Developer"));
    }

    #[tokio::test]
    async fn workspace_invite_by_email_takes_tenant_invite_path() {
        // Sequence: slug resolve (for scoping) → tenant invite POST.
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![
            ("200 OK".to_string(), WORKSPACE_SLUG_JSON.to_string()),
            (
                "201 Created".to_string(),
                r#"{"id":"inv-1","email":"alice@example.com","status":"Pending"}"#.to_string(),
            ),
        ]);
        let api = client::GyreClient::new(base, "tok".to_string());
        run_workspace_invite(
            &api,
            Some("platform"),
            "alice@example.com",
            Some("Developer"),
        )
        .await
        .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1]
            .request_line
            .starts_with("POST /api/v1/tenant/invite"));
        let body: serde_json::Value = serde_json::from_str(&requests[1].body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "email": "alice@example.com",
                "workspace": "platform",
                "role": "Developer",
            })
        );
    }

    #[tokio::test]
    async fn team_create_resolves_lead_then_posts_team() {
        // Sequence: slug resolve → lead lookup → team POST.
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![
            ("200 OK".to_string(), WORKSPACE_SLUG_JSON.to_string()),
            (
                "200 OK".to_string(),
                r#"{"id":"u-1","username":"jsell","display_name":"Jordan Sell"}"#.to_string(),
            ),
            (
                "201 Created".to_string(),
                r#"{"id":"t-1","workspace_id":"ws-1","name":"Platform Team","slug":"platform-team","lead":"u-1","member_ids":[],"created_at":1}"#.to_string(),
            ),
        ]);
        let api = client::GyreClient::new(base, "tok".to_string());
        run_team_create(
            &api,
            Some("platform"),
            "Platform Team",
            Some("@jsell"),
            Some("Owns the platform"),
        )
        .await
        .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[1]
            .request_line
            .starts_with("GET /api/v1/users/jsell"));
        assert!(requests[2]
            .request_line
            .starts_with("POST /api/v1/workspaces/ws-1/teams"));
        let body: serde_json::Value = serde_json::from_str(&requests[2].body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "name": "Platform Team",
                "lead": "u-1",
                "description": "Owns the platform",
            })
        );
    }

    #[tokio::test]
    async fn team_add_resolves_user_and_team_by_name_or_slug() {
        // Sequence: slug resolve → user lookup → team list → member POST.
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![
            ("200 OK".to_string(), WORKSPACE_SLUG_JSON.to_string()),
            (
                "200 OK".to_string(),
                r#"{"id":"u-9","username":"asmith","display_name":"Alice Smith"}"#.to_string(),
            ),
            (
                "200 OK".to_string(),
                r#"{"teams":[{"id":"t-2","name":"Other Team","slug":"other","lead":"u-1","member_ids":[]},{"id":"t-1","name":"Platform Team","slug":"platform-team","lead":"u-1","member_ids":[]}]}"#.to_string(),
            ),
            (
                "201 Created".to_string(),
                r#"{"team_id":"t-1","user_id":"u-9","added_at":1}"#.to_string(),
            ),
        ]);
        let api = client::GyreClient::new(base, "tok".to_string());
        run_team_membership(&api, Some("platform"), "@asmith", "Platform Team", false)
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 4);
        // "Platform Team" matched by NAME (the slug is platform-team), and
        // the membership POST went to that team's id.
        assert!(requests[2]
            .request_line
            .starts_with("GET /api/v1/workspaces/ws-1/teams"));
        assert!(requests[3]
            .request_line
            .starts_with("POST /api/v1/workspaces/ws-1/teams/t-1/members"));
        let body: serde_json::Value = serde_json::from_str(&requests[3].body).unwrap();
        assert_eq!(body, serde_json::json!({ "user_id": "u-9" }));
    }

    #[tokio::test]
    async fn team_remove_targets_resolved_team_member() {
        // Sequence: slug resolve → user lookup → team list → member DELETE.
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![
            ("200 OK".to_string(), WORKSPACE_SLUG_JSON.to_string()),
            (
                "200 OK".to_string(),
                r#"{"id":"u-9","username":"asmith","display_name":""}"#.to_string(),
            ),
            (
                "200 OK".to_string(),
                r#"{"teams":[{"id":"t-1","name":"Platform Team","slug":"platform-team","lead":"u-1","member_ids":[]}]}"#.to_string(),
            ),
            ("204 No Content".to_string(), String::new()),
        ]);
        let api = client::GyreClient::new(base, "tok".to_string());
        run_team_membership(&api, Some("platform"), "@asmith", "platform-team", true)
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 4);
        assert!(requests[3]
            .request_line
            .starts_with("DELETE /api/v1/workspaces/ws-1/teams/t-1/members/u-9"));
    }

    #[tokio::test]
    async fn tenant_invite_single_sends_preassignment() {
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![(
            "201 Created".to_string(),
            r#"{"id":"inv-1","email":"alice@example.com","status":"Pending"}"#.to_string(),
        )]);
        let api = client::GyreClient::new(base, "tok".to_string());
        run_tenant_invite(
            &api,
            Some("alice@example.com".to_string()),
            Some("platform".to_string()),
            Some("Developer".to_string()),
            None,
        )
        .await
        .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0]
            .request_line
            .starts_with("POST /api/v1/tenant/invite"));
        let body: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "email": "alice@example.com",
                "workspace": "platform",
                "role": "Developer",
            })
        );
    }

    #[tokio::test]
    async fn tenant_invite_role_without_workspace_is_rejected() {
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![]);
        let api = client::GyreClient::new(base, "tok".to_string());
        let err = run_tenant_invite(
            &api,
            Some("alice@example.com".to_string()),
            None,
            Some("Developer".to_string()),
            None,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("requires --workspace"));
        assert!(captured.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn tenant_invite_bulk_parses_csv_and_posts_rows() {
        let dir = tempfile::tempdir().unwrap();
        let csv_path = dir.path().join("users.csv");
        std::fs::write(
            &csv_path,
            "email,workspace,role\nalice@example.com\nbob@example.com,platform,Admin\n",
        )
        .unwrap();

        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![(
            "201 Created".to_string(),
            r#"{"invited":2}"#.to_string(),
        )]);
        let api = client::GyreClient::new(base, "tok".to_string());
        run_tenant_invite(&api, None, None, None, Some(csv_path))
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0]
            .request_line
            .starts_with("POST /api/v1/tenant/invite/bulk"));
        let body: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "invitations": [
                    { "email": "alice@example.com", "workspace": null, "role": null },
                    { "email": "bob@example.com", "workspace": "platform", "role": "Admin" },
                ]
            })
        );
    }

    #[tokio::test]
    async fn notifications_preferences_set_gets_merges_then_puts() {
        // Sequence: preferences GET → preferences PUT.
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![
            (
                "200 OK".to_string(),
                r#"{"preferences":[],"channels":{"in_app":true,"email":{"enabled":false,"digest":"Off","min_priority":"Medium"},"slack":{"webhook_url":"https://hooks.slack.com/keep","channel":null,"min_priority":"Low"}}}"#.to_string(),
            ),
            (
                "200 OK".to_string(),
                r#"{"preferences":[],"channels":{}}"#.to_string(),
            ),
        ]);
        let api = client::GyreClient::new(base, "tok".to_string());
        run_notification_preferences_set(
            &api,
            ChannelPrefsUpdate {
                email_digest: Some("hourly"),
                email_min_priority: Some("high"),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0]
            .request_line
            .starts_with("GET /api/v1/notifications/preferences"));
        assert!(requests[1]
            .request_line
            .starts_with("PUT /api/v1/notifications/preferences"));
        let body: serde_json::Value = serde_json::from_str(&requests[1].body).unwrap();
        // Email updated + enabled; the untouched Slack channel survives.
        assert_eq!(
            body["channels"]["email"],
            serde_json::json!({ "enabled": true, "digest": "Hourly", "min_priority": "High" })
        );
        assert_eq!(
            body["channels"]["slack"]["webhook_url"],
            serde_json::json!("https://hooks.slack.com/keep")
        );
    }

    #[tokio::test]
    async fn notifications_list_filters_unread_client_side() {
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![
            (
                "200 OK".to_string(),
                r#"{"notifications":[
                    {"id":"n-1","priority":3,"notification_type":"SpecApprovalRequested","title":"A","read":true,"created_at":1},
                    {"id":"n-2","priority":2,"notification_type":"AgentEscalation","title":"B","read":false,"created_at":2}
                ],"limit":200,"offset":0}"#
                    .to_string(),
            ),
        ]);
        let api = client::GyreClient::new(base, "tok".to_string());
        // --unread --priority high: the priority filter goes to the server;
        // the read flag is filtered client-side.
        run_notifications_list(&api, true, Some("high"), None, None, 50)
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let line = &requests[0].request_line;
        assert!(line.starts_with("GET /api/v1/notifications?"));
        assert!(line.contains("priority=high"));
        assert!(line.contains("limit=200"));
        assert!(!line.contains("unread"));
    }

    #[tokio::test]
    async fn notifications_list_rejects_unknown_priority_before_request() {
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![]);
        let api = client::GyreClient::new(base, "tok".to_string());
        let err = run_notifications_list(&api, false, Some("critical"), None, None, 50)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("priority"));
        assert!(captured.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn sessions_revoke_uses_spec_endpoint() {
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![(
            "204 No Content".to_string(),
            String::new(),
        )]);
        let api = client::GyreClient::new(base, "tok".to_string());
        api.revoke_session("s-42").await.unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0]
            .request_line
            .starts_with("DELETE /api/v1/users/me/sessions/s-42"));
    }

    #[tokio::test]
    async fn sessions_revoke_all_uses_spec_endpoint() {
        let (base, captured) = client::test_http::spawn_mock_http_server_sequence(vec![(
            "200 OK".to_string(),
            r#"{"revoked":3}"#.to_string(),
        )]);
        let api = client::GyreClient::new(base, "tok".to_string());
        let result = api.revoke_all_sessions().await.unwrap();
        assert_eq!(result["revoked"], serde_json::json!(3));

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0]
            .request_line
            .starts_with("POST /api/v1/users/me/sessions/revoke-all"));
    }
}
