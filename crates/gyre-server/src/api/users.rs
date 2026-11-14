//! User management, workspace membership, teams, and notification endpoints (HSI §2 + §12).
//!
//! POST /api/v1/users                 (bootstrap admin user creation - Admin-only)
//! GET  /api/v1/users/me
//! PUT  /api/v1/users/me
//! GET  /api/v1/users/me/notifications?workspace_id=&min_priority=&max_priority=&limit=&offset=
//! GET  /api/v1/notifications?workspace_id=&repo_id=&type=&priority=&limit=&offset=
//! GET  /api/v1/notifications/unread-count?workspace_id=
//! PUT  /api/v1/notifications/:id/read (POST kept for pre-existing callers)
//! POST /api/v1/notifications/mark-all-read?workspace_id=
//! GET  /api/v1/notifications/preferences
//! PUT  /api/v1/notifications/preferences
//! GET  /api/v1/notifications/stream?workspace_id= (SSE)
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
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream;
use gyre_common::{Id, Notification, NotificationPriority, NotificationType};
use gyre_domain::{
    JudgmentType, Team, TeamMembership, User, UserNotificationPreference, UserRole, UserToken,
    WorkspaceMembership, WorkspaceRole,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;

use crate::{auth::AuthenticatedAgent, AppState};

use super::error::ApiError;
use super::{new_id, now_secs};

/// How long the SSE notification stream waits on the broadcast channel
/// before emitting a heartbeat comment.
const STREAM_HEARTBEAT_SECS: Duration = Duration::from_secs(30);

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
    pub created_at: u64,
    pub updated_at: u64,
    /// OIDC issuer that authenticated this user (HSI §12 auth-provider info).
    /// Null for API-key/legacy provisioned users.
    pub oidc_issuer: Option<String>,
    /// Last authenticated-at (Unix secs) of the user (HSI §12).
    pub last_login_at: Option<u64>,
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
            created_at: u.created_at,
            updated_at: u.updated_at,
            oidc_issuer: u.oidc_issuer.clone(),
            last_login_at: u.last_login_at,
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
        created_at: 0,
        updated_at: 0,
        oidc_issuer: None,
        last_login_at: None,
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
        // The reserved system ABAC identity (task-129 review round 3):
        // API-key/JWT auth resolves the caller's ABAC subject.id from
        // display_name, and `system-full-access` (priority 1000) matches
        // that identity — accepting it here would let any user grant
        // themselves full access by renaming.
        if dn == gyre_domain::SYSTEM_ABAC_IDENTITY {
            return Err(ApiError::InvalidInput(format!(
                "display_name '{}' is reserved for the system principal",
                gyre_domain::SYSTEM_ABAC_IDENTITY
            )));
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
    let mut patched_channels: Option<gyre_domain::NotificationChannels> = None;
    if let Some(prefs_json) = req.preferences {
        // Partial-update semantics: omitted preference fields keep their
        // current value (matching the Option-based top-level fields). An
        // invalid value for a present field still fails the whole request.
        let patch: gyre_domain::UserPreferencesPatch = serde_json::from_value(prefs_json)
            .map_err(|e| ApiError::InvalidInput(format!("invalid preferences: {e}")))?;
        // A `notification_channels` value carried by this endpoint is the
        // SAME configuration the dedicated PUT /notifications/preferences
        // accepts — validate it identically and remember it so the delivery
        // store can be converged below (single effective config, review
        // bf1e11be F1: previously this field persisted to users.preferences
        // only, and delivery read user_channel_prefs only, so the config
        // silently never fired).
        if let Some(channels) = &patch.notification_channels {
            validate_channel_config(channels)?;
            patched_channels = Some(channels.clone());
        }
        user.preferences.apply_patch(patch);
    }
    user.updated_at = now_secs();
    state.users.update(&user).await?;
    if let Some(channels) = patched_channels {
        // Converge the delivery store to the same value the profile store
        // just persisted — a config set via PUT /users/me must reach the
        // dispatcher (which reads user_channel_prefs).
        state.user_channel_prefs.upsert(user_id, &channels).await?;
    }
    Ok(Json(UserProfileResponse::from(user)))
}

// ─── Notifications ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct NotificationParams {
    pub workspace_id: Option<String>,
    pub min_priority: Option<u8>,
    pub max_priority: Option<u8>,
    /// Filter by taxonomy type. Query key `type` per the spec's filter
    /// surface (`?type=`); `notification_type` kept as an alias for the
    /// pre-existing `/users/me/notifications` clients.
    #[serde(rename = "type", alias = "notification_type")]
    pub notification_type: Option<String>,
    /// Repo filter (user-management.md §In-App Notification UI: filter by
    /// type, priority, workspace, repo). Matches the notification's source
    /// repo.
    pub repo_id: Option<String>,
    /// Four-level priority name (Low/Medium/High/Urgent), case-insensitive —
    /// widens to the HSI §8 rank band it covers.
    pub priority: Option<String>,
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
    list_my_notifications_impl(user_id, params, state).await
}

/// GET /api/v1/notifications?workspace_id=&repo_id=&type=&priority=&limit=&offset=
///
/// Spec path (user-management.md §API > Notifications: "My notifications
/// (paginated, filterable)"). Same per-handler scoping as the legacy
/// `/users/me/notifications` route — the listing is keyed to the caller's
/// user identity, so no ABAC resource type applies.
pub async fn list_my_notifications(
    auth: AuthenticatedAgent,
    Query(params): Query<NotificationParams>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = resolve_user_id(&auth);
    list_my_notifications_impl(user_id, params, state).await
}

/// Shared listing implementation behind the spec path
/// (`GET /api/v1/notifications`) and the legacy `/users/me/notifications`.
/// Both routes resolve the caller's user identity in-handler and pass it
/// here — the listing is scoped to that identity.
async fn list_my_notifications_impl(
    user_id: Id,
    params: NotificationParams,
    state: Arc<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let workspace_id = params.workspace_id.as_deref().map(Id::new);
    let repo_id = params.repo_id.as_deref().map(Id::new);
    // `?priority=` takes the four-level name (Low/Medium/High/Urgent,
    // case-insensitive) and widens to the HSI §8 rank band it covers so the
    // stored rank column can serve the filter (user-management.md
    // §Notification Entity: the level is the band of the rank).
    let (band_min, band_max) = match params.priority.as_deref() {
        Some(level) => {
            let normalized = level.trim().to_ascii_lowercase();
            let parsed = NotificationPriority::from_level_str(&normalized).ok_or_else(|| {
                ApiError::InvalidInput(format!(
                    "invalid priority filter '{level}': expected one of Low, Medium, High, Urgent"
                ))
            })?;
            let band = parsed.rank_band();
            (Some(band.0), Some(band.1))
        }
        None => (None, None),
    };
    let min_priority = params.min_priority.or(band_min);
    let max_priority = params.max_priority.or(band_max);
    let limit = params.limit.unwrap_or(50).min(200);
    let offset = params.offset.unwrap_or(0);
    // HSI §12: types the user disabled via notification preferences are
    // excluded from the inbox (before limit/offset).
    let disabled = disabled_notification_types(&state, &user_id).await?;
    let disabled: Vec<&str> = disabled.iter().map(String::as_str).collect();
    let notifications = state
        .notifications
        .list_for_user(
            &user_id,
            workspace_id.as_ref(),
            min_priority,
            max_priority,
            params.notification_type.as_deref(),
            repo_id.as_ref(),
            &disabled,
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
    // Same preference exclusion as the inbox list, so the badge matches it.
    let disabled = disabled_notification_types(&state, &user_id).await?;
    let disabled: Vec<&str> = disabled.iter().map(String::as_str).collect();
    let count = state
        .notifications
        .count_unresolved(&user_id, workspace_id.as_ref(), &disabled)
        .await?;
    Ok(Json(serde_json::json!({ "count": count })))
}

/// GET /api/v1/notifications/unread-count?workspace_id=
///
/// Spec path (user-management.md §API > Notifications: "Unread count (for
/// bell badge)"). Counts active (unresolved, undismissed) notifications the
/// caller has not read. Scoped to the caller's user identity — no ABAC
/// resource type applies.
pub async fn get_unread_count(
    auth: AuthenticatedAgent,
    Query(params): Query<NotificationParams>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = resolve_user_id(&auth);
    let workspace_id = params.workspace_id.as_deref().map(Id::new);
    let count = state
        .notifications
        .count_unread(&user_id, workspace_id.as_ref())
        .await?;
    Ok(Json(serde_json::json!({ "count": count })))
}

/// POST /api/v1/notifications/mark-all-read?workspace_id=
///
/// Spec path (user-management.md §API > Notifications: "Mark all read").
/// Marks every unread notification owned by the caller read (stamping
/// `read_at`); an optional `workspace_id` narrows the sweep. Scoped to the
/// caller's user identity — no ABAC resource type applies.
pub async fn mark_all_read_notifications(
    auth: AuthenticatedAgent,
    Query(params): Query<NotificationParams>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = resolve_user_id(&auth);
    let workspace_id = params.workspace_id.as_deref().map(Id::new);
    let marked = state
        .notifications
        .mark_all_read(&user_id, workspace_id.as_ref())
        .await?;
    Ok(Json(serde_json::json!({ "marked": marked })))
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

/// PUT/POST /api/v1/notifications/:id/read
///
/// Marks the notification read (stamps `read_at`). Per-handler auth: verifies
/// notification belongs to caller AND caller.tenant_id matches
/// (user-management.md §Notification Entity read state; §API > Notifications
/// specifies PUT — POST is kept for pre-existing callers of the same
/// handler).
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

/// GET /api/v1/notifications/stream?workspace_id=
///
/// Real-time notification stream (user-management.md §API > Notifications:
/// "GET (SSE)"). Subscribes to the notification broadcast channel fed by
/// `AppState::create_notification` — every notification is emitted the
/// moment it is persisted. Per-handler auth (ABAC-exempt, like every other
/// notification route): each event is filtered against the caller — the
/// notification must belong to the caller's user identity AND its tenant
/// must match `auth.tenant_id`, otherwise it is dropped before it reaches
/// the wire. An optional `workspace_id` narrows delivery to one workspace.
pub async fn notification_stream(
    auth: AuthenticatedAgent,
    Query(params): Query<NotificationParams>,
    State(state): State<Arc<AppState>>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let user_id = resolve_user_id(&auth);
    let tenant_id = auth.tenant_id.clone();
    let workspace_filter = params.workspace_id.map(Id::new);
    let rx = state.notification_broadcast_tx.subscribe();
    let s = stream::unfold(
        (rx, user_id, tenant_id, workspace_filter),
        |(mut rx, user_id, tenant_id, workspace_filter)| async move {
            loop {
                // Poll with a heartbeat timeout so the connection stays alive
                // even when nothing is being created.
                let result = tokio::time::timeout(STREAM_HEARTBEAT_SECS, rx.recv()).await;
                let msg = match result {
                    Ok(Ok(msg)) => msg,
                    // Lagged: this subscriber fell behind the channel buffer;
                    // the channel is still live and the missed messages are
                    // simply gone — keep streaming rather than terminating
                    // the client's stream.
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                        continue;
                    }
                    // Closed: every sender (AppState) is gone — end the stream.
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => return None,
                    Err(_) => {
                        // Timeout - send a heartbeat comment.
                        return Some((
                            Ok(Event::default().comment("heartbeat")),
                            (rx, user_id, tenant_id, workspace_filter),
                        ));
                    }
                };
                // Per-handler scoping: parse the broadcast notification and
                // deliver only if it belongs to the caller (user + tenant),
                // and — when requested — the workspace. Unparseable payloads
                // and other users' notifications are silently skipped; the
                // stream itself never terminates on them.
                let Ok(notif) = serde_json::from_str::<Notification>(&msg) else {
                    continue;
                };
                if notif.user_id != user_id || notif.tenant_id != tenant_id {
                    continue;
                }
                if workspace_filter
                    .as_ref()
                    .is_some_and(|ws| notif.workspace_id != *ws)
                {
                    continue;
                }
                let data =
                    serde_json::to_string(&NotificationResponse::from(notif)).unwrap_or_default();
                if data.is_empty() {
                    continue;
                }
                return Some((
                    Ok(Event::default().event("notification").data(data)),
                    (rx, user_id, tenant_id, workspace_filter),
                ));
            }
        },
    );
    Sse::new(s).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

// ─── Workspace Members ───────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct MembershipResponse {
    pub id: String,
    pub user_id: String,
    pub workspace_id: String,
    pub role: String,
    pub accepted: bool,
    pub accepted_at: Option<u64>,
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
            accepted_at: m.accepted_at,
            created_at: m.created_at,
        }
    }
}

