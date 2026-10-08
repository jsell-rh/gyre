//! Tenant and workspace invitation endpoints (user-management.md
//! §Tenant-Level User Onboarding, §Workspace Invitation Flow,
//! §Invitation Expiry — task-110).
//!
//! Tenant invitation endpoints (TenantAdmin-only, enforced per-handler like
//! POST /api/v1/users — the global dev token resolves as system/Admin and
//! must pass, so the check cannot live in ABAC middleware alone):
//! - POST   /api/v1/tenant/invite           — invite user to tenant by email
//! - POST   /api/v1/tenant/invite/bulk      — batch invite
//! - GET    /api/v1/tenant/invitations      — list (filterable by status)
//! - DELETE /api/v1/tenant/invitations/:id  — revoke a pending invitation
//!
//! Workspace invitation endpoints (Owner/Admin only, per-handler):
//! - POST   /api/v1/workspaces/:id/invite          — invite existing user
//! - GET    /api/v1/workspaces/:id/invitations     — list invitations
//! - DELETE /api/v1/workspaces/:id/invitations/:id — revoke
//!
//! Magic-link acceptance (no Bearer auth — the invitation token IS the
//! auth factor; registered outside the auth middleware chain in lib.rs):
//! - POST   /api/v1/invite/:token/accept     — accept tenant invitation
//! - POST   /api/v1/invite/:token/decline    — decline tenant invitation
//! - POST   /api/v1/workspaces/invitations/:token/accept  — accept workspace invitation
//! - POST   /api/v1/workspaces/invitations/:token/decline — decline workspace invitation
//!
//! Token handling: tokens are 32 bytes from the system CSPRNG, hex-encoded
//! (256 bits of entropy — brute force is infeasible). Only the SHA-256
//! hash is stored. Acceptance is single-use: the status transitions to
//! Accepted/Declined/Expired/Revoked, and a terminal status rejects
//! re-use of the token.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{
    GlobalRole, InvitationPolicy, InvitationStatus, TenantInvitation, User, UserRole,
    WorkspaceInvitation, WorkspaceMembership, WorkspaceRole,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::auth::AuthenticatedAgent;
use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

/// Per-tenant invitation policy overrides, stored in the kv store under
/// `invitation_policy` keyed by tenant id. Falls back to the spec defaults
/// (user-management.md §Invitation Expiry).
const POLICY_KV_NAMESPACE: &str = "invitation_policy";

/// Default interval for the expiry background job (seconds).
pub const EXPIRY_JOB_INTERVAL_SECS: u64 = 3600;

// ─── Policy ──────────────────────────────────────────────────────────────────

/// Load the invitation policy for a tenant (defaults when unset).
pub async fn load_policy(state: &AppState, tenant_id: &str) -> InvitationPolicy {
    state
        .kv_store
        .kv_get(POLICY_KV_NAMESPACE, tenant_id)
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}

// ─── Token helpers ───────────────────────────────────────────────────────────

/// Generate a cryptographically random invitation token (32 bytes, hex).
fn generate_invitation_token() -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes)
        .expect("system CSPRNG must not fail");
    hex::encode(bytes)
}

/// SHA-256 hex hash of a token — the only form persisted.
fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

// ─── Auth guards ─────────────────────────────────────────────────────────────

/// TenantAdmin-only guard for tenant invitation management. Mirrors the
/// per-handler check in `create_user`: the global dev token resolves with
/// Admin role, and the ABAC middleware is permissive by default when no
/// tenant policies exist, so enforcement must hold in the handler.
fn require_tenant_admin(auth: &AuthenticatedAgent) -> Result<(), ApiError> {
    if !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "only TenantAdmin may manage tenant invitations".to_string(),
        ));
    }
    Ok(())
}

/// Resolve the caller's identity (the inviting admin). API keys and JWTs
/// linked to a user resolve to that user; the global dev token and agent
/// tokens carry no user link, so the token's agent id (e.g. "system") is
/// the audit identity — same convention as `users.rs::resolve_user_id`.
fn caller_user_id(auth: &AuthenticatedAgent) -> Id {
    auth.user_id
        .clone()
        .unwrap_or_else(|| Id::new(auth.agent_id.clone()))
}

// ─── Request/response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct InviteRequest {
    pub email: String,
    /// "TenantAdmin" or "Member" (default).
    #[serde(default)]
    pub role: Option<String>,
    /// Pre-assigned workspaces.
    #[serde(default)]
    pub workspace_ids: Vec<String>,
    /// Role per pre-assigned workspace, parallel to workspace_ids.
    #[serde(default)]
    pub workspace_roles: Vec<String>,
    /// Expiry in days; overrides tenant policy.
    #[serde(default)]
    pub expires_in_days: Option<u32>,
}

#[derive(Deserialize)]
pub struct BulkInviteRequest {
    pub invitations: Vec<InviteRequest>,
    #[serde(default)]
    pub expires_in_days: Option<u32>,
}

#[derive(Serialize)]
pub struct InvitationResponse {
    pub id: String,
    pub tenant_id: String,
    pub email: String,
    pub invited_by: String,
    pub role: String,
    pub workspace_ids: Vec<String>,
    pub workspace_roles: Vec<String>,
    pub status: String,
    pub expires_at: u64,
    pub created_at: u64,
    pub accepted_at: Option<u64>,
    /// Magic-link URL; present only in the create response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_url: Option<String>,
}

impl From<TenantInvitation> for InvitationResponse {
    fn from(i: TenantInvitation) -> Self {
        Self {
            id: i.id.to_string(),
            tenant_id: i.tenant_id.to_string(),
            email: i.email,
            invited_by: i.invited_by.to_string(),
            role: i.role_as_str().to_string(),
            workspace_ids: i.workspace_ids.iter().map(|w| w.to_string()).collect(),
            workspace_roles: i.workspace_roles.iter().map(|r| r.as_str().to_string()).collect(),
            status: i.status.as_str().to_string(),
            expires_at: i.expires_at,
            created_at: i.created_at,
            accepted_at: i.accepted_at,
            invite_url: None,
        }
    }
}

