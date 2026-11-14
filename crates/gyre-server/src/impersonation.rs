//! Admin user impersonation (identity-security.md §Impersonation).
//!
//! Flow (AP2-inspired: intent → signed mandate → receipt):
//! 1. **Intent** — an Admin calls `POST /api/v1/admin/impersonate` for a
//!    target user. The server generates a one-time approval code, stores
//!    only its SHA-256 hash, and delivers the raw code to the target user
//!    via an Inbox notification (+ WebSocket push when online). The code is
//!    never returned to the requesting admin — returning it would let the
//!    admin self-approve and defeat the spec requirement that the target
//!    provides the code.
//! 2. **Signed mandate** — the target user submits the code to
//!    `POST /api/v1/admin/impersonate/:request_id/approve`. On a
//!    constant-time match the server mints a short-lived EdDSA JWT whose
//!    `sub` is the target (permissions match) and whose `impersonator_id`
//!    always names the acting admin.
//! 3. **Receipt** — `ImpersonationStarted` / `ImpersonationEnded` audit
//!    events are emitted with full attribution, and every API call made
//!    with the token is recorded as an impersonation action attributed to
//!    the impersonator (see `abac_middleware`).
//!
//! Sessions and requests are persisted through the `KvJsonStore` port
//! (namespaces below) so they survive a restart; the JWT itself carries
//! `session_id`, and ending the session revokes the token before `exp`.
//!
//! Authorization constraints:
//! - Only the `Admin` role may request impersonation (ABAC `admin` resource
//!   + in-handler role check).
//! - Impersonating another Admin is prohibited (the platform role model has
//!   no SuperAdmin; the task plan's "requires SuperAdmin or is prohibited"
//!   resolves to prohibited).
//! - Max one active impersonation session per admin at a time.
//! - TTL defaults to 15 minutes, configurable via `GYRE_IMPERSONATION_TTL`.

use gyre_common::{Id, Notification, NotificationType, WsMessage};
use gyre_domain::{AuditEvent, AuditEventType, AuditOutcome, User, UserRole};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::error::ApiError;
use crate::auth::{AuthenticatedAgent, ImpersonationContext, ImpersonationJwtClaims};
use crate::AppState;

/// KV namespace for impersonation approval requests.
pub const REQUEST_NAMESPACE: &str = "impersonation_requests";
/// KV namespace for impersonation sessions.
pub const SESSION_NAMESPACE: &str = "impersonation_sessions";

/// How long an approval request (and its code) stays valid: 10 minutes.
pub const REQUEST_TTL_SECS: u64 = 600;

/// Default impersonation session TTL: 15 minutes.
pub const DEFAULT_SESSION_TTL_SECS: u64 = 900;

/// Impersonation session TTL, configurable via `GYRE_IMPERSONATION_TTL`
/// (seconds). Falls back to the 15-minute default when unset or invalid.
pub fn session_ttl_secs() -> u64 {
    std::env::var("GYRE_IMPERSONATION_TTL")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_SESSION_TTL_SECS)
}

/// Status of an impersonation approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpersonationRequestStatus {
    /// Awaiting the target user's approval code.
    Pending,
    /// Target user submitted the correct code; session issued.
    Approved,
    /// Target user explicitly denied the request.
    Denied,
    /// Request was not approved before `expires_at`.
    Expired,
}

/// An impersonation approval request (identity-security.md §Impersonation).
///
/// The raw approval code is never stored — only its SHA-256 hash. The code
/// is delivered to the target user out-of-band (Inbox notification) so the
/// requesting admin cannot self-approve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpersonationRequest {
    pub request_id: String,
    /// User id of the requesting admin.
    pub impersonator_id: String,
    /// Display name of the requesting admin (audit/notification strings).
    pub impersonator_name: String,
    /// User id of the user to be impersonated.
    pub target_user_id: String,
    /// SHA-256 hash of the one-time approval code.
    pub approval_code_hash: String,
    /// Tenant scope captured from the requesting admin's auth context.
    pub tenant_id: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub status: ImpersonationRequestStatus,
}

/// An active (or ended) impersonation session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpersonationSession {
    pub session_id: String,
    pub request_id: String,
    /// User id of the acting admin.
    pub impersonator_id: String,
    pub impersonator_name: String,
    /// User id of the impersonated user.
    pub target_user_id: String,
    pub tenant_id: String,
    pub started_at: u64,
    pub expires_at: u64,
    /// Count of API calls made with the impersonation token.
    pub actions_taken: u64,
    pub ended: bool,
    pub ended_at: Option<u64>,
    /// "ended_by_admin" | "expired"
    pub end_reason: Option<String>,
}

