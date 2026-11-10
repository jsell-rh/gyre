//! User management, workspace membership, teams, and notification endpoints (HSI §2 + §12).
//!
//! POST /api/v1/users                 (bootstrap admin user creation - Admin-only)
//! GET  /api/v1/users/me
//! PUT  /api/v1/users/me
//! GET  /api/v1/users/me/notifications?workspace_id=&min_priority=&max_priority=&limit=&offset=
//! POST /api/v1/notifications/:id/dismiss
//! POST /api/v1/notifications/:id/resolve
//! POST /api/v1/workspaces/:id/members   (invite)
//! GET  /api/v1/workspaces/:id/members
//! PUT  /api/v1/workspaces/:id/members/:user_id
//! DELETE /api/v1/workspaces/:id/members/:user_id
//! POST /api/v1/workspaces/:id/teams
//! GET  /api/v1/workspaces/:id/teams
//! PUT  /api/v1/workspaces/:id/teams/:team_id
//! DELETE /api/v1/workspaces/:id/teams/:team_id
//!
//! HSI §12 User Profile endpoints (all per-handler auth — NOT ABAC middleware):
//! GET    /api/v1/users/me/tokens
//! POST   /api/v1/users/me/tokens
//! DELETE /api/v1/users/me/tokens/:id
//! GET    /api/v1/users/me/notification-preferences
//! PUT    /api/v1/users/me/notification-preferences
//! GET    /api/v1/users/me/judgments?workspace_id=&type=&since=&limit=&offset=
//!
//! Session Management (user-management.md §Session Management):
//! GET    /api/v1/users/me/sessions
//! DELETE /api/v1/users/me/sessions/:id
//! POST   /api/v1/users/me/sessions/revoke-all
//! TenantAdmin variants (same spec section — "TenantAdmins can view and
//! revoke any user's sessions"):
//! GET    /api/v1/users/:user_id/sessions
//! DELETE /api/v1/users/:user_id/sessions/:id
//! POST   /api/v1/users/:user_id/sessions/revoke-all

//! All notification endpoints use per-handler auth (not ABAC):
//! the handler verifies notification.user_id == caller AND notification.tenant_id == caller.tenant_id.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::{
    JudgmentType, Team, User, UserNotificationPreference, UserRole, UserToken, WorkspaceMembership,
    WorkspaceRole,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

use super::error::ApiError;
use super::{new_id, now_secs};

// ─── User profile ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct UserProfileResponse {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub timezone: String,
    pub locale: String,
    pub global_role: String,
    pub roles: Vec<String>,
    pub preferences: serde_json::Value,
    pub last_login_at: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
    /// Tenant binding (platform-model.md §1: every user belongs to exactly
    /// one tenant). None for legacy users provisioned before the binding
    /// existed (F1); those fail closed at the auth extractor.
    pub tenant_id: Option<String>,
}

impl From<User> for UserProfileResponse {
    fn from(u: User) -> Self {
        let prefs = serde_json::to_value(&u.preferences).unwrap_or_default();
        Self {
            id: u.id.to_string(),
            username: u.username.clone(),
            display_name: u.display_name.clone(),
            email: u.email.clone(),
            avatar_url: u.avatar_url.clone(),
            timezone: u.timezone.clone(),
            locale: u.locale.clone(),
            global_role: format!("{:?}", u.global_role),
            roles: u.roles.iter().map(|r| r.as_str().to_string()).collect(),
            preferences: prefs,
            last_login_at: u.last_login_at,
            created_at: u.created_at,
            updated_at: u.updated_at,
            tenant_id: u.tenant_id.map(|t| t.to_string()),
        }
    }
}

pub async fn get_me(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
) -> Result<Json<UserProfileResponse>, ApiError> {
    if let Some(user_id) = &auth.user_id {
        if let Some(user) = state.users.find_by_id(user_id).await? {
            return Ok(Json(UserProfileResponse::from(user)));
        }
    }
    // Return a minimal profile derived from auth when no stored user exists.
    let global_role = if auth.roles.contains(&UserRole::Admin) {
        "Admin".to_string()
    } else {
        "Member".to_string()
    };
    let profile = UserProfileResponse {
        id: auth.agent_id.clone(),
        username: auth.agent_id.clone(),
        display_name: auth.agent_id.clone(),
        email: None,
        avatar_url: None,
        timezone: "UTC".to_string(),
        locale: "en".to_string(),
        global_role,
        roles: auth.roles.iter().map(|r| r.as_str().to_string()).collect(),
        preferences: serde_json::to_value(gyre_domain::UserPreferences::default())
            .unwrap_or_default(),
        last_login_at: None,
        created_at: 0,
        updated_at: 0,
        tenant_id: None,
    };
    Ok(Json(profile))
}

#[derive(Deserialize)]
pub struct UpdateProfileRequest {
    pub display_name: Option<String>,
    pub timezone: Option<String>,
    pub locale: Option<String>,
    pub avatar_url: Option<String>,
    pub preferences: Option<serde_json::Value>,
}

/// PUT /api/v1/users/me
///
/// Updates the caller's editable profile fields: display_name, timezone,
/// locale, avatar_url, and preferences (user-management.md §User
/// Preferences: server-side persistence). username, external_id, email,
/// and global_role are not editable here — username is immutable after
/// creation and email is derived from SSO (§Username vs Display Name).
pub async fn update_me(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateProfileRequest>,
) -> Result<Json<UserProfileResponse>, ApiError> {
    let user_id = auth
        .user_id
        .as_ref()
        .ok_or_else(|| ApiError::Forbidden("No user identity".to_string()))?;
    let mut user = state
        .users
        .find_by_id(user_id)
        .await?
        .ok_or(ApiError::NotFound("User not found".to_string()))?;

    if let Some(dn) = req.display_name {
        if dn.trim().is_empty() {
            return Err(ApiError::InvalidInput(
                "display_name must not be empty".to_string(),
            ));
        }
        user.display_name = dn;
    }
    if let Some(tz) = req.timezone {
        if tz.trim().is_empty() {
            return Err(ApiError::InvalidInput(
                "timezone must not be empty".to_string(),
            ));
        }
        user.timezone = tz;
    }
    if let Some(locale) = req.locale {
        if locale.trim().is_empty() {
            return Err(ApiError::InvalidInput(
                "locale must not be empty".to_string(),
            ));
        }
        user.locale = locale;
    }
    if let Some(avatar) = req.avatar_url {
        user.avatar_url = Some(avatar);
    }
    if let Some(prefs_json) = req.preferences {
        // Partial-update semantics: omitted preference fields keep their
        // current value (matching the Option-based top-level fields). An
        // invalid value for a present field still fails the whole request.
        let patch: gyre_domain::UserPreferencesPatch = serde_json::from_value(prefs_json)
            .map_err(|e| ApiError::InvalidInput(format!("invalid preferences: {e}")))?;
        user.preferences.apply_patch(patch);
    }
    user.updated_at = now_secs();
    state.users.update(&user).await?;
    Ok(Json(UserProfileResponse::from(user)))
}

// ─── Notifications ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct NotificationParams {
    pub workspace_id: Option<String>,
    pub min_priority: Option<u8>,
    pub max_priority: Option<u8>,
    pub notification_type: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Serialize)]
pub struct NotificationResponse {
    pub id: String,
    pub workspace_id: String,
    pub notification_type: String,
    /// HSI §8 numeric rank (1 highest – 10 lowest). Kept numeric for the
    /// Inbox UI's sort order and P-badge.
    pub priority: u8,
    /// Four-level priority (user-management.md §Notification Entity):
    /// Low / Medium / High / Urgent.
    pub priority_level: String,
    pub title: String,
    pub body: Option<String>,
    pub entity_ref: Option<String>,
    pub repo_id: Option<String>,
    /// Structured source kind ("agent", "mr", "spec", "gate", "budget",
    /// "workspace", "system").
    pub entity_type: String,
    /// Deep link to the relevant page (user-management.md §Notification Entity).
    pub action_url: Option<String>,
    pub read: bool,
    pub read_at: Option<i64>,
    pub resolved_at: Option<i64>,
    pub dismissed_at: Option<i64>,
    pub created_at: i64,
}

impl From<Notification> for NotificationResponse {
    fn from(n: Notification) -> Self {
        let entity_id = n.source.entity_id.clone();
        Self {
            id: n.id.to_string(),
            workspace_id: n.workspace_id.to_string(),
            notification_type: n.notification_type.as_str().to_string(),
            priority: n.priority_rank,
            priority_level: n.priority.as_str().to_string(),
            title: n.title,
            body: n.body,
            entity_ref: if entity_id.is_empty() {
                None
            } else {
                Some(entity_id)
            },
            repo_id: n.source.repo_id.as_ref().map(|r| r.as_str().to_string()),
            entity_type: n.source.entity_type,
            action_url: n.action_url,
            read: n.read,
            read_at: n.read_at,
            resolved_at: n.resolved_at,
            dismissed_at: n.dismissed_at,
            created_at: n.created_at,
        }
    }
}