#[derive(Deserialize, Default)]
pub struct ListInvitationsParams {
    /// Filter by status: Pending, Accepted, Declined, Expired, Revoked.
    /// Unset = all statuses.
    pub status: Option<String>,
}

// ─── Tenant invitation endpoints ─────────────────────────────────────────────

/// POST /api/v1/tenant/invite
///
/// TenantAdmin-only. Creates a pending invitation with a CSPRNG token
/// (stored hashed), expires per tenant policy or request override.
pub async fn invite_to_tenant(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Json(req): Json<InviteRequest>,
) -> Result<(StatusCode, Json<InvitationResponse>), ApiError> {
    require_tenant_admin(&auth)?;
    let response = create_tenant_invitation(&state, &auth, req).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

/// Shared creation path for single and bulk invite.
async fn create_tenant_invitation(
    state: &AppState,
    auth: &AuthenticatedAgent,
    req: InviteRequest,
) -> Result<InvitationResponse, ApiError> {
    let email = req.email.trim().to_lowercase();
    if !is_plausible_email(&email) {
        return Err(ApiError::InvalidInput(format!(
            "invalid email: {}",
            req.email
        )));
    }

    let role = match req.role.as_deref() {
        None | Some("Member") => GlobalRole::Member,
        Some("TenantAdmin") => GlobalRole::TenantAdmin,
        Some(other) => {
            return Err(ApiError::InvalidInput(format!(
                "unknown role: {other} (expected TenantAdmin or Member)"
            )))
        }
    };

    // Pre-assigned workspaces: roles must be valid and parallel in length
    // (spec: workspace_roles is "role per pre-assigned workspace").
    let mut workspace_ids = Vec::with_capacity(req.workspace_ids.len());
    for w in &req.workspace_ids {
        workspace_ids.push(Id::new(w.clone()));
    }
    let mut workspace_roles = Vec::with_capacity(req.workspace_roles.len());
    for r in &req.workspace_roles {
        workspace_roles.push(WorkspaceRole::parse_role(r).ok_or_else(|| {
            ApiError::InvalidInput(format!("unknown workspace role: {r}"))
        })?);
    }
    if !workspace_roles.is_empty() && workspace_roles.len() != workspace_ids.len() {
        return Err(ApiError::InvalidInput(
            "workspace_roles must be parallel to workspace_ids (one role per workspace)"
                .to_string(),
        ));
    }
    // When roles are omitted entirely, default every pre-assignment to Developer.
    if workspace_roles.is_empty() && !workspace_ids.is_empty() {
        workspace_roles = vec![WorkspaceRole::Developer; workspace_ids.len()];
    }

    let policy = load_policy(state, &auth.tenant_id).await;
    let days = req.expires_in_days.unwrap_or(policy.tenant_invite_expiry_days);
    if days == 0 {
        return Err(ApiError::InvalidInput(
            "expires_in_days must be at least 1".to_string(),
        ));
    }

    let now = now_secs();
    let token = generate_invitation_token();
    let invitation = TenantInvitation {
        id: new_id(),
        tenant_id: Id::new(auth.tenant_id.clone()),
        email: email.clone(),
        invited_by: caller_user_id(auth),
        role,
        workspace_ids,
        workspace_roles,
        status: InvitationStatus::Pending,
        token_hash: hash_token(&token),
        expires_at: now + (days as u64) * 86_400,
        created_at: now,
        accepted_at: None,
    };
    // Duplicate guard before insert: a pending invitation for the same
    // (tenant, email) is a 409 the caller can act on (revoke or wait for
    // expiry). The repository contract enforces the same invariant as a
    // race backstop, but its anyhow error would surface as a 500 here.
    let existing_pending = state
        .tenant_invitations
        .list_by_tenant(&Id::new(auth.tenant_id.clone()))
        .await?
        .into_iter()
        .any(|i| i.email == email && i.status == InvitationStatus::Pending);
    if existing_pending {
        return Err(ApiError::Conflict(format!(
            "a pending invitation for {email} in tenant {} already exists",
            auth.tenant_id
        )));
    }
    state.tenant_invitations.create(&invitation).await?;

    let invitee_note = serde_json::json!({
        "email": email,
        "role": invitation.role_as_str(),
        "workspace_ids": invitation.workspace_ids.iter().map(|w| w.to_string()).collect::<Vec<_>>(),
    });
    tracing::info!(
        tenant_id = %auth.tenant_id,
        invited_by = %auth.user_id.map(|i| i.to_string()).unwrap_or_default(),
        invitation_id = %invitation.id,
        "tenant invitation created: {}",
        serde_json::to_string(&invitee_note).unwrap_or_default()
    );

    let mut response = InvitationResponse::from(invitation);
    response.invite_url = Some(format!("{}/invite/{}", state.base_url, token));
    Ok(response)
}

/// POST /api/v1/tenant/invite/bulk
///
/// TenantAdmin-only. Creates one invitation per entry; per-entry validation
/// failures are reported without aborting the whole batch (partial success
/// with an errors array), because bulk CSV-style onboarding must not roll
/// back 99 valid rows over one bad one.
pub async fn bulk_invite_to_tenant(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Json(req): Json<BulkInviteRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_tenant_admin(&auth)?;
    if req.invitations.is_empty() {
        return Err(ApiError::InvalidInput(
            "invitations must contain at least one entry".to_string(),
        ));
    }

    let mut created = Vec::new();
    let mut errors = Vec::new();
    for (idx, entry) in req.invitations.into_iter().enumerate() {
        // Bulk-level expiry default applies when the entry omits one.
        let entry = if entry.expires_in_days.is_none() {
            InviteRequest {
                expires_in_days: req.expires_in_days,
                ..entry
            }
        } else {
            entry
        };
        match create_tenant_invitation(&state, &auth, entry).await {
            Ok(resp) => created.push(serde_json::json!({
                "email": resp.email,
                "id": resp.id,
                "invite_url": resp.invite_url,
            })),
            Err(e) => errors.push(serde_json::json!({
                "index": idx,
                "error": e.to_string(),
            })),
        }
    }

    Ok(Json(serde_json::json!({
        "created": created,
        "errors": errors,
        "created_count": created.len(),
        "error_count": errors.len(),
    })))
}

/// GET /api/v1/tenant/invitations?status=
///
/// TenantAdmin-only. Lists the caller's tenant's invitations, optionally
/// filtered by status.
pub async fn list_tenant_invitations(
    auth: AuthenticatedAgent,
    Query(params): Query<ListInvitationsParams>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_tenant_admin(&auth)?;
    let status_filter = match params.status.as_deref() {
        None => None,
        Some(s) => Some(
            InvitationStatus::parse(s)
                .ok_or_else(|| ApiError::InvalidInput(format!("unknown status: {s}")))?,
        ),
    };

    let tenant_id = Id::new(auth.tenant_id.clone());
    let all = state.tenant_invitations.list_by_tenant(&tenant_id).await?;
    let items: Vec<InvitationResponse> = all
        .into_iter()
        .filter(|i| status_filter.is_none_or(|f| i.status == f))
        .map(Into::into)
        .collect();
    Ok(Json(serde_json::json!({ "invitations": items })))
}

/// DELETE /api/v1/tenant/invitations/:id
///
/// TenantAdmin-only. Revokes a pending invitation (terminal status; the
/// row is kept for audit per spec §Invitation Expiry).
pub async fn revoke_tenant_invitation(
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    require_tenant_admin(&auth)?;
    let invitation = state
        .tenant_invitations
        .find_by_id(&Id::new(id.clone()))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("invitation {id} not found")))?;

    // Tenant containment: the invitation must belong to the caller's tenant.
    if invitation.tenant_id.as_str() != auth.tenant_id {
        return Err(ApiError::Forbidden(
            "invitation belongs to a different tenant".to_string(),
        ));
    }
    if invitation.status != InvitationStatus::Pending {
        return Err(ApiError::Conflict(format!(
            "invitation is {} — only pending invitations can be revoked",
            invitation.status.as_str()
        )));
    }

    state
        .tenant_invitations
        .update_status(&invitation.id, InvitationStatus::Revoked, None)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Tenant invitation acceptance (magic link) ───────────────────────────────

