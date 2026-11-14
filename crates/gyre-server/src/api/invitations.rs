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
//! Invitee-facing workspace invitation surface (spec §API > Workspace
//! Membership + §Workspace Invitation Flow steps 3-4 — the invitee has
//! no membership yet, so the workspace-scoped routes above are
//! unreachable for them):
//! - GET    /api/v1/users/me/invitations            — my invitations
//! - POST   /api/v1/workspaces/:id/invite/accept    — accept (authenticated)
//! - POST   /api/v1/workspaces/:id/invite/decline   — decline (authenticated)
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

/// Load the invitation policy for a tenant (spec defaults when unset).
///
/// Fails CLOSED on a corrupt stored row: a policy that cannot be read or
/// parsed must surface as an error instead of silently falling back to
/// the defaults — the defaults are the *permissive* side
/// (`allow_re_invite: true`, cap 50), so a silent fallback relaxes
/// enforcement exactly when the tenant asked for restrictions. The GET
/// endpoint surfaces the corruption as a 500; the PUT endpoint replaces
/// the row (the operator's remediation path).
pub async fn load_policy(state: &AppState, tenant_id: &str) -> Result<InvitationPolicy, ApiError> {
    match state.kv_store.kv_get(POLICY_KV_NAMESPACE, tenant_id).await {
        Ok(None) => Ok(InvitationPolicy::default()),
        Ok(Some(raw)) => serde_json::from_str(&raw).map_err(|e| {
            tracing::error!(tenant_id, error = %e, "stored invitation policy is corrupt");
            ApiError::Internal(anyhow::anyhow!(
                "stored invitation policy for tenant {tenant_id} is corrupt: {e}"
            ))
        }),
        Err(e) => Err(ApiError::Internal(anyhow::anyhow!(
            "load invitation policy for tenant {tenant_id}: {e}"
        ))),
    }
}

// ─── Token helpers ───────────────────────────────────────────────────────────

/// Generate a cryptographically random invitation token (32 bytes, hex).
fn generate_invitation_token() -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes).expect("system CSPRNG must not fail");
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