/// GET /api/v1/users/me/notifications?workspace_id=&min_priority=&max_priority=&limit=&offset=
///
/// No ABAC resource type — `/users/me/*` endpoints are implicitly scoped to the authenticated user.
pub async fn get_my_notifications(
    auth: AuthenticatedAgent,
    Query(params): Query<NotificationParams>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = resolve_user_id(&auth);
    let workspace_id = params.workspace_id.as_deref().map(Id::new);
    let limit = params.limit.unwrap_or(50).min(200);
    let offset = params.offset.unwrap_or(0);
    let notifications = state
        .notifications
        .list_for_user(
            &user_id,
            workspace_id.as_ref(),
            params.min_priority,
            params.max_priority,
            params.notification_type.as_deref(),
            limit,
            offset,
        )
        .await?;
    let items: Vec<NotificationResponse> = notifications.into_iter().map(Into::into).collect();
    Ok(Json(serde_json::json!({
        "notifications": items,
        "limit": limit,
        "offset": offset,
    })))
}

/// GET /api/v1/users/me/notifications/count?workspace_id=
///
/// Returns the count of active (unresolved, undismissed) notifications for the caller.
/// Used by the inbox badge in the dashboard.
pub async fn get_notification_count(
    auth: AuthenticatedAgent,
    Query(params): Query<NotificationParams>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = resolve_user_id(&auth);
    let workspace_id = params.workspace_id.as_deref().map(Id::new);
    let count = state
        .notifications
        .count_unresolved(&user_id, workspace_id.as_ref())
        .await?;
    Ok(Json(serde_json::json!({ "count": count })))
}

