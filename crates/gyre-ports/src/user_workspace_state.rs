use anyhow::Result;
use async_trait::async_trait;

/// Repository for per-user, per-workspace last-seen tracking.
///
/// This table is internal-only (no REST endpoint). It is accessed by:
/// - The `last_seen_middleware` which upserts on every authenticated workspace-scoped request.
/// - The briefing handler which reads `last_seen_at` as the default `since` timestamp.
#[async_trait]
pub trait UserWorkspaceStateRepository: Send + Sync {
    /// Upsert the last-seen timestamp (epoch seconds) for a user-workspace pair.
    async fn upsert_last_seen(
        &self,
        user_id: &str,
        workspace_id: &str,
        timestamp: i64,
    ) -> Result<()>;

    /// Return the last-seen timestamp (epoch seconds) for a user-workspace pair,
    /// or `None` if the user has never been seen in this workspace.
    async fn get_last_seen(&self, user_id: &str, workspace_id: &str) -> Result<Option<i64>>;
}