#[derive(Deserialize, Default)]
pub struct AcceptInviteRequest {
    /// Local mode: display name for the new account (defaults to the email
    /// local-part). SSO mode ignores this and links on next login.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Optional username; defaults to the email local-part.
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Serialize)]
pub struct AcceptInviteResponse {
    pub user_id: String,
    pub username: String,
    /// "created" (local mode) or "linked" (existing SSO account linked).
    pub mode: String,
    pub tenant_id: String,
    /// Workspaces activated by this acceptance.
    pub workspaces: Vec<String>,
}

/// POST /api/v1/invite/:token/accept
///
/// Accepts a tenant invitation via magic-link token. No Bearer auth: the
/// 256-bit CSPRNG token in the URL is the auth factor (same trust model as
/// the magic link the invitee received by email).
///
/// Flow (spec §Tenant invitation flow):
/// - token → invitation (hash lookup), must be Pending and unexpired
/// - Local mode: create the user (external_id namespaced by tenant+email so
///   the same email can exist in different tenants)
/// - SSO mode: if a user with the invitation email already exists in the
///   tenant, link (activate pre-assigned memberships); otherwise create a
///   local-mode account now — it will merge with the SSO subject on first
///   login via the existing find_or_create_user path only if emails match.
/// - Activate pre-assigned workspace memberships.
pub async fn accept_tenant_invitation(
    Path(token): Path<String>,
    State(state): State<Arc<AppState>>,
    body: Option<Json<AcceptInviteRequest>>,
) -> Result<(StatusCode, Json<AcceptInviteResponse>), ApiError> {
    let req = body.map(|Json(r)| r).unwrap_or_default();

    let token_hash = hash_token(&token);
    let invitation = state
        .tenant_invitations
        .find_by_token_hash(&token_hash)
        .await?
        .ok_or_else(|| ApiError::NotFound("invitation not found".to_string()))?;

    if invitation.status != InvitationStatus::Pending {
        return Err(ApiError::Conflict(format!(
            "invitation is {}",
            invitation.status.as_str()
        )));
    }
    let now = now_secs();
    if invitation.is_expired(now) {
        // Persist the Expired status now (the background job would also
        // catch it, but the caller deserves the terminal state immediately).
        state
            .tenant_invitations
            .update_status(&invitation.id, InvitationStatus::Expired, None)
            .await?;
        return Err(ApiError::Conflict("invitation has expired".to_string()));
    }

    // Find or create the user in this tenant.
    let email = invitation.email.clone();
    let tenant_ns = format!("tenant:{}:email:{}", invitation.tenant_id, email);
    let (user, mode) = match state.users.find_by_external_id(&tenant_ns).await? {
        Some(existing) => (existing, "linked"),
        None => {
            let local_part = email.split('@').next().unwrap_or("user");
            let username = req
                .username
                .clone()
                .unwrap_or_else(|| local_part.to_string());
            let display_name = req
                .display_name
                .clone()
                .unwrap_or_else(|| username.clone());
            let mut user = User::new(new_id(), tenant_ns, username, now);
            user.display_name = display_name;
            user.email = Some(email.clone());
            user.tenant_id = Some(invitation.tenant_id.clone());
            user.global_role = invitation.role.clone();
            user.roles = match invitation.role {
                GlobalRole::TenantAdmin => vec![UserRole::Admin],
                GlobalRole::Member => vec![UserRole::Developer],
            };
            state.users.create(&user).await?;
            (user, "created")
        }
    };

    // Activate pre-assigned workspace memberships (idempotent).
    let mut activated = Vec::new();
    let roles = if invitation.workspace_roles.is_empty() {
        vec![WorkspaceRole::Developer; invitation.workspace_ids.len()]
    } else {
        invitation.workspace_roles.clone()
    };
    for (ws_id, role) in invitation.workspace_ids.iter().zip(roles) {
        // Cross-tenant guard: only pre-assign workspaces that actually
        // belong to the invitation's tenant.
        let valid = state
            .workspaces
            .find_by_id(ws_id)
            .await
            .map(|w| w.is_some_and(|w| w.tenant_id == invitation.tenant_id))
            .unwrap_or(false);
        if !valid {
            tracing::warn!(
                workspace_id = %ws_id,
                tenant_id = %invitation.tenant_id,
                "skipping pre-assigned workspace from another tenant"
            );
            continue;
        }
        let existing = state
            .workspace_memberships
            .find_by_user_and_workspace(&user.id, ws_id)
            .await?;
        if existing.is_some() {
            continue;
        }
        let membership = WorkspaceMembership::new(
            new_id(),
            user.id.clone(),
            ws_id.clone(),
            role,
            invitation.invited_by.clone(),
            now,
        );
        state.workspace_memberships.create(&membership).await?;
        activated.push(ws_id.to_string());
    }

    state
        .tenant_invitations
        .update_status(&invitation.id, InvitationStatus::Accepted, Some(now))
        .await?;

    Ok((
        StatusCode::OK,
        Json(AcceptInviteResponse {
            user_id: user.id.to_string(),
            username: user.username.clone(),
            mode: mode.to_string(),
            tenant_id: invitation.tenant_id.to_string(),
            workspaces: activated,
        }),
    ))
}