// ── persistence (KvJsonStore port) ──────────────────────────────────────────

async fn save_request(state: &AppState, req: &ImpersonationRequest) -> Result<(), ApiError> {
    state
        .kv_store
        .kv_set(
            REQUEST_NAMESPACE,
            &req.request_id,
            serde_json::to_string(req).map_err(|e| ApiError::Internal(e.into()))?,
        )
        .await
        .map_err(ApiError::Internal)
}

async fn load_request(
    state: &AppState,
    request_id: &str,
) -> Result<Option<ImpersonationRequest>, ApiError> {
    let raw = state
        .kv_store
        .kv_get(REQUEST_NAMESPACE, request_id)
        .await
        .map_err(ApiError::Internal)?;
    raw.map(|v| serde_json::from_str(&v).map_err(|e| ApiError::Internal(e.into())))
        .transpose()
}

async fn save_session(state: &AppState, session: &ImpersonationSession) -> Result<(), ApiError> {
    state
        .kv_store
        .kv_set(
            SESSION_NAMESPACE,
            &session.session_id,
            serde_json::to_string(session).map_err(|e| ApiError::Internal(e.into()))?,
        )
        .await
        .map_err(ApiError::Internal)
}

async fn load_session(
    state: &AppState,
    session_id: &str,
) -> Result<Option<ImpersonationSession>, ApiError> {
    let raw = state
        .kv_store
        .kv_get(SESSION_NAMESPACE, session_id)
        .await
        .map_err(ApiError::Internal)?;
    raw.map(|v| serde_json::from_str(&v).map_err(|e| ApiError::Internal(e.into())))
        .transpose()
}

async fn list_sessions(state: &AppState) -> Result<Vec<ImpersonationSession>, ApiError> {
    let pairs = state
        .kv_store
        .kv_list(SESSION_NAMESPACE)
        .await
        .map_err(ApiError::Internal)?;
    pairs
        .iter()
        .map(|(_, v)| serde_json::from_str(v).map_err(|e| ApiError::Internal(e.into())))
        .collect()
}

/// The admin's active (not ended, not expired) impersonation session, if any.
/// Enforces the max-1-active-session-per-admin rate limit.
pub async fn active_session_for_admin(
    state: &AppState,
    impersonator_id: &str,
) -> Result<Option<ImpersonationSession>, ApiError> {
    let now = now_secs();
    Ok(list_sessions(state)
        .await?
        .into_iter()
        .find(|s| s.impersonator_id == impersonator_id && !s.ended && s.expires_at > now))
}

/// Serializes every session-record read-modify-write (increment of
/// `actions_taken`, session end). Without it, two concurrent impersonated
/// calls can both load `actions_taken = N` and both persist `N + 1`,
/// undercounting the session; worse, a racing increment can overwrite an
/// `end_session` write and resurrect an ended session. Single-process
/// lock: the kv store stays the record of truth, this only orders the RMW.
static SESSION_WRITE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ── approval code ───────────────────────────────────────────────────────────

/// Generate a one-time 8-digit numeric approval code (CSPRNG).
pub fn generate_approval_code() -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut buf = [0u8; 8];
    rng.fill(&mut buf)
        .expect("CSPRNG fill must not fail on supported platforms");
    buf.iter().map(|b| (b % 10).to_string()).collect()
}

/// SHA-256 hex hash of an approval code (same construction as API keys).
pub fn hash_approval_code(code: &str) -> String {
    crate::auth::hash_api_key(code)
}

// ── flow ────────────────────────────────────────────────────────────────────

/// Create an impersonation request from `admin` for `target`.
///
/// Persists the request (code hashed) and delivers the raw code to the
/// target user via Inbox notification + WebSocket push. Returns the stored
/// request; the raw code is NOT returned to the caller — only the target
/// receives it.
pub async fn create_request(
    state: &AppState,
    admin: &User,
    target: &User,
    tenant_id: &str,
) -> Result<ImpersonationRequest, ApiError> {
    let now = now_secs();
    let code = generate_approval_code();
    let req = ImpersonationRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        impersonator_id: admin.id.to_string(),
        impersonator_name: admin.display_name.clone(),
        target_user_id: target.id.to_string(),
        approval_code_hash: hash_approval_code(&code),
        tenant_id: tenant_id.to_string(),
        created_at: now,
        expires_at: now + REQUEST_TTL_SECS,
        status: ImpersonationRequestStatus::Pending,
    };
    // Max one active impersonation session per admin (rate limit). The
    // check and the request save run inside the session-write lock — the
    // same lock that serializes approval's session-creation critical
    // section — so a concurrent approval cannot open a session between
    // this read and the save (an unlocked gap would let the admin stack
    // an approvable request against a session opened in the gap). The
    // lock is released before notification delivery.
    {
        let _guard = SESSION_WRITE_LOCK.lock().await;
        if let Some(active) = active_session_for_admin(state, admin.id.as_str()).await? {
            return Err(ApiError::Conflict(format!(
                "admin already has an active impersonation session (started {})",
                active.started_at
            )));
        }
        save_request(state, &req).await?;
    }

    // Deliver the code to the target user (never to the requesting admin).
    deliver_approval_code(state, target, &req, &code).await;

    Ok(req)
}

