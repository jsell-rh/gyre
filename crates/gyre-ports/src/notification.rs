use anyhow::Result;
use async_trait::async_trait;
use gyre_common::{Id, Notification};

/// Repository for inbox notifications (HSI §2).
///
/// Per-handler auth — callers MUST verify that the notification's `user_id` and
/// `tenant_id` match the authenticated user before invoking mutating methods.
#[async_trait]
pub trait NotificationRepository: Send + Sync {
    async fn create(&self, notification: &Notification) -> Result<()>;

    /// Fetch a single notification scoped to the owning user (returns None if not found
    /// or if user_id does not match, preventing cross-user UUID guessing).
    async fn get(&self, id: &Id, user_id: &Id) -> Result<Option<Notification>>;

    /// List notifications for a user. Optionally filtered by workspace and priority range.
    /// When `workspace_id` is None, returns notifications across all workspaces (tenant Inbox).
    async fn list_for_user(
        &self,
        user_id: &Id,
        workspace_id: Option<&Id>,
        min_priority: Option<u8>,
        max_priority: Option<u8>,
        notification_type: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Notification>>;

    /// Set `dismissed_at` to now. Used by trust suggestions (30-day suppression).
    async fn dismiss(&self, id: &Id, user_id: &Id) -> Result<()>;

    /// Set `resolved_at` to now. `action_taken` is an optional audit label.
    async fn resolve(&self, id: &Id, user_id: &Id, action_taken: Option<&str>) -> Result<()>;

    /// Count active (not resolved, not dismissed) notifications.
    /// When `workspace_id` is None, counts across all workspaces (badge count).
    async fn count_unresolved(&self, user_id: &Id, workspace_id: Option<&Id>) -> Result<u64>;

    /// List most recent notifications across all users (for activity feed).
    /// Ordered by created_at descending.
    async fn list_recent(&self, limit: usize) -> Result<Vec<Notification>>;

    /// Returns true if there is a recent dismissal for the given (workspace, user, type)
    /// within `days` days. Used by the trust-suggestion job to suppress re-creation.
    async fn has_recent_dismissal(
        &self,
        workspace_id: &Id,
        user_id: &Id,
        notification_type: &str,
        days: u32,
    ) -> Result<bool>;
}
