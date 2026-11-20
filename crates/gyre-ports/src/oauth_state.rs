use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A durable OAuth 2.0 authorization-request state row (admin-panel.md /
/// platform-model.md §7 Claude Max enrollment: admin-initiated account
/// enrollment via OAuth + token refresh).
///
/// Created by the Admin-only initiation route with a random `state` and a
/// PKCE `verifier`; validated and consumed by the callback handler. The
/// initiating admin and tenant are derived SERVER-SIDE from the
/// authenticated caller — never client-supplied — so a completed callback
/// stores the resulting refresh token in the initiator's scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthStateRecord {
    /// Random 256-bit state value (hex) — the OAuth `state` parameter and
    /// the row key. Single-use.
    pub state: String,
    /// Tenant of the initiating admin (server-derived).
    pub tenant_id: String,
    /// Workspace the enrollment targets (server-validated against the
    /// caller's scope); `None` scopes the resulting secret at tenant level.
    pub workspace_id: Option<String>,
    /// Initiating admin user id (server-derived).
    pub initiated_by: String,
    /// PKCE code_verifier for the token exchange (random, kept server-side).
    pub pkce_verifier: String,
    /// Unix epoch seconds of creation.
    pub created_at: u64,
    /// Unix epoch seconds after which the row is expired and only eligible
    /// for cleanup.
    pub expires_at: u64,
    /// Set when the callback consumed this state (single use: a replayed
    /// state is rejected).
    pub consumed: bool,
    /// JSON provider config: authorization/token endpoints, client_id,
    /// redirect_uri, scopes — captured at initiation so the callback uses
    /// the same provider config the admin was redirected to.
    pub provider_config: String,
}

/// Durable OAuth state storage.
///
/// The `oauth_state_cleanup` background job deletes expired rows; the
/// callback rejects expired or already-consumed states.
#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait OAuthStateRepository: Send + Sync {
    /// Create a state row.
    ///
    /// Fails if a row with the same `state` already exists.
    async fn create(&self, record: &OAuthStateRecord) -> Result<()>;

    /// Look up a state row by its `state` value.
    async fn find_by_state(&self, state: &str) -> Result<Option<OAuthStateRecord>>;

    /// Atomically mark a state row consumed (single use).
    ///
    /// Returns `Ok(true)` when THIS call flipped the row from unconsumed
    /// to consumed; `Ok(false)` when the row does not exist or was
    /// already consumed. The flip MUST be a single conditional update
    /// (`UPDATE … SET consumed = 1 WHERE state = ? AND consumed = 0`, or
    /// the equivalent guarded in-code mutation) so that exactly one of
    /// any number of concurrent callers wins — a read-then-write sequence
    /// lets two concurrent callbacks both pass a pre-checked
    /// `record.consumed` flag.
    async fn mark_consumed(&self, state: &str) -> Result<bool>;

    /// Delete rows whose `expires_at` is before `cutoff_secs`. Returns the
    /// number of rows deleted. Used by the `oauth_state_cleanup` job.
    async fn delete_expired_before(&self, cutoff_secs: u64) -> Result<u64>;
}