/// Tenant magic-link guard (task-110): when a Bearer identity IS presented
/// on a token-authenticated invitation route, the caller must be the
/// invitee — an authenticated user must not accept, decline, or burn an
/// invitation issued to someone else's email.
///
/// A caller linked to a stored user matches by that user's email
/// (case-insensitive; invitations normalize to lowercase). Callers with no
/// user link (agent tokens, the global dev token) carry no email identity
/// and are rejected — the token alone is the auth factor for those, and an
/// identity-less accept goes through the unauthenticated path where the
/// guard is simply not applicable.
async fn require_matching_invitee(
    state: &AppState,
    auth: &AuthenticatedAgent,
    invited_email: &str,
) -> Result<(), ApiError> {
    let caller_id = caller_user_id(auth);
    let caller_email = state
        .users
        .find_by_id(&caller_id)
        .await?
        .and_then(|u| u.email)
        .map(|e| e.to_lowercase());
    match caller_email {
        Some(email) if email == invited_email.to_lowercase() => Ok(()),
        _ => Err(ApiError::Forbidden(
            "this invitation was issued to a different email address".to_string(),
        )),
    }
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
        let role = i.role_as_str().to_string();
        let status = i.status.as_str().to_string();
        let workspace_ids = i.workspace_ids.iter().map(|w| w.to_string()).collect();
        let workspace_roles = i
            .workspace_roles
            .iter()
            .map(|r| r.as_str().to_string())
            .collect();
        Self {
            id: i.id.to_string(),
            tenant_id: i.tenant_id.to_string(),
            email: i.email,
            invited_by: i.invited_by.to_string(),
            role,
            workspace_ids,
            workspace_roles,
            status,
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
        workspace_roles.push(
            WorkspaceRole::parse_role(r)
                .ok_or_else(|| ApiError::InvalidInput(format!("unknown workspace role: {r}")))?,
        );
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

    let policy = load_policy(state, &auth.tenant_id).await?;
    let days = req
        .expires_in_days
        .unwrap_or(policy.tenant_invite_expiry_days);
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
    // expiry). The repository contract enforces the same invariant —
    // backed by the pending-scope partial unique index (migration
    // 000061) — as a race backstop, but its anyhow error would surface
    // as a 500 here.
    let existing_for_email = state
        .tenant_invitations
        .list_by_tenant(&Id::new(auth.tenant_id.clone()))
        .await?;
    if existing_for_email
        .iter()
        .any(|i| i.email == email && i.status == InvitationStatus::Pending)
    {
        return Err(ApiError::Conflict(format!(
            "a pending invitation for {email} in tenant {} already exists",
            auth.tenant_id
        )));
    }
    // InvitationPolicy.allow_re_invite (user-management.md §Invitation
    // Expiry, default true): when false, an invitation in a terminal
    // state (Accepted/Declined/Expired/Revoked) permanently blocks
    // re-inviting the same email — the default flow (revoke or wait for
    // expiry, then invite again with a fresh token) is opt-out.
    if !policy.allow_re_invite
        && existing_for_email
            .iter()
            .any(|i| i.email == email && i.status != InvitationStatus::Pending)
    {
        return Err(ApiError::Conflict(format!(
            "tenant invitation policy disallows re-inviting {email} \
             (allow_re_invite is disabled and a prior invitation exists)"
        )));
    }
    state.tenant_invitations.create(&invitation).await?;
    let invitee_note = serde_json::json!({
        "email": email,
        "role": invitation.role_as_str(),
        "workspace_ids": invitation
            .workspace_ids
            .iter()
            .map(|w| w.to_string())
            .collect::<Vec<_>>(),
    });
    tracing::info!(
        tenant_id = %auth.tenant_id,
        invited_by = %auth.user_id.as_ref().map(|i| i.to_string()).unwrap_or_default(),
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

    // Conditional transition: a concurrent accept/decline/expiry that
    // landed between the read above and this write must not be overwritten
    // by Revoked — the Pending precondition is enforced in the write.
    let revoked = state
        .tenant_invitations
        .transition_status(
            &invitation.id,
            InvitationStatus::Pending,
            InvitationStatus::Revoked,
            None,
        )
        .await?;
    if !revoked {
        return Err(ApiError::Conflict(
            "invitation is no longer pending — only pending invitations can be revoked".to_string(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─── Per-tenant invitation policy ─────────────────────────────────────────────

/// GET /api/v1/tenant/invitations/policy
///
/// TenantAdmin-only. Returns the caller's tenant's invitation policy
/// (spec §Invitation Expiry: tenant_invite_expiry_days,
/// workspace_invite_expiry_days, max_pending_invitations,
/// allow_re_invite). Unset fields report the spec defaults.
pub async fn get_invitation_policy(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
) -> Result<Json<InvitationPolicy>, ApiError> {
    require_tenant_admin(&auth)?;
    let policy = load_policy(&state, &auth.tenant_id).await?;
    Ok(Json(policy))
}

#[derive(Deserialize, Default)]
pub struct UpdateInvitationPolicyRequest {
    pub tenant_invite_expiry_days: Option<u32>,
    pub workspace_invite_expiry_days: Option<u32>,
    pub max_pending_invitations: Option<u32>,
    pub allow_re_invite: Option<bool>,
}

/// PUT /api/v1/tenant/invitations/policy
///
/// TenantAdmin-only. Partial update: omitted fields keep their current
/// value (defaults for a tenant that never set a policy). The stored
/// policy takes effect on every subsequent invitation creation.
pub async fn update_invitation_policy(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateInvitationPolicyRequest>,
) -> Result<Json<InvitationPolicy>, ApiError> {
    require_tenant_admin(&auth)?;

    // Load-merge-persist: omitted fields keep their current value. A
    // corrupt stored row is replaced rather than propagated: this PUT
    // persists a fresh valid row, which is the operator's remediation
    // path for the corruption the GET (and every enforcement path)
    // surfaces as an error — the unreadable fields fall back to the
    // spec defaults for the merge basis only.
    let mut policy = match load_policy(&state, &auth.tenant_id).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(
                tenant_id = %auth.tenant_id,
                error = %e,
                "stored invitation policy is corrupt; PUT replaces it (defaults + updates)"
            );
            InvitationPolicy::default()
        }
    };
    if let Some(d) = req.tenant_invite_expiry_days {
        policy.tenant_invite_expiry_days = d;
    }
    if let Some(d) = req.workspace_invite_expiry_days {
        policy.workspace_invite_expiry_days = d;
    }
    if let Some(m) = req.max_pending_invitations {
        policy.max_pending_invitations = m;
    }
    if let Some(r) = req.allow_re_invite {
        policy.allow_re_invite = r;
    }

    if policy.tenant_invite_expiry_days == 0 || policy.workspace_invite_expiry_days == 0 {
        return Err(ApiError::InvalidInput(
            "expiry days must be at least 1".to_string(),
        ));
    }

    let json = serde_json::to_string(&policy)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialize policy: {e}")))?;
    state
        .kv_store
        .kv_set(POLICY_KV_NAMESPACE, &auth.tenant_id, json)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("store invitation policy: {e}")))?;

    tracing::info!(
        tenant_id = %auth.tenant_id,
        "invitation policy updated: {}",
        serde_json::to_string(&policy).unwrap_or_default()
    );
    Ok(Json(policy))
}

// ─── Tenant invitation acceptance (magic link) ───────────────────────────────

/// Name recorded for the first API key minted at invitation acceptance
/// (mirrors the "bootstrap" key name of POST /api/v1/users).
const INVITE_KEY_NAME: &str = "invitation";

#[derive(Deserialize, Default)]
pub struct AcceptInviteRequest {
    /// Local mode: display name for the new account (defaults to the email
    /// local-part). SSO mode ignores this and links on next login.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Optional username; defaults to a handle derived from the email
    /// local-part (sanitized, with a deterministic unique suffix on
    /// collision). An explicit username must satisfy the URL-safe handle
    /// contract (1-64 chars `[a-z0-9_-]`, no leading/trailing or
    /// consecutive separators) and be free — 400 / 409 otherwise.
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Serialize)]
pub struct AcceptInviteApiKey {
    /// Plaintext API key - returned exactly once. Only the SHA-256 hash
    /// is stored (same contract as the POST /api/v1/users bootstrap key).
    pub key: String,
    pub name: String,
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
    /// Local mode only: the account's first API key, minted at acceptance
    /// (review d6b8a97). The magic-link token in the URL is the auth factor
    /// — the acceptor proved control of the invited email — so acceptance
    /// is the moment a credential is delivered, mirroring the
    /// POST /api/v1/users bootstrap pattern. This is what makes local
    /// accounts first-class in air-gapped/no-IdP tenants: without it the
    /// accepted invitee could never authenticate (the platform has no
    /// password-login path; API keys are the local credential).
    ///
    /// Absent (`null`) when a pre-existing account with an independent
    /// identity binding was linked: SSO-provisioned accounts authenticate
    /// via their IdP, and minting a key from token possession would let an
    /// invitation holder bypass that IdP for someone else's account.
    pub api_key: Option<AcceptInviteApiKey>,
}

/// POST /api/v1/invite/:token/accept
///
/// Accepts a tenant invitation via magic-link token. Bearer auth is
/// optional: the 256-bit CSPRNG token in the URL is the auth factor (same
/// trust model as the magic link the invitee received by email), since the
/// invitee may have no credentials yet.
///
/// When a Bearer identity IS presented, it must match the invitation's
/// email — an authenticated caller must not accept (or burn) an
/// invitation issued to someone else's address.
///
/// Flow (spec §Tenant invitation flow):
/// - token → invitation (hash lookup), must be Pending and unexpired
/// - Local mode: create the user (external_id namespaced by tenant+email so
///   the same email can exist in different tenants) and mint the account's
///   first API key in the response — the magic-link credential delivery
///   ("Invitee receives a magic link"; the acceptor holds the one-time
///   256-bit token, so they are the account owner)
/// - SSO mode: if a user with the invitation email already exists in the
///   tenant, link (activate pre-assigned memberships); otherwise create a
///   local-mode account now — on first SSO login, the verified email
///   link in auth.rs `find_or_create_user_tenant` claims this account
///   (re-points its external_id at the Keycloak subject) when the emails
///   match within the same tenant.
/// - Activate pre-assigned workspace memberships.
pub async fn accept_tenant_invitation(
    auth: Option<AuthenticatedAgent>,
    Path(token): Path<String>,
    State(state): State<Arc<AppState>>,
    body: Option<Json<AcceptInviteRequest>>,
) -> Result<(StatusCode, Json<AcceptInviteResponse>), ApiError> {
    let req = body.map(|Json(r)| r).unwrap_or_default();

    // An explicit username must satisfy the URL-safe handle contract
    // (user-management.md §Username vs Display Name — the same
    // validate_username gate as the auth.rs SSO and SCIM provisioning
    // paths) BEFORE the single-use claim: invalid input must not consume
    // (claim and roll back) the invitation.
    if let Some(username) = req.username.as_deref() {
        User::validate_username(username)
            .map_err(|e| ApiError::InvalidInput(format!("invalid username: {e}")))?;
    }

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
    if let Some(auth) = &auth {
        require_matching_invitee(&state, auth, &invitation.email).await?;
    }
    let now = now_secs();
    if invitation.is_expired(now) {
        // Persist the Expired status now (the background job would also
        // catch it, but the caller deserves the terminal state immediately).
        state
            .tenant_invitations
            .transition_status(
                &invitation.id,
                InvitationStatus::Pending,
                InvitationStatus::Expired,
                None,
            )
            .await?;
        return Err(ApiError::Conflict("invitation has expired".to_string()));
    }

    // Single-use claim, BEFORE any side effects: the conditional
    // Pending→Accepted transition is atomic at the store, so two
    // concurrent accepts of the same token cannot both proceed — only
    // the claim winner creates the user and memberships. On failure
    // after the claim, the status is rolled back so the invitation
    // stays usable.
    let claimed = state
        .tenant_invitations
        .transition_status(
            &invitation.id,
            InvitationStatus::Pending,
            InvitationStatus::Accepted,
            Some(now),
        )
        .await?;
    if !claimed {
        return Err(ApiError::Conflict(format!(
            "invitation is {}",
            invitation.status.as_str()
        )));
    }

    let outcome = accept_tenant_invitation_claimed(&state, &invitation, &req, now).await;
    match outcome {
        Ok(response) => Ok((StatusCode::OK, Json(response))),
        Err(e) => {
            // Best-effort rollback: the side effects failed, so the
            // invitation must not stay Accepted with nothing delivered.
            let _ = state
                .tenant_invitations
                .transition_status(
                    &invitation.id,
                    InvitationStatus::Accepted,
                    InvitationStatus::Pending,
                    None,
                )
                .await;
            Err(e)
        }
    }
}

/// Side effects of a successfully claimed tenant invitation: find, link,
/// or create the user, activate pre-assigned workspace memberships.
async fn accept_tenant_invitation_claimed(
    state: &Arc<AppState>,
    invitation: &TenantInvitation,
    req: &AcceptInviteRequest,
    now: u64,
) -> Result<AcceptInviteResponse, ApiError> {
    // non-atomic-create:ok - user + pre-assigned memberships are created
    // across two repositories with no cross-repo transaction facility, but
    // the failure is fully recoverable: the caller (accept_tenant_invitation)
    // compensates by rolling the invitation back to Pending on any error,
    // and a retry dedups — the user is found via find_by_external_id instead
    // of re-created, and membership creation skips existing rows.
    // Find, link, or create the user in this tenant:
    //
    // 1. Local-mode retry dedupe: a prior accept (whose membership
    //    activation later failed) already created this account — reuse it.
    // 2. SSO-provisioned account with the invitation email in this tenant
    //    (external_id = Keycloak subject, the normal state in SSO-mode
    //    tenants): LINK it — the pre-assigned memberships and invited
    //    GlobalRole attach to the real account instead of creating a
    //    parallel local-mode user that would shadow it (spec
    //    §Tenant-Level User Onboarding flow step 3: "SSO mode: redirected
    //    to Keycloak login, then account linked").
    // 3. Otherwise create a local-mode account — on first SSO login, the
    //    verified-email link in auth.rs `find_or_create_user_tenant`
    //    claims this account (re-points its external_id at the Keycloak
    //    subject) when the emails match within the same tenant.
    let email = invitation.email.clone();
    let tenant_ns = format!("tenant:{}:email:{}", invitation.tenant_id, email);
    // Credential delivery (review d6b8a97): mint the account's first API
    // key iff the account exists because of invitation acceptance to this
    // email — a fresh create (case 3) or a local-mode account carrying the
    // invitation namespace external_id (case 1: a retry/re-invite whose
    // earlier key, if any, never reached the client). A pre-existing
    // account with an independent identity binding (case 2: SSO subject,
    // SCIM, or a bootstrap `local:{username}` account) gets NO minted key:
    // those accounts authenticate through their existing factor, and a
    // key minted from token possession would bypass it.
    let mut mint_credential = false;
    let (user, mode) = match state.users.find_by_external_id(&tenant_ns).await? {
        Some(existing) => {
            mint_credential = true;
            (existing, "linked")
        }
        None => match find_tenant_user_by_email(state, &invitation.tenant_id, &email).await? {
            Some(mut sso_user) => {
                // The inviting TenantAdmin chose the role at invite time;
                // acceptance applies it to the linked account exactly as
                // the local-mode path does for a fresh account.
                sso_user.global_role = invitation.role.clone();
                sso_user.roles = match invitation.role {
                    GlobalRole::TenantAdmin => vec![UserRole::Admin],
                    GlobalRole::Member => vec![UserRole::Developer],
                };
                state.users.update(&sso_user).await?;
                tracing::info!(
                    user_id = %sso_user.id,
                    tenant_id = %invitation.tenant_id,
                    "linked tenant invitation to existing SSO-provisioned account"
                );
                (sso_user, "linked")
            }
            None => {
                let id = new_id();
                // Username: an explicit handle must satisfy the URL-safe
                // handle contract (user-management.md §Username vs Display
                // Name; validate_username — same gate as the auth.rs SSO
                // and SCIM creation paths) and be free (409 the invitee
                // can retry without). The derived default is sanitized
                // and resolved to a globally unique handle — usernames
                // are unique ACROSS tenants (idx_users_username), and the
                // spec's cross-tenant model explicitly has the same
                // person in two tenants — with the same deterministic
                // suffix scheme as the SSO provisioning path
                // (base, base-2, ...), so "bob@acme.com" after
                // "bob@google.com" yields bob-2 instead of a 500 that
                // would roll the invitation back to Pending forever.
                let username = match req.username.as_deref() {
                    // Format was validated before the claim; a taken
                    // explicit handle is a 409 the invitee can retry
                    // without (the handle derives from the email
                    // otherwise).
                    Some(explicit) => {
                        if state.users.find_by_username(explicit).await?.is_some() {
                            return Err(ApiError::Conflict(format!(
                                "username {explicit} is already taken"
                            )));
                        }
                        explicit.to_string()
                    }
                    None => {
                        let local_part = email.split('@').next().unwrap_or("user");
                        let base = User::sanitize_username(local_part)
                            .unwrap_or_else(|| format!("u-{}", id.as_str()));
                        crate::auth::resolve_unique_username(state, &base)
                            .await
                            .map_err(|e| {
                                ApiError::Internal(anyhow::anyhow!("derive unique username: {e}"))
                            })?
                    }
                };
                let display_name = req.display_name.clone().unwrap_or_else(|| username.clone());
                let mut user = User::new(id, tenant_ns, username, now);
                user.display_name = display_name;
                user.email = Some(email.clone());
                user.tenant_id = Some(invitation.tenant_id.clone());
                user.global_role = invitation.role.clone();
                user.roles = match invitation.role {
                    GlobalRole::TenantAdmin => vec![UserRole::Admin],
                    GlobalRole::Member => vec![UserRole::Developer],
                };
                state.users.create(&user).await?;
                mint_credential = true;
                (user, "created")
            }
        },
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
        match existing {
            // Already active — nothing to do (idempotent re-accept).
            Some(m) if m.accepted => continue,
            // A pending membership (e.g. seeded by an earlier partial
            // accept): acceptance of the invitation IS the acceptance of
            // the membership — activate it.
            Some(m) => {
                state.workspace_memberships.accept(&m.id, now).await?;
                activated.push(ws_id.to_string());
            }
            None => {
                let mut membership = WorkspaceMembership::new(
                    new_id(),
                    user.id.clone(),
                    ws_id.clone(),
                    role,
                    invitation.invited_by.clone(),
                    now,
                );
                // Activate immediately: the invitee just accepted the
                // invitation that pre-assigned this membership (same
                // semantics as the workspace-invitation accept path).
                membership.accept(now);
                state.workspace_memberships.create(&membership).await?;
                activated.push(ws_id.to_string());
            }
        }
    }

    // Mint the first API key only after every side effect succeeded: if
    // this errors, the invitation rolls back to Pending and a retry mints
    // again (the user dedups via the namespace lookup above), so no
    // credential is handed out for an acceptance that did not complete.
    // Same generation and storage contract as the POST /api/v1/users
    // bootstrap key: CSPRNG UUID v4 raw key, only the SHA-256 hash
    // persisted, registered in `state.api_keys` — the store the auth
    // extractor consults, so the key authenticates through the real
    // routing chain immediately.
    let api_key = if mint_credential {
        let raw_key = format!("gyre_{}", uuid::Uuid::new_v4().simple());
        state
            .api_keys
            .create(
                &crate::auth::hash_api_key(&raw_key),
                &user.id,
                INVITE_KEY_NAME,
            )
            .await?;
        Some(AcceptInviteApiKey {
            key: raw_key,
            name: INVITE_KEY_NAME.to_string(),
        })
    } else {
        None
    };

    Ok(AcceptInviteResponse {
        user_id: user.id.to_string(),
        username: user.username.clone(),
        mode: mode.to_string(),
        tenant_id: invitation.tenant_id.to_string(),
        workspaces: activated,
        api_key,
    })
}

/// Find the tenant's user account for an email address — the SSO-mode
/// link target for invitation acceptance. Emails are not unique across
/// tenants (the spec's cross-tenant model has the same person in two
/// tenants), so the lookup is scoped by tenant; when several accounts in
/// the tenant carry the same email, the earliest-created wins
/// (deterministic) and the ambiguity is logged.
async fn find_tenant_user_by_email(
    state: &Arc<AppState>,
    tenant_id: &Id,
    email: &str,
) -> Result<Option<User>, ApiError> {
    let matches = state.users.find_by_email(email).await?;
    let scoped: Vec<User> = matches
        .into_iter()
        .filter(|u| u.tenant_id.as_ref() == Some(tenant_id))
        .collect();
    if scoped.len() > 1 {
        tracing::warn!(
            tenant_id = %tenant_id,
            email,
            count = scoped.len(),
            "multiple users share the invitation email in this tenant; linking the earliest"
        );
    }
    Ok(scoped.into_iter().next())
}

/// POST /api/v1/invite/:token/decline
///
/// Bearer auth optional (token is the auth factor); when presented, the
/// identity must match the invitation's email — same guard as accept.
pub async fn decline_tenant_invitation(
    auth: Option<AuthenticatedAgent>,
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
    if let Some(auth) = &auth {
        require_matching_invitee(&state, auth, &invitation.email).await?;
    }
    if invitation.is_expired(now_secs()) {
        state
            .tenant_invitations
            .transition_status(
                &invitation.id,
                InvitationStatus::Pending,
                InvitationStatus::Expired,
                None,
            )
            .await?;
        return Err(ApiError::Conflict("invitation has expired".to_string()));
    }
    // Conditional transition — a concurrent accept that landed between
    // the read and this write must win, not be overwritten by Declined.
    let declined = state
        .tenant_invitations
        .transition_status(
            &invitation.id,
            InvitationStatus::Pending,
            InvitationStatus::Declined,
            None,
        )
        .await?;
    if !declined {
        return Err(ApiError::Conflict(format!(
            "invitation is {}",
            invitation.status.as_str()
        )));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─── Workspace invitation endpoints ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct WorkspaceInviteRequest {
    /// User id of an existing tenant user. One of `user_id`/`username`/
    /// `email` must be provided (spec §Workspace Invitation Flow step 1:
    /// invitations are issued "by username (tenant-internal) or email";
    /// user_id is accepted for API callers that already resolved the
    /// user).
    #[serde(default)]
    pub user_id: Option<String>,
    /// Username of an existing tenant user (tenant-internal lookup).
    #[serde(default)]
    pub username: Option<String>,
    /// Email of an existing tenant user (lowercase match).
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default = "default_ws_role")]
    pub role: String,
    #[serde(default)]
    pub expires_in_days: Option<u32>,
}

/// Resolve the invitee for a workspace invitation. Accepts user_id,
/// username, or email (spec §Workspace Invitation Flow step 1); when
/// several selectors are given they must all resolve to the same tenant
/// user, otherwise the request is ambiguous and rejected.
async fn resolve_workspace_invitee(
    state: &AppState,
    tenant_id: &Id,
    req: &WorkspaceInviteRequest,
) -> Result<User, ApiError> {
    let selectors = [
        req.user_id.as_deref(),
        req.username.as_deref(),
        req.email.as_deref(),
    ]
    .iter()
    .flatten()
    .map(|s| s.trim())
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>();
    if selectors.is_empty() {
        return Err(ApiError::InvalidInput(
            "one of user_id, username, or email is required".to_string(),
        ));
    }

    // Resolve each selector to a user within the inviting workspace's
    // tenant — usernames and emails are only meaningful tenant-internally,
    // and cross-tenant invites are rejected anyway.
    let users = state.users.list().await?;
    let mut resolved: Option<&User> = None;
    for selector in selectors {
        let lower = selector.to_lowercase();
        let found = users.iter().find(|u| {
            u.tenant_id.as_ref() == Some(tenant_id)
                && (u.id.as_str() == selector
                    || u.username.to_lowercase() == lower
                    || u.email.as_ref().is_some_and(|e| e.to_lowercase() == lower))
        });
        let found = found.ok_or_else(|| {
            ApiError::NotFound(format!("no user matching '{selector}' in this tenant"))
        })?;
        match resolved {
            None => resolved = Some(found),
            Some(prev) if prev.id == found.id => {}
            Some(_) => {
                return Err(ApiError::InvalidInput(
                    "selectors resolve to different users".to_string(),
                ));
            }
        }
    }
    Ok(resolved
        .expect("selectors is non-empty, so resolved is Some")
        .clone())
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
    // non-atomic-create:ok - the workspace invitation is the primary
    // durable effect; the invitee notification is an explicitly best-effort
    // `let _ =` side channel, not a dependency of the invitation — the
    // invitee's durable surface is GET /api/v1/users/me/invitations, so a
    // lost notification cannot orphan the invitation.
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
        || caller_membership
            .is_some_and(|m| matches!(m.role, WorkspaceRole::Owner | WorkspaceRole::Admin));
    if !authorized {
        return Err(ApiError::Forbidden(
            "only workspace Owner/Admin or TenantAdmin may invite".to_string(),
        ));
    }

    let role = WorkspaceRole::parse_role(&req.role)
        .ok_or_else(|| ApiError::InvalidInput(format!("unknown role: {}", req.role)))?;

    // The invitee must exist and belong to the same tenant. A user with no
    // tenant has no verifiable scope — reject rather than fabricate one.
    let invitee = resolve_workspace_invitee(&state, &workspace.tenant_id, &req).await?;
    let user_id = invitee.id.clone();
    if invitee.tenant_id.as_ref() != Some(&workspace.tenant_id) {
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
    let policy = load_policy(&state, &auth.tenant_id).await?;
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
    // InvitationPolicy.allow_re_invite (user-management.md §Invitation
    // Expiry, default true): when false, an invitation in a terminal
    // state for this (workspace, user) blocks re-invite — the tenant-side
    // rule applied to the workspace flow (task-110 review F1).
    if !policy.allow_re_invite
        && ws_pending
            .iter()
            .any(|i| i.user_id == user_id && i.status != InvitationStatus::Pending)
    {
        return Err(ApiError::Conflict(format!(
            "invitation policy disallows re-inviting user {user_id} to workspace \
             {workspace_id} (allow_re_invite is disabled and a prior invitation exists)"
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
    // Typed InvitationReceived (user-management.md §Notification Entity:
    // "A user was invited to a workspace") so the invitee's Inbox groups
    // it under workspace invitations, not trust-policy suggestions.
    // The body carries the accept route so the invitee can act on the
    // invitation end-to-end: the invitee has no membership yet (so the
    // workspace-scoped listing is unreachable) and must not need the
    // inviter's token — the authenticated accept path
    // (POST /api/v1/workspaces/{id}/invite/accept) and the invitee's own
    // pending-invitations view (GET /api/v1/users/me/invitations) are the
    // invitee-facing surfaces.
    let mut notif = gyre_common::Notification::new(
        new_id(),
        ws_id.clone(),
        user_id.clone(),
        gyre_common::NotificationType::InvitationReceived,
        format!(
            "You have been invited to workspace {} with role {}",
            workspace.name,
            invitation.role.as_str()
        ),
        workspace.tenant_id.as_str().to_string(),
        now as i64,
    );
    notif.body = Some(
        serde_json::json!({
            "kind": "workspace_invitation",
            "workspace_id": ws_id.to_string(),
            "role": invitation.role.as_str(),
            "accept_url": format!(
                "{}/api/v1/workspaces/{}/invite/accept",
                state.base_url, workspace_id
            ),
            "decline_url": format!(
                "{}/api/v1/workspaces/{}/invite/decline",
                state.base_url, workspace_id
            ),
            "invitations_url": format!("{}/api/v1/users/me/invitations", state.base_url),
        })
        .to_string(),
    );
    // The constructor sources this notification at the workspace itself
    // (NotificationSource::workspace), so no extra source assignment is
    // needed — entity_id is the workspace id.
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
        || caller_membership
            .is_some_and(|m| matches!(m.role, WorkspaceRole::Owner | WorkspaceRole::Admin));
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
    // Conditional transition — a concurrent accept must not be overwritten
    // by Revoked (same guard as the tenant revoke).
    let revoked = state
        .workspace_invitations
        .transition_status(
            &invitation.id,
            InvitationStatus::Pending,
            InvitationStatus::Revoked,
            None,
        )
        .await?;
    if !revoked {
        return Err(ApiError::Conflict(
            "invitation is no longer pending — only pending invitations can be revoked".to_string(),
        ));
    }
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
            .transition_status(
                &invitation.id,
                InvitationStatus::Pending,
                InvitationStatus::Expired,
                None,
            )
            .await?;
        return Err(ApiError::Conflict("invitation has expired".to_string()));
    }

    // Single-use claim BEFORE the membership side effects — see
    // accept_tenant_invitation for the rationale.
    let claimed = state
        .workspace_invitations
        .transition_status(
            &invitation.id,
            InvitationStatus::Pending,
            InvitationStatus::Accepted,
            Some(now),
        )
        .await?;
    if !claimed {
        return Err(ApiError::Conflict(format!(
            "invitation is {}",
            invitation.status.as_str()
        )));
    }
    match activate_membership(&state, &invitation, now).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            let _ = state
                .workspace_invitations
                .transition_status(
                    &invitation.id,
                    InvitationStatus::Accepted,
                    InvitationStatus::Pending,
                    None,
                )
                .await;
            Err(e)
        }
    }
}

/// Activate (or re-accept) the membership a claimed workspace invitation
/// grants. Idempotent: an existing membership is only flipped to accepted,
/// never duplicated.
async fn activate_membership(
    state: &AppState,
    invitation: &WorkspaceInvitation,
    now: u64,
) -> Result<(), ApiError> {
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
            let mut m = WorkspaceMembership::new(
                new_id(),
                invitation.user_id.clone(),
                invitation.workspace_id.clone(),
                invitation.role,
                invitation.invited_by.clone(),
                now,
            );
            // activate immediately — acceptance IS the activation.
            m.accept(now);
            state.workspace_memberships.create(&m).await?;
            m
        }
    };

    tracing::info!(
        workspace_id = %invitation.workspace_id,
        user_id = %invitation.user_id,
        membership_id = %membership.id,
        "workspace invitation accepted"
    );
    Ok(())
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
            .transition_status(
                &invitation.id,
                InvitationStatus::Pending,
                InvitationStatus::Expired,
                None,
            )
            .await?;
        return Err(ApiError::Conflict("invitation has expired".to_string()));
    }
    // Conditional transition — a concurrent accept must not be overwritten.
    let declined = state
        .workspace_invitations
        .transition_status(
            &invitation.id,
            InvitationStatus::Pending,
            InvitationStatus::Declined,
            None,
        )
        .await?;
    if !declined {
        return Err(ApiError::Conflict(format!(
            "invitation is {}",
            invitation.status.as_str()
        )));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─── Invitee-facing workspace invitation surface ─────────────────────────────
//
// Spec §API > Workspace Membership defines the invitee's accept route as
// POST /api/v1/workspaces/{id}/invite/accept (authenticated invitee
// accepts THEIR pending invitation — no token), and §Workspace
// Invitation Flow step 4 requires the invitee to accept or decline. The
// invitee has no workspace membership yet, so the workspace-scoped
// listing is unreachable for them; these routes plus
// GET /api/v1/users/me/invitations are the invitee-facing surface.

/// GET /api/v1/users/me/invitations
///
/// The caller's workspace invitations across workspaces (the invitee's
/// "pending invitations" view — spec §Workspace Invitation Flow). Scoped
/// to the authenticated user; no token material is exposed.
pub async fn list_my_invitations(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let caller_id = caller_user_id(&auth);
    let items: Vec<WorkspaceInvitationResponse> = state
        .workspace_invitations
        .list_by_user(&caller_id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(serde_json::json!({ "invitations": items })))
}

/// POST /api/v1/workspaces/:id/invite/accept
///
/// Authenticated invitee accepts their pending invitation for the
/// workspace (spec §API > Workspace Membership). The invitation is
/// located by (workspace, caller) — no token needed, because the
/// caller's authenticated identity IS the invitee proof.
pub async fn accept_my_workspace_invitation(
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let invitation = find_my_pending_invitation(&state, &auth, &workspace_id).await?;
    let now = now_secs();
    if invitation.is_expired(now) {
        state
            .workspace_invitations
            .transition_status(
                &invitation.id,
                InvitationStatus::Pending,
                InvitationStatus::Expired,
                None,
            )
            .await?;
        return Err(ApiError::Conflict("invitation has expired".to_string()));
    }
    let claimed = state
        .workspace_invitations
        .transition_status(
            &invitation.id,
            InvitationStatus::Pending,
            InvitationStatus::Accepted,
            Some(now),
        )
        .await?;
    if !claimed {
        return Err(ApiError::Conflict(format!(
            "invitation is {}",
            invitation.status.as_str()
        )));
    }
    match activate_membership(&state, &invitation, now).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            let _ = state
                .workspace_invitations
                .transition_status(
                    &invitation.id,
                    InvitationStatus::Accepted,
                    InvitationStatus::Pending,
                    None,
                )
                .await;
            Err(e)
        }
    }
}

/// POST /api/v1/workspaces/:id/invite/decline
///
/// Authenticated invitee declines their pending invitation for the
/// workspace (spec §Workspace Invitation Flow step 4).
pub async fn decline_my_workspace_invitation(
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let invitation = find_my_pending_invitation(&state, &auth, &workspace_id).await?;
    let now = now_secs();
    if invitation.is_expired(now) {
        state
            .workspace_invitations
            .transition_status(
                &invitation.id,
                InvitationStatus::Pending,
                InvitationStatus::Expired,
                None,
            )
            .await?;
        return Err(ApiError::Conflict("invitation has expired".to_string()));
    }
    let declined = state
        .workspace_invitations
        .transition_status(
            &invitation.id,
            InvitationStatus::Pending,
            InvitationStatus::Declined,
            None,
        )
        .await?;
    if !declined {
        return Err(ApiError::Conflict(format!(
            "invitation is {}",
            invitation.status.as_str()
        )));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Shared lookup for the invitee-facing accept/decline routes: resolve the
/// workspace, enforce tenant containment, and find the caller's pending
/// invitation in it. 404 when the caller has no pending invitation there
/// (an invitee must not learn whether OTHER users have pending
/// invitations), 403 when the workspace belongs to another tenant.
async fn find_my_pending_invitation(
    state: &AppState,
    auth: &AuthenticatedAgent,
    workspace_id: &str,
) -> Result<WorkspaceInvitation, ApiError> {
    let ws_id = Id::new(workspace_id.to_string());
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
    let caller_id = caller_user_id(auth);
    let invitation = state
        .workspace_invitations
        .list_by_workspace(&ws_id)
        .await?
        .into_iter()
        .find(|i| i.user_id == caller_id && i.status == InvitationStatus::Pending)
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "no pending invitation for you in workspace {workspace_id}"
            ))
        })?;
    Ok(invitation)
}

