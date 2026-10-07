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
    pub preferences: serde_json::Value,
    pub created_at: u64,
    pub updated_at: u64,
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
            preferences: prefs,
            created_at: u.created_at,
            updated_at: u.updated_at,
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
        preferences: serde_json::json!({}),
        created_at: 0,
        updated_at: 0,
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

    let now = now_secs();
    if let Some(dn) = req.display_name {
        user.display_name = dn;
    }
    if let Some(tz) = req.timezone {
        user.timezone = tz;
    }
    if let Some(locale) = req.locale {
        user.locale = locale;
    }
    if let Some(avatar) = req.avatar_url {
        user.avatar_url = Some(avatar);
    }
    if let Some(prefs_json) = req.preferences {
        if let Ok(prefs) = serde_json::from_value(prefs_json) {
            user.preferences = prefs;
        }
    }
    user.updated_at = now;
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
    pub priority: u8,
    pub title: String,
    pub body: Option<String>,
    pub entity_ref: Option<String>,
    pub repo_id: Option<String>,
    pub resolved_at: Option<i64>,
    pub dismissed_at: Option<i64>,
    pub created_at: i64,
}

impl From<Notification> for NotificationResponse {
    fn from(n: Notification) -> Self {
        Self {
            id: n.id.to_string(),
            workspace_id: n.workspace_id.to_string(),
            notification_type: n.notification_type.as_str().to_string(),
            priority: n.priority,
            title: n.title,
            body: n.body,
            entity_ref: n.entity_ref,
            repo_id: n.repo_id,
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

    // Notify the invited user (TrustSuggestion priority 8 — workspace-scope action needed).
    let notif = Notification::new(
        new_id(),
        membership.workspace_id.clone(),
        membership.user_id.clone(),
        NotificationType::TrustSuggestion,
        format!(
            "You have been invited to workspace {}",
            membership.workspace_id
        ),
        auth.tenant_id.clone(),
        now as i64,
    );
    let _ = state.notifications.create(&notif).await;

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

// ─── Bootstrap: admin user creation ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
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
        return Err(ApiError::Forbidden(
            "only Admin role may create users".to_string(),
        ));
    }

    let username = req.username.trim().to_string();
    if username.is_empty() || username.len() > 64 {
        return Err(ApiError::InvalidInput(
            "username must be 1-64 characters".to_string(),
        ));
    }

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
        return Err(ApiError::InvalidInput(format!(
            "user with external id {external_id} already exists (id {})",
            existing.id
        )));
    }

    let now = now_secs();
    let mut user = User::new(new_id(), external_id, username, now);
    user.roles = roles;
    user.global_role = if user.roles.contains(&UserRole::Admin) {
        gyre_domain::GlobalRole::TenantAdmin
    } else {
        gyre_domain::GlobalRole::Member
    };
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
            NotificationType::SpecPendingApproval,
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
        assert_eq!(notifs[0]["notification_type"], "SpecPendingApproval");
        assert_eq!(notifs[0]["title"], "Spec needs approval");
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
            NotificationType::SpecPendingApproval,
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

    #[tokio::test]
    async fn admin_creates_user_with_authenticating_api_key() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"username":"jsell"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["user"]["username"], "jsell");
        assert_eq!(json["user"]["global_role"], "TenantAdmin");
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
    async fn create_user_rejects_duplicate_external_id() {
        let state = test_state();
        let body = r#"{"username":"dup"}"#;
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
                    StatusCode::BAD_REQUEST,
                    "duplicate external_id must be rejected"
                );
            }
        }
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
                    .body(Body::from(r#"{"username":"x","roles":["Superuser"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
