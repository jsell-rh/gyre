//! User management, workspace membership, teams, and notification endpoints (M22.8).
//!
//! GET  /api/v1/users/me
//! PUT  /api/v1/users/me
//! GET  /api/v1/users/me/agents
//! GET  /api/v1/users/me/tasks
//! GET  /api/v1/users/me/mrs
//! GET  /api/v1/users/me/notifications
//! PUT  /api/v1/users/me/notifications/:id/read
//! POST /api/v1/workspaces/:id/members   (invite)
//! GET  /api/v1/workspaces/:id/members
//! PUT  /api/v1/workspaces/:id/members/:user_id
//! DELETE /api/v1/workspaces/:id/members/:user_id
//! POST /api/v1/workspaces/:id/teams
//! GET  /api/v1/workspaces/:id/teams
//! PUT  /api/v1/workspaces/:id/teams/:team_id
//! DELETE /api/v1/workspaces/:id/teams/:team_id

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{
    Notification, NotificationPriority, NotificationType, Team, User, WorkspaceMembership,
    WorkspaceRole,
};
use serde::{Deserialize, Serialize};
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
    let profile = UserProfileResponse {
        id: auth.agent_id.clone(),
        username: auth.agent_id.clone(),
        display_name: auth.agent_id.clone(),
        email: None,
        avatar_url: None,
        timezone: "UTC".to_string(),
        locale: "en".to_string(),
        global_role: "Member".to_string(),
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

pub async fn get_my_agents(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let agents = state.agents.list().await?;
    let my_agents: Vec<_> = agents
        .into_iter()
        .filter(|a| {
            if let Some(uid) = &auth.user_id {
                a.spawned_by
                    .as_ref()
                    .map(|sb| sb == uid.as_str())
                    .unwrap_or(false)
            } else {
                false
            }
        })
        .map(|a| serde_json::json!({"id": a.id.to_string(), "name": a.name, "status": format!("{:?}", a.status)}))
        .collect();
    Ok(Json(serde_json::json!({"agents": my_agents})))
}

pub async fn get_my_tasks(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let tasks = state.tasks.list().await?;
    let my_tasks: Vec<_> = tasks
        .into_iter()
        .filter(|t| {
            if let Some(uid) = &auth.user_id {
                t.assigned_to
                    .as_ref()
                    .map(|at| at.as_str() == uid.as_str())
                    .unwrap_or(false)
            } else {
                false
            }
        })
        .map(|t| {
            serde_json::json!({
                "id": t.id.to_string(),
                "title": t.title,
                "status": format!("{:?}", t.status),
                "priority": format!("{:?}", t.priority),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({"tasks": my_tasks})))
}

pub async fn get_my_mrs(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mrs = state.merge_requests.list().await?;
    let my_mrs: Vec<_> = mrs
        .into_iter()
        .filter(|mr| {
            if let Some(uid) = &auth.user_id {
                mr.author_agent_id
                    .as_ref()
                    .map(|a| a == uid)
                    .unwrap_or(false)
            } else {
                false
            }
        })
        .map(|mr| {
            serde_json::json!({
                "id": mr.id.to_string(),
                "title": mr.title,
                "status": format!("{:?}", mr.status),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({"merge_requests": my_mrs})))
}

// ─── Notifications ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct NotificationParams {
    pub unread: Option<bool>,
}

#[derive(Serialize)]
pub struct NotificationResponse {
    pub id: String,
    pub notification_type: String,
    pub title: String,
    pub body: String,
    pub priority: String,
    pub action_url: Option<String>,
    pub read: bool,
    pub read_at: Option<u64>,
    pub created_at: u64,
}

impl From<Notification> for NotificationResponse {
    fn from(n: Notification) -> Self {
        Self {
            id: n.id.to_string(),
            notification_type: format!("{:?}", n.notification_type),
            title: n.title,
            body: n.body,
            priority: format!("{:?}", n.priority),
            action_url: n.action_url,
            read: n.read,
            read_at: n.read_at,
            created_at: n.created_at,
        }
    }
}

pub async fn get_my_notifications(
    auth: AuthenticatedAgent,
    Query(params): Query<NotificationParams>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = resolve_user_id(&auth);
    let unread_only = params.unread.unwrap_or(false);
    let notifications = state
        .notifications
        .list_by_user(&user_id, unread_only)
        .await?;
    let items: Vec<NotificationResponse> = notifications.into_iter().map(Into::into).collect();
    Ok(Json(serde_json::json!({"notifications": items})))
}

pub async fn mark_notification_read(
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let user_id = resolve_user_id(&auth);
    let notif_id = Id::new(id);
    let notif = state
        .notifications
        .find_by_id(&notif_id)
        .await?
        .ok_or(ApiError::NotFound("Notification not found".to_string()))?;
    if notif.user_id != user_id {
        return Err(ApiError::Forbidden("Not your notification".to_string()));
    }
    state.notifications.mark_read(&notif_id, now_secs()).await?;
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
    _admin: crate::auth::AdminOnly,
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<InviteMemberRequest>,
) -> Result<(StatusCode, Json<MembershipResponse>), ApiError> {
    let role = WorkspaceRole::parse_role(&req.role)
        .ok_or_else(|| ApiError::InvalidInput(format!("Unknown role: {}", req.role)))?;

    let caller_id = Id::new(_admin.agent_id.clone());
    let ws_id = Id::new(workspace_id);
    let user_id = Id::new(req.user_id);
    let now = now_secs();

    let membership = WorkspaceMembership::new(new_id(), user_id, ws_id, role, caller_id, now);
    state.workspace_memberships.create(&membership).await?;

    // Notify the invited user.
    let notif = Notification::new(
        new_id(),
        membership.user_id.clone(),
        NotificationType::InvitationReceived,
        "Workspace invitation",
        format!(
            "You have been invited to workspace {}",
            membership.workspace_id
        ),
        NotificationPriority::Medium,
        now,
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
    _admin: crate::auth::AdminOnly,
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
    _admin: crate::auth::AdminOnly,
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
    _admin: crate::auth::AdminOnly,
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
    _admin: crate::auth::AdminOnly,
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
    _admin: crate::auth::AdminOnly,
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