/// POST /api/v1/invite/:token/decline
pub async fn decline_tenant_invitation(
    Path(token): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let token_hash = hash_token(&token);
    let invitation = state
        .tenant_invitations
        .find_by_token_hash(&token_hash)
        .await?
        .ok_or_else(|| ApiError::NotFound("invitation not found".to_string()))?;
    if invitation.status != InvitationStatus::Pending {
        return Err(ApiError::Conflict(format!(
            "invitation is {}",
            invitation.status.as_str()
        )));
    }
    if invitation.is_expired(now_secs()) {
        state
            .tenant_invitations
            .update_status(&invitation.id, InvitationStatus::Expired, None)
            .await?;
        return Err(ApiError::Conflict("invitation has expired".to_string()));
    }
    state
        .tenant_invitations
        .update_status(&invitation.id, InvitationStatus::Declined, None)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Workspace invitation endpoints ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct WorkspaceInviteRequest {
    /// User id of an existing tenant user.
    pub user_id: String,
    #[serde(default = "default_ws_role")]
    pub role: String,
    #[serde(default)]
    pub expires_in_days: Option<u32>,
}

fn default_ws_role() -> String {
    "Developer".to_string()
}

#[derive(Serialize)]
pub struct WorkspaceInvitationResponse {
    pub id: String,
    pub workspace_id: String,
    pub user_id: String,
    pub invited_by: String,
    pub role: String,
    pub status: String,
    pub expires_at: u64,
    pub created_at: u64,
    pub accepted_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_url: Option<String>,
}

impl From<WorkspaceInvitation> for WorkspaceInvitationResponse {
    fn from(i: WorkspaceInvitation) -> Self {
        Self {
            id: i.id.to_string(),
            workspace_id: i.workspace_id.to_string(),
            user_id: i.user_id.to_string(),
            invited_by: i.invited_by.to_string(),
            role: i.role.as_str().to_string(),
            status: i.status.as_str().to_string(),
            expires_at: i.expires_at,
            created_at: i.created_at,
            accepted_at: i.accepted_at,
            invite_url: None,
        }
    }
}

/// POST /api/v1/workspaces/:id/invite
///
/// Owner/Admin-only (per-handler). Invites an existing tenant user to the
/// workspace. Cross-tenant invites are rejected (spec: tenants are hard
/// isolation boundaries).
pub async fn invite_to_workspace(
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<WorkspaceInviteRequest>,
) -> Result<(StatusCode, Json<WorkspaceInvitationResponse>), ApiError> {
    let ws_id = Id::new(workspace_id.clone());
    let workspace = state
        .workspaces
        .find_by_id(&ws_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {workspace_id} not found")))?;

    // Tenant containment + role guard: caller must be an Owner/Admin member
    // of this workspace, or a TenantAdmin (tenant scope operators can
    // manage any workspace in their tenant).
    if workspace.tenant_id.as_str() != auth.tenant_id {
        return Err(ApiError::Forbidden(
            "workspace belongs to a different tenant".to_string(),
        ));
    }
    let caller_id = caller_user_id(&auth);
    let caller_membership = state
        .workspace_memberships
        .find_by_user_and_workspace(&caller_id, &ws_id)
        .await?;
    let authorized = auth.roles.contains(&UserRole::Admin)
        || caller_membership.is_some_and(|m| {
            matches!(m.role, WorkspaceRole::Owner | WorkspaceRole::Admin)
        });
    if !authorized {
        return Err(ApiError::Forbidden(
            "only workspace Owner/Admin or TenantAdmin may invite".to_string(),
        ));
    }

    let role = WorkspaceRole::parse_role(&req.role)
        .ok_or_else(|| ApiError::InvalidInput(format!("unknown role: {}", req.role)))?;
    let user_id = Id::new(req.user_id.clone());

    // The invitee must exist and belong to the same tenant. A user with no
    // tenant has no verifiable scope — reject rather than fabricate one.
    let invitee = state
        .users
        .find_by_id(&user_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("user {user_id} not found")))?;
    let invitee_tenant = invitee
        .tenant_id
        .as_ref()
        .map(|t| t.as_str())
        .ok_or_else(|| {
            ApiError::Forbidden(format!(
                "user {user_id} has no tenant scope; cannot verify tenant containment"
            ))
        })?;
    if invitee_tenant != workspace.tenant_id.as_str() {
        return Err(ApiError::Forbidden(
            "cross-tenant workspace invitations are not supported".to_string(),
        ));
    }

    // Already a member → conflict, no invitation needed.
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

    // max_pending_invitations cap per workspace.
    let policy = load_policy(&state, &auth.tenant_id).await;
    let ws_pending = state
        .workspace_invitations
        .list_by_workspace(&ws_id)
        .await?;
    // Duplicate guard before insert: a pending invitation for the same
    // (workspace, user) is a 409 (the repository contract remains the race
    // backstop, but its anyhow error would surface as a 500 here).
    if ws_pending
        .iter()
        .any(|i| i.user_id == user_id && i.status == InvitationStatus::Pending)
    {
        return Err(ApiError::Conflict(format!(
            "a pending invitation for user {user_id} in workspace {workspace_id} already exists"
        )));
    }
    // max_pending_invitations cap per workspace.
    let pending = ws_pending
        .into_iter()
        .filter(|i| i.status == InvitationStatus::Pending)
        .count() as u32;
    if pending >= policy.max_pending_invitations {
        return Err(ApiError::TooManyRequests(format!(
            "workspace {} has reached the max of {} pending invitations",
            workspace_id, policy.max_pending_invitations
        )));
    }

    let days = req
        .expires_in_days
        .unwrap_or(policy.workspace_invite_expiry_days);
    if days == 0 {
        return Err(ApiError::InvalidInput(
            "expires_in_days must be at least 1".to_string(),
        ));
    }

    let now = now_secs();
    let token = generate_invitation_token();
    let invitation = WorkspaceInvitation {
        id: new_id(),
        tenant_id: workspace.tenant_id.clone(),
        workspace_id: ws_id.clone(),
        user_id: user_id.clone(),
        invited_by: caller_id,
        role,
        status: InvitationStatus::Pending,
        token_hash: hash_token(&token),
        expires_at: now + (days as u64) * 86_400,
        created_at: now,
        accepted_at: None,
    };
    state.workspace_invitations.create(&invitation).await?;

    // In-app notification to the invitee (spec step 3: "in-app always").
    let notif = gyre_common::Notification::new(
        new_id(),
        ws_id.clone(),
        user_id.clone(),
        gyre_common::NotificationType::TrustSuggestion,
        format!(
            "You have been invited to workspace {} with role {}",
            workspace_id,
            invitation.role.as_str()
        ),
        workspace.tenant_id.clone(),
        now as i64,
    );
    let _ = state.notifications.create(&notif).await;

    let mut response = WorkspaceInvitationResponse::from(invitation);
    response.invite_url = Some(format!(
        "{}/workspaces/invitations/{}/accept",
        state.base_url, token
    ));
    Ok((StatusCode::CREATED, Json(response)))
}

/// GET /api/v1/workspaces/:id/invitations
///
/// Lists workspace invitations. Caller must be a member of the workspace
/// (any role) or TenantAdmin.
pub async fn list_workspace_invitations(
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ws_id = Id::new(workspace_id.clone());
    let workspace = state
        .workspaces
        .find_by_id(&ws_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {workspace_id} not found")))?;
    if workspace.tenant_id.as_str() != auth.tenant_id {
        return Err(ApiError::Forbidden(
            "workspace belongs to a different tenant".to_string(),
        ));
    }
    let caller_id = caller_user_id(&auth);
    let member = state
        .workspace_memberships
        .find_by_user_and_workspace(&caller_id, &ws_id)
        .await?;
    if !auth.roles.contains(&UserRole::Admin) && member.is_none() {
        return Err(ApiError::Forbidden(
            "only workspace members or TenantAdmin may list invitations".to_string(),
        ));
    }

    let items: Vec<WorkspaceInvitationResponse> = state
        .workspace_invitations
        .list_by_workspace(&ws_id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(serde_json::json!({ "invitations": items })))
}

/// DELETE /api/v1/workspaces/:id/invitations/:id
///
/// Owner/Admin/TenantAdmin-only. Revokes a pending workspace invitation.
pub async fn revoke_workspace_invitation(
    auth: AuthenticatedAgent,
    Path((workspace_id, invitation_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let ws_id = Id::new(workspace_id.clone());
    let workspace = state
        .workspaces
        .find_by_id(&ws_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {workspace_id} not found")))?;
    if workspace.tenant_id.as_str() != auth.tenant_id {
        return Err(ApiError::Forbidden(
            "workspace belongs to a different tenant".to_string(),
        ));
    }
    let caller_id = caller_user_id(&auth);
    let caller_membership = state
        .workspace_memberships
        .find_by_user_and_workspace(&caller_id, &ws_id)
        .await?;
    let authorized = auth.roles.contains(&UserRole::Admin)
        || caller_membership.is_some_and(|m| {
            matches!(m.role, WorkspaceRole::Owner | WorkspaceRole::Admin)
        });
    if !authorized {
        return Err(ApiError::Forbidden(
            "only workspace Owner/Admin or TenantAdmin may revoke invitations".to_string(),
        ));
    }

    let invitation = state
        .workspace_invitations
        .find_by_id(&Id::new(invitation_id.clone()))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("invitation {invitation_id} not found")))?;
    if invitation.workspace_id != ws_id {
        return Err(ApiError::NotFound(format!(
            "invitation {invitation_id} not found in workspace {workspace_id}"
        )));
    }
    if invitation.status != InvitationStatus::Pending {
        return Err(ApiError::Conflict(format!(
            "invitation is {} — only pending invitations can be revoked",
            invitation.status.as_str()
        )));
    }
    state
        .workspace_invitations
        .update_status(&invitation.id, InvitationStatus::Revoked, None)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Workspace invitation acceptance (token) ─────────────────────────────────

/// POST /api/v1/workspaces/invitations/:token/accept
///
/// Accepts a workspace invitation by token. Auth optional: the token is
/// the auth factor; when a Bearer identity IS provided it must match the
/// invited user (prevents token forwarding accepting on someone's behalf
/// under a different identity).
pub async fn accept_workspace_invitation(
    auth: Option<AuthenticatedAgent>,
    Path(token): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let token_hash = hash_token(&token);
    let invitation = state
        .workspace_invitations
        .find_by_token_hash(&token_hash)
        .await?
        .ok_or_else(|| ApiError::NotFound("invitation not found".to_string()))?;
    if invitation.status != InvitationStatus::Pending {
        return Err(ApiError::Conflict(format!(
            "invitation is {}",
            invitation.status.as_str()
        )));
    }
    if let Some(auth) = &auth {
        let caller = caller_user_id(auth);
        if caller != invitation.user_id {
            return Err(ApiError::Forbidden(
                "this invitation was issued to a different user".to_string(),
            ));
        }
    }
    let now = now_secs();
    if invitation.is_expired(now) {
        state
            .workspace_invitations
            .update_status(&invitation.id, InvitationStatus::Expired, None)
            .await?;
        return Err(ApiError::Conflict("invitation has expired".to_string()));
    }

    // Idempotent membership activation.
    let existing = state
        .workspace_memberships
        .find_by_user_and_workspace(&invitation.user_id, &invitation.workspace_id)
        .await?;
    let membership = match existing {
        Some(mut m) => {
            if !m.accepted {
                m.accept(now);
                state.workspace_memberships.accept(&m.id, now).await?;
            }
            m
        }
        None => {
            let m = WorkspaceMembership::new(
                new_id(),
                invitation.user_id.clone(),
                invitation.workspace_id.clone(),
                invitation.role.clone(),
                invitation.invited_by.clone(),
                now,
            );
            // activate immediately — acceptance IS the activation.
            let mut m = m;
            m.accept(now);
            state.workspace_memberships.create(&m).await?;
            m
        }
    };

    state
        .workspace_invitations
        .update_status(&invitation.id, InvitationStatus::Accepted, Some(now))
        .await?;

    tracing::info!(
        workspace_id = %invitation.workspace_id,
        user_id = %invitation.user_id,
        membership_id = %membership.id,
        "workspace invitation accepted"
    );
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/workspaces/invitations/:token/decline
pub async fn decline_workspace_invitation(
    auth: Option<AuthenticatedAgent>,
    Path(token): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let token_hash = hash_token(&token);
    let invitation = state
        .workspace_invitations
        .find_by_token_hash(&token_hash)
        .await?
        .ok_or_else(|| ApiError::NotFound("invitation not found".to_string()))?;
    if invitation.status != InvitationStatus::Pending {
        return Err(ApiError::Conflict(format!(
            "invitation is {}",
            invitation.status.as_str()
        )));
    }
    if let Some(auth) = &auth {
        let caller = caller_user_id(auth);
        if caller != invitation.user_id {
            return Err(ApiError::Forbidden(
                "this invitation was issued to a different user".to_string(),
            ));
        }
    }
    if invitation.is_expired(now_secs()) {
        state
            .workspace_invitations
            .update_status(&invitation.id, InvitationStatus::Expired, None)
            .await?;
        return Err(ApiError::Conflict("invitation has expired".to_string()));
    }
    state
        .workspace_invitations
        .update_status(&invitation.id, InvitationStatus::Declined, None)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Expiry job ──────────────────────────────────────────────────────────────

/// Run one invitation-expiry cycle: marks all Pending invitations past
/// `expires_at` as Expired (kept for audit, never deleted).
pub async fn run_expiry_once(state: &AppState) -> anyhow::Result<u64> {
    let now = now_secs();
    let mut expired_count = 0u64;

    for inv in state
        .tenant_invitations
        .list_by_status(InvitationStatus::Pending)
        .await?
    {
        if inv.is_expired(now) {
            state
                .tenant_invitations
                .update_status(&inv.id, InvitationStatus::Expired, None)
                .await?;
            expired_count += 1;
        }
    }

    for inv in state
        .workspace_invitations
        .list_by_status(InvitationStatus::Pending)
        .await?
    {
        if inv.is_expired(now) {
            state
                .workspace_invitations
                .update_status(&inv.id, InvitationStatus::Expired, None)
                .await?;
            expired_count += 1;
        }
    }

    if expired_count > 0 {
        tracing::info!(count = expired_count, "marked invitations as expired");
    }
    Ok(expired_count)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

impl TenantInvitation {
    fn role_as_str(&self) -> &'static str {
        match self.role {
            GlobalRole::TenantAdmin => "TenantAdmin",
            GlobalRole::Member => "Member",
        }
    }
}

/// Minimal RFC-ish email sanity check (exactly one @, non-empty local part
/// and domain, domain contains a dot). Full RFC 5322 validation is not the
/// point — the address is a delivery target, not parsed input.
fn is_plausible_email(email: &str) -> bool {
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() != 2 {
        return false;
    }
    let (local, domain) = (parts[0], parts[1]);
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{delete, get, post},
        Router,
    };
    use tower::ServiceExt;

    fn app() -> Router {
        let state = test_state();
        Router::new()
            .route("/api/v1/tenant/invite", post(invite_to_tenant))
            .route("/api/v1/tenant/invite/bulk", post(bulk_invite_to_tenant))
            .route("/api/v1/tenant/invitations", get(list_tenant_invitations))
            .route(
                "/api/v1/tenant/invitations/:id",
                delete(revoke_tenant_invitation),
            )
            .route("/api/v1/invite/:token/accept", post(accept_tenant_invitation))
            .route("/api/v1/invite/:token/decline", post(decline_tenant_invitation))
            .route(
                "/api/v1/workspaces/:id/invite",
                post(invite_to_workspace),
            )
            .route(
                "/api/v1/workspaces/:id/invitations",
                get(list_workspace_invitations),
            )
            .route(
                "/api/v1/workspaces/:id/invitations/:id",
                delete(revoke_workspace_invitation),
            )
            .route(
                "/api/v1/workspaces/invitations/:token/accept",
                post(accept_workspace_invitation),
            )
            .route(
                "/api/v1/workspaces/invitations/:token/decline",
                post(decline_workspace_invitation),
            )
            .with_state(state)
    }


    // NOTE: these tests run against `test_state()` whose auth_token is
    // "test-token" (mem.rs). The extractor resolves it as system/Admin.

    #[tokio::test]
    async fn tenant_invite_lifecycle_create_list_revoke() {
        let app = app();

        // Create.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenant/invite")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "email": "Alice@Example.com",
                            "role": "Member",
                            "workspace_ids": [],
                            "workspace_roles": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["email"], "alice@example.com", "email is normalized");
        assert_eq!(json["status"], "Pending");
        let invite_url = json["invite_url"].as_str().unwrap().to_string();
        assert!(invite_url.contains("/invite/"), "magic link returned");
        let inv_id = json["id"].as_str().unwrap().to_string();
        // Token is never stored in plaintext: the stored hash is SHA-256 of
        // the URL token.
        let token = invite_url.rsplit('/').next().unwrap().to_string();

        // List.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/tenant/invitations?status=Pending")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["invitations"].as_array().unwrap().len(), 1);

        // Duplicate pending invitation for the same email → 409 (Conflict
        // via the repository's port contract).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenant/invite")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": "alice@example.com"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        // Accept via magic link token.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/invite/{token}/accept"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"display_name": "Alice Smith"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["mode"], "created");
        assert_eq!(json["username"], "alice");

        // Token is single-use: second accept → 409.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/invite/{token}/accept"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        // Revoke after acceptance → 409 (not pending).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/tenant/invitations/{inv_id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn tenant_invite_revocation_blocks_accept() {
        let app = app();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenant/invite")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": "bob@example.com"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let inv_id = json["id"].as_str().unwrap().to_string();
        let token = json["invite_url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();

        // Revoke.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/tenant/invitations/{inv_id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Accepting a revoked invitation fails.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/invite/{token}/accept"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn expired_invitation_cannot_be_accepted_and_job_marks_expired() {
        let state = test_state();
        // Seed an already-expired pending invitation directly through the
        // repository (the API enforces >= 1 day expiry, so we cannot create
        // an expired one through the endpoint).
        let invitation = TenantInvitation {
            id: Id::new("expired-inv"),
            tenant_id: Id::new("default"),
            email: "expired@example.com".to_string(),
            invited_by: Id::new("someadmin"),
            role: GlobalRole::Member,
            workspace_ids: vec![],
            workspace_roles: vec![],
            status: InvitationStatus::Pending,
            token_hash: hash_token("expired-token"),
            expires_at: now_secs() - 10,
            created_at: now_secs() - 100,
            accepted_at: None,
        };
        state.tenant_invitations.create(&invitation).await.unwrap();
        // Second still-pending expired invitation the accept path never
        // touches — the expiry job must be the one to mark it.
        let untouched = TenantInvitation {
            id: Id::new("expired-inv-2"),
            tenant_id: Id::new("default"),
            email: "expired2@example.com".to_string(),
            invited_by: Id::new("someadmin"),
            role: GlobalRole::Member,
            workspace_ids: vec![],
            workspace_roles: vec![],
            status: InvitationStatus::Pending,
            token_hash: hash_token("expired-token-2"),
            expires_at: now_secs() - 5,
            created_at: now_secs() - 100,
            accepted_at: None,
        };
        state.tenant_invitations.create(&untouched).await.unwrap();

        // Accept → 409 expired, and status transitions to Expired.
        let app = Router::new()
            .route("/api/v1/invite/:token/accept", post(accept_tenant_invitation))
            .with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/invite/expired-token/accept")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let stored = state
            .tenant_invitations
            .find_by_id(&Id::new("expired-inv"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, InvitationStatus::Expired);

        // The expiry job marks the remaining pending+past invitations —
        // the second seed is still Pending, so the job must mark it.
        let marked = run_expiry_once(&state).await.unwrap();
        assert_eq!(marked, 1, "job marks exactly the untouched seed");
        let marked_inv = state
            .tenant_invitations
            .find_by_id(&Id::new("expired-inv-2"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(marked_inv.status, InvitationStatus::Expired);
    }

    #[tokio::test]
    async fn bulk_invite_creates_all_and_reports_errors() {
        let app = app();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenant/invite/bulk")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "invitations": [
                                {"email": "one@example.com"},
                                {"email": "two@example.com", "workspace_ids": [], "workspace_roles": []},
                                {"email": "not-an-email"},
                                {"email": "three@example.com", "role": "BogusRole"}
                            ]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["created_count"], 2);
        assert_eq!(json["error_count"], 2);
    }

    #[tokio::test]
    async fn non_admin_cannot_invite() {
        let state = test_state();
        // A Developer-role user's API key... simplest: craft an
        // AuthenticatedAgent-equivalent by using a non-admin token. The
        // test_state dev token is Admin; use an agent token (Agent role).
        // Agent tokens live in kv_store("agent_tokens") — but seeding one
        // there requires the token to resolve through the extractor. The
        // direct unit-level check is simpler: call the guard.
        let auth = AuthenticatedAgent {
            agent_id: "agent-1".to_string(),
            user_id: None,
            roles: vec![UserRole::Agent],
            tenant_id: "default".to_string(),
            jwt_claims: None,
            deprecated_token_auth: false,
        };
        assert!(require_tenant_admin(&auth).is_err());
    }

    #[tokio::test]
    async fn workspace_invitation_full_lifecycle() {
        let state = test_state();
        // Seed: workspace + inviter (Owner) + invitee user.
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-lc"),
            Id::new("default"),
            "LC",
            "lc",
            now_secs(),
        );
        state.workspaces.create(&ws).await.unwrap();
        let mut owner = User::new(Id::new("owner-1"), "tenant:default:email:o@x.com", "owner", now_secs());
        owner.tenant_id = Some(Id::new("default"));
        state.users.create(&owner).await.unwrap();
        let mut invitee = User::new(Id::new("invitee-1"), "tenant:default:email:i@x.com", "invitee", now_secs());
        invitee.tenant_id = Some(Id::new("default"));
        state.users.create(&invitee).await.unwrap();
        let owner_membership = WorkspaceMembership::new(
            Id::new("m-owner"),
            owner.id.clone(),
            ws.id.clone(),
            WorkspaceRole::Owner,
            owner.id.clone(),
            now_secs(),
        );
        state.workspace_memberships.create(&owner_membership).await.unwrap();

        // Owner's API key: create one so the extractor resolves user_id.
        let raw_key = format!("gyre_{}", uuid::Uuid::new_v4().simple());
        state
            .api_keys
            .create(
                &crate::auth::hash_api_key(&raw_key),
                &owner.id,
                "test",
            )
            .await
            .unwrap();

        let app = Router::new()
            .route("/api/v1/workspaces/:id/invite", post(invite_to_workspace))
            .route(
                "/api/v1/workspaces/:id/invitations",
                get(list_workspace_invitations),
            )
            .route(
                "/api/v1/workspaces/:id/invitations/:id",
                delete(revoke_workspace_invitation),
            )
            .route(
                "/api/v1/workspaces/invitations/:token/accept",
                post(accept_workspace_invitation),
            )
            .route(
                "/api/v1/workspaces/invitations/:token/decline",
                post(decline_workspace_invitation),
            )
            .with_state(state.clone());
        let auth_header = format!("Bearer {raw_key}");

        // Invite.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-lc/invite")
                    .header("authorization", auth_header.clone())
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"user_id": "invitee-1", "role": "Developer"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "Pending");
        let token = json["invite_url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .nth(1)
            .unwrap()
            .to_string();

        // Duplicate pending invite for the same user → 409.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-lc/invite")
                    .header("authorization", auth_header.clone())
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"user_id": "invitee-1", "role": "Viewer"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        // Accept.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/invitations/{token}/accept"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Membership is now active and accepted.
        let m = state
            .workspace_memberships
            .find_by_user_and_workspace(&invitee.id, &ws.id)
            .await
            .unwrap()
            .expect("membership exists after acceptance");
        assert!(m.accepted, "membership is activated on acceptance");
        assert_eq!(m.role, WorkspaceRole::Developer);

        // Second accept → 409 (single-use).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/invitations/{token}/accept"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn workspace_invitation_decline_leaves_no_membership() {
        let state = test_state();
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-d"),
            Id::new("default"),
            "D",
            "d",
            now_secs(),
        );
        state.workspaces.create(&ws).await.unwrap();
        let invitation = WorkspaceInvitation {
            id: Id::new("wi-d"),
            tenant_id: Id::new("default"),
            workspace_id: ws.id.clone(),
            user_id: Id::new("u-d"),
            invited_by: Id::new("owner-d"),
            role: WorkspaceRole::Viewer,
            status: InvitationStatus::Pending,
            token_hash: hash_token("decline-token"),
            expires_at: now_secs() + 86_400,
            created_at: now_secs(),
            accepted_at: None,
        };
        state.workspace_invitations.create(&invitation).await.unwrap();

        let app = Router::new()
            .route(
                "/api/v1/workspaces/invitations/:token/decline",
                post(decline_workspace_invitation),
            )
            .with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/invitations/decline-token/decline")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let stored = state
            .workspace_invitations
            .find_by_id(&Id::new("wi-d"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, InvitationStatus::Declined);
        assert!(
            state
                .workspace_memberships
                .find_by_user_and_workspace(&Id::new("u-d"), &ws.id)
                .await
                .unwrap()
                .is_none(),
            "declining must not create a membership"
        );
    }

    #[tokio::test]
    async fn email_validation_rejects_garbage() {
        assert!(is_plausible_email("a@b.com"));
        assert!(!is_plausible_email("no-at-sign"));
        assert!(!is_plausible_email("@b.com"));
        assert!(!is_plausible_email("a@"));
        assert!(!is_plausible_email("a@b"));
        assert!(!is_plausible_email("a@.b"));
        assert!(!is_plausible_email("a@b."));
        assert!(!is_plausible_email("a@b@c.com"));
    }

    #[tokio::test]
    async fn token_is_csprng_and_hashed() {
        let t1 = generate_invitation_token();
        let t2 = generate_invitation_token();
        assert_eq!(t1.len(), 64, "32 bytes hex-encoded");
        assert_ne!(t1, t2, "tokens are unique");
        assert!(t1.chars().all(|c| c.is_ascii_hexdigit()));
        // Hash is a proper SHA-256 (deterministic, 64 hex chars).
        assert_eq!(hash_token("abc"), hash_token("abc"));
        assert_ne!(hash_token("abc"), hash_token("abd"));
        assert_eq!(hash_token("abc").len(), 64);
        // Known-answer test: SHA-256("abc").
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