/// Approve an impersonation request with the target user's code.
///
/// `caller_user_id` must be the request's target user and `code` must match
/// the stored hash (constant-time). On success: creates the session, mints
/// the impersonation JWT, emits `ImpersonationStarted`, and notifies the
/// target that impersonation began.
pub async fn approve_request(
    state: &AppState,
    request_id: &str,
    caller_user_id: &Id,
    code: &str,
) -> Result<(ImpersonationSession, String, u64), ApiError> {
    // The session-creation critical section (request load → Pending
    // check → max-1-active check → session save → request-status save)
    // runs under the session-write lock. Without it, two concurrent
    // approvals of the same request both load it as Pending, both pass
    // the rate limit and both save live sessions — two impersonation
    // tokens minted for one consent and two ImpersonationStarted events,
    // violating the max-1-active-session-per-admin constraint. The lock
    // is shared with create_request's rate-limit read, deny_request's
    // transition, the action counter and end_session, so the whole
    // request/session state machine is serialized (single-process lock;
    // the kv store stays the record of truth). Released before token
    // minting, audit emission and notification delivery.
    let _guard = SESSION_WRITE_LOCK.lock().await;

    let mut req = load_request(state, request_id).await?.ok_or_else(|| {
        ApiError::NotFound(format!("impersonation request {request_id} not found"))
    })?;

    // Per-handler authorization (ABAC-exempt route): only the target user
    // of this request may approve it. Checked BEFORE any status or expiry
    // is revealed — otherwise any authenticated user could probe which
    // users an admin asked to impersonate and whether consent was given
    // (a non-target probing an approved request must get 403, not 409).
    if req.target_user_id != caller_user_id.as_str() {
        return Err(ApiError::Forbidden(
            "only the impersonation target may approve this request".to_string(),
        ));
    }

    if req.status != ImpersonationRequestStatus::Pending {
        return Err(ApiError::Conflict(format!(
            "impersonation request {request_id} is already {}",
            match req.status {
                ImpersonationRequestStatus::Approved => "approved",
                ImpersonationRequestStatus::Denied => "denied",
                ImpersonationRequestStatus::Expired => "expired",
                ImpersonationRequestStatus::Pending => unreachable!(),
            }
        )));
    }

    let now = now_secs();
    if now >= req.expires_at {
        req.status = ImpersonationRequestStatus::Expired;
        save_request(state, &req).await?;
        return Err(ApiError::Conflict(
            "impersonation request has expired".to_string(),
        ));
    }

    // The code is the target user's consent proof — compare hashes in
    // constant time.
    if !crate::auth::tokens_equal(&hash_approval_code(code), &req.approval_code_hash) {
        return Err(ApiError::Forbidden("invalid approval code".to_string()));
    }

    // Max one active session per admin (rate limit).
    if let Some(active) = active_session_for_admin(state, &req.impersonator_id).await? {
        return Err(ApiError::Conflict(format!(
            "admin already has an active impersonation session (started {})",
            active.started_at
        )));
    }

    // Session + mandate.
    let ttl = session_ttl_secs();
    let session = ImpersonationSession {
        session_id: uuid::Uuid::new_v4().to_string(),
        request_id: req.request_id.clone(),
        impersonator_id: req.impersonator_id.clone(),
        impersonator_name: req.impersonator_name.clone(),
        target_user_id: req.target_user_id.clone(),
        tenant_id: req.tenant_id.clone(),
        started_at: now,
        expires_at: now + ttl,
        actions_taken: 0,
        ended: false,
        ended_at: None,
        end_reason: None,
    };
    save_session(state, &session).await?;

    req.status = ImpersonationRequestStatus::Approved;
    save_request(state, &req).await?;

    drop(_guard);

    let token = state
        .agent_signing_key
        .mint_impersonation(
            &req.target_user_id,
            &req.impersonator_id,
            &session.session_id,
            &req.tenant_id,
            &state.base_url,
            ttl,
        )
        .map_err(|e| ApiError::Internal(anyhow::anyhow!(e)))?;

    // Receipt: ImpersonationStarted with full attribution.
    emit_audit(
        state,
        AuditEventType::ImpersonationStarted,
        Some(Id::new(req.impersonator_id.clone())),
        serde_json::json!({
            "impersonator_id": req.impersonator_id,
            "target_user_id": req.target_user_id,
            // Spec field `approval_token`: the hash identifying the
            // consumed approval. The raw code is single-use and never
            // persisted; the hash is the durable proof of which approval
            // authorized this session.
            "approval_token": req.approval_code_hash,
            "request_id": req.request_id,
            "session_id": session.session_id,
            "expires_at": session.expires_at,
        }),
        &session.session_id,
        AuditOutcome::Success,
    )
    .await;

    // Notify the target that impersonation began.
    notify_impersonation_alert(state, &req, &session).await;

    Ok((session.clone(), token, session.expires_at))
}

