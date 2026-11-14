use anyhow::Result;
use async_trait::async_trait;
use gyre_common::{Id, Notification};

/// Repository for inbox notifications (HSI §2).
///
/// Per-handler auth — callers MUST verify that the notification's `user_id` and
/// `tenant_id` match the authenticated user before invoking mutating methods.
#[async_trait]
// async_trait generates a must_use future per method; the pre-existing HSI §2
// inbox filter surface (`list_for_user`'s 7 filter/pagination parameters) is
// allowed at the method site below.
#[allow(clippy::double_must_use)]
pub trait NotificationRepository: Send + Sync {
    async fn create(&self, notification: &Notification) -> Result<()>;

    /// Fetch a single notification scoped to the owning user (returns None if not found
    /// or if user_id does not match, preventing cross-user UUID guessing).
    async fn get(&self, id: &Id, user_id: &Id) -> Result<Option<Notification>>;

    /// List notifications for a user. Optionally filtered by workspace, repo,
    /// numeric priority range, and a notification-type set. When
    /// `workspace_id` is None, returns notifications across all workspaces
    /// (tenant Inbox).
    ///
    /// `notification_types` is an exact-match set filter on the persisted
    /// `notification_type` column: a notification is returned when its
    /// stored type string is one of the given values. An EMPTY set matches
    /// nothing (it is a filter, not a no-op). Serves both the `?type=`
    /// filter and the four-level `?priority=` filter, which expands to the
    /// set of type strings whose band is that level
    /// (`NotificationPriority::notification_type_strings`) — the band
    /// derives from the notification type per the amended §Notification
    /// Entity membership table, never from the numeric rank, so a level
    /// filter cannot be expressed as a rank-range query. Callers filtering
    /// by a level must include the legacy DB names of renamed variants
    /// (they are part of the expansion) so pre-unification rows keep
    /// filtering by their band.
    #[allow(clippy::too_many_arguments)] // filter + pagination parameters; pre-existing signature
    async fn list_for_user(
        &self,
        user_id: &Id,
        workspace_id: Option<&Id>,
        min_priority: Option<u8>,
        max_priority: Option<u8>,
        notification_types: Option<&[&str]>,
        repo_id: Option<&Id>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Notification>>;

    /// Replace the body of unresolved notifications of the given type whose
    /// `entity_ref` matches. Returns the number of rows updated. Used by
    /// save_spec's append-to-existing-MR path to keep the SpecPendingApproval
    /// card's `spec_sha` in sync with iterative saves — a stale SHA would make
    /// every later Approve record an approval for content that never becomes
    /// the ledger's current_sha. Resolved/dismissed cards are left untouched
    /// (their historical body is an audit record of what was acted on).
    async fn update_body_by_entity_ref(
        &self,
        entity_ref: &str,
        notification_type: &str,
        body: &str,
    ) -> Result<u64>;

    /// Set `dismissed_at` to now. Used by trust suggestions (30-day suppression).
    async fn dismiss(&self, id: &Id, user_id: &Id) -> Result<()>;

    /// Set `resolved_at` to now. `action_taken` is an optional audit label.
    async fn resolve(&self, id: &Id, user_id: &Id, action_taken: Option<&str>) -> Result<()>;

    /// Set or clear the in-app read state (user-management.md §Notification
    /// Entity): `read = true` stamps `read_at` with now, `read = false`
    /// clears both. Scoped to the owning user like `get`/`dismiss`/`resolve`.
    async fn mark_read(&self, id: &Id, user_id: &Id, read: bool) -> Result<()>;

    /// Count active (not resolved, not dismissed) notifications.
    /// When `workspace_id` is None, counts across all workspaces (badge count).
    async fn count_unresolved(&self, user_id: &Id, workspace_id: Option<&Id>) -> Result<u64>;

    /// Count active (not resolved, not dismissed) notifications that the user
    /// has not read (user-management.md §API > Notifications: unread count for
    /// the bell badge). When `workspace_id` is None, counts across all
    /// workspaces.
    async fn count_unread(&self, user_id: &Id, workspace_id: Option<&Id>) -> Result<u64>;

    /// Mark every unread notification owned by `user_id` as read, stamping
    /// `read_at` (user-management.md §API > Notifications: mark-all-read).
    /// Dismissed cards are included so the unread count deterministically
    /// reaches zero. When `workspace_id` is Some, only that workspace's
    /// notifications are marked. Returns the number of rows updated.
    async fn mark_all_read(&self, user_id: &Id, workspace_id: Option<&Id>) -> Result<u64>;

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

    /// Returns true if an active (unresolved, undismissed) notification of the
    /// given type exists for the user in the workspace, referencing `entity_ref`
    /// as its source entity id and created at or after `since` (epoch seconds).
    /// Used to de-duplicate event-driven notification creation (budget
    /// threshold warnings, all-gates-passed announcements) so repeated
    /// triggers within a period do not spam the inbox.
    async fn has_active_notification(
        &self,
        workspace_id: &Id,
        user_id: &Id,
        notification_type: &str,
        entity_ref: &str,
        since: i64,
    ) -> Result<bool>;

    /// Hard-delete notifications past their retention cutoffs, split by read state:
    /// read (resolved, dismissed, or marked read) notifications older than
    /// `read_cutoff_secs` are deleted; unread ones only when older than
    /// `unread_cutoff_secs`.
    /// Returns the number of rows purged. Used by the nightly retention job
    /// (business-continuity.md §5 — 90 days read / 365 days unread by default).
    async fn delete_older_than(
        &self,
        read_cutoff_secs: u64,
        unread_cutoff_secs: u64,
    ) -> Result<u64>;
}