/// POST /api/v1/notifications/:id/dismiss
///
/// Per-handler auth: verifies notification belongs to caller AND caller.tenant_id matches.
pub async fn dismiss_notification(
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let user_id = resolve_user_id(&auth);
    let notif_id = Id::new(id);
    let notif = state
        .notifications
        .get(&notif_id, &user_id)
        .await?
        .ok_or(ApiError::NotFound("Notification not found".to_string()))?;
    // Cross-tenant guard: the notification's tenant must match the caller's tenant.
    if notif.tenant_id != auth.tenant_id {
        return Err(ApiError::Forbidden(
            "Cross-tenant notification access denied".to_string(),
        ));
    }
    state.notifications.dismiss(&notif_id, &user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ResolveRequest {
    pub action_taken: Option<String>,
}

/// POST /api/v1/notifications/:id/resolve
///
/// Per-handler auth: verifies notification belongs to caller AND caller.tenant_id matches.
pub async fn resolve_notification(
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<ResolveRequest>,
) -> Result<StatusCode, ApiError> {
    let user_id = resolve_user_id(&auth);
    let notif_id = Id::new(id);
    let notif = state
        .notifications
        .get(&notif_id, &user_id)
        .await?
        .ok_or(ApiError::NotFound("Notification not found".to_string()))?;
    if notif.tenant_id != auth.tenant_id {
        return Err(ApiError::Forbidden(
            "Cross-tenant notification access denied".to_string(),
        ));
    }
    state
        .notifications
        .resolve(&notif_id, &user_id, req.action_taken.as_deref())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/notifications/:id/read
///
/// Marks the notification read (stamps `read_at`). Per-handler auth: verifies
/// notification belongs to caller AND caller.tenant_id matches
/// (user-management.md §Notification Entity read state).
pub async fn mark_notification_read(
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let user_id = resolve_user_id(&auth);
    let notif_id = Id::new(id);
    let notif = state
        .notifications
        .get(&notif_id, &user_id)
        .await?
        .ok_or(ApiError::NotFound("Notification not found".to_string()))?;
    if notif.tenant_id != auth.tenant_id {
        return Err(ApiError::Forbidden(
            "Cross-tenant notification access denied".to_string(),
        ));
    }
    state
        .notifications
        .mark_read(&notif_id, &user_id, true)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/notifications/:id/unread
///
/// Clears the read state (and `read_at`). Per-handler auth: verifies
/// notification belongs to caller AND caller.tenant_id matches.
pub async fn mark_notification_unread(
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let user_id = resolve_user_id(&auth);
    let notif_id = Id::new(id);
    let notif = state
        .notifications
        .get(&notif_id, &user_id)
        .await?
        .ok_or(ApiError::NotFound("Notification not found".to_string()))?;
    if notif.tenant_id != auth.tenant_id {
        return Err(ApiError::Forbidden(
            "Cross-tenant notification access denied".to_string(),
        ));
    }
    state
        .notifications
        .mark_read(&notif_id, &user_id, false)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Workspace Members ───────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct MembershipResponse {
    pub id: String,
    pub user_id: String,
    pub workspace_id: String,
    pub role: String,
    pub accepted: bool,
    pub created_at: u64,
}

impl From<WorkspaceMembership> for MembershipResponse {
    fn from(m: WorkspaceMembership) -> Self {
        Self {
            id: m.id.to_string(),
            user_id: m.user_id.to_string(),
            workspace_id: m.workspace_id.to_string(),
            role: m.role.as_str().to_string(),
            accepted: m.accepted,
            created_at: m.created_at,
        }
    }
}

#[derive(Deserialize)]
pub struct InviteMemberRequest {
    pub user_id: String,
    pub role: String,
}

pub async fn invite_member(
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<InviteMemberRequest>,
) -> Result<(StatusCode, Json<MembershipResponse>), ApiError> {
    let role = WorkspaceRole::parse_role(&req.role)
        .ok_or_else(|| ApiError::InvalidInput(format!("Unknown role: {}", req.role)))?;

    let caller_id = Id::new(auth.agent_id.clone());
    let ws_id = Id::new(workspace_id);
    let user_id = Id::new(req.user_id);
    let now = now_secs();

    let membership = WorkspaceMembership::new(new_id(), user_id, ws_id, role, caller_id, now);
    state.workspace_memberships.create(&membership).await?;

    // Notify the invited user (user-management.md §Notification Entity:
    // InvitationReceived, Low — informational). Routed through notify_rich
    // (the codebase's notification helper): a notification is a courtesy
    // side-effect, not a dependent record of the membership — its failure
    // must not fail the invite, and notify_rich logs-and-continues internally.
    crate::notifications::notify_rich(
        &state,
        membership.workspace_id.clone(),
        membership.user_id.clone(),
        NotificationType::InvitationReceived,
        format!(
            "You have been invited to workspace {}",
            membership.workspace_id
        ),
        auth.tenant_id.clone(),
        None,
        None,
        None,
        "workspace",
        None,
    )
    .await;

    Ok((
        StatusCode::CREATED,
        Json(MembershipResponse::from(membership)),
    ))
}

pub async fn list_members(
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ws_id = Id::new(workspace_id);
    let members = state
        .workspace_memberships
        .list_by_workspace(&ws_id)
        .await?;
    let items: Vec<MembershipResponse> = members.into_iter().map(Into::into).collect();
    Ok(Json(serde_json::json!({"members": items})))
}

#[derive(Deserialize)]
pub struct UpdateMemberRoleRequest {
    pub role: String,
}

pub async fn update_member_role(
    Path((workspace_id, user_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateMemberRoleRequest>,
) -> Result<StatusCode, ApiError> {
    let role = WorkspaceRole::parse_role(&req.role)
        .ok_or_else(|| ApiError::InvalidInput(format!("Unknown role: {}", req.role)))?;
    let ws_id = Id::new(workspace_id);
    let uid = Id::new(user_id);
    let membership = state
        .workspace_memberships
        .find_by_user_and_workspace(&uid, &ws_id)
        .await?
        .ok_or(ApiError::NotFound("Membership not found".to_string()))?;
    state
        .workspace_memberships
        .update_role(&membership.id, role)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_member(
    Path((workspace_id, user_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let ws_id = Id::new(workspace_id);
    let uid = Id::new(user_id);
    let membership = state
        .workspace_memberships
        .find_by_user_and_workspace(&uid, &ws_id)
        .await?
        .ok_or(ApiError::NotFound("Membership not found".to_string()))?;
    state.workspace_memberships.delete(&membership.id).await?;

    // Notify the removed user (user-management.md §Notification Entity:
    // MembershipChanged — Low, informational). Best-effort: the membership is
    // already deleted; notification failure must not fail the removal. The
    // tenant scope comes from the workspace lookup — skip rather than
    // fabricate one when it cannot be resolved.
    if let Ok(Some(ws)) = state.workspaces.find_by_id(&ws_id).await {
        crate::notifications::notify_rich(
            state.as_ref(),
            ws_id.clone(),
            uid.clone(),
            NotificationType::MembershipChanged,
            format!("Your membership in workspace '{}' was removed", ws.name),
            ws.tenant_id.to_string(),
            Some(
                serde_json::json!({
                    "workspace_id": ws_id.as_str(),
                    "user_id": uid.as_str(),
                })
                .to_string(),
            ),
            Some(ws_id.to_string()),
            None,
            "workspace",
            None,
        )
        .await;
    }

    Ok(StatusCode::NO_CONTENT)
}

// ─── Teams ───────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TeamResponse {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub description: Option<String>,
    pub member_ids: Vec<String>,
    pub created_at: u64,
}

impl From<Team> for TeamResponse {
    fn from(t: Team) -> Self {
        Self {
            id: t.id.to_string(),
            workspace_id: t.workspace_id.to_string(),
            name: t.name,
            description: t.description,
            member_ids: t.member_ids.iter().map(|id| id.to_string()).collect(),
            created_at: t.created_at,
        }
    }
}

#[derive(Deserialize)]
pub struct CreateTeamRequest {
    pub name: String,
    pub description: Option<String>,
}

pub async fn create_team(
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTeamRequest>,
) -> Result<(StatusCode, Json<TeamResponse>), ApiError> {
    let ws_id = Id::new(workspace_id);
    let mut team = Team::new(new_id(), ws_id, req.name, now_secs());
    team.description = req.description;
    state.teams.create(&team).await?;
    Ok((StatusCode::CREATED, Json(TeamResponse::from(team))))
}

pub async fn list_teams(
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ws_id = Id::new(workspace_id);
    let teams = state.teams.list_by_workspace(&ws_id).await?;
    let items: Vec<TeamResponse> = teams.into_iter().map(Into::into).collect();
    Ok(Json(serde_json::json!({"teams": items})))
}

#[derive(Deserialize)]
pub struct UpdateTeamRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

pub async fn update_team(
    Path((workspace_id, team_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateTeamRequest>,
) -> Result<Json<TeamResponse>, ApiError> {
    let _ws_id = Id::new(workspace_id);
    let tid = Id::new(team_id);
    let mut team = state
        .teams
        .find_by_id(&tid)
        .await?
        .ok_or(ApiError::NotFound("Team not found".to_string()))?;
    if let Some(name) = req.name {
        team.name = name;
    }
    if let Some(desc) = req.description {
        team.description = Some(desc);
    }
    state.teams.update(&team).await?;
    Ok(Json(TeamResponse::from(team)))
}

pub async fn delete_team(
    Path((workspace_id, team_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let _ws_id = Id::new(workspace_id);
    let tid = Id::new(team_id);
    state.teams.delete(&tid).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn resolve_user_id(auth: &AuthenticatedAgent) -> Id {
    auth.user_id
        .clone()
        .unwrap_or_else(|| Id::new(auth.agent_id.clone()))
}

// ─── HSI §12: API Tokens ─────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct UserTokenResponse {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub last_used_at: Option<u64>,
    pub expires_at: Option<u64>,
}

impl From<UserToken> for UserTokenResponse {
    fn from(t: UserToken) -> Self {
        Self {
            id: t.id.to_string(),
            name: t.name,
            created_at: t.created_at,
            last_used_at: t.last_used_at,
            expires_at: t.expires_at,
        }
    }
}

/// GET /api/v1/users/me/tokens
pub async fn list_tokens(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = resolve_user_id(&auth);
    let tokens = state.user_tokens.list_for_user(&user_id).await?;
    let items: Vec<UserTokenResponse> = tokens.into_iter().map(Into::into).collect();
    Ok(Json(serde_json::json!({ "tokens": items })))
}

#[derive(Deserialize)]
pub struct CreateTokenRequest {
    pub name: String,
    pub expires_at: Option<u64>,
}

#[derive(Serialize)]
pub struct CreateTokenResponse {
    pub id: String,
    pub name: String,
    pub token: String, // plaintext, returned once
    pub created_at: u64,
    pub expires_at: Option<u64>,
}

/// POST /api/v1/users/me/tokens
///
/// Creates a new API token. Returns the plaintext token exactly once.
/// Only the SHA-256 hash is stored.
pub async fn create_token(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTokenRequest>,
) -> Result<(StatusCode, Json<CreateTokenResponse>), ApiError> {
    let user_id = resolve_user_id(&auth);

    // Generate a cryptographically random token from two UUID v4 values.
    let raw_token = {
        use uuid::Uuid;
        let a = Uuid::new_v4().simple().to_string();
        let b = Uuid::new_v4().simple().to_string();
        format!("gyre_{a}{b}")
    };

    let token_hash = {
        let mut hasher = Sha256::new();
        hasher.update(raw_token.as_bytes());
        format!("{:x}", hasher.finalize())
    };

    let now = now_secs();
    let token_id = new_id();
    let token = UserToken::new(token_id.clone(), user_id, &req.name, token_hash, now);
    let mut token = token;
    token.expires_at = req.expires_at;
    state.user_tokens.create(&token).await?;

    Ok((
        StatusCode::CREATED,
        Json(CreateTokenResponse {
            id: token.id.to_string(),
            name: token.name,
            token: raw_token,
            created_at: token.created_at,
            expires_at: token.expires_at,
        }),
    ))
}

// ─── Session Management (user-management.md §Session Management) ────────────

#[derive(Serialize)]
pub struct UserSessionResponse {
    pub id: String,
    /// Device the session presents from: User-Agent header captured at
    /// session creation (empty for WebSocket-minted sessions, which carry
    /// no request headers).
    pub user_agent: String,
    pub ip_address: String,
    pub created_at: u64,
    pub last_active_at: u64,
    pub expires_at: u64,
}

impl From<gyre_domain::UserSession> for UserSessionResponse {
    fn from(s: gyre_domain::UserSession) -> Self {
        Self {
            id: s.id.to_string(),
            user_agent: s.user_agent,
            ip_address: s.ip_address,
            created_at: s.created_at,
            last_active_at: s.last_active_at,
            expires_at: s.expires_at,
        }
    }
}

/// GET /api/v1/users/me/sessions
///
/// Lists the current user's active sessions (non-revoked, non-expired).
/// The session token hash is never exposed — only device, IP, and activity
/// timestamps.
pub async fn list_my_sessions(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = resolve_user_id(&auth);
    let now = now_secs();
    let sessions = state.sessions.list_for_user(&user_id).await?;
    let items: Vec<UserSessionResponse> = sessions
        .into_iter()
        .filter(|s| s.is_active(now))
        .map(Into::into)
        .collect();
    Ok(Json(serde_json::json!({ "sessions": items })))
}

/// DELETE /api/v1/users/me/sessions/:id
///
/// Revokes one of the current user's sessions. Scoped: revoking another
/// user's session id is a 404, not a silent success.
pub async fn revoke_my_session(
    auth: AuthenticatedAgent,
    Path(session_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let user_id = resolve_user_id(&auth);
    let sid = Id::new(session_id);
    // Confirm the session exists AND belongs to the caller before revoking.
    let session = state
        .sessions
        .find_by_id(&sid)
        .await?
        .ok_or(ApiError::NotFound("Session not found".to_string()))?;
    if session.user_id != user_id {
        return Err(ApiError::NotFound("Session not found".to_string()));
    }
    state.sessions.revoke(&sid, &user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/users/me/sessions/revoke-all
///
/// "Sign out everywhere": revokes every session for the current user.
/// The auth middleware rejects API keys whose every session is revoked, so
/// this signs the credential out too.
pub async fn revoke_all_my_sessions(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let user_id = resolve_user_id(&auth);
    state.sessions.revoke_all_for_user(&user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/users/:user_id/sessions — TenantAdmin views any user's
/// active sessions (user-management.md §Session Management: "TenantAdmins
/// can view and revoke any user's sessions").
///
/// Per-handler authorization (ABAC-exempt route): the caller must hold the
/// Admin role (TenantAdmin) AND be scoped to the target user's tenant (see
/// [`ensure_same_tenant_session_target`]). The target user must exist — an
/// unknown user id is a 404, not an empty list, so the endpoint cannot be
/// used to probe which user ids exist.
pub async fn list_user_sessions(
    auth: AuthenticatedAgent,
    Path(user_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "only Admin role may view another user's sessions".to_string(),
        ));
    }
    let target_id = Id::new(user_id);
    let target = state
        .users
        .find_by_id(&target_id)
        .await?
        .ok_or(ApiError::NotFound("User not found".to_string()))?;
    ensure_same_tenant_session_target(&auth, &target)?;
    let now = now_secs();
    let sessions = state.sessions.list_for_user(&target_id).await?;
    let items: Vec<UserSessionResponse> = sessions
        .into_iter()
        .filter(|s| s.is_active(now))
        .map(Into::into)
        .collect();
    Ok(Json(serde_json::json!({ "sessions": items })))
}

/// DELETE /api/v1/users/:user_id/sessions/:id — TenantAdmin revokes one of
/// any user's sessions (user-management.md §Session Management).
///
/// Per-handler authorization (ABAC-exempt route): Admin role required; the
/// caller must be scoped to the target user's tenant (see
/// [`ensure_same_tenant_session_target`]); the session must exist and
/// belong to the path's user (404 otherwise).
pub async fn revoke_user_session(
    auth: AuthenticatedAgent,
    Path((user_id, session_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    if !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "only Admin role may revoke another user's sessions".to_string(),
        ));
    }
    let target_id = Id::new(user_id);
    let target = state
        .users
        .find_by_id(&target_id)
        .await?
        .ok_or(ApiError::NotFound("User not found".to_string()))?;
    ensure_same_tenant_session_target(&auth, &target)?;
    let sid = Id::new(session_id);
    // Confirm the session exists AND belongs to the target user before
    // revoking (a mismatched id is a 404, same as the /me variant).
    let session = state
        .sessions
        .find_by_id(&sid)
        .await?
        .ok_or(ApiError::NotFound("Session not found".to_string()))?;
    if session.user_id != target_id {
        return Err(ApiError::NotFound("Session not found".to_string()));
    }
    state.sessions.revoke(&sid, &target_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/users/:user_id/sessions/revoke-all — TenantAdmin signs any
/// user out everywhere (user-management.md §Session Management).
///
/// Per-handler authorization (ABAC-exempt route): Admin role required; the
/// caller must be scoped to the target user's tenant (see
/// [`ensure_same_tenant_session_target`]); the target user must exist.
pub async fn revoke_all_user_sessions(
    auth: AuthenticatedAgent,
    Path(user_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    if !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "only Admin role may revoke another user's sessions".to_string(),
        ));
    }
    let target_id = Id::new(user_id);
    let target = state
        .users
        .find_by_id(&target_id)
        .await?
        .ok_or(ApiError::NotFound("User not found".to_string()))?;
    ensure_same_tenant_session_target(&auth, &target)?;
    state.sessions.revoke_all_for_user(&target_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Bootstrap: admin user creation ──────────────────────────────────────────

/// Tenant containment for the TenantAdmin session endpoints (per-handler
/// authorization on ABAC-exempt routes, repo invariant: load the scoped
/// entity, compare tenant against the caller, reject on mismatch).
/// `GlobalRole::TenantAdmin` is tenant-scoped (user-management.md §Roles),
/// so an Admin of one tenant must not view or revoke another tenant's
/// user's sessions: the listing hands back ip_address/user_agent/
/// last_active_at and the revokes sign the victim out. Fail-closed when
/// the target user has no tenant binding (there is no tenant to compare,
/// so the operation is skipped, not guessed). The system principal (the
/// global dev token, the first-run bootstrap path before any admin
/// exists) is exempt — same exemption as `create_user` below.
fn ensure_same_tenant_session_target(
    auth: &AuthenticatedAgent,
    target: &User,
) -> Result<(), ApiError> {
    if auth.is_system_principal() {
        return Ok(());
    }
    let same_tenant = target
        .tenant_id
        .as_ref()
        .is_some_and(|t| t.to_string() == auth.tenant_id);
    if !same_tenant {
        return Err(ApiError::Forbidden(format!(
            "cannot access sessions of user {} (caller is scoped to tenant {})",
            target.id, auth.tenant_id
        )));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    /// Tenant this user belongs to (platform-model.md §1). REQUIRED: the
    /// user is scoped at creation; a user without a tenant would force the
    /// auth extractor to fabricate a scope (task-099 F1).
    pub tenant_id: String,
    /// Role names (UserRole variants). Defaults to ["Admin"] when omitted.
    pub roles: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct CreateUserApiKeyResponse {
    /// Plaintext API key - returned exactly once. Only the SHA-256 hash is stored.
    pub key: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct CreateUserResponse {
    pub user: UserProfileResponse,
    pub api_key: CreateUserApiKeyResponse,
}

/// POST /api/v1/users
///
/// Creates a user (bootstrap: the admin user of a new tenant) and mints an
/// API key in the same call so the CLI can authenticate as this user
/// immediately. Admin-only: this is the user-provisioning path for first-run
/// setup; the caller must already hold the Admin role (global dev token or
/// an existing admin's API key).
///
/// The API key is registered in `state.api_keys` (not `user_tokens`) because
/// that is the store the auth extractor consults when resolving API keys.
pub async fn create_user(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<CreateUserResponse>), ApiError> {
    // Admin-only: arbitrary user creation is a provisioning privilege.
    // Enforced here (not in ABAC middleware) so the check holds regardless
    // of route registry state - the global token bypasses ABAC as system,
    // and Developer-role users would pass developer-write-access otherwise.
    if !auth.roles.contains(&UserRole::Admin) {
        crate::rbac::record_rbac_denied(&state, &auth, "Admin", "POST /api/v1/users").await;
        return Err(ApiError::Forbidden(
            "only Admin role may create users".to_string(),
        ));
    }

    // Tenant containment (repo per-handler invariant): an Admin of one
    // tenant must not provision a user bound to another tenant — the
    // response hands back that user's raw API key, so a cross-tenant mint
    // is privilege escalation, not just a data-scope bug. System
    // principals (the global dev token) are exempt: they are the
    // first-run bootstrap path before any admin exists.
    if !auth.is_system_principal() && auth.tenant_id != req.tenant_id {
        return Err(ApiError::Forbidden(format!(
            "cannot provision users for tenant {} (caller is scoped to tenant {})",
            req.tenant_id, auth.tenant_id
        )));
    }

    let username = req.username.trim().to_string();
    // URL-safe handle contract (user-management.md §Username vs Display
    // Name): lowercase letters, digits, '-', '_'; no edges/consecutive
    // separators; 1-64 chars. Rejects rather than silently rewriting the
    // admin's chosen handle.
    User::validate_username(&username).map_err(ApiError::InvalidInput)?;

    // Parse roles; default to Admin (this is the bootstrap admin-user path).
    let mut roles: Vec<UserRole> = Vec::new();
    for r in req.roles.unwrap_or_else(|| vec!["Admin".to_string()]) {
        let parsed = UserRole::from_str(&r)
            .ok_or_else(|| ApiError::InvalidInput(format!("unknown role: {r}")))?;
        if !roles.contains(&parsed) {
            roles.push(parsed);
        }
    }
    if roles.is_empty() {
        return Err(ApiError::InvalidInput(
            "at least one role must be specified".to_string(),
        ));
    }

    // Synthetic external_id: this user is created locally by an admin, not
    // provisioned from OIDC. Stable + namespaced so re-running bootstrap with
    // the same username is detectable via find_by_external_id.
    let external_id = format!("local:{username}");
    if let Some(existing) = state.users.find_by_external_id(&external_id).await? {
        return Err(ApiError::Conflict(format!(
            "user with external id {external_id} already exists (id {})",
            existing.id
        )));
    }
    // Username is unique (spec §Username vs Display Name). The adapter also
    // enforces this, but checking here yields a precise 409 instead of a
    // raw storage error surfacing as 500.
    if let Some(existing) = state.users.find_by_username(&username).await? {
        return Err(ApiError::Conflict(format!(
            "username {username} already taken (user {})",
            existing.id
        )));
    }

    // F1: the user must be bound to a real tenant at creation. Load the
    // tenant so a typo'd or foreign id is rejected here, not silently
    // stored as an unscoped user.
    let tenant = state
        .tenants
        .find_by_id(&Id::new(&req.tenant_id))
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput(format!(
                "tenant '{}' does not exist; create it before provisioning users",
                req.tenant_id
            ))
        })?;

    let now = now_secs();
    let mut user = User::new(new_id(), external_id, username, now);
    user.roles = roles;
    user.global_role = if user.roles.contains(&UserRole::Admin) {
        gyre_domain::GlobalRole::TenantAdmin
    } else {
        gyre_domain::GlobalRole::Member
    };
    user.tenant_id = Some(tenant.id);
    state.users.create(&user).await?;

    // Mint an API key registered in the auth-resolving store. Raw key is
    // generated from a UUID v4 (CSPRNG); only the hash is persisted.
    let raw_key = format!("gyre_{}", uuid::Uuid::new_v4().simple());
    let key_name = "bootstrap";
    state
        .api_keys
        .create(&crate::auth::hash_api_key(&raw_key), &user.id, key_name)
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(CreateUserResponse {
            user: UserProfileResponse::from(user),
            api_key: CreateUserApiKeyResponse {
                key: raw_key,
                name: key_name.to_string(),
            },
        }),
    ))
}

/// DELETE /api/v1/users/me/tokens/:id
pub async fn delete_token(
    auth: AuthenticatedAgent,
    Path(token_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let user_id = resolve_user_id(&auth);
    let tid = Id::new(token_id);
    // find_by_id to confirm existence, then scoped delete.
    state
        .user_tokens
        .find_by_id(&tid)
        .await?
        .ok_or(ApiError::NotFound("Token not found".to_string()))?;
    state.user_tokens.delete(&tid, &user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── HSI §12: Notification Preferences ──────────────────────────────────────

#[derive(Serialize)]
pub struct NotifPrefResponse {
    pub notification_type: String,
    pub enabled: bool,
}

impl From<UserNotificationPreference> for NotifPrefResponse {
    fn from(p: UserNotificationPreference) -> Self {
        Self {
            notification_type: p.notification_type,
            enabled: p.enabled,
        }
    }
}

/// GET /api/v1/users/me/notification-preferences
pub async fn get_notification_preferences(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = resolve_user_id(&auth);
    let prefs = state
        .user_notification_prefs
        .list_for_user(&user_id)
        .await?;
    let items: Vec<NotifPrefResponse> = prefs.into_iter().map(Into::into).collect();
    Ok(Json(serde_json::json!({ "preferences": items })))
}

#[derive(Deserialize)]
pub struct NotifPrefItem {
    pub notification_type: String,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateNotifPrefsRequest {
    pub preferences: Vec<NotifPrefItem>,
}

/// PUT /api/v1/users/me/notification-preferences
pub async fn update_notification_preferences(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateNotifPrefsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = resolve_user_id(&auth);
    let prefs: Vec<UserNotificationPreference> = req
        .preferences
        .into_iter()
        .map(|item| {
            UserNotificationPreference::new(user_id.clone(), item.notification_type, item.enabled)
        })
        .collect();
    state.user_notification_prefs.upsert_batch(&prefs).await?;
    let items: Vec<NotifPrefResponse> = prefs.into_iter().map(Into::into).collect();
    Ok(Json(serde_json::json!({ "preferences": items })))
}

// ─── HSI §12: Judgment Ledger ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct JudgmentParams {
    pub workspace_id: Option<String>,
    #[serde(rename = "type")]
    pub judgment_type: Option<String>,
    pub since: Option<u64>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Serialize)]
pub struct JudgmentEntryResponse {
    pub judgment_type: String,
    pub entity_ref: String,
    pub workspace_id: Option<String>,
    pub timestamp: u64,
    pub detail: Option<String>,
}

/// GET /api/v1/users/me/judgments
pub async fn get_judgments(
    auth: AuthenticatedAgent,
    Query(params): Query<JudgmentParams>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = resolve_user_id(&auth);
    let workspace_id = params.workspace_id.as_deref().map(Id::new);
    let judgment_type = params
        .judgment_type
        .as_deref()
        .and_then(JudgmentType::from_db_str);
    let limit = params.limit.unwrap_or(50).min(200);
    let offset = params.offset.unwrap_or(0);

    let entries = state
        .judgment_ledger
        .list_for_user(
            user_id.as_str(),
            workspace_id.as_ref(),
            judgment_type,
            params.since,
            limit,
            offset,
        )
        .await?;

    let items: Vec<JudgmentEntryResponse> = entries
        .into_iter()
        .map(|e| JudgmentEntryResponse {
            judgment_type: e.judgment_type.as_str().to_string(),
            entity_ref: e.entity_ref,
            workspace_id: e.workspace_id.map(|id| id.to_string()),
            timestamp: e.timestamp,
            detail: e.detail,
        })
        .collect();

    Ok(Json(serde_json::json!({
        "judgments": items,
        "limit": limit,
        "offset": offset,
    })))
}

// ─── Integration tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_common::{Id, Notification, NotificationType};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Helper: seed one notification for user "test-token" (which maps to agent_id="test-token").
    async fn seed_notification(
        state: &std::sync::Arc<crate::AppState>,
        notification_type: NotificationType,
        title: &str,
    ) {
        let now = 1_700_000_000i64;
        let notif = Notification::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new("ws-default"),
            Id::new("system"), // user_id matches the global test auth token (resolves as "system")
            notification_type,
            title,
            "default",
            now,
        );
        state.notifications.create(&notif).await.unwrap();
    }

    #[tokio::test]
    async fn get_my_notifications_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/notifications")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["notifications"].as_array().unwrap().len(), 0);
    }

    /// HSI §12 "What the Profile Is NOT": there is no per-user "My Tasks" /
    /// "My MRs" / "My Agents" surface, so `/users/me/{agents,tasks,mrs}` must not
    /// be routed at all. Regression guard against reintroducing the surface
    /// (task-208).
    #[tokio::test]
    async fn my_stuff_endpoints_are_removed() {
        for uri in [
            "/api/v1/users/me/agents",
            "/api/v1/users/me/tasks",
            "/api/v1/users/me/mrs",
        ] {
            let resp = app()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .header("Authorization", "Bearer test-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "{uri} must be unroutable (HSI §12 — no per-user my-stuff surface)"
            );
        }
    }

    #[tokio::test]
    async fn notification_count_returns_zero_when_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/notifications/count")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["count"], 0);
    }

    #[tokio::test]
    async fn notification_count_reflects_seeded_records() {
        let state = test_state();
        seed_notification(
            &state,
            NotificationType::GateFailure,
            "Gate failed on MR 123",
        )
        .await;
        seed_notification(
            &state,
            NotificationType::AgentCompleted,
            "Agent worker-1 completed",
        )
        .await;

        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/notifications/count")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(
            json["count"], 2,
            "badge count must match seeded notifications"
        );
    }

    #[tokio::test]
    async fn get_my_notifications_lists_seeded_record() {
        let state = test_state();
        seed_notification(
            &state,
            NotificationType::SpecApprovalRequested,
            "Spec needs approval",
        )
        .await;

        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/notifications")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let notifs = json["notifications"].as_array().unwrap();
        assert_eq!(notifs.len(), 1);
        assert_eq!(notifs[0]["notification_type"], "SpecApprovalRequested");
        assert_eq!(notifs[0]["title"], "Spec needs approval");
        // user-management.md §Notification Entity fields are exposed on the
        // list response: four-level priority, structured source, deep link,
        // and read state.
        assert_eq!(notifs[0]["priority"], 2);
        assert_eq!(notifs[0]["priority_level"], "Urgent");
        assert_eq!(notifs[0]["entity_type"], "workspace");
        assert_eq!(notifs[0]["read"], false);
        assert!(notifs[0]["read_at"].is_null());
        assert!(notifs[0]["action_url"].is_null());
    }

    /// POST /api/v1/notifications/:id/read marks the caller's notification
    /// read (stamping read_at); POST .../unread clears both. The read state
    /// is scoped to the owning user exactly like dismiss/resolve
    /// (user-management.md §Notification Entity).
    #[tokio::test]
    async fn mark_notification_read_and_unread_endpoints() {
        let state = test_state();
        let notif_id = Id::new("notif-read-1");
        let notif = Notification::new(
            notif_id.clone(),
            Id::new("ws-default"),
            Id::new("system"), // matches the global test token's resolved user
            NotificationType::GatePassed,
            "All gates passed",
            "default",
            1_700_000_000,
        );
        state.notifications.create(&notif).await.unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/notifications/notif-read-1/read")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let n = state
            .notifications
            .get(&notif_id, &Id::new("system"))
            .await
            .unwrap()
            .unwrap();
        assert!(n.read);
        assert!(n.read_at.is_some(), "read must stamp read_at");

        // Unread clears both — no stale read timestamp.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/notifications/notif-read-1/unread")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let n = state
            .notifications
            .get(&notif_id, &Id::new("system"))
            .await
            .unwrap()
            .unwrap();
        assert!(!n.read);
        assert!(n.read_at.is_none());

        // Another user's notification is invisible: 404, no mutation.
        let other = Notification::new(
            Id::new("notif-read-2"),
            Id::new("ws-default"),
            Id::new("someone-else"),
            NotificationType::GatePassed,
            "Not yours",
            "default",
            1_700_000_000,
        );
        state.notifications.create(&other).await.unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/notifications/notif-read-2/read")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let n = state
            .notifications
            .get(&Id::new("notif-read-2"), &Id::new("someone-else"))
            .await
            .unwrap()
            .unwrap();
        assert!(!n.read, "cross-user mark_read must not mutate");
    }

    #[tokio::test]
    async fn agent_completed_notification_type_round_trips() {
        // Verify AgentCompleted and AgentEscalation types serialize/deserialize correctly.
        let state = test_state();
        seed_notification(&state, NotificationType::AgentCompleted, "Agent done").await;
        seed_notification(&state, NotificationType::AgentEscalation, "Agent escalated").await;

        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/notifications")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let notifs = json["notifications"].as_array().unwrap();
        assert_eq!(notifs.len(), 2);
        let types: Vec<&str> = notifs
            .iter()
            .map(|n| n["notification_type"].as_str().unwrap())
            .collect();
        assert!(
            types.contains(&"AgentCompleted"),
            "AgentCompleted must be present: {types:?}"
        );
        assert!(
            types.contains(&"AgentEscalation"),
            "AgentEscalation must be present: {types:?}"
        );
    }

    #[tokio::test]
    async fn notification_type_filter_returns_only_matching() {
        let state = test_state();
        seed_notification(&state, NotificationType::GateFailure, "Gate failed").await;
        seed_notification(
            &state,
            NotificationType::ConflictingInterpretations,
            "Divergence alert",
        )
        .await;
        seed_notification(
            &state,
            NotificationType::SpecApprovalRequested,
            "Approve spec",
        )
        .await;

        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(
                        "/api/v1/users/me/notifications?notification_type=ConflictingInterpretations",
                    )
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let notifs = json["notifications"].as_array().unwrap();
        assert_eq!(
            notifs.len(),
            1,
            "should return only ConflictingInterpretations: got {notifs:?}"
        );
        assert_eq!(notifs[0]["notification_type"], "ConflictingInterpretations");
    }

    /// Seed a tenant so create_user's tenant validation can resolve it
    /// (task-099 F1: the user is bound to a real tenant at creation).
    async fn seed_tenant(state: &std::sync::Arc<crate::AppState>) {
        let tenant = gyre_domain::Tenant::new(gyre_common::Id::new("t-1"), "Acme", "acme", 0);
        state.tenants.create(&tenant).await.unwrap();
    }

    #[tokio::test]
    async fn admin_creates_user_with_authenticating_api_key() {
        let state = test_state();
        seed_tenant(&state).await;
        let app = crate::api::api_router().with_state(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"username":"jsell","tenant_id":"t-1"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["user"]["username"], "jsell");
        assert_eq!(json["user"]["global_role"], "TenantAdmin");
        // F1: the user is bound to the requested tenant on the wire.
        assert_eq!(json["user"]["tenant_id"], "t-1");
        let user_id = json["user"]["id"].as_str().unwrap().to_string();
        assert!(!user_id.is_empty());

        // The minted API key must authenticate as the new user via the
        // auth extractor's API-key path (state.api_keys hash lookup).
        let raw_key = json["api_key"]["key"].as_str().unwrap().to_string();
        assert!(raw_key.starts_with("gyre_"), "key: {raw_key}");
        let resolved = state
            .api_keys
            .find_user_id(&crate::auth::hash_api_key(&raw_key))
            .await
            .unwrap()
            .expect("API key must resolve to the new user");
        assert_eq!(resolved.to_string(), user_id);
    }

    #[tokio::test]
    async fn api_key_authenticates_through_full_router_with_tenant_binding() {
        // F7: exercise the minted key through the REAL auth extractor (full
        // router, Authorization: Bearer <raw key>), not just the store.
        // Asserts the request authenticates as the new user AND carries the
        // user's tenant binding (F1) — no fabricated "default" scope.
        let state = test_state();
        seed_tenant(&state).await;
        let app = crate::api::api_router().with_state(state.clone());

        // Create the user + key via the API (Admin via global token).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"username":"jsell","tenant_id":"t-1"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let raw_key = json["api_key"]["key"].as_str().unwrap().to_string();
        let user_id = json["user"]["id"].as_str().unwrap().to_string();

        // Now authenticate with the raw key through the full router.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/users/me")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "raw key must authenticate");
        let me = body_json(resp).await;
        assert_eq!(me["id"], user_id.as_str());
        assert_eq!(me["username"], "jsell");
        // The profile carries the tenant binding minted at creation.
        assert_eq!(me["tenant_id"], "t-1");
    }

    #[tokio::test]
    async fn create_user_rejects_unknown_tenant() {
        // F1: a user cannot be provisioned against a tenant that does not
        // exist — that would store an unscoped user.
        let state = test_state();
        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"username":"orphan","tenant_id":"no-such-tenant"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_user_rejects_duplicate_external_id() {
        let state = test_state();
        seed_tenant(&state).await;
        let body = r#"{"username":"dup","tenant_id":"t-1"}"#;
        for i in 0..2 {
            let resp = crate::api::api_router()
                .with_state(state.clone())
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/users")
                        .header("Authorization", "Bearer test-token")
                        .header("Content-Type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            if i == 0 {
                assert_eq!(resp.status(), StatusCode::CREATED);
            } else {
                assert_eq!(
                    resp.status(),
                    StatusCode::CONFLICT,
                    "duplicate external_id must be rejected with 409 Conflict"
                );
            }
        }
    }

    #[tokio::test]
    async fn create_user_rejects_cross_tenant_admin() {
        // Per-handler containment: an Admin API-key user of one tenant must
        // not mint a user (and its raw API key) bound to another tenant —
        // the key in the response makes a cross-tenant mint an escalation
        // path, not just a data-scope bug.
        let (state, _user_id, key) = bootstrap_user("acme-admin").await;
        let beta = gyre_domain::Tenant::new(gyre_common::Id::new("t-2"), "Beta", "beta", 0);
        state.tenants.create(&beta).await.unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users")
                    .header("Authorization", format!("Bearer {key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"username":"beta-infiltrator","tenant_id":"t-2"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "cross-tenant user creation must be rejected with 403"
        );

        // Containment, not a blanket block: the caller's own tenant is the
        // supported path.
        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users")
                    .header("Authorization", format!("Bearer {key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"username":"acme-member","tenant_id":"t-1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "same-tenant user creation by the tenant's own admin must succeed"
        );
    }

    #[tokio::test]
    async fn create_user_rejects_unknown_role() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"username":"x","tenant_id":"t-1","roles":["Superuser"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// Helper: bootstrap a user via POST /api/v1/users (Admin-only) and
    /// return (state, user_id, raw_api_key). The API key authenticates as
    /// the new user, exercising the same auth extractor path production
    /// clients use. The user is bound to seeded tenant t-1 (task-099 F1:
    /// tenant_id is REQUIRED at creation).
    async fn bootstrap_user(username: &str) -> (std::sync::Arc<crate::AppState>, String, String) {
        let state = test_state();
        seed_tenant(&state).await;
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"username":"{username}","tenant_id":"t-1"}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        (
            state,
            json["user"]["id"].as_str().unwrap().to_string(),
            json["api_key"]["key"].as_str().unwrap().to_string(),
        )
    }

    // ── Session Management (user-management.md §Session Management) ──────

    /// Provision a user with an authenticating API key; returns (state, raw key).
    /// Thin wrapper over [`bootstrap_user`]: the session tests below do not
    /// need the user id, only a state + a key that authenticates.
    async fn provision_user_with_key() -> (std::sync::Arc<crate::AppState>, String) {
        let (state, _user_id, raw_key) = bootstrap_user("session-user").await;
        (state, raw_key)
    }

    #[tokio::test]
    async fn sessions_endpoint_lists_session_created_by_auth() {
        let (state, raw_key) = provision_user_with_key().await;

        // Authenticated request through the real extractor — creates the
        // session row for this credential+device.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-cli/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let sessions = json["sessions"].as_array().unwrap();
        assert_eq!(
            sessions.len(),
            1,
            "auth must have created exactly one session: {sessions:?}"
        );
        assert_eq!(sessions[0]["user_agent"], "gyre-cli/1.0");
        // The token hash is never exposed.
        assert!(sessions[0].get("token_hash").is_none());
    }

    #[tokio::test]
    async fn revoke_session_then_credential_is_rejected() {
        let (state, raw_key) = provision_user_with_key().await;

        // Create the session via an authenticated request.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-cli/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let session_id = json["sessions"][0]["id"].as_str().unwrap().to_string();

        // Revoke it.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/users/me/sessions/{session_id}"))
                    // Same User-Agent as the session being revoked: the
                    // revoke request itself authenticates as the same
                    // device (it must not mint a phantom second session).
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-cli/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // The revoked session signs the credential out: auth middleware
        // must reject the API key whose only session was revoked.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-cli/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "revoked session must reject the credential"
        );
    }

    #[tokio::test]
    async fn revoking_one_device_session_does_not_sign_out_other_devices() {
        let (state, raw_key) = provision_user_with_key().await;

        // Two devices authenticate with the same credential.
        let app = crate::api::api_router().with_state(state.clone());
        for ua in ["gyre-cli/1.0", "gyre-web/1.0"] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/users/me/sessions")
                        .header("Authorization", format!("Bearer {raw_key}"))
                        .header("User-Agent", ua)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        // List from the CLI device to find its session id.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-cli/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        let sessions = json["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 2, "two devices = two sessions");
        let cli_session_id = sessions
            .iter()
            .find(|s| s["user_agent"].as_str().unwrap() == "gyre-cli/1.0")
            .map(|s| s["id"].as_str().unwrap().to_string())
            .expect("CLI session must be listed");

        // Revoke only the CLI device's session — from the web device, so
        // the revoking request is not the session being revoked.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/users/me/sessions/{cli_session_id}"))
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-web/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // The revoked device is signed out: its next request is rejected.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-cli/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "revoked device session must be rejected in auth middleware"
        );

        // The other device stays signed in (its session is untouched).
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-web/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "unrevoked sibling device must keep access"
        );
    }

    #[tokio::test]
    async fn revoke_all_sessions_signs_out_everywhere() {
        let (state, raw_key) = provision_user_with_key().await;

        // Create two sessions for the same credential from two devices.
        let app = crate::api::api_router().with_state(state.clone());
        for ua in ["gyre-cli/1.0", "gyre-web/1.0"] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/users/me/sessions")
                        .header("Authorization", format!("Bearer {raw_key}"))
                        .header("User-Agent", ua)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-cli/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        assert_eq!(
            json["sessions"].as_array().unwrap().len(),
            2,
            "two devices = two sessions"
        );

        // Revoke all.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users/me/sessions/revoke-all")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // "Sign out everywhere": the credential is rejected afterwards.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-cli/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "revoke-all must sign the credential out everywhere"
        );
    }

    /// "Sign out everywhere" must cover devices never seen before the
    /// revoke-all: the same credential presented from a NEW device (fresh
    /// User-Agent, no session row of its own) is rejected by the
    /// credential-wide `credential_revoked` check in the auth extractor —
    /// the per-device session check cannot catch it because this device
    /// has no revoked row. This is the stolen-credential scenario: without
    /// the extractor-level check, a fresh device would mint a new session
    /// and receive 200.
    #[tokio::test]
    async fn revoke_all_blocks_credential_from_never_seen_device() {
        let (state, raw_key) = provision_user_with_key().await;

        // One known device authenticates, creating its session row.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-cli/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Sign out everywhere from that device.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users/me/sessions/revoke-all")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-cli/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // A device that never authenticated before the revoke-all presents
        // the same key. It has no session row (revoked or otherwise), so
        // only the credential-wide check can reject it.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "attacker-client/9.9")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "revoke-all must reject the credential itself, including from devices never seen before"
        );
    }

    /// GET /users/me/sessions lists only ACTIVE sessions: a revoked
    /// session and an expired session must not appear, even though both
    /// rows persist in the store (revoked rows are the durable sign-out
    /// record and are never deleted; expired rows await retention
    /// cleanup). Removing the active-only filter from `list_my_sessions`
    /// makes this test fail.
    #[tokio::test]
    async fn my_session_listing_excludes_revoked_and_expired_sessions() {
        let (state, user_id, raw_key) = bootstrap_user("listing-user").await;

        // Two live devices authenticate.
        let app = crate::api::api_router().with_state(state.clone());
        for ua in ["gyre-cli/1.0", "gyre-web/1.0"] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/users/me/sessions")
                        .header("Authorization", format!("Bearer {raw_key}"))
                        .header("User-Agent", ua)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        // A long-expired row for a third device, seeded directly: the row
        // exists in the store but must never surface in the active
        // listing (expiry is not activity).
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expired = gyre_domain::UserSession::new(
            Id::new("sess-listing-expired"),
            Id::new(user_id.clone()),
            "cred-listing-expired",
            "10.3.3.3",
            "gyre-old/1.0",
            now - 7200,
            now - 60,
        );
        state.sessions.create(&expired).await.unwrap();

        // List from the web device: exactly the two live sessions — the
        // expired row is absent.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-web/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let sessions = json["sessions"].as_array().unwrap();
        let ids: Vec<&str> = sessions.iter().map(|s| s["id"].as_str().unwrap()).collect();
        assert_eq!(
            ids.len(),
            2,
            "only the two live sessions may be listed, got {sessions:?}"
        );
        assert!(
            !ids.contains(&"sess-listing-expired"),
            "an expired session must not appear in the active listing"
        );
        let cli_session_id = sessions
            .iter()
            .find(|s| s["user_agent"].as_str().unwrap() == "gyre-cli/1.0")
            .map(|s| s["id"].as_str().unwrap().to_string())
            .expect("CLI session must be listed");

        // Revoke the CLI device's session — from the web device, so the
        // listing request below still authenticates.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/users/me/sessions/{cli_session_id}"))
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-web/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // List again from the web device: the revoked CLI session is gone
        // — only the web device's own session remains.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .header("User-Agent", "gyre-web/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let sessions = json["sessions"].as_array().unwrap();
        assert_eq!(
            sessions.len(),
            1,
            "a revoked session must not appear in the active listing, got {sessions:?}"
        );
        assert_eq!(
            sessions[0]["user_agent"], "gyre-web/1.0",
            "the remaining listed session must be the live web device"
        );
    }

    /// The TenantAdmin listing (GET /users/:user_id/sessions) applies the
    /// same active-only filter: revoked and expired rows of the target
    /// user must not appear. Removing the filter from
    /// `list_user_sessions` makes this test fail.
    #[tokio::test]
    async fn admin_session_listing_excludes_revoked_and_expired_sessions() {
        let (state, user_id, raw_key) = bootstrap_user("admin-listing-user").await;

        // Two live devices authenticate.
        let app = crate::api::api_router().with_state(state.clone());
        for ua in ["gyre-cli/1.0", "gyre-web/1.0"] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/users/me/sessions")
                        .header("Authorization", format!("Bearer {raw_key}"))
                        .header("User-Agent", ua)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        // A long-expired row for a third device.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expired = gyre_domain::UserSession::new(
            Id::new("sess-admin-listing-expired"),
            Id::new(user_id.clone()),
            "cred-admin-listing-expired",
            "10.4.4.4",
            "gyre-old/1.0",
            now - 7200,
            now - 60,
        );
        state.sessions.create(&expired).await.unwrap();

        // Admin lists the target user's sessions: exactly the two live
        // ones; the expired row is absent.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/users/{user_id}/sessions"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let sessions = json["sessions"].as_array().unwrap();
        assert_eq!(
            sessions.len(),
            2,
            "admin listing must show only the two live sessions, got {sessions:?}"
        );
        let ids: Vec<&str> = sessions.iter().map(|s| s["id"].as_str().unwrap()).collect();
        assert!(
            !ids.contains(&"sess-admin-listing-expired"),
            "an expired session must not appear in the admin active listing"
        );
        let cli_session_id = sessions
            .iter()
            .find(|s| s["user_agent"].as_str().unwrap() == "gyre-cli/1.0")
            .map(|s| s["id"].as_str().unwrap().to_string())
            .expect("CLI session must be listed");

        // Admin revokes the CLI device's session, then lists again: the
        // revoked session no longer appears.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/users/{user_id}/sessions/{cli_session_id}"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/users/{user_id}/sessions"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let sessions = json["sessions"].as_array().unwrap();
        assert_eq!(
            sessions.len(),
            1,
            "a revoked session must not appear in the admin active listing, got {sessions:?}"
        );
        assert_eq!(
            sessions[0]["user_agent"], "gyre-web/1.0",
            "the remaining listed session must be the live web device"
        );
    }

    #[tokio::test]
    async fn revoke_cannot_target_another_users_session() {
        let (state, victim_key) = provision_user_with_key().await;

        // Victim authenticates → session created.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {victim_key}"))
                    .header("User-Agent", "victim-agent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        let session_id = json["sessions"][0]["id"].as_str().unwrap().to_string();

        // Attacker: a different user provisioned in the SAME state so both
        // users share one session store (cross-user scoping is the point).
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"username":"attacker","tenant_id":"t-1"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let attacker_key = json["api_key"]["key"].as_str().unwrap().to_string();

        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/users/me/sessions/{session_id}"))
                    .header("Authorization", format!("Bearer {attacker_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "revoking another user's session id must 404, not silently succeed"
        );

        // The victim's session is still active.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {victim_key}"))
                    .header("User-Agent", "victim-agent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(
            json["sessions"].as_array().unwrap().len(),
            1,
            "victim's session must survive the cross-user revoke attempt"
        );
    }

    /// TenantAdmin views and revokes another user's sessions
    /// (user-management.md §Session Management: "TenantAdmins can view
    /// and revoke any user's sessions").
    #[tokio::test]
    async fn tenant_admin_can_view_and_revoke_another_users_sessions() {
        let (state, victim_key) = provision_user_with_key().await;

        // Victim authenticates → session created.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {victim_key}"))
                    .header("User-Agent", "victim-agent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let victim_session_id = json["sessions"][0]["id"].as_str().unwrap().to_string();

        // The victim's user id is needed for the admin path. Create the
        // user with a known username via the bootstrap endpoint, then find
        // it in the store.
        let victim_user_id = state
            .users
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|u| u.username == "session-user")
            .map(|u| u.id.to_string())
            .expect("provisioned victim user must exist");

        // Admin (global test token resolves as Admin) views the victim's
        // sessions through the admin endpoint.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/users/{victim_user_id}/sessions"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "TenantAdmin must be able to view any user's sessions"
        );
        let json = body_json(resp).await;
        let sessions = json["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1, "victim has exactly one session");
        assert_eq!(sessions[0]["id"].as_str().unwrap(), victim_session_id);

        // Admin revokes the victim's session.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/users/{victim_user_id}/sessions/{victim_session_id}"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // The victim's device is signed out: next request rejected.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {victim_key}"))
                    .header("User-Agent", "victim-agent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "admin-revoked session must be rejected in auth middleware"
        );
    }

    /// Non-admin callers cannot use the TenantAdmin session endpoints.
    #[tokio::test]
    async fn non_admin_cannot_view_or_revoke_another_users_sessions() {
        let (state, victim_key) = provision_user_with_key().await;

        // Victim authenticates → session created; find the victim's user id.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {victim_key}"))
                    .header("User-Agent", "victim-agent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let victim_user_id = state
            .users
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|u| u.username == "session-user")
            .map(|u| u.id.to_string())
            .expect("provisioned victim user must exist");

        // Attacker: a non-admin user (roles: ["ReadOnly"]) in the same state.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"username":"attacker","tenant_id":"t-1","roles":["ReadOnly"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let attacker_key = json["api_key"]["key"].as_str().unwrap().to_string();

        // View → Forbidden.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/users/{victim_user_id}/sessions"))
                    .header("Authorization", format!("Bearer {attacker_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "non-admin must not view another user's sessions"
        );

        // Revoke-all → Forbidden.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/users/{victim_user_id}/sessions/revoke-all"
                    ))
                    .header("Authorization", format!("Bearer {attacker_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "non-admin must not revoke another user's sessions"
        );

        // The victim's session survives the forbidden attempts.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {victim_key}"))
                    .header("User-Agent", "victim-agent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// TenantAdmin "sign out everywhere" for another user: revoke-all on
    /// the admin path signs the victim's credential out.
    #[tokio::test]
    async fn tenant_admin_revoke_all_signs_user_out_everywhere() {
        let (state, victim_key) = provision_user_with_key().await;

        // Victim authenticates from two devices.
        let app = crate::api::api_router().with_state(state.clone());
        for ua in ["victim-cli/1.0", "victim-web/1.0"] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/users/me/sessions")
                        .header("Authorization", format!("Bearer {victim_key}"))
                        .header("User-Agent", ua)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        let victim_user_id = state
            .users
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|u| u.username == "session-user")
            .map(|u| u.id.to_string())
            .expect("provisioned victim user must exist");

        // Admin revokes all of the victim's sessions.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/users/{victim_user_id}/sessions/revoke-all"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Both devices are signed out (credential fully revoked).
        for ua in ["victim-cli/1.0", "victim-web/1.0"] {
            let app = crate::api::api_router().with_state(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/users/me/sessions")
                        .header("Authorization", format!("Bearer {victim_key}"))
                        .header("User-Agent", ua)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::UNAUTHORIZED,
                "admin revoke-all must sign every device out ({ua})"
            );
        }
    }

    /// The admin session endpoints must not leak user existence: an
    /// unknown user id is a 404 for admins and a 403 for non-admins.
    #[tokio::test]
    async fn admin_session_endpoints_unknown_user_is_404() {
        let (state, _raw_key) = provision_user_with_key().await;

        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/no-such-user/sessions")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "unknown user id must 404, not return an empty session list"
        );
    }

    /// A TenantAdmin is scoped to their own tenant (user-management.md
    /// §Roles): an Admin-role API-key principal of tenant t-2 must not
    /// view or revoke the sessions of a tenant-t-1 user, while an Admin of
    /// the SAME tenant still can (so the check is tenant containment, not
    /// a blanket rejection of API-key principals).
    #[tokio::test]
    async fn cross_tenant_admin_cannot_view_or_revoke_another_tenants_user_sessions() {
        let (state, victim_user_id, victim_key) = bootstrap_user("session-user").await;
        // Second tenant so the cross-tenant attacker can be provisioned
        // (create_user validates tenant existence, task-099 F1).
        let t2 = gyre_domain::Tenant::new(gyre_common::Id::new("t-2"), "Beta", "beta", 0);
        state.tenants.create(&t2).await.unwrap();

        // Victim authenticates → session created.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {victim_key}"))
                    .header("User-Agent", "victim-agent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let victim_session_id = json["sessions"][0]["id"].as_str().unwrap().to_string();

        // Provision two Admin-role users: one in the victim's tenant t-1,
        // one in a different tenant t-2.
        let app = crate::api::api_router().with_state(state.clone());
        let (same_tenant_key, cross_tenant_key) = {
            let mut keys = Vec::new();
            for (username, tenant) in [("same-admin", "t-1"), ("cross-admin", "t-2")] {
                let resp = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri("/api/v1/users")
                            .header("Authorization", "Bearer test-token")
                            .header("Content-Type", "application/json")
                            .body(Body::from(format!(
                                r#"{{"username":"{username}","tenant_id":"{tenant}"}}"#
                            )))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(resp.status(), StatusCode::CREATED);
                let json = body_json(resp).await;
                assert_eq!(
                    json["user"]["tenant_id"], tenant,
                    "provisioned admin must be bound to the requested tenant"
                );
                keys.push(json["api_key"]["key"].as_str().unwrap().to_string());
            }
            (keys[0].clone(), keys[1].clone())
        };

        // Cross-tenant Admin: view → Forbidden.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/users/{victim_user_id}/sessions"))
                    .header("Authorization", format!("Bearer {cross_tenant_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "Admin of tenant t-2 must not view a tenant-t-1 user's sessions"
        );

        // Cross-tenant Admin: revoke one session → Forbidden.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/users/{victim_user_id}/sessions/{victim_session_id}"
                    ))
                    .header("Authorization", format!("Bearer {cross_tenant_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "Admin of tenant t-2 must not revoke a tenant-t-1 user's session"
        );

        // Cross-tenant Admin: revoke-all → Forbidden.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/users/{victim_user_id}/sessions/revoke-all"
                    ))
                    .header("Authorization", format!("Bearer {cross_tenant_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "Admin of tenant t-2 must not sign a tenant-t-1 user out everywhere"
        );

        // The victim's session survived every forbidden attempt.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/sessions")
                    .header("Authorization", format!("Bearer {victim_key}"))
                    .header("User-Agent", "victim-agent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "victim's session must survive the cross-tenant forbidden attempts"
        );

        // Same-tenant Admin still can (the containment is a tenant
        // comparison, not a rejection of every API-key principal).
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/users/{victim_user_id}/sessions"))
                    .header("Authorization", format!("Bearer {same_tenant_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "Admin of the SAME tenant must still view the user's sessions"
        );
        let json = body_json(resp).await;
        let sessions = json["sessions"].as_array().unwrap();
        assert_eq!(
            sessions.len(),
            1,
            "same-tenant Admin sees the victim's live session"
        );
        assert_eq!(sessions[0]["id"].as_str().unwrap(), victim_session_id);
    }

    #[tokio::test]
    async fn get_me_returns_stored_profile_with_preferences() {
        // user-management.md §User Preferences: preferences are stored
        // server-side and returned via GET /api/v1/users/me.
        let (state, user_id, key) = bootstrap_user("prof-me").await;
        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me")
                    .header("Authorization", format!("Bearer {key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["id"], user_id);
        assert_eq!(json["username"], "prof-me");
        assert_eq!(json["preferences"]["theme"], "System");
        assert_eq!(json["preferences"]["ui_density"], "Comfortable");
        assert_eq!(json["preferences"]["code_font_size"], 14);
        assert_eq!(json["preferences"]["diff_view"], "SideBySide");
        assert_eq!(json["preferences"]["activity_feed_scope"], "MyActivity");
    }

    #[tokio::test]
    async fn put_me_updates_display_name_and_preferences() {
        // user-management.md: PUT /api/v1/users/me accepts display_name,
        // timezone, locale, and preferences updates — persisted server-side
        // (visible on a subsequent GET through a fresh auth round-trip).
        let (state, _user_id, key) = bootstrap_user("put-me").await;
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/users/me")
                    .header("Authorization", format!("Bearer {key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"display_name":"Jordan Sell","timezone":"America/New_York","locale":"en-US","preferences":{"theme":"Dark","ui_density":"Compact","code_font_size":16,"diff_view":"Unified","activity_feed_scope":"All"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["display_name"], "Jordan Sell");
        assert_eq!(json["timezone"], "America/New_York");
        assert_eq!(json["locale"], "en-US");
        assert_eq!(json["preferences"]["theme"], "Dark");
        assert_eq!(json["preferences"]["ui_density"], "Compact");

        // Persistence: re-read through a new request (fresh auth lookups).
        let app2 = crate::api::api_router().with_state(state);
        let resp2 = app2
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me")
                    .header("Authorization", format!("Bearer {key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        let json2 = body_json(resp2).await;
        assert_eq!(json2["display_name"], "Jordan Sell");
        assert_eq!(json2["timezone"], "America/New_York");
        assert_eq!(json2["preferences"]["theme"], "Dark");
        assert_eq!(json2["preferences"]["code_font_size"], 16);
        assert_eq!(json2["username"], "put-me", "username immutable via PUT");

        // Partial preferences payload: omitted fields keep their current
        // value (merge, not replace-with-defaults). The PUT above omitted
        // notification_channels and default_workspace_id entirely.
        assert_eq!(
            json2["preferences"]["notification_channels"],
            serde_json::json!({"in_app": true, "email_enabled": false, "email_digest": "Off"}),
            "omitted notification_channels must keep the stored value, not reset to default"
        );
        assert_eq!(
            json2["preferences"]["default_workspace_id"],
            serde_json::Value::Null,
            "omitted default_workspace_id must keep the stored value"
        );
    }

    #[tokio::test]
    async fn put_me_rejects_malformed_preferences() {
        // An invalid preferences payload must fail the request (400), not
        // be silently dropped while the rest of the update applies.
        let (state, _user_id, key) = bootstrap_user("put-me-bad").await;
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/users/me")
                    .header("Authorization", format!("Bearer {key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"display_name":"Should Not Apply","preferences":{"theme":"Neon","code_font_size":16}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "invalid theme must be rejected"
        );

        // And nothing was applied — display_name unchanged.
        let app2 = crate::api::api_router().with_state(state);
        let resp2 = app2
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me")
                    .header("Authorization", format!("Bearer {key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json2 = body_json(resp2).await;
        assert_eq!(
            json2["display_name"], "put-me-bad",
            "rejected update must not partially apply"
        );
    }

    #[tokio::test]
    async fn create_user_rejects_non_url_safe_username() {
        let state = test_state();
        seed_tenant(&state).await;
        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"username":"Bad Handle!","tenant_id":"t-1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_user_rejects_duplicate_username() {
        // Same username, different external_id namespace collision is
        // impossible via this path (external_id is local:{username}), so
        // seed the conflict directly: create u1, then attempt the same
        // handle through a user whose external_id differs.
        let state = test_state();
        seed_tenant(&state).await;
        let seed = gyre_domain::User::new_sso(
            gyre_common::Id::new("seed-user-1"),
            "ext-seed",
            "taken",
            "Taken Handle",
            1000,
        );
        state.users.create(&seed).await.unwrap();

        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"username":"taken","tenant_id":"t-1"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "duplicate username must be a 409 conflict"
        );
    }
}