/// Deny an impersonation request. Only the target user may deny.
pub async fn deny_request(
    state: &AppState,
    request_id: &str,
    caller_user_id: &Id,
) -> Result<(), ApiError> {
    // Serialized with approve's critical section (same lock) so a
    // concurrent approve/deny pair cannot both pass the Pending check
    // and overwrite each other's terminal status.
    let _guard = SESSION_WRITE_LOCK.lock().await;
    let mut req = load_request(state, request_id).await?.ok_or_else(|| {
        ApiError::NotFound(format!("impersonation request {request_id} not found"))
    })?;

    // Per-handler authorization (ABAC-exempt route): only the target user
    // may deny. Checked before any status is revealed.
    if req.target_user_id != caller_user_id.as_str() {
        return Err(ApiError::Forbidden(
            "only the impersonation target may deny this request".to_string(),
        ));
    }

    if req.status != ImpersonationRequestStatus::Pending {
        return Err(ApiError::Conflict(
            "impersonation request is no longer pending".to_string(),
        ));
    }

    req.status = ImpersonationRequestStatus::Denied;
    save_request(state, &req).await?;
    Ok(())
}

/// End an impersonation session (explicit admin end or TTL expiry).
///
/// Idempotent: a session that is already ended is left as-is so exactly one
/// `ImpersonationEnded` event is emitted per session. Returns the ended
/// session.
pub async fn end_session(
    state: &AppState,
    session_id: &str,
    reason: &str,
) -> Result<ImpersonationSession, ApiError> {
    // Hold the session-write lock across the read-modify-write so a
    // concurrently recording impersonated action can neither interleave
    // a stale increment over the end marker nor lose its own count.
    let _guard = SESSION_WRITE_LOCK.lock().await;
    let mut session = load_session(state, session_id).await?.ok_or_else(|| {
        ApiError::NotFound(format!("impersonation session {session_id} not found"))
    })?;

    if session.ended {
        return Ok(session);
    }

    let now = now_secs();
    session.ended = true;
    session.ended_at = Some(now);
    session.end_reason = Some(reason.to_string());
    save_session(state, &session).await?;

    emit_audit(
        state,
        AuditEventType::ImpersonationEnded,
        Some(Id::new(session.impersonator_id.clone())),
        serde_json::json!({
            "impersonator_id": session.impersonator_id,
            "target_user_id": session.target_user_id,
            "duration_secs": now.saturating_sub(session.started_at),
            "actions_taken": session.actions_taken,
            "session_id": session.session_id,
            "reason": reason,
        }),
        &session.session_id,
        AuditOutcome::Success,
    )
    .await;

    Ok(session)
}

/// Find the session created from a request id (the admin-facing handle).
pub async fn find_session_by_request(
    state: &AppState,
    request_id: &str,
) -> Result<Option<ImpersonationSession>, ApiError> {
    Ok(list_sessions(state)
        .await?
        .into_iter()
        .find(|s| s.request_id == request_id))
}