#[derive(Deserialize)]
pub struct InviteMemberRequest {
    pub user_id: String,
    pub role: String,
}

/// Load a workspace and enforce the caller's tenant scope on it
/// (user-management.md §Workspace Membership: tenants are hard isolation
/// boundaries — cross-tenant sharing is not supported).
///
/// The system principal (bootstrap token) bypasses the tenant match, mirroring
/// `resolve_repo_by_slug` in git_http.rs: it is the principal that created the
/// tenant/workspace hierarchy in the first place.
async fn load_workspace_scoped(
    state: &AppState,
    auth: &AuthenticatedAgent,
    workspace_id: &str,
) -> Result<gyre_domain::Workspace, ApiError> {
    let ws = state
        .workspaces
        .find_by_id(&Id::new(workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {workspace_id} not found")))?;
    if !auth.is_system_principal() && ws.tenant_id.to_string() != auth.tenant_id {
        return Err(ApiError::Forbidden(
            "cross-tenant workspace access denied".to_string(),
        ));
    }
    Ok(ws)
}

/// Whether the caller may manage membership of this workspace: the system
/// principal, a global TenantAdmin, or a workspace Owner/Admin member
/// (user-management.md §Workspace Membership — "Manage members" is an
/// Owner/Admin capability; §Ownership Transfer & Reclamation lets the
/// TenantAdmin reassign ownership).
async fn can_manage_membership(
    state: &AppState,
    auth: &AuthenticatedAgent,
    ws: &gyre_domain::Workspace,
) -> Result<bool, ApiError> {
    if auth.is_system_principal() || auth.roles.contains(&UserRole::Admin) {
        return Ok(true);
    }
    // A TenantAdmin's authority comes from their stored global role; JWT
    // role claims can lag a demotion, so the stored record is authoritative
    // when resolvable.
    if let Some(uid) = &auth.user_id {
        if let Ok(Some(user)) = state.users.find_by_id(uid).await {
            if user.global_role == gyre_domain::GlobalRole::TenantAdmin {
                return Ok(true);
            }
        }
    }
    let Some(uid) = &auth.user_id else {
        return Ok(false);
    };
    Ok(state
        .workspace_memberships
        .find_by_user_and_workspace(uid, &ws.id)
        .await?
        .map(|m| m.role.can_manage_members())
        .unwrap_or(false))
}

/// Count active Owner memberships of a workspace, optionally excluding one
/// user (the member being removed/downgraded). An Owner membership is
/// active when its user record still exists — a deactivated/removed user's
/// lingering membership is not an active Owner.
async fn count_active_owners_excluding(
    state: &AppState,
    workspace_id: &Id,
    exclude_user: Option<&Id>,
) -> Result<usize, ApiError> {
    let members = state
        .workspace_memberships
        .list_by_workspace(workspace_id)
        .await?;
    let mut owners = 0usize;
    for m in &members {
        if m.role != WorkspaceRole::Owner {
            continue;
        }
        if Some(&m.user_id) == exclude_user {
            continue;
        }
        if let Ok(Some(_)) = state.users.find_by_id(&m.user_id).await {
            owners += 1;
        }
    }
    Ok(owners)
}

pub async fn invite_member(
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<InviteMemberRequest>,
) -> Result<(StatusCode, Json<MembershipResponse>), ApiError> {
    let role = WorkspaceRole::parse_role(&req.role)
        .ok_or_else(|| ApiError::InvalidInput(format!("Unknown role: {}", req.role)))?;

    let ws = load_workspace_scoped(&state, &auth, &workspace_id).await?;
    if !can_manage_membership(&state, &auth, &ws).await? {
        return Err(ApiError::Forbidden(
            "workspace membership can only be granted by an Owner, Admin, or TenantAdmin"
                .to_string(),
        ));
    }

    let caller_id = Id::new(auth.agent_id.clone());
    let ws_id = ws.id.clone();
    let user_id = Id::new(req.user_id);
    let now = now_secs();

    // Cross-tenant sharing is not supported (user-management.md §Workspace
    // Membership): the invitee must already exist in the workspace's tenant.
    let invitee = state.users.find_by_id(&user_id).await?.ok_or_else(|| {
        ApiError::InvalidInput(format!(
            "user {user_id} does not exist in this tenant; users must exist before being \
                 invited to a workspace"
        ))
    })?;
    if invitee.tenant_id.as_ref() != Some(&ws.tenant_id) {
        return Err(ApiError::Forbidden(
            "cross-tenant workspace sharing is not supported".to_string(),
        ));
    }

    if state
        .workspace_memberships
        .find_by_user_and_workspace(&user_id, &ws_id)
        .await?
        .is_some()
    {
        return Err(ApiError::Conflict(format!(
            "user {user_id} is already a member of workspace {workspace_id}"
        )));
    }

    let membership = WorkspaceMembership::new(new_id(), user_id, ws_id, role, caller_id, now);
    state.workspace_memberships.create(&membership).await?;

    // Notify the invited user (user-management.md §Notification Entity:
    // InvitationReceived, Low — informational). Routed through notify_rich
    // (the codebase's notification helper): a notification is a courtesy
    // side-effect, not a dependent record of the membership — its failure
    // must not fail the invite, and notify_rich logs-and-continues internally.
    crate::notifications::notify_rich(
        state.as_ref(),
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
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ws = load_workspace_scoped(&state, &auth, &workspace_id).await?;
    let members = state
        .workspace_memberships
        .list_by_workspace(&ws.id)
        .await?;
    let items: Vec<MembershipResponse> = members.into_iter().map(Into::into).collect();
    Ok(Json(serde_json::json!({"members": items})))
}

#[derive(Deserialize)]
pub struct UpdateMemberRoleRequest {
    pub role: String,
    /// Optional reason recorded in the audit trail. Required semantics per
    /// user-management.md §Ownership Transfer & Reclamation: the transfer is
    /// audit-logged with the previous owner and reason.
    pub reason: Option<String>,
}

/// Record a workspace membership audit event (observability.md §Audit Event
/// Schema; event-specific data in `detail`).
async fn emit_membership_audit(
    state: &AppState,
    auth: &AuthenticatedAgent,
    ws: &gyre_domain::Workspace,
    event_type: &str,
    detail: serde_json::Value,
) {
    use gyre_domain::audit::{AuditEvent, AuditEventType, AuditOutcome};
    let event = AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        AuditEventType::Custom(event_type.to_string()),
        Some(Id::new(auth.agent_id.clone())),
        auth.user_id.clone(),
        None,
        Some(ws.id.clone()),
        None,
        "workspace_member".to_string(),
        Some(ws.id.to_string()),
        AuditOutcome::Success,
        detail,
        None,
        None,
        now_secs(),
    );
    if let Err(e) = state.audit.record(&event).await {
        tracing::warn!(
            workspace_id = %ws.id,
            event_type,
            "membership audit event recording failed: {e}"
        );
    }
}

/// PUT /api/v1/workspaces/:id/members/:user_id — change a member's role,
/// including ownership transfer and TenantAdmin reassignment
/// (user-management.md §Ownership Transfer & Reclamation).
pub async fn update_member_role(
    auth: AuthenticatedAgent,
    Path((workspace_id, user_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateMemberRoleRequest>,
) -> Result<StatusCode, ApiError> {
    let new_role = WorkspaceRole::parse_role(&req.role)
        .ok_or_else(|| ApiError::InvalidInput(format!("Unknown role: {}", req.role)))?;
    let ws = load_workspace_scoped(&state, &auth, &workspace_id).await?;
    if !can_manage_membership(&state, &auth, &ws).await? {
        return Err(ApiError::Forbidden(
            "member roles can only be changed by a workspace Owner, Admin, or TenantAdmin"
                .to_string(),
        ));
    }
    let uid = Id::new(user_id);
    let membership = state
        .workspace_memberships
        .find_by_user_and_workspace(&uid, &ws.id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Membership not found".to_string()))?;

    // Last-Owner protection (user-management.md §Ownership Transfer &
    // Reclamation: "Cannot remove last Owner"; a voluntary transfer must
    // designate a new Owner before downgrading).
    if membership.role == WorkspaceRole::Owner && new_role != WorkspaceRole::Owner {
        let others = count_active_owners_excluding(&state, &ws.id, Some(&uid)).await?;
        if others == 0 {
            return Err(ApiError::Conflict(
                "cannot downgrade the last Owner of a workspace; designate a new Owner first \
                 (PUT /api/v1/workspaces/{id}/members/{new_owner_id}/role with {\"role\": \"Owner\"})"
                    .to_string(),
            ));
        }
    }

    let previous_role = membership.role;
    state
        .workspace_memberships
        .update_role(&membership.id, new_role)
        .await?;

    // Audit trail. An ownership transfer (either direction involving the
    // Owner role) is logged with the previous owner and the reason
    // (user-management.md §Ownership Transfer & Reclamation).
    let involves_owner = previous_role == WorkspaceRole::Owner || new_role == WorkspaceRole::Owner;
    let event_type = if involves_owner {
        "workspace_ownership_transferred"
    } else {
        "workspace_member_role_changed"
    };
    let detail = serde_json::json!({
        "workspace_id": ws.id.to_string(),
        "target_user_id": uid.to_string(),
        "previous_role": previous_role.as_str(),
        "new_role": new_role.as_str(),
        "previous_owner": if previous_role == WorkspaceRole::Owner {
            serde_json::json!(uid.to_string())
        } else {
            serde_json::Value::Null
        },
        "reason": req.reason,
        "actor": auth.agent_id,
    });
    emit_membership_audit(&state, &auth, &ws, event_type, detail).await;

    // Ownership (re)assignment reclaims an ownerless/orphaned workspace:
    // clear the tracking record so it stops being flagged immediately.
    if new_role == WorkspaceRole::Owner {
        crate::workspace_ownership::clear_ownerless(&state, &ws.id).await;
    }

    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/workspaces/:id/members/:user_id — remove a member.
pub async fn remove_member(
    auth: AuthenticatedAgent,
    Path((workspace_id, user_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let ws = load_workspace_scoped(&state, &auth, &workspace_id).await?;
    if !can_manage_membership(&state, &auth, &ws).await? {
        return Err(ApiError::Forbidden(
            "members can only be removed by a workspace Owner, Admin, or TenantAdmin".to_string(),
        ));
    }
    let uid = Id::new(user_id);
    let membership = state
        .workspace_memberships
        .find_by_user_and_workspace(&uid, &ws.id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Membership not found".to_string()))?;

    // Last-Owner protection: "Cannot remove themselves as Owner if they're
    // the last Owner" (user-management.md §Ownership Transfer & Reclamation).
    if membership.role == WorkspaceRole::Owner {
        let others = count_active_owners_excluding(&state, &ws.id, Some(&uid)).await?;
        if others == 0 {
            return Err(ApiError::Conflict(
                "cannot remove the last Owner of a workspace; designate a new Owner first \
                 (PUT /api/v1/workspaces/{id}/members/{new_owner_id}/role with {\"role\": \"Owner\"})"
                    .to_string(),
            ));
        }
    }

    state.workspace_memberships.delete(&membership.id).await?;

    emit_membership_audit(
        &state,
        &auth,
        &ws,
        "workspace_member_removed",
        serde_json::json!({
            "workspace_id": ws.id.to_string(),
            "removed_user_id": uid.to_string(),
            "removed_role": membership.role.as_str(),
            "actor": auth.agent_id,
        }),
    )
    .await;

    // Notify the removed user (user-management.md §Notification Entity:
    // MembershipChanged — Low, informational). Best-effort: the membership is
    // already deleted; notification failure must not fail the removal. The
    // tenant scope comes from the workspace already loaded and authorized
    // above — no re-fetch and no fabricated scope needed.
    crate::notifications::notify_rich(
        state.as_ref(),
        ws.id.clone(),
        uid.clone(),
        NotificationType::MembershipChanged,
        format!("Your membership in workspace '{}' was removed", ws.name),
        ws.tenant_id.to_string(),
        Some(
            serde_json::json!({
                "workspace_id": ws.id.as_str(),
                "user_id": uid.as_str(),
            })
            .to_string(),
        ),
        Some(ws.id.to_string()),
        None,
        "workspace",
        None,
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

// ─── Teams ───────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TeamResponse {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub lead: String,
    pub member_ids: Vec<String>,
    pub created_at: u64,
}

impl TeamResponse {
    /// Build the response for a team, joining its memberships so
    /// `member_ids` reflects the real `team_memberships` table.
    async fn from_team(state: &AppState, t: Team) -> Result<Self, ApiError> {
        let member_ids = state
            .teams
            .list_members(&t.id)
            .await?
            .into_iter()
            .map(|m| m.user_id.to_string())
            .collect();
        Ok(Self {
            id: t.id.to_string(),
            workspace_id: t.workspace_id.to_string(),
            name: t.name,
            slug: t.slug,
            description: t.description,
            lead: t.lead.to_string(),
            member_ids,
            created_at: t.created_at,
        })
    }
}

#[derive(Deserialize)]
pub struct CreateTeamRequest {
    pub name: String,
    /// Optional explicit slug; derived from the name when omitted.
    #[serde(default)]
    pub slug: Option<String>,
    pub description: Option<String>,
    /// User responsible for the team (a workspace member).
    pub lead: String,
}

/// Resolve the slug for a create/update: an explicit slug must be URL-safe
/// (rejected otherwise — never silently rewritten); an omitted slug is
/// derived from the team name.
fn resolve_team_slug(explicit: Option<&str>, name: &str) -> Result<String, ApiError> {
    let slug = match explicit {
        Some(s) => {
            let s = s.trim();
            Team::validate_slug(s)
                .map_err(|e| ApiError::InvalidInput(format!("invalid team slug: {e}")))?;
            s.to_string()
        }
        None => Team::sanitize_slug(name).ok_or_else(|| {
            ApiError::InvalidInput(
                "team name yields no URL-safe slug; provide an explicit 'slug' field".to_string(),
            )
        })?,
    };
    Ok(slug)
}

/// Validate that `lead` is a member of the workspace (user-management.md §
/// Team Management: the lead is "User responsible for the team"; teams never
/// grant permissions beyond workspace membership, so the lead must already
/// be a workspace member).
async fn validate_team_lead(
    state: &AppState,
    ws: &gyre_domain::Workspace,
    lead: &str,
) -> Result<Id, ApiError> {
    let lead_id = Id::new(lead.to_string());
    let membership = state
        .workspace_memberships
        .find_by_user_and_workspace(&lead_id, &ws.id)
        .await?;
    if membership.is_none() {
        return Err(ApiError::InvalidInput(format!(
            "team lead {lead} is not a member of workspace {}",
            ws.id.as_str()
        )));
    }
    Ok(lead_id)
}

/// Reject a slug that another team in the workspace already owns (the
/// repository unique index is the race-safe backstop; this produces the
/// clean 409).
async fn reject_duplicate_slug(
    state: &AppState,
    ws_id: &Id,
    slug: &str,
    exclude_team: Option<&Id>,
) -> Result<(), ApiError> {
    if let Some(existing) = state.teams.find_by_slug(ws_id, slug).await? {
        if Some(&existing.id) != exclude_team {
            return Err(ApiError::Conflict(format!(
                "team slug '{slug}' already exists in this workspace"
            )));
        }
    }
    Ok(())
}

pub async fn create_team(
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTeamRequest>,
) -> Result<(StatusCode, Json<TeamResponse>), ApiError> {
    let ws = load_workspace_scoped(&state, &auth, &workspace_id).await?;
    if !can_manage_membership(&state, &auth, &ws).await? {
        return Err(ApiError::Forbidden(
            "teams can only be managed by an Owner, Admin, or TenantAdmin".to_string(),
        ));
    }

    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::InvalidInput(
            "team name must not be empty".to_string(),
        ));
    }
    let slug = resolve_team_slug(req.slug.as_deref(), &name)?;
    reject_duplicate_slug(&state, &ws.id, &slug, None).await?;
    let lead = validate_team_lead(&state, &ws, req.lead.trim()).await?;

    let mut team = Team::new(new_id(), ws.id.clone(), name, slug, lead, now_secs());
    team.description = req.description;
    state.teams.create(&team).await?;
    Ok((
        StatusCode::CREATED,
        Json(TeamResponse::from_team(&state, team).await?),
    ))
}

pub async fn list_teams(
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ws = load_workspace_scoped(&state, &auth, &workspace_id).await?;
    let teams = state.teams.list_by_workspace(&ws.id).await?;
    let mut items = Vec::with_capacity(teams.len());
    for team in teams {
        items.push(TeamResponse::from_team(&state, team).await?);
    }
    Ok(Json(serde_json::json!({"teams": items})))
}

#[derive(Deserialize)]
pub struct UpdateTeamRequest {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub lead: Option<String>,
}

pub async fn update_team(
    auth: AuthenticatedAgent,
    Path((workspace_id, team_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateTeamRequest>,
) -> Result<Json<TeamResponse>, ApiError> {
    let ws = load_workspace_scoped(&state, &auth, &workspace_id).await?;
    if !can_manage_membership(&state, &auth, &ws).await? {
        return Err(ApiError::Forbidden(
            "teams can only be managed by an Owner, Admin, or TenantAdmin".to_string(),
        ));
    }
    let tid = Id::new(team_id);
    let mut team = state
        .teams
        .find_by_id(&tid)
        .await?
        .ok_or(ApiError::NotFound("Team not found".to_string()))?;
    // Per-handler scope check: the team must belong to the path's workspace
    // (the path workspace is caller-scoped; a team from another workspace
    // is a 404, not a cross-workspace write).
    if team.workspace_id != ws.id {
        return Err(ApiError::NotFound("Team not found".to_string()));
    }

    if let Some(name) = req.name {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(ApiError::InvalidInput(
                "team name must not be empty".to_string(),
            ));
        }
        team.name = name;
    }
    if let Some(slug) = req.slug {
        let slug = resolve_team_slug(Some(slug.trim()), &team.name)?;
        reject_duplicate_slug(&state, &ws.id, &slug, Some(&team.id)).await?;
        team.slug = slug;
    }
    if let Some(desc) = req.description {
        team.description = Some(desc);
    }
    if let Some(lead) = req.lead {
        team.lead = validate_team_lead(&state, &ws, lead.trim()).await?;
    }

    state.teams.update(&team).await?;
    Ok(Json(TeamResponse::from_team(&state, team).await?))
}

pub async fn delete_team(
    auth: AuthenticatedAgent,
    Path((workspace_id, team_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let ws = load_workspace_scoped(&state, &auth, &workspace_id).await?;
    if !can_manage_membership(&state, &auth, &ws).await? {
        return Err(ApiError::Forbidden(
            "teams can only be managed by an Owner, Admin, or TenantAdmin".to_string(),
        ));
    }
    let tid = Id::new(team_id);
    let team = state
        .teams
        .find_by_id(&tid)
        .await?
        .ok_or(ApiError::NotFound("Team not found".to_string()))?;
    if team.workspace_id != ws.id {
        return Err(ApiError::NotFound("Team not found".to_string()));
    }
    state.teams.delete(&tid).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct AddTeamMemberRequest {
    pub user_id: String,
}

#[derive(Serialize)]
pub struct TeamMembershipResponse {
    pub team_id: String,
    pub user_id: String,
    pub added_at: u64,
}

/// POST /api/v1/workspaces/:id/teams/:team_id/members — add a member to a
/// team (user-management.md §API > Teams). The user must already be a
/// workspace member — teams are organizational and never expand access
/// beyond workspace membership.
pub async fn add_team_member(
    auth: AuthenticatedAgent,
    Path((workspace_id, team_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddTeamMemberRequest>,
) -> Result<(StatusCode, Json<TeamMembershipResponse>), ApiError> {
    let ws = load_workspace_scoped(&state, &auth, &workspace_id).await?;
    if !can_manage_membership(&state, &auth, &ws).await? {
        return Err(ApiError::Forbidden(
            "team membership can only be managed by an Owner, Admin, or TenantAdmin".to_string(),
        ));
    }
    let tid = Id::new(team_id);
    let team = state
        .teams
        .find_by_id(&tid)
        .await?
        .ok_or(ApiError::NotFound("Team not found".to_string()))?;
    if team.workspace_id != ws.id {
        return Err(ApiError::NotFound("Team not found".to_string()));
    }

    let user_id = Id::new(req.user_id);
    if state
        .workspace_memberships
        .find_by_user_and_workspace(&user_id, &ws.id)
        .await?
        .is_none()
    {
        return Err(ApiError::InvalidInput(format!(
            "user {} is not a member of workspace {}; teams cannot expand access beyond \
             workspace membership",
            user_id.as_str(),
            ws.id.as_str()
        )));
    }

    let membership = TeamMembership::new(team.id.clone(), user_id.clone(), now_secs());
    // Port contract: duplicate membership fails. Surface it as 409 here
    // (mirroring `reject_duplicate_slug`) so the adapter's guard never
    // bubbles up as an undifferentiated 500.
    let existing_members = state.teams.list_members(&tid).await?;
    if existing_members.iter().any(|m| m.user_id == user_id) {
        return Err(ApiError::Conflict(format!(
            "user {} is already a member of team {}",
            user_id.as_str(),
            tid.as_str()
        )));
    }
    state.teams.add_member(&membership).await?;
    Ok((
        StatusCode::CREATED,
        Json(TeamMembershipResponse {
            team_id: membership.team_id.to_string(),
            user_id: membership.user_id.to_string(),
            added_at: membership.added_at,
        }),
    ))
}

/// DELETE /api/v1/workspaces/:id/teams/:team_id/members/:user_id — remove a
/// member from a team (user-management.md §API > Teams). Idempotent.
pub async fn remove_team_member(
    auth: AuthenticatedAgent,
    Path((workspace_id, team_id, user_id)): Path<(String, String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let ws = load_workspace_scoped(&state, &auth, &workspace_id).await?;
    if !can_manage_membership(&state, &auth, &ws).await? {
        return Err(ApiError::Forbidden(
            "team membership can only be managed by an Owner, Admin, or TenantAdmin".to_string(),
        ));
    }
    let tid = Id::new(team_id);
    let team = state
        .teams
        .find_by_id(&tid)
        .await?
        .ok_or(ApiError::NotFound("Team not found".to_string()))?;
    if team.workspace_id != ws.id {
        return Err(ApiError::NotFound("Team not found".to_string()));
    }
    state.teams.remove_member(&tid, &Id::new(user_id)).await?;
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

    // The reserved system ABAC identity (task-129 review round 3):
    // `User::new` derives display_name from the username, and API-key/JWT
    // auth resolves the user's ABAC subject.id from display_name — a user
    // provisioned under this handle would match `system-full-access`
    // (priority 1000) and gain full access.
    if username == gyre_domain::SYSTEM_ABAC_IDENTITY {
        return Err(ApiError::InvalidInput(format!(
            "username '{}' is reserved: it would become the user's display_name \
             and reserved system ABAC identity",
            gyre_domain::SYSTEM_ABAC_IDENTITY
        )));
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

/// GET /api/v1/notifications/preferences (spec path; also served at
/// /api/v1/users/me/notification-preferences): the caller's per-type
/// notification preferences AND delivery channel configuration
/// (user-management.md §Delivery Channels). Per-handler auth
/// (self-scope only): the handler reads exclusively the authenticated
/// user's own rows, so no cross-scope decision is possible.
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
    let channels = crate::notification_dispatcher::resolve_channels(&state, &user_id).await;
    Ok(Json(
        serde_json::json!({ "preferences": items, "channels": channels }),
    ))
}

#[derive(Deserialize)]
pub struct NotifPrefItem {
    pub notification_type: String,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateNotifPrefsRequest {
    /// Per-type enable/disable preferences (HSI §12). Optional: a request
    /// may update only `channels`.
    pub preferences: Option<Vec<NotifPrefItem>>,
    /// Delivery channel configuration (user-management.md §Delivery
    /// Channels). Optional: a request may update only `preferences`.
    pub channels: Option<gyre_domain::NotificationChannels>,
}

/// Canonical type names the user has explicitly disabled (HSI §12).
/// Used as the inbox/count exclusion set; an empty result filters nothing.
async fn disabled_notification_types(
    state: &AppState,
    user_id: &Id,
) -> Result<Vec<String>, ApiError> {
    let prefs = state.user_notification_prefs.list_for_user(user_id).await?;
    Ok(prefs
        .into_iter()
        .filter(|p| !p.enabled)
        .map(|p| p.notification_type)
        .collect())
}

/// Shared validation for a `NotificationChannels` update, whichever endpoint
/// carries it (PUT /notifications/preferences or the `notification_channels`
/// field of PUT /users/me): `in_app` is forced true ("Always true (can't
/// disable)") and configured endpoints must be absolute URLs.
fn validate_channel_config(channels: &gyre_domain::NotificationChannels) -> Result<(), ApiError> {
    if !channels.in_app {
        return Err(ApiError::InvalidInput(
            "in_app cannot be disabled (user-management.md §Delivery Channels)".to_string(),
        ));
    }
    if let Some(hook) = &channels.webhook {
        if reqwest::Url::parse(&hook.url).is_err() {
            return Err(ApiError::InvalidInput(format!(
                "webhook.url is not a valid absolute URL: {}",
                hook.url
            )));
        }
    }
    if let Some(slack) = &channels.slack {
        if reqwest::Url::parse(&slack.webhook_url).is_err() {
            return Err(ApiError::InvalidInput(format!(
                "slack.webhook_url is not a valid absolute URL: {}",
                slack.webhook_url
            )));
        }
    }
    Ok(())
}

/// PUT /api/v1/notifications/preferences (spec path; also served at
/// /api/v1/users/me/notification-preferences): update the caller's per-type
/// notification preferences and/or delivery channel configuration. The body
/// may carry `preferences`, `channels`, or both; at least one is required.
/// Per-handler auth (self-scope only).
///
/// A channel update is persisted to BOTH the dedicated `user_channel_prefs`
/// store (what delivery reads) and the user's
/// `users.preferences.notification_channels` (what GET /users/me echoes) —
/// one effective configuration, no split-brain (review bf1e11be F1).
pub async fn update_notification_preferences(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateNotifPrefsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = resolve_user_id(&auth);
    if req.preferences.is_none() && req.channels.is_none() {
        return Err(ApiError::InvalidInput(
            "body must set `preferences` and/or `channels`".to_string(),
        ));
    }

    // Normalize to the canonical `NotificationType::as_str()` name before
    // storing: `parse` accepts legacy aliases (`SpecPendingApproval`,
    // `BudgetWarning`), but notification rows are persisted with canonical
    // names and the inbox filter is a raw string comparison — storing an
    // alias would silently create a preference that never matches any
    // notification (a dead toggle).
    let mut items: Vec<NotifPrefResponse> = Vec::new();
    if let Some(prefs) = req.preferences {
        let mut normalized: Vec<UserNotificationPreference> = Vec::with_capacity(prefs.len());
        for item in prefs {
            let nt = NotificationType::parse(&item.notification_type).ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "unknown notification type '{}' (expected canonical name, e.g. \"GateFailure\")",
                    item.notification_type
                ))
            })?;
            normalized.push(UserNotificationPreference::new(
                user_id.clone(),
                nt.as_str().to_string(),
                item.enabled,
            ));
        }
        state
            .user_notification_prefs
            .upsert_batch(&normalized)
            .await?;
        items = normalized.into_iter().map(Into::into).collect();
    }

    let mut channels: Option<gyre_domain::NotificationChannels> = None;
    if let Some(ch) = req.channels {
        validate_channel_config(&ch)?;
        state.user_channel_prefs.upsert(&user_id, &ch).await?;
        // Mirror the same value into the user profile store so the
        // preferences round-trip (GET /users/me → PUT /users/me) cannot
        // silently revert delivery to a stale configuration.
        if let Some(mut user) = state.users.find_by_id(&user_id).await? {
            user.preferences.notification_channels = ch.clone();
            user.updated_at = now_secs();
            state.users.update(&user).await?;
        }
        channels = Some(ch);
    }

    Ok(Json(
        serde_json::json!({ "preferences": items, "channels": channels }),
    ))
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
    // An unparseable ?type= must 400 (same contract as the sibling
    // notification-preferences PUT / notifications priority param):
    // silently treating it as "no filter" returns unfiltered results the
    // caller did not ask for.
    let judgment_type = match params.judgment_type.as_deref() {
        Some(t) => Some(JudgmentType::from_db_str(t).ok_or_else(|| {
            ApiError::InvalidInput(format!(
                "unknown judgment type '{t}' (expected one of approval|rejection|gate|trust|meta-spec)"
            ))
        })?),
        None => None,
    };
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
    use futures_util::StreamExt;
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

    /// Spec path (user-management.md §API > Notifications): GET
    /// /api/v1/notifications lists the caller's notifications with pagination
    /// and the specced filter surface (type, priority level, workspace, repo).
    #[tokio::test]
    async fn spec_path_notifications_list_with_filters() {
        let state = test_state();
        let mut with_repo = Notification::new(
            Id::new("notif-spec-1"),
            Id::new("ws-default"),
            Id::new("system"),
            NotificationType::GateFailure, // rank 3 → High band
            "Gate failed on repo-a MR",
            "default",
            1_700_000_000,
        );
        with_repo.source.entity_type = "mr".to_string();
        with_repo.source.entity_id = "mr-1".to_string();
        with_repo.source.repo_id = Some(Id::new("repo-a"));
        state.create_notification(&with_repo).await.unwrap();
        let no_repo = Notification::new(
            Id::new("notif-spec-2"),
            Id::new("ws-default"),
            Id::new("system"),
            NotificationType::AgentCompleted, // rank 9 → Low band
            "Agent worker-1 completed",
            "default",
            1_700_000_001,
        );
        state.create_notification(&no_repo).await.unwrap();

        let app = crate::api::api_router().with_state(state);

        async fn list(app: &Router, uri: &str) -> serde_json::Value {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .header("Authorization", "Bearer test-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "GET {uri}");
            body_json(resp).await
        }

        // Unfiltered: both seeded notifications.
        let json = list(&app, "/api/v1/notifications").await;
        assert_eq!(json["notifications"].as_array().unwrap().len(), 2);

        // ?type= filter (spec filter surface).
        let json = list(&app, "/api/v1/notifications?type=AgentCompleted").await;
        let notifs = json["notifications"].as_array().unwrap();
        assert_eq!(notifs.len(), 1, "type filter must narrow to the match");
        assert_eq!(notifs[0]["id"], "notif-spec-2");

        // ?repo_id= filter — matches the notification's source repo.
        let json = list(&app, "/api/v1/notifications?repo_id=repo-a").await;
        let notifs = json["notifications"].as_array().unwrap();
        assert_eq!(notifs.len(), 1, "repo filter must narrow to the match");
        assert_eq!(notifs[0]["id"], "notif-spec-1");

        // ?priority= filter — four-level name widens to its rank band:
        // GateFailure (rank 3) is High, AgentCompleted (rank 9) is Low.
        let json = list(&app, "/api/v1/notifications?priority=high").await;
        let notifs = json["notifications"].as_array().unwrap();
        assert_eq!(
            notifs.len(),
            1,
            "priority=high must match only the High item"
        );
        assert_eq!(notifs[0]["id"], "notif-spec-1");
        let json = list(&app, "/api/v1/notifications?priority=low").await;
        let notifs = json["notifications"].as_array().unwrap();
        assert_eq!(notifs.len(), 1, "priority=low must match only the Low item");
        assert_eq!(notifs[0]["id"], "notif-spec-2");

        // An unknown priority level is a 400, not a silently ignored filter.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/notifications?priority=critical")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Pagination: limit=1 returns one item and echoes the window.
        let json = list(&app, "/api/v1/notifications?limit=1&offset=1").await;
        let notifs = json["notifications"].as_array().unwrap();
        assert_eq!(notifs.len(), 1);
        assert_eq!(json["limit"], 1);
        assert_eq!(json["offset"], 1);
    }

    /// Spec path (user-management.md §API > Notifications): unread-count
    /// counts active-but-unread notifications for the bell badge, and
    /// mark-all-read drives it to zero (only the caller's rows are touched).
    #[tokio::test]
    async fn spec_path_unread_count_and_mark_all_read() {
        let state = test_state();
        let seed = |id: &str, user: &str, ty: NotificationType| {
            Notification::new(
                Id::new(id),
                Id::new("ws-default"),
                Id::new(user),
                ty,
                id,
                "default",
                1_700_000_000,
            )
        };
        // Caller ("system"): one unread, one already read, one dismissed-but-unread.
        let unread = seed("notif-unread", "system", NotificationType::GatePassed);
        state.create_notification(&unread).await.unwrap();
        let read = seed("notif-read", "system", NotificationType::GatePassed);
        state.create_notification(&read).await.unwrap();
        state
            .notifications
            .mark_read(&Id::new("notif-read"), &Id::new("system"), true)
            .await
            .unwrap();
        let dismissed = seed("notif-dismissed", "system", NotificationType::GatePassed);
        state.create_notification(&dismissed).await.unwrap();
        state
            .notifications
            .dismiss(&Id::new("notif-dismissed"), &Id::new("system"))
            .await
            .unwrap();
        // Another user's unread notification must not count for the caller.
        let other = seed("notif-other", "someone-else", NotificationType::GatePassed);
        state.create_notification(&other).await.unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/notifications/unread-count")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(
            json["count"], 1,
            "only the caller's active unread item counts (read and dismissed excluded)"
        );

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/notifications/mark-all-read")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(
            json["marked"], 2,
            "the unread and dismissed-unread rows flip; the already-read row does not"
        );

        // Badge reaches zero and the other user's row stays unread.
        let count = state
            .notifications
            .count_unread(&Id::new("system"), None)
            .await
            .unwrap();
        assert_eq!(count, 0);
        let other_row = state
            .notifications
            .get(&Id::new("notif-other"), &Id::new("someone-else"))
            .await
            .unwrap()
            .unwrap();
        assert!(
            !other_row.read,
            "mark-all-read must stay scoped to the caller"
        );
    }

    /// Spec path/method (user-management.md §API > Notifications): PUT
    /// /api/v1/notifications/{id}/read marks read.
    #[tokio::test]
    async fn put_read_is_the_spec_method() {
        let state = test_state();
        let notif = Notification::new(
            Id::new("notif-put-read"),
            Id::new("ws-default"),
            Id::new("system"),
            NotificationType::MrMerged,
            "MR merged",
            "default",
            1_700_000_000,
        );
        state.create_notification(&notif).await.unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/notifications/notif-put-read/read")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let n = state
            .notifications
            .get(&Id::new("notif-put-read"), &Id::new("system"))
            .await
            .unwrap()
            .unwrap();
        assert!(n.read);
        assert!(n.read_at.is_some());
    }

    /// Spec path (user-management.md §API > Notifications): preferences
    /// get/put at /api/v1/notifications/preferences (same handlers as the
    /// HSI §12 /users/me/notification-preferences route).
    #[tokio::test]
    async fn spec_path_preferences_roundtrip() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/notifications/preferences")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["preferences"].as_array().unwrap().len(), 0);

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/notifications/preferences")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"preferences": [
                            {"notification_type": "AgentBudgetWarning", "enabled": false}
                        ]})
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/notifications/preferences")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let prefs = json["preferences"].as_array().unwrap();
        assert_eq!(prefs.len(), 1);
        assert_eq!(prefs[0]["notification_type"], "AgentBudgetWarning");
        assert_eq!(prefs[0]["enabled"], false);
    }

    /// Spec path (user-management.md §API > Notifications): the SSE stream
    /// delivers the caller's notifications as they are persisted and drops
    /// other users' (and other tenants') notifications before the wire.
    #[tokio::test]
    async fn notification_stream_delivers_own_notifications_only() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        // Connect: the handler subscribes to the broadcast channel before
        // the response (Sse) is returned, so once oneshot resolves the
        // subscription is live.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/notifications/stream")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers().get("content-type").unwrap();
        assert!(ct.to_str().unwrap().contains("text/event-stream"));

        // Create one notification for the caller and one for another user
        // (plus a cross-tenant one addressed to the caller's user id).
        let own = Notification::new(
            Id::new("notif-stream-own"),
            Id::new("ws-default"),
            Id::new("system"),
            NotificationType::SpecApprovalRequested,
            "Streamed to caller",
            "default",
            1_700_000_000,
        );
        state.create_notification(&own).await.unwrap();
        let other_user = Notification::new(
            Id::new("notif-stream-other-user"),
            Id::new("ws-default"),
            Id::new("someone-else"),
            NotificationType::SpecApprovalRequested,
            "Not the caller",
            "default",
            1_700_000_001,
        );
        state.create_notification(&other_user).await.unwrap();
        let other_tenant = Notification::new(
            Id::new("notif-stream-other-tenant"),
            Id::new("ws-default"),
            Id::new("system"),
            NotificationType::SpecApprovalRequested,
            "Wrong tenant",
            "another-tenant",
            1_700_000_002,
        );
        state.create_notification(&other_tenant).await.unwrap();

        // Read SSE chunks until the heartbeat timeout: the caller's event
        // must arrive; the other two must never reach the wire.
        let mut body = resp.into_body().into_data_stream();
        let mut received = Vec::<u8>::new();
        loop {
            let next = tokio::time::timeout(std::time::Duration::from_secs(1), body.next()).await;
            match next {
                Err(_) => break, // 1s with no chunk: nothing more is coming
                Ok(Some(Ok(chunk))) => received.extend_from_slice(&chunk),
                Ok(Some(Err(_))) | Ok(None) => break,
            }
        }
        let text = String::from_utf8_lossy(&received).to_string();
        assert!(
            text.contains("event: notification"),
            "stream must emit notification events; got: {text}"
        );
        assert!(
            text.contains("notif-stream-own"),
            "the caller's notification must be delivered; got: {text}"
        );
        assert!(
            !text.contains("notif-stream-other-user"),
            "another user's notification must be filtered out; got: {text}"
        );
        assert!(
            !text.contains("notif-stream-other-tenant"),
            "a cross-tenant notification must be filtered out; got: {text}"
        );
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

    /// HSI §12: disabling a notification type via
    /// PUT /users/me/notification-preferences must exclude it from the inbox
    /// list AND the badge count — the handler wiring, not just the query.
    #[tokio::test]
    async fn disabled_preference_excludes_type_from_inbox_and_count() {
        let state = test_state();
        seed_notification(&state, NotificationType::GateFailure, "Gate failed").await;
        seed_notification(
            &state,
            NotificationType::SpecApprovalRequested,
            "Approve spec",
        )
        .await;

        let app = crate::api::api_router().with_state(state);
        // Baseline: both visible.
        let resp = app
            .clone()
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
        assert_eq!(
            json["notifications"].as_array().unwrap().len(),
            2,
            "baseline: both types visible: {json:?}"
        );

        // Disable GateFailure.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/users/me/notification-preferences")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"preferences":[{"notification_type":"GateFailure","enabled":false}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Inbox excludes the disabled type.
        let resp = app
            .clone()
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
        assert_eq!(notifs.len(), 1, "disabled type must be excluded: {json:?}");
        assert_eq!(notifs[0]["notification_type"], "SpecApprovalRequested");

        // Badge count matches the inbox.
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
        assert_eq!(json["count"], 1, "badge must not count the disabled type");
    }

    /// Legacy parse aliases must normalize to the canonical
    /// `NotificationType::as_str()` name before storing: notification rows
    /// are persisted with canonical names and the inbox exclusion is a raw
    /// string comparison, so storing 'SpecPendingApproval' (alias for
    /// 'SpecApprovalRequested') would be a silently inert toggle.
    #[tokio::test]
    async fn preference_alias_normalizes_to_canonical_before_storing() {
        let state = test_state();
        seed_notification(
            &state,
            NotificationType::SpecApprovalRequested,
            "Approve spec",
        )
        .await;

        let app = crate::api::api_router().with_state(state.clone());
        // Disable via the legacy alias spelling.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/users/me/notification-preferences")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"preferences":[{"notification_type":"SpecPendingApproval","enabled":false}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        // The stored (and echoed) preference carries the canonical name.
        assert_eq!(
            json["preferences"][0]["notification_type"], "SpecApprovalRequested",
            "alias must be normalized before storing: {json:?}"
        );

        // The canonical-typed notification is excluded from the inbox...
        let resp = app
            .clone()
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
        assert_eq!(
            json["notifications"].as_array().unwrap().len(),
            0,
            "alias-named disable must filter the canonical-typed notification: {json:?}"
        );

        // ...and from the badge count.
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
        assert_eq!(json["count"], 0);
    }

    /// HSI §12 auth provider info: a successful OIDC/JWT authentication must
    /// record the verified issuer + login time, and GET /users/me must return
    /// both read-only. PUT /users/me has no field for either, so they cannot
    /// be client-set.
    #[tokio::test(flavor = "multi_thread")]
    async fn get_me_returns_recorded_auth_provider_info() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};

        let state = make_test_state_with_jwt();
        let app = crate::api::api_router().with_state(state.clone());

        // Authenticate via a real OIDC JWT — this drives validate_jwt's
        // record_login path against the user store.
        let jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "me-issuer-sub",
                "preferred_username": "issuer-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me")
                    .header("authorization", format!("Bearer {jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let me = body_json(resp).await;
        // The issuer is the test JwtConfig's verified issuer.
        assert_eq!(me["oidc_issuer"], "http://localhost:8080/realms/gyre");
        let login = me["last_login_at"]
            .as_u64()
            .expect("last_login_at recorded");
        assert!(login > 0);

        // Editable profile fields round-trip without touching auth-provider info.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/users/me")
                    .header("authorization", format!("Bearer {jwt}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"display_name":"Renamed","timezone":"Europe/Berlin"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let me2 = body_json(resp).await;
        assert_eq!(me2["display_name"], "Renamed");
        assert_eq!(me2["timezone"], "Europe/Berlin");
        assert_eq!(me2["oidc_issuer"], "http://localhost:8080/realms/gyre");
        assert_eq!(me2["last_login_at"].as_u64(), Some(login));
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

    /// Task-129 review round 3 (privilege escalation): admin-created users
    /// derive display_name from the username, and display_name becomes the
    /// user's ABAC subject.id under API-key/JWT auth — a user provisioned
    /// under the reserved system identity would match `system-full-access`
    /// (priority 1000) and gain full access.
    #[tokio::test]
    async fn create_user_rejects_reserved_system_username() {
        let state = test_state();
        seed_tenant(&state).await;
        let resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/users")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"username":"gyre-system-token","tenant_id":"t-1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "the reserved system ABAC identity must not be provisionable as a username"
        );

        // And no user row was created under the reserved handle.
        assert!(
            state
                .users
                .find_by_username("gyre-system-token")
                .await
                .unwrap()
                .is_none(),
            "rejected reserved-identity provisioning must not create a user"
        );
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

    #[tokio::test]
    async fn get_channel_preferences_returns_defaults() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/notifications/preferences")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let channels = &json["channels"];
        assert_eq!(channels["in_app"], true, "in_app defaults on");
        assert_eq!(channels["email"]["enabled"], false, "email defaults off");
        assert!(
            channels["webhook"].is_null(),
            "webhook defaults unconfigured"
        );
        assert!(channels["slack"].is_null(), "slack defaults unconfigured");
    }

    #[tokio::test]
    async fn put_channel_preferences_round_trips() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let payload = serde_json::json!({
            "channels": {
                "in_app": true,
                "email": {
                    "enabled": true,
                    "digest": "Daily",
                    "min_priority": "Medium"
                },
                "webhook": {
                    "url": "https://hooks.example.test/gyre",
                    "secret": "s3cret",
                    "min_priority": "High"
                },
                "slack": {
                    "webhook_url": "https://hooks.slack.test/T/B/X",
                    "channel": "#ops",
                    "min_priority": "Low"
                }
            }
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/notifications/preferences")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "valid config must be accepted"
        );

        // A fresh router over the same state must read the persisted config
        // (durable store, not in-handler memory).
        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/notifications/preferences")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let channels = &json["channels"];
        assert_eq!(channels["email"]["enabled"], true);
        assert_eq!(channels["email"]["digest"], "Daily");
        assert_eq!(
            channels["webhook"]["url"],
            "https://hooks.example.test/gyre"
        );
        assert_eq!(channels["webhook"]["min_priority"], "High");
        assert_eq!(channels["slack"]["channel"], "#ops");
    }

    #[tokio::test]
    async fn put_channel_preferences_rejects_disabled_in_app() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/notifications/preferences")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"channels":{"in_app":false,"email":{"enabled":false,"digest":"Off","min_priority":"Low"},"webhook":null,"slack":null}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "in_app=false must be rejected — spec marks it can't-disable"
        );
    }

    #[tokio::test]
    async fn put_channel_preferences_rejects_relative_webhook_url() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/notifications/preferences")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"channels":{"in_app":true,"email":{"enabled":false,"digest":"Off","min_priority":"Low"},"webhook":{"url":"./local-hook","secret":"s","min_priority":"Low"},"slack":null}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "non-absolute webhook URL must be rejected"
        );
    }

    /// Guard against the split-brain config defect (review bf1e11be F1):
    /// a channel configuration set via `PUT /users/me`'s
    /// `preferences.notification_channels` field must reach the delivery
    /// store the dispatcher reads — email configured through the profile
    /// endpoint has to queue in the durable outbox, exactly as it would
    /// when configured through `PUT /notifications/preferences`.
    #[tokio::test]
    async fn put_me_notification_channels_reach_delivery_outbox() {
        let (state, user_id, key) = bootstrap_user("put-me-channels").await;
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/users/me")
                    .header("Authorization", format!("Bearer {key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"preferences":{"notification_channels":{"in_app":true,"email":{"enabled":true,"digest":"Immediate","min_priority":"Low"},"webhook":null,"slack":null}}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "channel config via PUT /users/me must be accepted"
        );

        // Persist an in-app notification for this user and run the
        // production fan-out path (notify: create + dispatch_to_channels).
        let notif = Notification::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new("ws-default"),
            Id::new(user_id.clone()),
            NotificationType::GateFailure, // High band
            "gate failed",
            "t-1",
            1_700_000_000i64,
        );
        state.notifications.create(&notif).await.unwrap();
        crate::notification_dispatcher::dispatch_to_channels(&state, &notif).await;

        let outbox = state
            .kv_store
            .kv_list(crate::notification_dispatcher::EMAIL_OUTBOX_NS)
            .await
            .unwrap();
        assert_eq!(
            outbox.len(),
            1,
            "email configured via PUT /users/me must queue in the durable outbox"
        );
        let (k, v) = &outbox[0];
        assert!(k.starts_with(&format!("{user_id}:")), "queued for the user");
        let parsed: serde_json::Value = serde_json::from_str(v).unwrap();
        assert_eq!(parsed["user_id"], user_id);
    }

    /// PUT /users/me must apply the SAME channel validation as the dedicated
    /// endpoint — `in_app: false` and relative URLs cannot be smuggled in
    /// through the profile preferences field.
    #[tokio::test]
    async fn put_me_notification_channels_rejects_invalid_config() {
        let (state, _user_id, key) = bootstrap_user("put-me-channels-bad").await;
        let app = crate::api::api_router().with_state(state.clone());
        for (label, payload) in [
            (
                "in_app false",
                r#"{"preferences":{"notification_channels":{"in_app":false,"email":{"enabled":true,"digest":"Off","min_priority":"Low"},"webhook":null,"slack":null}}}"#,
            ),
            (
                "relative webhook url",
                r#"{"preferences":{"notification_channels":{"in_app":true,"email":{"enabled":false,"digest":"Off","min_priority":"Low"},"webhook":{"url":"./local","secret":"s","min_priority":"Low"},"slack":null}}}"#,
            ),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri("/api/v1/users/me")
                        .header("Authorization", format!("Bearer {key}"))
                        .header("Content-Type", "application/json")
                        .body(Body::from(payload))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::BAD_REQUEST,
                "invalid channel config ({label}) must be rejected by PUT /users/me"
            );
        }
    }

    /// The reverse direction of the single-effective-config invariant: a
    /// config set via `PUT /notifications/preferences` must be echoed by
    /// `GET /users/me`, so the preferences round-trip (GET then PUT the whole
    /// object back) cannot silently revert delivery to a stale config.
    #[tokio::test]
    async fn put_channel_preferences_syncs_users_preferences() {
        let (state, user_id, key) = bootstrap_user("notif-prefs-sync").await;
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/notifications/preferences")
                    .header("Authorization", format!("Bearer {key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"channels":{"in_app":true,"email":{"enabled":true,"digest":"Daily","min_priority":"High"},"webhook":null,"slack":null}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = app
            .clone()
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
        assert_eq!(
            json["preferences"]["notification_channels"],
            serde_json::json!({
                "in_app": true,
                "email": {"enabled": true, "digest": "Daily", "min_priority": "High"},
                "webhook": null,
                "slack": null
            }),
            "GET /users/me must echo the same effective config the dispatcher reads"
        );
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
        // notification_channels now serializes in the nested §Delivery
        // Channels shape (merged task-112 config: email/webhook/slack).
        assert_eq!(
            json2["preferences"]["notification_channels"],
            serde_json::json!({
                "in_app": true,
                "email": {"enabled": false, "digest": "Off", "min_priority": "Medium"},
                "webhook": null,
                "slack": null
            }),
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

    /// Task-129 review round 3 (privilege escalation): display_name becomes
    /// the caller's ABAC subject.id under API-key/JWT auth, and the seeded
    /// `system-full-access` policy (priority 1000) matches the reserved
    /// system identity — so a user renaming themselves to it must be
    /// rejected, and nothing applied.
    #[tokio::test]
    async fn put_me_rejects_reserved_system_display_name() {
        let (state, _user_id, key) = bootstrap_user("put-me-reserved").await;
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/users/me")
                    .header("Authorization", format!("Bearer {key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"display_name":"gyre-system-token","timezone":"UTC"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "the reserved system ABAC identity must not be assignable as display_name"
        );

        // Nothing was applied — the display_name is unchanged.
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
        assert_ne!(
            json2["display_name"], "gyre-system-token",
            "rejected reserved-identity rename must not apply"
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

    // ── Workspace membership endpoints (user-management.md §Workspace
    // Membership, §Ownership Transfer & Reclamation) ─────────────────────

    use gyre_domain::{User, Workspace, WorkspaceMembership, WorkspaceRole};

    /// Seed a workspace in tenant t-1 with one accepted Owner membership
    /// whose user record exists. Returns (workspace_id, owner_user_id).
    async fn seed_workspace_with_owner(
        state: &std::sync::Arc<crate::AppState>,
    ) -> (String, String) {
        let now = 1_700_000_000u64;
        let tenant = gyre_domain::Tenant::new(Id::new("t-1"), "Acme", "acme", 0);
        state.tenants.create(&tenant).await.unwrap();

        let ws_id = uuid::Uuid::new_v4().to_string();
        let ws = Workspace::new(
            Id::new(ws_id.clone()),
            Id::new("t-1"),
            format!("membership-ws-{ws_id}"),
            format!("membership-ws-{ws_id}"),
            now,
        );
        state.workspaces.create(&ws).await.unwrap();

        let owner_id = uuid::Uuid::new_v4().to_string();
        let mut owner = User::new(
            Id::new(owner_id.clone()),
            format!("owner-ext-{owner_id}"),
            format!("owner-{owner_id}"),
            now,
        );
        owner.tenant_id = Some(Id::new("t-1"));
        state.users.create(&owner).await.unwrap();

        let membership = WorkspaceMembership::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new(owner_id.clone()),
            Id::new(ws_id.clone()),
            WorkspaceRole::Owner,
            Id::new("system"),
            now,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        (ws_id, owner_id)
    }

    #[tokio::test]
    async fn last_owner_cannot_be_removed_or_downgraded() {
        // user-management.md §Ownership Transfer & Reclamation: "Cannot
        // remove last Owner" — removal and role downgrade must both be
        // rejected until a new Owner is designated.
        let state = test_state();
        let (ws_id, owner_id) = seed_workspace_with_owner(&state).await;
        let app = crate::api::api_router().with_state(state.clone());

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/workspaces/{ws_id}/members/{owner_id}"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "removing the last Owner must be a 409 conflict"
        );

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/members/{owner_id}/role"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"role": "Developer"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "downgrading the last Owner must be a 409 conflict"
        );

        // The membership is untouched — the owner is still an Owner.
        let m = state
            .workspace_memberships
            .find_by_user_and_workspace(&Id::new(owner_id), &Id::new(ws_id))
            .await
            .unwrap()
            .expect("membership must survive the rejected operations");
        assert_eq!(m.role, WorkspaceRole::Owner);
    }

    #[tokio::test]
    async fn ownership_transfer_unblocks_old_owner_removal_and_is_audited() {
        // user-management.md §Ownership Transfer & Reclamation: the Tenant
        // Admin reassigns ownership via the role-change endpoint; the
        // transfer is audit-logged and the previous Owner may then leave.
        let state = test_state();
        let (ws_id, owner_id) = seed_workspace_with_owner(&state).await;
        let now = 1_700_000_000u64;

        // A second member receives ownership.
        let new_owner_id = uuid::Uuid::new_v4().to_string();
        let mut new_owner = User::new(
            Id::new(new_owner_id.clone()),
            format!("new-owner-ext-{new_owner_id}"),
            format!("new-owner-{new_owner_id}"),
            now,
        );
        new_owner.tenant_id = Some(Id::new("t-1"));
        state.users.create(&new_owner).await.unwrap();
        let m = WorkspaceMembership::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new(new_owner_id.clone()),
            Id::new(ws_id.clone()),
            WorkspaceRole::Developer,
            Id::new("system"),
            now,
        );
        state.workspace_memberships.create(&m).await.unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/members/{new_owner_id}/role"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "role": "Owner",
                            "reason": "ownership transfer"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // The transfer is audit-logged with previous/new role and reason.
        let filter = gyre_ports::AuditQueryFilter {
            event_type: Some("workspace_ownership_transferred".to_string()),
            workspace_id: Some(ws_id.clone()),
            limit: 10,
            ..Default::default()
        };
        let events = state.audit.query(&filter).await.unwrap();
        assert_eq!(events.len(), 1, "exactly one transfer audit event");
        assert_eq!(events[0].detail["new_role"], serde_json::json!("Owner"));
        assert_eq!(
            events[0].detail["previous_role"],
            serde_json::json!("Developer")
        );
        assert_eq!(
            events[0].detail["reason"],
            serde_json::json!("ownership transfer")
        );

        // With a second active Owner, the original Owner may be removed.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/workspaces/{ws_id}/members/{owner_id}"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NO_CONTENT,
            "the previous Owner must be removable after the transfer"
        );
        assert!(state
            .workspace_memberships
            .find_by_user_and_workspace(&Id::new(owner_id), &Id::new(ws_id))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn invite_member_rejects_duplicate_membership() {
        let state = test_state();
        let (ws_id, owner_id) = seed_workspace_with_owner(&state).await;
        let app = crate::api::api_router().with_state(state.clone());

        let body = serde_json::json!({"user_id": owner_id, "role": "Developer"}).to_string();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/members"))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "inviting an existing member must be a 409 conflict"
        );
    }

    #[tokio::test]
    async fn member_management_requires_owner_admin_or_tenant_admin() {
        // A plain Developer member (API-key auth) must not be able to grant
        // workspace membership. Runs the PRODUCTION pipeline (api router +
        // abac_middleware + builtin policies, as build_router layers them):
        // the request passes ABAC — the Developer global role may write, and
        // the recorded decision is Allow — so the 403 provably comes from
        // the handler's membership check, not from policy evaluation.
        let state = test_state();
        let (ws_id, _owner_id) = seed_workspace_with_owner(&state).await;
        let now = 1_700_000_000u64;

        let dev_id = uuid::Uuid::new_v4().to_string();
        let mut dev = User::new(
            Id::new(dev_id.clone()),
            format!("dev-ext-{dev_id}"),
            format!("dev-{dev_id}"),
            now,
        );
        dev.tenant_id = Some(Id::new("t-1"));
        dev.global_role = gyre_domain::GlobalRole::Member;
        dev.roles = vec![gyre_domain::UserRole::Developer];
        state.users.create(&dev).await.unwrap();
        let m = WorkspaceMembership::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new(dev_id.clone()),
            Id::new(ws_id.clone()),
            WorkspaceRole::Developer,
            Id::new("system"),
            now,
        );
        state.workspace_memberships.create(&m).await.unwrap();
        state
            .api_keys
            .create(
                &crate::auth::hash_api_key("dev-key-membership"),
                &Id::new(dev_id.clone()),
                "test",
            )
            .await
            .unwrap();

        crate::abac_middleware::init_resolver();
        crate::abac_middleware::seed_builtin_policies(&state)
            .await
            .expect("seed builtin policies");
        let app = crate::api::api_router()
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::abac_middleware::abac_middleware,
            ))
            .with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/members"))
                    .header("Authorization", "Bearer dev-key-membership")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"user_id": dev_id, "role": "Viewer"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "a Developer member must not be able to grant membership"
        );

        // The 403 came from the handler: ABAC evaluated this exact request
        // to Allow (Developer global role may write on workspace_member).
        let decisions = state
            .policies
            .list_decisions(None, Some("workspace_member"), 50)
            .await
            .unwrap();
        assert!(
            decisions
                .iter()
                .any(|d| { d.decision == gyre_domain::PolicyEffect::Allow && d.action == "write" }),
            "the request must have passed ABAC (Allow) so the 403 is the handler's"
        );
    }

    // ─── Teams (user-management.md §Team Management) ─────────────────────────

    /// Seed a Developer workspace member (tenant t-1) and return the user id.
    async fn seed_workspace_developer(
        state: &std::sync::Arc<crate::AppState>,
        ws_id: &str,
    ) -> String {
        let now = 1_700_000_000u64;
        let dev_id = uuid::Uuid::new_v4().to_string();
        let mut dev = User::new(
            Id::new(dev_id.clone()),
            format!("dev-ext-{dev_id}"),
            format!("dev-{dev_id}"),
            now,
        );
        dev.tenant_id = Some(Id::new("t-1"));
        state.users.create(&dev).await.unwrap();
        let membership = WorkspaceMembership::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new(dev_id.clone()),
            Id::new(ws_id.to_string()),
            WorkspaceRole::Developer,
            Id::new("system"),
            now,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();
        dev_id
    }

    /// Seed a tenant user with NO workspace membership; returns the user id.
    async fn seed_tenant_user_without_membership(
        state: &std::sync::Arc<crate::AppState>,
    ) -> String {
        let now = 1_700_000_000u64;
        let uid = uuid::Uuid::new_v4().to_string();
        let mut user = User::new(
            Id::new(uid.clone()),
            format!("loner-ext-{uid}"),
            format!("loner-{uid}"),
            now,
        );
        user.tenant_id = Some(Id::new("t-1"));
        state.users.create(&user).await.unwrap();
        uid
    }

    async fn create_team_via_api(
        app: &Router,
        ws_id: &str,
        body: serde_json::Value,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/teams"))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn team_crud_slug_lead_and_membership_flow() {
        // user-management.md §Team Management + §API > Teams: create carries
        // slug (derived from the name) + lead (a workspace member);
        // membership add/remove endpoints carry added_at.
        let state = test_state();
        let (ws_id, owner_id) = seed_workspace_with_owner(&state).await;
        let dev_id = seed_workspace_developer(&state, &ws_id).await;
        let app = crate::api::api_router().with_state(state.clone());

        let resp = create_team_via_api(
            &app,
            &ws_id,
            serde_json::json!({"name": "Platform Team", "lead": owner_id}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let team = body_json(resp).await;
        assert_eq!(team["slug"], "platform-team", "slug derives from the name");
        assert_eq!(team["lead"], owner_id);
        assert_eq!(team["member_ids"].as_array().unwrap().len(), 0);
        let team_id = team["id"].as_str().unwrap().to_string();

        // Add a member — the response carries the membership's added_at.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/teams/{team_id}/members"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"user_id": dev_id}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let membership = body_json(resp).await;
        assert_eq!(membership["team_id"], team_id);
        assert_eq!(membership["user_id"], dev_id);
        assert!(
            membership["added_at"].as_u64().unwrap() > 0,
            "membership must carry a real added_at timestamp"
        );

        // Duplicate membership → 409.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/teams/{team_id}/members"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"user_id": dev_id}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        // List reflects the real membership table.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/workspaces/{ws_id}/teams"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let list = body_json(resp).await;
        assert_eq!(list["teams"].as_array().unwrap().len(), 1);
        assert_eq!(list["teams"][0]["member_ids"].as_array().unwrap().len(), 1);
        assert_eq!(list["teams"][0]["member_ids"][0], dev_id);

        // Update carries the new fields.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/teams/{team_id}"))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"description": "Owns the platform"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let updated = body_json(resp).await;
        assert_eq!(updated["description"], "Owns the platform");
        assert_eq!(updated["slug"], "platform-team");

        // Remove the member — idempotent.
        for expected in [StatusCode::NO_CONTENT, StatusCode::NO_CONTENT] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("DELETE")
                        .uri(format!(
                            "/api/v1/workspaces/{ws_id}/teams/{team_id}/members/{dev_id}"
                        ))
                        .header("Authorization", "Bearer test-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), expected);
        }
        let members = state
            .teams
            .list_members(&Id::new(team_id.clone()))
            .await
            .unwrap();
        assert_eq!(members.len(), 0);

        // Delete the team.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/workspaces/{ws_id}/teams/{team_id}"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(state
            .teams
            .find_by_id(&Id::new(team_id))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn team_slug_validation_and_uniqueness() {
        let state = test_state();
        let (ws_id, owner_id) = seed_workspace_with_owner(&state).await;
        let app = crate::api::api_router().with_state(state.clone());

        // Explicit slugs must be URL-safe — never silently rewritten.
        for bad_slug in [
            "Bad Slug!",
            "-platform",
            "platform-",
            "platform--team",
            "Platform",
        ] {
            let resp = create_team_via_api(
                &app,
                &ws_id,
                serde_json::json!({"name": "Platform", "slug": bad_slug, "lead": owner_id}),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::BAD_REQUEST,
                "explicit slug {bad_slug:?} must be rejected"
            );
        }

        // A name with no URL-safe derivation requires an explicit slug.
        let resp = create_team_via_api(
            &app,
            &ws_id,
            serde_json::json!({"name": "!!!", "lead": owner_id}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Create two teams whose derived slugs collide → second is a 409
        // (slug is unique within the workspace).
        let resp = create_team_via_api(
            &app,
            &ws_id,
            serde_json::json!({"name": "Platform Team", "lead": owner_id}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = create_team_via_api(
            &app,
            &ws_id,
            serde_json::json!({"name": "platform team", "lead": owner_id}),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "duplicate slug within a workspace must be a 409"
        );

        // The same slug in a DIFFERENT workspace is fine.
        let (ws2_id, owner2_id) = seed_workspace_with_owner(&state).await;
        let resp = create_team_via_api(
            &app,
            &ws2_id,
            serde_json::json!({"name": "Platform Team", "lead": owner2_id}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn team_lead_and_members_must_be_workspace_members() {
        // "Teams don't grant additional permissions beyond workspace
        // membership" — the lead and every member must already belong to the
        // workspace.
        let state = test_state();
        let (ws_id, owner_id) = seed_workspace_with_owner(&state).await;
        let loner = seed_tenant_user_without_membership(&state).await;
        let app = crate::api::api_router().with_state(state.clone());

        // Lead outside the workspace → 400.
        let resp = create_team_via_api(
            &app,
            &ws_id,
            serde_json::json!({"name": "Platform", "lead": loner}),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "a lead who is not a workspace member must be rejected"
        );

        // Member outside the workspace → 400.
        let resp = create_team_via_api(
            &app,
            &ws_id,
            serde_json::json!({"name": "Platform", "lead": owner_id}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let team_id = body_json(resp).await["id"].as_str().unwrap().to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/teams/{team_id}/members"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"user_id": loner}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "a team member who is not a workspace member must be rejected"
        );
        // Nothing was written.
        assert_eq!(
            state
                .teams
                .list_members(&Id::new(team_id))
                .await
                .unwrap()
                .len(),
            0
        );
    }

    #[tokio::test]
    async fn team_routes_are_workspace_scoped() {
        // A team from workspace B addressed through workspace A's path is a
        // 404, not a cross-workspace read/write.
        let state = test_state();
        let (ws1_id, owner1) = seed_workspace_with_owner(&state).await;
        let (ws2_id, owner2) = seed_workspace_with_owner(&state).await;
        let app = crate::api::api_router().with_state(state.clone());

        let resp = create_team_via_api(
            &app,
            &ws2_id,
            serde_json::json!({"name": "Platform", "lead": owner2}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let team_id = body_json(resp).await["id"].as_str().unwrap().to_string();

        for (method, uri) in [
            (
                "PUT",
                format!("/api/v1/workspaces/{ws1_id}/teams/{team_id}"),
            ),
            (
                "DELETE",
                format!("/api/v1/workspaces/{ws1_id}/teams/{team_id}"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/{ws1_id}/teams/{team_id}/members"),
            ),
            (
                "DELETE",
                format!("/api/v1/workspaces/{ws1_id}/teams/{team_id}/members/{owner1}"),
            ),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri.clone())
                        .header("Authorization", "Bearer test-token")
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::json!({"user_id": owner1}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "{method} {uri} must be workspace-scoped"
            );
        }
    }

    #[tokio::test]
    async fn team_update_validates_slug_conflict_and_lead() {
        let state = test_state();
        let (ws_id, owner_id) = seed_workspace_with_owner(&state).await;
        let dev_id = seed_workspace_developer(&state, &ws_id).await;
        let app = crate::api::api_router().with_state(state.clone());

        let resp = create_team_via_api(
            &app,
            &ws_id,
            serde_json::json!({"name": "Platform", "lead": owner_id}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let _t1 = body_json(resp).await["id"].as_str().unwrap().to_string();

        let resp = create_team_via_api(
            &app,
            &ws_id,
            serde_json::json!({"name": "SRE", "lead": owner_id}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let t2 = body_json(resp).await["id"].as_str().unwrap().to_string();

        // Renaming t2 onto t1's slug → 409.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/teams/{t2}"))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"slug": "platform"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        // Changing the lead to a non-member → 400.
        let loner = seed_tenant_user_without_membership(&state).await;
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/teams/{t2}"))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::json!({"lead": loner}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // A valid update (lead swap to a real member) lands.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/teams/{t2}"))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::json!({"lead": dev_id}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["lead"], dev_id);
    }

    #[tokio::test]
    async fn team_management_requires_owner_admin_or_tenant_admin() {
        // Team management is an Owner/Admin capability (user-management.md
        // §UI Pages: "Team Management | Create/edit teams, manage
        // membership"). A plain Developer member must get 403 from the
        // handler — the same guard as membership management.
        let state = test_state();
        let (ws_id, owner_id) = seed_workspace_with_owner(&state).await;
        let dev_id = seed_workspace_developer(&state, &ws_id).await;

        // API key for the Developer so auth resolves through the real path.
        state
            .api_keys
            .create(
                &crate::auth::hash_api_key("dev-key-teams"),
                &Id::new(dev_id),
                "test",
            )
            .await
            .unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/teams"))
                    .header("Authorization", "Bearer dev-key-teams")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "Platform", "lead": owner_id}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "a Developer member must not be able to create teams"
        );
        assert_eq!(
            state
                .teams
                .list_by_workspace(&Id::new(ws_id))
                .await
                .unwrap()
                .len(),
            0
        );
    }

    /// GET /users/me/judgments?type=<unknown> must 400 — the same contract
    /// as the sibling notification-preferences PUT and the notifications
    /// priority param. Silently mapping an unparseable type to "no filter"
    /// returns unfiltered results the caller did not ask for (review F5).
    #[tokio::test]
    async fn judgments_unknown_type_returns_400() {
        let app = app();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/judgments?type=bogus")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "an unparseable ?type= must be rejected, not ignored"
        );

        // Every canonical JudgmentType string is accepted (no filter drop).
        for t in ["approval", "rejection", "gate", "trust", "meta-spec"] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/v1/users/me/judgments?type={t}"))
                        .header("Authorization", "Bearer test-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "type={t} must be valid");
        }
    }
}