// ─── Expiry job ──────────────────────────────────────────────────────────────

/// Run one invitation-expiry cycle: marks all Pending invitations past
/// `expires_at` as Expired (kept for audit, never deleted).
pub async fn run_expiry_once(state: &AppState) -> anyhow::Result<u64> {
    let now = now_secs();
    let mut expired_count = 0u64;

    // Conditional Pending→Expired transitions: an invitation that was
    // accepted/declined/revoked between the list read and the write must
    // keep its terminal status — only successful transitions count.
    for inv in state
        .tenant_invitations
        .list_by_status(InvitationStatus::Pending)
        .await?
    {
        if inv.is_expired(now)
            && state
                .tenant_invitations
                .transition_status(
                    &inv.id,
                    InvitationStatus::Pending,
                    InvitationStatus::Expired,
                    None,
                )
                .await?
        {
            expired_count += 1;
        }
    }

    for inv in state
        .workspace_invitations
        .list_by_status(InvitationStatus::Pending)
        .await?
    {
        if inv.is_expired(now)
            && state
                .workspace_invitations
                .transition_status(
                    &inv.id,
                    InvitationStatus::Pending,
                    InvitationStatus::Expired,
                    None,
                )
                .await?
        {
            expired_count += 1;
        }
    }

    if expired_count > 0 {
        tracing::info!(count = expired_count, "marked invitations as expired");
    }
    Ok(expired_count)
}