/// End every active impersonation session the given user participates in
/// (as impersonator OR target). Part of the deactivation cascade
/// (user-management.md §API > User Management): a deactivated account
/// fails authentication on every path, and an unexpired impersonation JWT
/// is no exception — ending the session revokes the token before `exp`
/// and closes the audit trail with an `ImpersonationEnded` event.
/// `resolve_impersonation_auth` additionally re-checks both accounts'
/// deactivation state on every call, so this is belt-and-suspenders with
/// the resolver, not the sole gate. Returns the number of sessions ended.
pub async fn end_active_sessions_for_user(state: &AppState, user_id: &str, reason: &str) -> u64 {
    let mut ended = 0u64;
    for session in list_sessions(state).await.unwrap_or_default() {
        if session.ended {
            continue;
        }
        if session.impersonator_id == user_id || session.target_user_id == user_id {
            match end_session(state, &session.session_id, reason).await {
                Ok(_) => ended += 1,
                Err(e) => tracing::warn!(
                    session_id = %session.session_id,
                    user_id = user_id,
                    error = %e,
                    "deactivation cascade: failed to end impersonation session"
                ),
            }
        }
    }
    ended
}

/// TTL-expiry sweep (background job `impersonation_expiry`).
///
/// Ends every active session past its `expires_at` (emitting
/// `ImpersonationEnded` with reason "expired") and marks stale pending
/// requests as expired so they can no longer be approved. The JWT itself is
/// rejected at validation time (zero leeway on `exp`); this sweep closes the
/// audit trail and frees the admin's active-session slot.
pub async fn expire_sessions(state: &AppState) -> anyhow::Result<u64> {
    let now = now_secs();
    let mut ended = 0u64;

    for session in list_sessions(state).await.map_err(|e| match e {
        ApiError::Internal(err) => err,
        other => anyhow::anyhow!("{other}"),
    })? {
        if !session.ended && now >= session.expires_at {
            if let Err(e) = end_session(state, &session.session_id, "expired").await {
                tracing::warn!(
                    session_id = %session.session_id,
                    error = %e,
                    "impersonation_expiry: failed to end expired session"
                );
            } else {
                ended += 1;
            }
        }
    }

    // Mark stale pending requests so approve rejects them with a clear
    // conflict instead of relying solely on the expiry check. The
    // read-modify-write runs under the same write lock as approve's
    // critical section, so the sweep cannot overwrite a request that a
    // concurrent approve just marked Approved (and vice versa). The
    // session-ending loop above acquires and releases the lock per
    // session, so taking it here cannot deadlock.
    let _guard = SESSION_WRITE_LOCK.lock().await;
    let requests = state
        .kv_store
        .kv_list(REQUEST_NAMESPACE)
        .await
        .unwrap_or_default();
    for (_, raw) in requests {
        if let Ok(mut req) = serde_json::from_str::<ImpersonationRequest>(&raw) {
            if req.status == ImpersonationRequestStatus::Pending && now >= req.expires_at {
                req.status = ImpersonationRequestStatus::Expired;
                let _ = state
                    .kv_store
                    .kv_set(
                        REQUEST_NAMESPACE,
                        &req.request_id,
                        serde_json::to_string(&req).unwrap_or_default(),
                    )
                    .await;
            }
        }
    }

    Ok(ended)
}

/// Request-extensions marker: the impersonated action for this HTTP
/// request has already been recorded, so later `AuthenticatedAgent`
/// extractions on the same request must not double-count it.
/// `abac_middleware` records evaluated routes after policy evaluation
/// (with the decision outcome) and exempt/unresolved routes before the
/// passthrough; the auth-resolution layer records per-handler surfaces
/// (git HTTP, MCP, conversations, explorer, ws-ticket) at extraction.
#[derive(Clone, Copy, Debug)]
pub struct ImpersonationActionRecorded;

/// Record one API call made under an impersonation session: increments the
/// session's `actions_taken` counter and writes an audit event attributed
/// to the impersonator.
///
/// `surface` names where the call was attributed:
/// - `"abac"` — an `/api/v1` route that went through policy evaluation
///   (`allowed` carries the decision outcome);
/// - `"abac-exempt"` / `"abac-unresolved"` — an `/api/v1` route that
///   bypassed policy evaluation (the call proceeds, `allowed = true`);
/// - `"per-handler"` — a surface that authenticates via the
///   `AuthenticatedAgent` extractor without the ABAC middleware (git
///   HTTP, MCP, conversations, explorer, ws-ticket). `allowed = true`
///   means the call authenticated; the handler's own outcome is not
///   visible at the auth-resolution layer;
/// - `"websocket"` — a WebSocket/TTY connection opened with the token.
pub async fn record_impersonated_action(
    state: &AppState,
    ctx: &ImpersonationContext,
    method: &str,
    path: &str,
    allowed: bool,
    surface: &'static str,
) {
    // Hold the session-write lock across the read-modify-write so
    // concurrent impersonated calls cannot lose counts (an unlocked RMW
    // lets two concurrent calls both persist the same count and
    // underreport the session in ImpersonationEnded — verified by
    // `concurrent_impersonated_actions_are_counted_atomically` with a
    // yielding kv adapter).
    let _guard = SESSION_WRITE_LOCK.lock().await;
    let mut wrote_event = false;
    if let Ok(Some(mut session)) = load_session(state, &ctx.session_id).await {
        if !session.ended {
            session.actions_taken += 1;
            if save_session(state, &session).await.is_ok() {
                emit_audit(
                    state,
                    AuditEventType::Custom("impersonation_action".to_string()),
                    Some(ctx.impersonator_id.clone()),
                    serde_json::json!({
                        "impersonator_id": ctx.impersonator_id,
                        "target_user_id": ctx.target_user_id,
                        "session_id": ctx.session_id,
                        "method": method,
                        "path": path,
                        "surface": surface,
                        "actions_taken": session.actions_taken,
                    }),
                    &ctx.session_id,
                    if allowed {
                        AuditOutcome::Success
                    } else {
                        AuditOutcome::Blocked
                    },
                )
                .await;
                wrote_event = true;
            }
        }
    }
    if !wrote_event {
        tracing::debug!(
            session_id = %ctx.session_id,
            "impersonation action on inactive session (already ended?)"
        );
    }
}

