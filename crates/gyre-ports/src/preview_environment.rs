use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::PreviewEnvironment;

/// Repository for ephemeral preview environments (activity-dashboard.md
/// §"Try it" links — durable-state invariant: environments are rows, never
/// in-memory maps, so they survive a server restart and remain queryable).
///
/// Per-handler auth — callers MUST verify that the environment's
/// `tenant_id`/`workspace_id` match the authenticated caller's scope before
/// invoking any method.
///
/// Port contract (enforced by EVERY adapter):
/// - `create` fails if an environment with the same `id` already exists.
/// - `update` fails if no environment with the given `id` exists.
#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future per method.
pub trait PreviewEnvironmentRepository: Send + Sync {
    /// Insert a new environment row. Fails if an environment with the same
    /// `id` already exists — a duplicate provisioning request must create a
    /// NEW environment (new id), never silently overwrite an existing one.
    async fn create(&self, env: &PreviewEnvironment) -> Result<()>;

    /// Fetch one environment by id.
    async fn find_by_id(&self, id: &Id) -> Result<Option<PreviewEnvironment>>;

    /// All environments provisioned for a merge request, newest first —
    /// the MR-scoped status/list surface.
    async fn find_by_mr(&self, mr_id: &Id) -> Result<Vec<PreviewEnvironment>>;

    /// All environments in a workspace, newest first.
    async fn find_by_workspace(&self, workspace_id: &Id) -> Result<Vec<PreviewEnvironment>>;

    /// Environments whose TTL deadline (`expires_at`) has passed but which
    /// are still in a live state (`provisioning`/`active`) — the expiry
    /// job's input. Ordered by `expires_at` ascending.
    async fn find_live_expired_before(&self, now: u64) -> Result<Vec<PreviewEnvironment>>;

    /// Persist the full row (status transitions, mesh peer binding, end
    /// bookkeeping). Fails if the environment does not exist.
    async fn update(&self, env: &PreviewEnvironment) -> Result<()>;

    /// Hard-delete the row. Used only by retention-style cleanup; teardown
    /// keeps the row (status `expired`) as the audit record of what ran.
    async fn delete(&self, id: &Id) -> Result<()>;
}