/// Spawn the invitation-expiry background job (user-management.md
/// §Invitation Expiry): every [`EXPIRY_JOB_INTERVAL_SECS`] (default 1h,
/// override via `GYRE_INVITE_EXPIRY_INTERVAL_SECS`), mark all Pending
/// invitations past `expires_at` as Expired. Rows are never deleted —
/// expired invitations are kept for audit.
pub fn spawn_invitation_expiry(state: Arc<AppState>) {
    let state = state.clone();
    let interval = std::env::var("GYRE_INVITE_EXPIRY_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        // tokio::time::interval PANICS on a zero duration ("duration must
        // be non-zero"), so an env misconfiguration
        // (GYRE_INVITE_EXPIRY_INTERVAL_SECS=0) must fall back to the
        // default instead of crashing server startup. Unparsable values
        // already fall through the same way.
        .filter(|v| *v > 0)
        .unwrap_or(EXPIRY_JOB_INTERVAL_SECS);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval));
        // First tick completes immediately — expire backlog at startup.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if let Err(e) = run_expiry_once(&state).await {
                tracing::warn!("invitation expiry cycle failed: {e:#}");
            }
        }
    });
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
                "/api/v1/tenant/invitations/policy",
                get(get_invitation_policy).put(update_invitation_policy),
            )
            .route(
                "/api/v1/tenant/invitations/:id",
                delete(revoke_tenant_invitation),
            )
            .route(
                "/api/v1/invite/:token/accept",
                post(accept_tenant_invitation),
            )
            .route(
                "/api/v1/invite/:token/decline",
                post(decline_tenant_invitation),
            )
            .route("/api/v1/workspaces/:id/invite", post(invite_to_workspace))
            .route(
                "/api/v1/workspaces/:id/invite/accept",
                post(accept_my_workspace_invitation),
            )
            .route(
                "/api/v1/workspaces/:id/invite/decline",
                post(decline_my_workspace_invitation),
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
            .route("/api/v1/users/me/invitations", get(list_my_invitations))
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
            .route(
                "/api/v1/invite/:token/accept",
                post(accept_tenant_invitation),
            )
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

    /// HTTP-layer negative authorization (review F4): a non-admin
    /// authenticated caller must get 403 from POST /api/v1/tenant/invite
    /// through the real extractor + handler chain — a routing or extractor
    /// regression that bypasses the guard must fail here, not pass silently.
    #[tokio::test]
    async fn non_admin_cannot_invite_via_http() {
        let state = test_state();
        // A plain Member user (ReadOnly roles) with a real API key.
        let mut pleb = User::new(
            Id::new("pleb-1"),
            "tenant:default:email:pleb@x.com",
            "pleb",
            now_secs(),
        );
        pleb.email = Some("pleb@x.com".to_string());
        pleb.tenant_id = Some(Id::new("default"));
        state.users.create(&pleb).await.unwrap();
        let raw_key = format!("gyre_{}", uuid::Uuid::new_v4().simple());
        state
            .api_keys
            .create(&crate::auth::hash_api_key(&raw_key), &pleb.id, "test")
            .await
            .unwrap();

        let app = Router::new()
            .route("/api/v1/tenant/invite", post(invite_to_tenant))
            .with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenant/invite")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": "someone@x.com"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        // And nothing was created.
        let pending = state
            .tenant_invitations
            .list_by_tenant(&Id::new("default"))
            .await
            .unwrap();
        assert!(pending.is_empty(), "403 must not create an invitation");
    }

    /// Task-110 identity guard: a Bearer-authenticated caller whose email
    /// does NOT match the invitation cannot accept or decline the tenant
    /// magic link — 403, the invitation stays Pending (not burned), and a
    /// later correct (token-only) acceptance still succeeds.
    #[tokio::test]
    async fn tenant_accept_rejects_mismatched_bearer_identity() {
        let state = test_state();
        // A stored user with a DIFFERENT email than the invitation.
        let mut mallory = User::new(
            Id::new("mallory-1"),
            "tenant:default:email:mallory@evil.example.com",
            "mallory",
            now_secs(),
        );
        mallory.email = Some("mallory@evil.example.com".to_string());
        mallory.tenant_id = Some(Id::new("default"));
        state.users.create(&mallory).await.unwrap();
        let raw_key = format!("gyre_{}", uuid::Uuid::new_v4().simple());
        state
            .api_keys
            .create(&crate::auth::hash_api_key(&raw_key), &mallory.id, "test")
            .await
            .unwrap();

        let app = Router::new()
            .route("/api/v1/tenant/invite", post(invite_to_tenant))
            .route(
                "/api/v1/invite/:token/accept",
                post(accept_tenant_invitation),
            )
            .route(
                "/api/v1/invite/:token/decline",
                post(decline_tenant_invitation),
            )
            .with_state(state.clone());

        // Admin invites alice@example.com.
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
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let token = json["invite_url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();

        // Mallory (authenticated, different email) tries to accept → 403 …
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/invite/{token}/accept"))
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // … and to decline → 403 too.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/invite/{token}/decline"))
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // The invitation was not burned: the invitee can still accept
        // token-only (no Bearer) and gets 200.
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
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// Task-110 identity guard (positive): the invitee's own authenticated
    /// identity CAN accept — matching email passes the guard and links the
    /// existing account.
    #[tokio::test]
    async fn tenant_accept_allows_matching_bearer_identity() {
        let state = test_state();
        let mut alice = User::new(
            Id::new("alice-1"),
            "tenant:default:email:alice@example.com",
            "alice",
            now_secs(),
        );
        alice.email = Some("alice@example.com".to_string());
        alice.tenant_id = Some(Id::new("default"));
        state.users.create(&alice).await.unwrap();
        let raw_key = format!("gyre_{}", uuid::Uuid::new_v4().simple());
        state
            .api_keys
            .create(&crate::auth::hash_api_key(&raw_key), &alice.id, "test")
            .await
            .unwrap();

        let app = Router::new()
            .route("/api/v1/tenant/invite", post(invite_to_tenant))
            .route(
                "/api/v1/invite/:token/accept",
                post(accept_tenant_invitation),
            )
            .with_state(state.clone());

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
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let token = json["invite_url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();

        // Alice accepts with her own API key: same email → allowed, and
        // the mode is "linked" because her account already exists.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/invite/{token}/accept"))
                    .header("authorization", format!("Bearer {raw_key}"))
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
        assert_eq!(json["mode"], "linked");
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
        let mut owner = User::new(
            Id::new("owner-1"),
            "tenant:default:email:o@x.com",
            "owner",
            now_secs(),
        );
        owner.tenant_id = Some(Id::new("default"));
        state.users.create(&owner).await.unwrap();
        let mut invitee = User::new(
            Id::new("invitee-1"),
            "tenant:default:email:i@x.com",
            "invitee",
            now_secs(),
        );
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
        state
            .workspace_memberships
            .create(&owner_membership)
            .await
            .unwrap();

        // Owner's API key: create one so the extractor resolves user_id.
        let raw_key = format!("gyre_{}", uuid::Uuid::new_v4().simple());
        state
            .api_keys
            .create(&crate::auth::hash_api_key(&raw_key), &owner.id, "test")
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
                        serde_json::json!({"user_id": "invitee-1", "role": "Viewer"}).to_string(),
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
        let ws =
            gyre_domain::Workspace::new(Id::new("ws-d"), Id::new("default"), "D", "d", now_secs());
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
        state
            .workspace_invitations
            .create(&invitation)
            .await
            .unwrap();

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

    /// task-110 repair F3: per-tenant InvitationPolicy configurability
    /// was unreachable (the kv namespace had zero writers). The policy
    /// endpoints are the writer; an invitation created after the update
    /// must pick up the configured expiry, not the hardcoded default.
    #[tokio::test]
    async fn invitation_policy_is_configurable_and_applies() {
        let state = test_state();
        let app = Router::new()
            .route(
                "/api/v1/tenant/invitations/policy",
                get(get_invitation_policy).put(update_invitation_policy),
            )
            .route("/api/v1/tenant/invite", post(invite_to_tenant))
            .with_state(state.clone());

        // Defaults before any write.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/tenant/invitations/policy")
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
        assert_eq!(json["tenant_invite_expiry_days"], 7, "spec default");
        assert_eq!(json["workspace_invite_expiry_days"], 7);
        assert_eq!(json["max_pending_invitations"], 50);

        // Partial update: only tenant expiry changes, the rest keep
        // their values.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/tenant/invitations/policy")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"tenant_invite_expiry_days": 30}).to_string(),
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
        assert_eq!(json["tenant_invite_expiry_days"], 30);
        assert_eq!(json["workspace_invite_expiry_days"], 7, "omitted kept");

        // Zero-day expiry is rejected (would create instantly-expired
        // invitations).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/tenant/invitations/policy")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"tenant_invite_expiry_days": 0}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // The configured policy drives real invitation creation: a
        // fresh invitation expires in 30 days, not 7.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenant/invite")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": "policy@example.com"}).to_string(),
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
        let inv_id = Id::new(json["id"].as_str().unwrap().to_string());
        let stored = state
            .tenant_invitations
            .find_by_id(&inv_id)
            .await
            .unwrap()
            .unwrap();
        let day = 86_400u64;
        let age = stored.expires_at.saturating_sub(stored.created_at);
        assert!(
            age >= 29 * day && age <= 31 * day,
            "expiry driven by configured policy, got {age}s (~{} days)",
            age / day
        );
    }

    /// Task-110 review F1: InvitationPolicy.allow_re_invite=false must be
    /// ENFORCED, not just stored — after a terminal invitation (here:
    /// Revoked; Expired/Declined/Accepted are the same terminal class), a
    /// re-invite for the same email must 409 while the policy is disabled,
    /// and must succeed again once re-invites are re-enabled. Removes the
    /// policy check in create_tenant_invitation and this test fails.
    #[tokio::test]
    async fn allow_re_invite_false_blocks_tenant_reinvite_after_revoke() {
        let state = test_state();
        let app = Router::new()
            .route(
                "/api/v1/tenant/invitations/policy",
                get(get_invitation_policy).put(update_invitation_policy),
            )
            .route("/api/v1/tenant/invite", post(invite_to_tenant))
            .route(
                "/api/v1/tenant/invitations/:id",
                delete(revoke_tenant_invitation),
            )
            .with_state(state.clone());

        let invite = |app: Router| {
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenant/invite")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": "reinvite@example.com"}).to_string(),
                    ))
                    .unwrap(),
            )
        };
        let put_policy = |app: Router, body: serde_json::Value| {
            app.oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/tenant/invitations/policy")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
        };

        // Default policy (allow_re_invite=true): invite, revoke, re-invite
        // with a fresh token — the spec's default flow.
        let resp = invite(app.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let inv_id = json["id"].as_str().unwrap().to_string();

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

        let resp = invite(app.clone()).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "default policy allows re-invite after revoke"
        );
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let inv_id2 = json["id"].as_str().unwrap().to_string();

        // Now disable re-invites and revoke the second invitation: a third
        // invite for the same email must be rejected — the inert-field
        // regression this test guards (review F1).
        let resp = put_policy(app.clone(), serde_json::json!({"allow_re_invite": false}))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["allow_re_invite"], false, "policy persisted");

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/tenant/invitations/{inv_id2}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let resp = invite(app.clone()).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "allow_re_invite=false must block re-invite after a terminal invitation"
        );

        // The block is the policy, not a permanent side effect: enabling
        // re-invites again restores the default flow.
        let resp = put_policy(app.clone(), serde_json::json!({"allow_re_invite": true}))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = invite(app).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Two terminal (Revoked) invitations plus one live pending one.
        let all = state
            .tenant_invitations
            .list_by_tenant(&Id::new("default"))
            .await
            .unwrap();
        assert_eq!(
            all.iter()
                .filter(|i| i.status == InvitationStatus::Revoked)
                .count(),
            2
        );
        assert_eq!(
            all.iter()
                .filter(|i| i.status == InvitationStatus::Pending)
                .count(),
            1
        );
    }

    /// Task-110 review F1, workspace counterpart: with
    /// allow_re_invite=false, re-inviting a user to a workspace that has a
    /// terminal (Revoked) invitation for them must 409; with the default
    /// policy the re-invite succeeds with a fresh token.
    #[tokio::test]
    async fn allow_re_invite_false_blocks_workspace_reinvite_after_revoke() {
        let f = invitee_fixture().await;
        let app = Router::new()
            .route(
                "/api/v1/tenant/invitations/policy",
                get(get_invitation_policy).put(update_invitation_policy),
            )
            .route("/api/v1/workspaces/:id/invite", post(invite_to_workspace))
            .route(
                "/api/v1/workspaces/:id/invitations/:id",
                delete(revoke_workspace_invitation),
            )
            .with_state(f.state.clone());

        let invite = |app: Router| {
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-inv/invite")
                    .header("authorization", format!("Bearer {}", f.owner_key))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"username": "invitee"}).to_string(),
                    ))
                    .unwrap(),
            )
        };

        // Default policy: invite, revoke, re-invite — allowed.
        let resp = invite(app.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let inv_id = json["id"].as_str().unwrap().to_string();

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/workspaces/ws-inv/invitations/{inv_id}"))
                    .header("authorization", format!("Bearer {}", f.owner_key))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let resp = invite(app.clone()).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "default policy allows re-invite after revoke"
        );
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let inv_id2 = json["id"].as_str().unwrap().to_string();

        // Disable re-invites, revoke the second invitation, and try again.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/tenant/invitations/policy")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"allow_re_invite": false}).to_string(),
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
                    .method("DELETE")
                    .uri(format!("/api/v1/workspaces/ws-inv/invitations/{inv_id2}"))
                    .header("authorization", format!("Bearer {}", f.owner_key))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let resp = invite(app).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "allow_re_invite=false must block workspace re-invite after a terminal invitation"
        );

        let all = f
            .state
            .workspace_invitations
            .list_by_workspace(&Id::new("ws-inv"))
            .await
            .unwrap();
        let for_invitee: Vec<_> = all.iter().filter(|i| i.user_id == f.invitee_id).collect();
        assert_eq!(for_invitee.len(), 2, "exactly the two revoked invitations");
        assert!(
            for_invitee
                .iter()
                .all(|i| i.status == InvitationStatus::Revoked),
            "both invitations terminal, no new pending row created by the blocked re-invite"
        );
    }

    /// Shared fixture for the invitee-facing tests: a workspace with an
    /// Owner (inviter) and a second tenant user (invitee), both with real
    /// API keys so the extractor resolves their identities.
    struct InviteeFixture {
        state: Arc<AppState>,
        owner_key: String,
        invitee_key: String,
        invitee_id: Id,
    }

    async fn invitee_fixture() -> InviteeFixture {
        let state = test_state();
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-inv"),
            Id::new("default"),
            "Invite WS",
            "invite-ws",
            now_secs(),
        );
        state.workspaces.create(&ws).await.unwrap();
        let mut owner = User::new(
            Id::new("owner-inv"),
            "tenant:default:email:owner-inv@x.com",
            "owner-inv",
            now_secs(),
        );
        owner.email = Some("owner-inv@x.com".to_string());
        owner.tenant_id = Some(Id::new("default"));
        state.users.create(&owner).await.unwrap();
        let mut invitee = User::new(
            Id::new("invitee-inv"),
            "tenant:default:email:invitee@x.com",
            "invitee",
            now_secs(),
        );
        invitee.email = Some("invitee@x.com".to_string());
        invitee.tenant_id = Some(Id::new("default"));
        state.users.create(&invitee).await.unwrap();
        let owner_membership = WorkspaceMembership::new(
            Id::new("m-owner-inv"),
            owner.id.clone(),
            ws.id.clone(),
            WorkspaceRole::Owner,
            owner.id.clone(),
            now_secs(),
        );
        state
            .workspace_memberships
            .create(&owner_membership)
            .await
            .unwrap();

        let owner_key = format!("gyre_{}", uuid::Uuid::new_v4().simple());
        state
            .api_keys
            .create(&crate::auth::hash_api_key(&owner_key), &owner.id, "test")
            .await
            .unwrap();
        let invitee_key = format!("gyre_{}", uuid::Uuid::new_v4().simple());
        state
            .api_keys
            .create(
                &crate::auth::hash_api_key(&invitee_key),
                &invitee.id,
                "test",
            )
            .await
            .unwrap();
        InviteeFixture {
            state,
            owner_key,
            invitee_key,
            invitee_id: invitee.id,
        }
    }

    fn invitee_router(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/api/v1/workspaces/:id/invite", post(invite_to_workspace))
            .route(
                "/api/v1/workspaces/:id/invite/accept",
                post(accept_my_workspace_invitation),
            )
            .route(
                "/api/v1/workspaces/:id/invite/decline",
                post(decline_my_workspace_invitation),
            )
            .route("/api/v1/users/me/invitations", get(list_my_invitations))
            .with_state(state)
    }

    /// HTTP-layer negative authorization (review F4): a tenant user who is
    /// NOT a member of the workspace must get 403 from
    /// POST /api/v1/workspaces/{id}/invite through the real chain.
    #[tokio::test]
    async fn non_member_cannot_invite_to_workspace() {
        let f = invitee_fixture().await;
        let app = invitee_router(f.state.clone());
        // The invitee (a plain tenant user, not a member) tries to invite.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-inv/invite")
                    .header("authorization", format!("Bearer {}", f.invitee_key))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"username": "owner-inv"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let all = f
            .state
            .workspace_invitations
            .list_by_workspace(&Id::new("ws-inv"))
            .await
            .unwrap();
        assert!(all.is_empty(), "403 must not create an invitation");
    }

    /// Review F1/F2: the invitee-facing surface must close the loop
    /// end-to-end — invite by username (spec step 1), the invitee lists
    /// THEIR pending invitations via GET /api/v1/users/me/invitations,
    /// accepts via the specced authenticated route
    /// POST /api/v1/workspaces/{id}/invite/accept (no token needed), and
    /// the membership activates.
    #[tokio::test]
    async fn invitee_lists_and_accepts_their_invitation() {
        let f = invitee_fixture().await;
        let app = invitee_router(f.state.clone());

        // Owner invites by username (spec §Workspace Invitation Flow
        // step 1: "by username (tenant-internal) or email").
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-inv/invite")
                    .header("authorization", format!("Bearer {}", f.owner_key))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"username": "invitee", "role": "Developer"}).to_string(),
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
        assert_eq!(json["user_id"], f.invitee_id.to_string());

        // The invitee sees their pending invitation — this is the surface
        // that was previously unreachable (list_by_user had no route).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/users/me/invitations")
                    .header("authorization", format!("Bearer {}", f.invitee_key))
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
        let invitations = json["invitations"].as_array().unwrap();
        assert_eq!(invitations.len(), 1, "exactly the invitee's invitation");
        assert_eq!(invitations[0]["workspace_id"], "ws-inv");
        assert_eq!(invitations[0]["status"], "Pending");
        assert_eq!(invitations[0]["role"], "Developer");
        assert!(
            invitations[0].get("invite_url").is_none(),
            "invitee view must not leak the token"
        );

        // The invitee accepts via the authenticated route — no token.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-inv/invite/accept")
                    .header("authorization", format!("Bearer {}", f.invitee_key))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Membership is active.
        let m = f
            .state
            .workspace_memberships
            .find_by_user_and_workspace(&f.invitee_id, &Id::new("ws-inv"))
            .await
            .unwrap()
            .expect("membership exists after acceptance");
        assert!(m.accepted);
        assert_eq!(m.role, WorkspaceRole::Developer);

        // Single-use: a second accept finds no pending invitation → 404.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-inv/invite/accept")
                    .header("authorization", format!("Bearer {}", f.invitee_key))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // The listing now shows the accepted (terminal) state.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/users/me/invitations")
                    .header("authorization", format!("Bearer {}", f.invitee_key))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["invitations"][0]["status"], "Accepted");
    }

    /// A different authenticated user must not accept someone else's
    /// invitation through the invitee-facing route: there is no pending
    /// invitation for THEM, so 404 (and the real invitee can still accept).
    #[tokio::test]
    async fn other_user_cannot_accept_my_invitation() {
        let f = invitee_fixture().await;
        let app = invitee_router(f.state.clone());

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-inv/invite")
                    .header("authorization", format!("Bearer {}", f.owner_key))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"username": "invitee"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // The OWNER (a member, but not the invitee) has no pending
        // invitation for themselves → 404, not a membership grant.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-inv/invite/accept")
                    .header("authorization", format!("Bearer {}", f.owner_key))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // The invitation is untouched — the invitee can still accept.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-inv/invite/accept")
                    .header("authorization", format!("Bearer {}", f.invitee_key))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    /// Spec §Workspace Invitation Flow step 4: the invitee can decline via
    /// the authenticated route; no membership is created.
    #[tokio::test]
    async fn invitee_declines_via_authenticated_route() {
        let f = invitee_fixture().await;
        let app = invitee_router(f.state.clone());

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-inv/invite")
                    .header("authorization", format!("Bearer {}", f.owner_key))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": "invitee@x.com"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED, "invite by email works");

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-inv/invite/decline")
                    .header("authorization", format!("Bearer {}", f.invitee_key))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let stored = f
            .state
            .workspace_invitations
            .list_by_user(&f.invitee_id)
            .await
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].status, InvitationStatus::Declined);
        assert!(
            f.state
                .workspace_memberships
                .find_by_user_and_workspace(&f.invitee_id, &Id::new("ws-inv"))
                .await
                .unwrap()
                .is_none(),
            "declining must not create a membership"
        );
    }

    /// Review F3 (single-use race): two concurrent claims of the same
    /// pending invitation must produce exactly one winner — the mem
    /// adapter's check-and-set is inside one critical section, so this is
    /// deterministic even under tokio::join!.
    #[tokio::test]
    async fn concurrent_claims_produce_exactly_one_winner() {
        let state = test_state();
        let invitation = WorkspaceInvitation {
            id: Id::new("wi-race"),
            tenant_id: Id::new("default"),
            workspace_id: Id::new("ws-race"),
            user_id: Id::new("u-race"),
            invited_by: Id::new("owner-race"),
            role: WorkspaceRole::Developer,
            status: InvitationStatus::Pending,
            token_hash: hash_token("race-token"),
            expires_at: now_secs() + 86_400,
            created_at: now_secs(),
            accepted_at: None,
        };
        state
            .workspace_invitations
            .create(&invitation)
            .await
            .unwrap();

        let repo = state.workspace_invitations.clone();
        let a = repo.transition_status(
            &invitation.id,
            InvitationStatus::Pending,
            InvitationStatus::Accepted,
            Some(1),
        );
        let b = repo.transition_status(
            &invitation.id,
            InvitationStatus::Pending,
            InvitationStatus::Accepted,
            Some(2),
        );
        let (a, b) = tokio::join!(a, b);
        let winners = a.unwrap() as u8 + b.unwrap() as u8;
        assert_eq!(winners, 1, "exactly one concurrent claim wins");
        let stored = state
            .workspace_invitations
            .find_by_id(&invitation.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, InvitationStatus::Accepted);
        assert!(
            stored.accepted_at.is_some(),
            "winner's timestamp is recorded"
        );
    }

    /// Review F1: the in-app notification must carry the accept path so
    /// the invitee can act on the invitation without the inviter's token,
    /// and must be typed InvitationReceived so the Inbox classifies it
    /// as a workspace invitation (not a trust-policy suggestion).
    #[tokio::test]
    async fn invite_notification_carries_accept_path() {
        let f = invitee_fixture().await;
        let app = invitee_router(f.state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-inv/invite")
                    .header("authorization", format!("Bearer {}", f.owner_key))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"username": "invitee"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let notifs = f
            .state
            .notifications
            .list_for_user(&f.invitee_id, None, None, None, None, None, 50, 0)
            .await
            .unwrap();
        let notif = notifs
            .iter()
            .find(|n| n.source.entity_id == "ws-inv" && n.source.entity_type == "workspace")
            .expect("invitation notification created for the invitee");
        // Review F1: the notification must be typed InvitationReceived
        // (user-management.md §Notification Entity) — a TrustSuggestion
        // type misclassifies it as a trust-policy suggestion in the Inbox.
        assert_eq!(
            notif.notification_type,
            gyre_common::NotificationType::InvitationReceived
        );
        assert_ne!(
            notif.notification_type,
            gyre_common::NotificationType::TrustSuggestion
        );
        let body: serde_json::Value = serde_json::from_str(notif.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["kind"], "workspace_invitation");
        let accept = body["accept_url"].as_str().unwrap();
        assert!(
            accept.contains("/api/v1/workspaces/ws-inv/invite/accept"),
            "accept_url names the authenticated accept route: {accept}"
        );
    }

    // ─── Review round: SSO linking, username contract, containment, caps ──

    /// HTTP helpers: invite via the admin token, accept via magic link.
    async fn http_invite(app: &Router, email: &str) -> (StatusCode, serde_json::Value) {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenant/invite")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({"email": email}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
        )
    }

    async fn http_accept(
        app: &Router,
        token: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let builder = if body.is_null() {
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/invite/{token}/accept"))
        } else {
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/invite/{token}/accept"))
                .header("content-type", "application/json")
        };
        let resp = app
            .clone()
            .oneshot(
                builder
                    .body(Body::from(if body.is_null() {
                        String::new()
                    } else {
                        body.to_string()
                    }))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    /// Review F1: accepting a tenant invitation must LINK the existing
    /// SSO-provisioned account (external_id = Keycloak subject, the
    /// normal state in SSO-mode tenants) instead of creating a parallel
    /// local-mode account that shadows it — the real account gets the
    /// pre-assigned memberships and the invited GlobalRole.
    #[tokio::test]
    async fn accept_links_existing_sso_user_instead_of_parallel_account() {
        let state = test_state();
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-sso"),
            Id::new("default"),
            "SSO WS",
            "sso-ws",
            now_secs(),
        );
        state.workspaces.create(&ws).await.unwrap();
        // The invitee already auto-provisioned via SSO: external_id is the
        // Keycloak subject (NOT the tenant:email namespace), and the
        // stored email carries the IdP's raw casing.
        let mut sso_user = User::new(
            Id::new("sso-alice-1"),
            "keycloak-subj-sso-alice",
            "sso-alice",
            now_secs(),
        );
        sso_user.email = Some("Sso-Alice@Example.com".to_string());
        sso_user.tenant_id = Some(Id::new("default"));
        state.users.create(&sso_user).await.unwrap();
        let users_before = state.users.list().await.unwrap().len();

        let app = Router::new()
            .route("/api/v1/tenant/invite", post(invite_to_tenant))
            .route(
                "/api/v1/invite/:token/accept",
                post(accept_tenant_invitation),
            )
            .with_state(state.clone());

        let (status, json) = {
            let app = Router::new()
                .route("/api/v1/tenant/invite", post(invite_to_tenant))
                .with_state(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/tenant/invite")
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "email": "sso-alice@example.com",
                                "workspace_ids": ["ws-sso"]
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = resp.status();
            let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            (
                status,
                serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
            )
        };
        assert_eq!(status, StatusCode::CREATED);
        let token = json["invite_url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();

        // Unauthenticated accept: the magic-link token is the auth factor.
        let (status, json) = http_accept(&app, &token, serde_json::Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["mode"], "linked", "the existing SSO account is linked");
        assert_eq!(json["user_id"], "sso-alice-1");
        assert_eq!(
            json["username"], "sso-alice",
            "the linked account keeps its immutable handle"
        );
        assert!(
            json["api_key"].is_null(),
            "token possession must not mint a credential for a pre-existing \
             SSO-provisioned account — the IdP remains its auth factor"
        );

        // No parallel local-mode account was created.
        assert_eq!(
            state.users.list().await.unwrap().len(),
            users_before,
            "accept must not create a second account for the same person"
        );
        assert!(
            state
                .users
                .find_by_external_id("tenant:default:email:sso-alice@example.com")
                .await
                .unwrap()
                .is_none(),
            "no parallel local-mode (tenant:email-namespace) account"
        );

        // The pre-assigned membership and the invited role attach to the
        // REAL account.
        let membership = state
            .workspace_memberships
            .find_by_user_and_workspace(&Id::new("sso-alice-1"), &Id::new("ws-sso"))
            .await
            .unwrap()
            .expect("membership granted to the SSO account");
        assert!(membership.accepted, "membership is active");
        let linked = state
            .users
            .find_by_id(&Id::new("sso-alice-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(linked.global_role, GlobalRole::Member);
        assert_eq!(
            linked.roles,
            vec![UserRole::Developer],
            "invited Member role applied to the linked account"
        );
        // The link must not have disturbed the SSO identity binding.
        assert_eq!(linked.external_id, "keycloak-subj-sso-alice");
    }

    /// Review d6b8a97: local-mode acceptance must deliver a credential —
    /// the platform has no password-login path, so without a minted API
    /// key the accepted invitee could never authenticate (the only other
    /// unauthenticated mint, POST /api/v1/users, creates a NEW user and
    /// cannot serve them). The minted key must authenticate as the new
    /// user through the REAL auth extractor, not just resolve in the
    /// store.
    #[tokio::test]
    async fn local_mode_accept_mints_authenticating_api_key() {
        let state = test_state();
        let app = Router::new()
            .route("/api/v1/tenant/invite", post(invite_to_tenant))
            .route(
                "/api/v1/invite/:token/accept",
                post(accept_tenant_invitation),
            )
            .route("/api/v1/users/me", get(crate::api::users::get_me))
            .with_state(state.clone());

        let (status, json) = http_invite(&app, "nadia@example.com").await;
        assert_eq!(status, StatusCode::CREATED);
        let token = json["invite_url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();

        // Unauthenticated accept (magic-link token is the auth factor).
        let (status, json) =
            http_accept(&app, &token, serde_json::json!({"display_name": "Nadia"})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["mode"], "created");
        let raw_key = json["api_key"]["key"]
            .as_str()
            .expect("local-mode accept must return a first API key")
            .to_string();
        assert!(raw_key.starts_with("gyre_"), "key: {raw_key}");
        assert_eq!(json["api_key"]["name"], "invitation");
        let user_id = json["user_id"].as_str().unwrap().to_string();
        let tenant_id = json["tenant_id"].as_str().unwrap().to_string();

        // The key authenticates as the new user through the full routing
        // chain (auth extractor's state.api_keys hash lookup).
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/users/me")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "the minted key must authenticate the accepted invitee"
        );
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let me: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(me["id"], user_id.as_str());
        assert_eq!(me["username"], "nadia");
        assert_eq!(me["display_name"], "Nadia");
        // First-class local user: scoped to the inviting tenant, not a
        // fabricated "default".
        assert_eq!(me["tenant_id"], tenant_id.as_str());

        // Only the hash is persisted: the store cannot reconstruct the
        // plaintext key handed to the invitee.
        let stored = state
            .api_keys
            .find_user_id(&crate::auth::hash_api_key(&raw_key))
            .await
            .unwrap()
            .expect("minted key resolves to the accepted user");
        assert_eq!(stored.to_string(), user_id);
    }

    /// Review d6b8a97, dedupe arm: a SECOND invitation to an already
    /// accepted tenant+email finds the local-mode account via the
    /// tenant:email namespace — acceptance must deliver a FRESH key
    /// (a prior accept that failed mid-flight never delivered its key to
    /// the invitee, and the plaintext is unrecoverable) that
    /// authenticates as the SAME user, not a parallel account.
    #[tokio::test]
    async fn reinvite_acceptance_mints_fresh_key_for_existing_local_account() {
        let state = test_state();
        let app = Router::new()
            .route("/api/v1/tenant/invite", post(invite_to_tenant))
            .route(
                "/api/v1/invite/:token/accept",
                post(accept_tenant_invitation),
            )
            .route("/api/v1/users/me", get(crate::api::users::get_me))
            .with_state(state.clone());

        let (status, json) = http_invite(&app, "oscar@example.com").await;
        assert_eq!(status, StatusCode::CREATED);
        let token1 = json["invite_url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();
        let (status, json) = http_accept(&app, &token1, serde_json::Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["mode"], "created");
        let key1 = json["api_key"]["key"].as_str().unwrap().to_string();
        let user_id = json["user_id"].as_str().unwrap().to_string();

        // Re-invite the same email (e.g. the invitee lost the first key,
        // or the first accept failed after the claim rolled back).
        let (status, json) = http_invite(&app, "oscar@example.com").await;
        assert_eq!(status, StatusCode::CREATED);
        let token2 = json["invite_url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();
        let (status, json) = http_accept(&app, &token2, serde_json::Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            json["mode"], "linked",
            "the namespace lookup finds the existing local account"
        );
        assert_eq!(json["user_id"], user_id.as_str());
        let key2 = json["api_key"]["key"]
            .as_str()
            .expect("re-acceptance must deliver a fresh credential")
            .to_string();
        assert_ne!(
            key2, key1,
            "a fresh key is minted, not the old one replayed"
        );

        // The fresh key authenticates as the SAME user — no parallel
        // account was created.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/users/me")
                    .header("authorization", format!("Bearer {key2}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let me: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(me["id"], user_id.as_str());
        assert_eq!(me["username"], "oscar");
    }

    /// Review F2: usernames are globally unique across tenants; a second
    /// accept whose derived handle collides (same local-part, different
    /// domain — or the same person invited in two tenants) must get the
    /// deterministic suffix (bob-2), not a 500 that rolls the invitation
    /// back to Pending and makes it permanently un-acceptable.
    #[tokio::test]
    async fn username_collision_on_second_accept_gets_suffix_not_500() {
        let state = test_state();
        let app = Router::new()
            .route("/api/v1/tenant/invite", post(invite_to_tenant))
            .route(
                "/api/v1/invite/:token/accept",
                post(accept_tenant_invitation),
            )
            .with_state(state.clone());

        let (status, json) = http_invite(&app, "bob@acme.com").await;
        assert_eq!(status, StatusCode::CREATED);
        let token1 = json["invite_url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();
        let (status, json) = http_accept(&app, &token1, serde_json::Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["username"], "bob");
        assert_eq!(json["mode"], "created");

        // Same local-part, different domain, same tenant.
        let (status, json) = http_invite(&app, "bob@google.com").await;
        assert_eq!(status, StatusCode::CREATED);
        let token2 = json["invite_url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();
        let (status, json) = http_accept(&app, &token2, serde_json::Value::Null).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "a colliding derived handle must resolve with a suffix, not a 500"
        );
        assert_eq!(json["username"], "bob-2", "deterministic suffix scheme");
        assert_eq!(json["mode"], "created");

        // Cross-tenant variant: the same email invited in a second tenant
        // derives the same local-part handle and must also resolve.
        let t2_inv = TenantInvitation {
            id: Id::new("inv-t2"),
            tenant_id: Id::new("tenant-two"),
            email: "bob@acme.com".to_string(),
            invited_by: Id::new("someadmin"),
            role: GlobalRole::Member,
            workspace_ids: vec![],
            workspace_roles: vec![],
            status: InvitationStatus::Pending,
            token_hash: hash_token("t2-token"),
            expires_at: now_secs() + 86_400,
            created_at: now_secs(),
            accepted_at: None,
        };
        state.tenant_invitations.create(&t2_inv).await.unwrap();
        let (status, json) = http_accept(&app, "t2-token", serde_json::Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            json["username"], "bob-3",
            "global uniqueness across tenants"
        );
    }

    /// Review F3: an explicit username must satisfy the URL-safe handle
    /// contract (the same validate_username gate as the auth.rs SSO and
    /// SCIM provisioning paths) — garbage is a 400, a taken handle is a
    /// 409 — and both leave the invitation Pending so the invitee can
    /// retry (e.g. without the username field).
    #[tokio::test]
    async fn accept_validates_and_reserves_explicit_username() {
        let state = test_state();
        // An existing user owns the handle "bob".
        let mut bob = User::new(
            Id::new("bob-1"),
            "tenant:default:email:bob@x.com",
            "bob",
            now_secs(),
        );
        bob.email = Some("bob@x.com".to_string());
        bob.tenant_id = Some(Id::new("default"));
        state.users.create(&bob).await.unwrap();

        let app = Router::new()
            .route("/api/v1/tenant/invite", post(invite_to_tenant))
            .route(
                "/api/v1/invite/:token/accept",
                post(accept_tenant_invitation),
            )
            .with_state(state.clone());
        let (status, json) = http_invite(&app, "carol@example.com").await;
        assert_eq!(status, StatusCode::CREATED);
        let inv_id = json["id"].as_str().unwrap().to_string();
        let token = json["invite_url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();

        // Not URL-safe → 400, invitation rolled back to Pending.
        let (status, _) = http_accept(
            &app,
            &token,
            serde_json::json!({"username": "Carol Smith!!"}),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "non-URL-safe usernames must be rejected, not persisted verbatim"
        );
        let stored = state
            .tenant_invitations
            .find_by_id(&Id::new(inv_id.clone()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, InvitationStatus::Pending, "retryable");

        // URL-safe but taken → 409, still Pending.
        let (status, _) = http_accept(&app, &token, serde_json::json!({"username": "bob"})).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let stored = state
            .tenant_invitations
            .find_by_id(&Id::new(inv_id.clone()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, InvitationStatus::Pending, "retryable");

        // Retry without a username: the derived handle succeeds.
        let (status, json) = http_accept(&app, &token, serde_json::Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["username"], "carol");
    }

    /// Review F6: the cross-tenant containment guards must actually fire
    /// under test — every prior fixture lived in tenant "default", so
    /// deleting any of these guards would not have failed the suite.
    /// This fixture spans TWO tenants.
    #[tokio::test]
    async fn cross_tenant_guards_reject_foreign_callers() {
        let state = test_state();
        // Tenant "other": a workspace, a TenantAdmin, and a plain user,
        // both with real API keys (the extractor resolves their tenant
        // from the stored user binding).
        let ws_other = gyre_domain::Workspace::new(
            Id::new("ws-other"),
            Id::new("other"),
            "Other WS",
            "other-ws",
            now_secs(),
        );
        state.workspaces.create(&ws_other).await.unwrap();
        let mut other_admin = User::new(
            Id::new("other-admin-1"),
            "tenant:other:email:admin@other.com",
            "other-admin",
            now_secs(),
        );
        other_admin.email = Some("admin@other.com".to_string());
        other_admin.tenant_id = Some(Id::new("other"));
        other_admin.roles = vec![UserRole::Admin];
        other_admin.global_role = GlobalRole::TenantAdmin;
        state.users.create(&other_admin).await.unwrap();
        let other_key = format!("gyre_{}", uuid::Uuid::new_v4().simple());
        state
            .api_keys
            .create(
                &crate::auth::hash_api_key(&other_key),
                &other_admin.id,
                "test",
            )
            .await
            .unwrap();
        let mut other_user = User::new(
            Id::new("other-user-1"),
            "tenant:other:email:user@other.com",
            "other-user",
            now_secs(),
        );
        other_user.email = Some("user@other.com".to_string());
        other_user.tenant_id = Some(Id::new("other"));
        state.users.create(&other_user).await.unwrap();

        // A pending invitation in tenant "default" (system token's
        // tenant) for the other-admin's revoke probe.
        let (status, json) = {
            let app = Router::new()
                .route("/api/v1/tenant/invite", post(invite_to_tenant))
                .with_state(state.clone());
            http_invite(&app, "someone@default.com").await
        };
        assert_eq!(status, StatusCode::CREATED);
        let default_inv_id = json["id"].as_str().unwrap().to_string();

        // A pending workspace invitation in ws-other for the list/revoke
        // probes (seeded directly: the API path is exercised elsewhere).
        let ws_inv = WorkspaceInvitation {
            id: Id::new("ws-inv-other"),
            tenant_id: Id::new("other"),
            workspace_id: Id::new("ws-other"),
            user_id: other_user.id.clone(),
            invited_by: other_admin.id.clone(),
            role: WorkspaceRole::Developer,
            status: InvitationStatus::Pending,
            token_hash: hash_token("other-ws-token"),
            expires_at: now_secs() + 86_400,
            created_at: now_secs(),
            accepted_at: None,
        };
        state.workspace_invitations.create(&ws_inv).await.unwrap();

        let app = Router::new()
            .route(
                "/api/v1/tenant/invitations/:id",
                delete(revoke_tenant_invitation),
            )
            .route("/api/v1/workspaces/:id/invite", post(invite_to_workspace))
            .route(
                "/api/v1/workspaces/:id/invite/accept",
                post(accept_my_workspace_invitation),
            )
            .route(
                "/api/v1/workspaces/:id/invitations",
                get(list_workspace_invitations),
            )
            .route(
                "/api/v1/workspaces/:id/invitations/:id",
                delete(revoke_workspace_invitation),
            )
            .with_state(state.clone());

        // default-tenant caller (system token, tenant "default", Admin
        // role — passes every role gate) vs the "other"-tenant workspace:
        // the tenant containment guard must be the thing that fires.
        let probes = [
            (
                "POST",
                "/api/v1/workspaces/ws-other/invite",
                Some(serde_json::json!({"user_id": "other-user-1", "role": "Developer"})),
            ),
            ("GET", "/api/v1/workspaces/ws-other/invitations", None),
            (
                "DELETE",
                "/api/v1/workspaces/ws-other/invitations/ws-inv-other",
                None,
            ),
            ("POST", "/api/v1/workspaces/ws-other/invite/accept", None),
        ];
        for (method, uri, body) in probes {
            let mut builder = Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", "Bearer test-token");
            let body = match body {
                Some(b) => {
                    builder = builder.header("content-type", "application/json");
                    Body::from(b.to_string())
                }
                None => Body::empty(),
            };
            let resp = app
                .clone()
                .oneshot(builder.body(body).unwrap())
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "default-tenant caller must be contained: {method} {uri}"
            );
        }

        // Symmetric probe: the "other"-tenant admin cannot revoke a
        // tenant "default" invitation.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/tenant/invitations/{default_inv_id}"))
                    .header("authorization", format!("Bearer {other_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "foreign-tenant admin must not revoke tenant default's invitation"
        );
        let stored = state
            .tenant_invitations
            .find_by_id(&Id::new(default_inv_id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, InvitationStatus::Pending, "not burned");
    }

    /// Review F6: max_pending_invitations must actually be enforced —
    /// previously only the default value (50) was asserted, never the
    /// cap itself. Also proves revocation frees capacity.
    #[tokio::test]
    async fn workspace_invite_cap_is_enforced() {
        let state = test_state();
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-cap"),
            Id::new("default"),
            "Cap WS",
            "cap-ws",
            now_secs(),
        );
        state.workspaces.create(&ws).await.unwrap();
        let mut owner = User::new(
            Id::new("cap-owner"),
            "tenant:default:email:cap-owner@x.com",
            "cap-owner",
            now_secs(),
        );
        owner.email = Some("cap-owner@x.com".to_string());
        owner.tenant_id = Some(Id::new("default"));
        state.users.create(&owner).await.unwrap();
        let owner_membership = WorkspaceMembership::new(
            Id::new("m-cap-owner"),
            owner.id.clone(),
            ws.id.clone(),
            WorkspaceRole::Owner,
            owner.id.clone(),
            now_secs(),
        );
        state
            .workspace_memberships
            .create(&owner_membership)
            .await
            .unwrap();
        let owner_key = format!("gyre_{}", uuid::Uuid::new_v4().simple());
        state
            .api_keys
            .create(&crate::auth::hash_api_key(&owner_key), &owner.id, "test")
            .await
            .unwrap();
        let mut invitee_ids = Vec::new();
        for n in 1..=3 {
            let mut invitee = User::new(
                Id::new(format!("cap-user-{n}")),
                format!("tenant:default:email:cap-user-{n}@x.com"),
                format!("cap-user-{n}"),
                now_secs(),
            );
            invitee.email = Some(format!("cap-user-{n}@x.com"));
            invitee.tenant_id = Some(Id::new("default"));
            state.users.create(&invitee).await.unwrap();
            invitee_ids.push(invitee.id.to_string());
        }

        let app = Router::new()
            .route(
                "/api/v1/tenant/invitations/policy",
                get(get_invitation_policy).put(update_invitation_policy),
            )
            .route("/api/v1/workspaces/:id/invite", post(invite_to_workspace))
            .route(
                "/api/v1/workspaces/:id/invitations/:id",
                delete(revoke_workspace_invitation),
            )
            .with_state(state.clone());

        // Cap the tenant's workspace invitations at 2.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/tenant/invitations/policy")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"max_pending_invitations": 2}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let auth = format!("Bearer {owner_key}");
        let invite = |user_id: String| {
            let app = app.clone();
            let auth = auth.clone();
            async move {
                app.oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/workspaces/ws-cap/invite")
                        .header("authorization", auth)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({"user_id": user_id, "role": "Developer"})
                                .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };

        let resp = invite(invitee_ids[0].clone()).await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = invite(invitee_ids[1].clone()).await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = invite(invitee_ids[2].clone()).await;
        assert_eq!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "the cap must reject the third pending invitation"
        );

        // Revoking one frees capacity for the blocked invitee.
        let all = state
            .workspace_invitations
            .list_by_workspace(&Id::new("ws-cap"))
            .await
            .unwrap();
        let first = all
            .iter()
            .find(|i| i.user_id.as_str() == invitee_ids[0])
            .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/workspaces/ws-cap/invitations/{}",
                        first.id
                    ))
                    .header("authorization", auth.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = invite(invitee_ids[2].clone()).await;
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "revocation frees pending capacity"
        );
    }

    /// Review F6: workspace-invitation expiry rejection was untested
    /// (only the tenant side had an expired-accept test). An expired
    /// pending workspace invitation must be rejected on accept and
    /// marked Expired.
    #[tokio::test]
    async fn expired_workspace_invitation_cannot_be_accepted() {
        let state = test_state();
        // Seed an already-expired pending workspace invitation directly
        // through the repository (the API enforces >= 1 day expiry).
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-exp"),
            Id::new("default"),
            "Exp WS",
            "exp-ws",
            now_secs(),
        );
        state.workspaces.create(&ws).await.unwrap();
        let mut invitee = User::new(
            Id::new("exp-user"),
            "tenant:default:email:exp-user@x.com",
            "exp-user",
            now_secs(),
        );
        invitee.tenant_id = Some(Id::new("default"));
        state.users.create(&invitee).await.unwrap();
        let invitation = WorkspaceInvitation {
            id: Id::new("ws-inv-exp"),
            tenant_id: Id::new("default"),
            workspace_id: Id::new("ws-exp"),
            user_id: invitee.id.clone(),
            invited_by: Id::new("someadmin"),
            role: WorkspaceRole::Developer,
            status: InvitationStatus::Pending,
            token_hash: hash_token("expired-ws-token"),
            expires_at: now_secs() - 10,
            created_at: now_secs() - 100,
            accepted_at: None,
        };
        state
            .workspace_invitations
            .create(&invitation)
            .await
            .unwrap();

        let app = Router::new()
            .route(
                "/api/v1/workspaces/invitations/:token/accept",
                post(accept_workspace_invitation),
            )
            .with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/invitations/expired-ws-token/accept")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let stored = state
            .workspace_invitations
            .find_by_id(&Id::new("ws-inv-exp"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, InvitationStatus::Expired);
        // No membership was activated by the expired acceptance.
        assert!(state
            .workspace_memberships
            .find_by_user_and_workspace(&invitee.id, &Id::new("ws-exp"))
            .await
            .unwrap()
            .is_none());
    }
}