/// Fire-and-forget wrapper around [`record_impersonated_action`] so the
/// audit write never blocks the request path.
pub fn spawn_record_impersonated_action(
    state: &Arc<AppState>,
    ctx: &ImpersonationContext,
    method: &str,
    path: &str,
    allowed: bool,
    surface: &'static str,
) {
    let state = state.clone();
    let ctx = ctx.clone();
    let method = method.to_string();
    let path = path.to_string();
    tokio::spawn(async move {
        record_impersonated_action(&state, &ctx, &method, &path, allowed, surface).await;
    });
}

/// Resolve an authenticated principal from validated impersonation JWT
/// claims. Returns `None` when the session is missing, ended, or expired —
/// ending a session revokes its token before `exp`.
pub async fn resolve_impersonation_auth(
    claims: &ImpersonationJwtClaims,
    state: &Arc<AppState>,
) -> Option<AuthenticatedAgent> {
    let session = load_session(state, &claims.session_id).await.ok()??;
    let now = now_secs();
    if session.ended || now >= session.expires_at {
        return None;
    }

    // The target user must still exist and the token must agree with the
    // session record on both identities.
    let target = state
        .users
        .find_by_id(&Id::new(claims.sub.clone()))
        .await
        .ok()??;
    if session.target_user_id != claims.target_user_id
        || session.impersonator_id != claims.impersonator_id
    {
        return None;
    }

    // Deactivation cascade (user-management.md §API > User Management:
    // the deactivated account "fails authentication on every path").
    // This is the impersonation path: a deactivated target loses the
    // mandate outright, and a deactivated impersonator loses it too — a
    // rogue TenantAdmin's unexpired impersonation JWT must not keep full
    // API access as the target user until `exp`. The impersonator's
    // account record must also still exist: a deleted account is in no
    // position to hold a mandate. `deactivate_user` ends the affected
    // sessions inline, but this check is the enforcement that stands even
    // if the session record outlives the account state change.
    if target.is_deactivated() {
        return None;
    }
    let impersonator = state
        .users
        .find_by_id(&Id::new(session.impersonator_id.clone()))
        .await
        .ok()??;
    if impersonator.is_deactivated() {
        return None;
    }

    Some(AuthenticatedAgent {
        agent_id: target.display_name.clone(),
        user_id: Some(target.id.clone()),
        roles: target.roles.clone(),
        tenant_id: session.tenant_id.clone(),
        // Impersonation targets a user principal: user tokens carry no
        // §1 hierarchy claims (platform-model.md §1), so the resolved
        // principal is gated by ABAC/RBAC, not token scope.
        workspace_id: None,
        repo_id: None,
        scope: Vec::new(),
        jwt_claims: None,
        bearer_token: None,
        deprecated_token_auth: false,
        impersonation: Some(ImpersonationContext {
            impersonator_id: Id::new(session.impersonator_id.clone()),
            impersonator_name: session.impersonator_name.clone(),
            target_user_id: target.id.clone(),
            session_id: session.session_id.clone(),
        }),
    })
}

// ── audit ───────────────────────────────────────────────────────────────────

