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
    ///
    /// `exclude_types` names canonical `NotificationType` strings whose notifications MUST NOT
    /// be returned — this carries the HSI §12 disabled notification preferences. The exclusion
    /// is applied by the query, before `limit`/`offset`, so a page never comes back short
    /// because rows were dropped after fetching. Pass `&[]` on surfaces that must see every
    /// notification regardless of the recipient's toggles (background de-duplication jobs).
    async fn list_for_user(
        &self,
        user_id: &Id,
        workspace_id: Option<&Id>,
        min_priority: Option<u8>,
        max_priority: Option<u8>,
        notification_type: Option<&str>,
        exclude_types: &[&str],
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Notification>>;

    /// Set `dismissed_at` to now. Used by trust suggestions (30-day suppression).
    async fn dismiss(&self, id: &Id, user_id: &Id) -> Result<()>;

    /// Set `resolved_at` to now. `action_taken` is an optional audit label.
    async fn resolve(&self, id: &Id, user_id: &Id, action_taken: Option<&str>) -> Result<()>;

    /// Count active (not resolved, not dismissed) notifications.
    /// When `workspace_id` is None, counts across all workspaces (badge count).
    /// `exclude_types` follows the semantics of
    /// [`NotificationRepository::list_for_user`]: the badge MUST NOT count a notification the
    /// inbox itself would hide.
    async fn count_unresolved(
        &self,
        user_id: &Id,
        workspace_id: Option<&Id>,
        exclude_types: &[&str],
    ) -> Result<u64>;

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

    /// Hard-delete notifications past their retention cutoffs, split by read state:
    /// read (resolved or dismissed) notifications older than `read_cutoff_secs` are
    /// deleted; unread ones only when older than `unread_cutoff_secs`.
    /// Returns the number of rows purged. Used by the nightly retention job
    /// (business-continuity.md §5 — 90 days read / 365 days unread by default).
    async fn delete_older_than(
        &self,
        read_cutoff_secs: u64,
        unread_cutoff_secs: u64,
    ) -> Result<u64>;
}