/// Record an impersonation audit event and broadcast it on the SSE stream.
async fn emit_audit(
    state: &AppState,
    event_type: AuditEventType,
    user_id: Option<Id>,
    detail: serde_json::Value,
    session_id: &str,
    outcome: AuditOutcome,
) {
    let event = AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        event_type,
        None,
        user_id,
        None,
        None,
        None,
        "impersonation".to_string(),
        Some(session_id.to_string()),
        outcome,
        detail,
        None,
        None,
        now_secs(),
    );
    if let Err(e) = state.audit.record(&event).await {
        tracing::warn!(error = %e, "failed to record impersonation audit event");
    }
    let _ = state
        .audit_broadcast_tx
        .send(serde_json::to_string(&event).unwrap_or_default());
}

// ── notifications ───────────────────────────────────────────────────────────

/// The target user's first workspace (Inbox notifications are
/// workspace-labelled). `None` when the user has no workspace membership —
/// in that case the in-app notification is skipped (logged) because there
/// is no honest workspace to attribute it to.
async fn workspace_for_user(state: &AppState, user_id: &Id) -> Option<Id> {
    state
        .workspace_memberships
        .list_by_user(user_id)
        .await
        .ok()?
        .into_iter()
        .next()
        .map(|m| m.workspace_id)
}

/// Deliver the approval code to the target user: Inbox notification
/// (persisted) + WebSocket push when online.
async fn deliver_approval_code(
    state: &AppState,
    target: &User,
    req: &ImpersonationRequest,
    code: &str,
) {
    let workspace_id = match workspace_for_user(state, &target.id).await {
        Some(ws) => ws,
        None => {
            tracing::warn!(
                target_user_id = %target.id,
                "impersonation request: target has no workspace membership; \
                 skipping in-app approval-code notification"
            );
            return;
        }
    };

    let notif = Notification::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        workspace_id,
        target.id.clone(),
        NotificationType::ImpersonationRequest,
        format!(
            "Admin {} requested to impersonate your account",
            req.impersonator_name
        ),
        req.tenant_id.clone(),
        now_secs() as i64,
    );
    let mut notif = notif;
    notif.body = Some(
        serde_json::json!({
            "request_id": req.request_id,
            "approval_code": code,
            "impersonator_id": req.impersonator_id,
            "expires_at": req.expires_at,
        })
        .to_string(),
    );
    notif.source.entity_type = "impersonation".to_string();
    notif.source.entity_id = req.request_id.clone();

    if let Err(e) = state.create_notification(&notif).await {
        tracing::warn!(error = %e, "failed to persist impersonation request notification");
    } else {
        // Channel fan-out per user-management.md §Delivery Channels.
        crate::notification_dispatcher::dispatch_to_channels(state, &notif).await;
    }
    push_notification_to_user(state, &target.id, &notif).await;
}

/// Notify the target user that impersonation has begun.
async fn notify_impersonation_alert(
    state: &AppState,
    req: &ImpersonationRequest,
    session: &ImpersonationSession,
) {
    let target_id = Id::new(req.target_user_id.clone());
    let workspace_id = match workspace_for_user(state, &target_id).await {
        Some(ws) => ws,
        None => {
            tracing::warn!(
                target_user_id = %target_id,
                "impersonation start: target has no workspace membership; \
                 skipping in-app alert notification"
            );
            return;
        }
    };

    let mut notif = Notification::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        workspace_id,
        target_id.clone(),
        NotificationType::ImpersonationAlert,
        format!(
            "Admin {} is impersonating your account",
            session.impersonator_name
        ),
        session.tenant_id.clone(),
        now_secs() as i64,
    );
    notif.body = Some(
        serde_json::json!({
            "session_id": session.session_id,
            "impersonator_id": session.impersonator_id,
            "started_at": session.started_at,
            "expires_at": session.expires_at,
        })
        .to_string(),
    );
    notif.source.entity_type = "impersonation".to_string();
    notif.source.entity_id = session.session_id.clone();

    if let Err(e) = state.create_notification(&notif).await {
        tracing::warn!(error = %e, "failed to persist impersonation alert notification");
    } else {
        // Channel fan-out per user-management.md §Delivery Channels.
        crate::notification_dispatcher::dispatch_to_channels(state, &notif).await;
    }
    push_notification_to_user(state, &target_id, &notif).await;
}

/// Push a notification to every live WebSocket connection authenticated as
/// `user_id` (identity-security.md §Impersonation: notify the impersonated
/// user when online). Connections are matched by the server-verified user
/// identity recorded at connection time, never by client-supplied ids.
pub async fn push_notification_to_user(state: &AppState, user_id: &Id, notif: &Notification) {
    let payload = match serde_json::to_string(&WsMessage::NotificationPush {
        notification: notif.clone(),
    }) {
        Ok(p) => p,
        Err(_) => return,
    };

    let conns = state.ws_connections.read().await;
    let users = state.ws_connection_users.read().await;
    let mut sent = 0usize;
    for (conn_id, tx) in conns.iter() {
        let matches = users
            .get(conn_id)
            .map(|uid| uid == user_id.as_str())
            .unwrap_or(false);
        if matches && tx.try_send(payload.clone()).is_ok() {
            sent += 1;
        }
    }
    if sent == 0 {
        tracing::debug!(
            user_id = %user_id,
            "notification WS push: user not online, Inbox notification only"
        );
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Guard shared by the impersonation handlers: the caller must be a real
/// admin user (not the shared system token, not an agent, not itself
/// impersonating).
pub fn require_admin_actor(auth: &AuthenticatedAgent) -> Result<Id, ApiError> {
    if auth.impersonation.is_some() {
        return Err(ApiError::Forbidden(
            "cannot use impersonation to request impersonation".to_string(),
        ));
    }
    if !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "only Admin role may manage impersonation".to_string(),
        ));
    }
    auth.user_id.clone().ok_or_else(|| {
        ApiError::Forbidden(
            "impersonation requires an admin user identity (the shared system \
             token cannot be impersonated or impersonate)"
                .to_string(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn approval_code_and_alert_fan_out_to_configured_channels() {
        // Emitter-path guard (task-112 review F1): both impersonation
        // notification-creation paths must go through the channel
        // dispatcher — a configured email channel queues outbox entries
        // for the approval code AND the impersonation-start alert.
        let state = crate::mem::test_state();

        // Target user with a workspace membership (routing scope).
        let target = User::new(Id::new("user-target"), "ext-target", "target", 0);
        state.users.create(&target).await.unwrap();
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-imp"),
            Id::new("tenant-1"),
            "Impersonation WS",
            "imp-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();
        let m = gyre_domain::WorkspaceMembership::new(
            Id::new("mem-imp"),
            target.id.clone(),
            ws.id.clone(),
            gyre_domain::WorkspaceRole::Developer,
            Id::new("inviter"),
            0,
        );
        state.workspace_memberships.create(&m).await.unwrap();
        // Email channel configured (Immediate, all bands).
        state
            .user_channel_prefs
            .upsert(
                &target.id,
                &gyre_domain::NotificationChannels {
                    in_app: true,
                    email: gyre_domain::EmailConfig {
                        enabled: true,
                        digest: gyre_domain::DigestFrequency::Immediate,
                        min_priority: gyre_domain::NotificationPriority::Low,
                    },
                    webhook: None,
                    slack: None,
                },
            )
            .await
            .unwrap();

        let req = ImpersonationRequest {
            request_id: "req-1".to_string(),
            impersonator_id: "admin-1".to_string(),
            impersonator_name: "Admin One".to_string(),
            target_user_id: "user-target".to_string(),
            approval_code_hash: "hash".to_string(),
            tenant_id: "tenant-1".to_string(),
            created_at: 0,
            expires_at: 900,
            status: ImpersonationRequestStatus::Pending,
        };
        deliver_approval_code(&state, &target, &req, "123456").await;

        let session = ImpersonationSession {
            session_id: "sess-1".to_string(),
            request_id: req.request_id.clone(),
            impersonator_id: req.impersonator_id.clone(),
            impersonator_name: req.impersonator_name.clone(),
            target_user_id: req.target_user_id.clone(),
            tenant_id: req.tenant_id.clone(),
            started_at: 0,
            expires_at: 900,
            actions_taken: 0,
            ended: false,
            ended_at: None,
            end_reason: None,
        };
        notify_impersonation_alert(&state, &req, &session).await;

        // In-app records exist for both events …
        let notifs = state
            .notifications
            .list_for_user(&target.id, None, None, None, None, None, &[], 10, 0)
            .await
            .unwrap();
        assert_eq!(notifs.len(), 2, "approval code + start alert");
        assert!(notifs
            .iter()
            .any(|n| n.notification_type == NotificationType::ImpersonationRequest));
        assert!(notifs
            .iter()
            .any(|n| n.notification_type == NotificationType::ImpersonationAlert));
        // … and the configured email channel fired for BOTH (the alert is
        // Urgent-band; the request High-band — both pass a Low threshold).
        let outbox = state
            .kv_store
            .kv_list(crate::notification_dispatcher::EMAIL_OUTBOX_NS)
            .await
            .unwrap();
        assert_eq!(
            outbox
                .iter()
                .filter(|(k, _)| k.starts_with("user-target:"))
                .count(),
            2,
            "both impersonation notifications must reach the configured email channel"
        );
    }
}
