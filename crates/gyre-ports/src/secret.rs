use anyhow::Result;
use async_trait::async_trait;
use gyre_common::{Id, Secret, SecretScope};

/// Repository for scoped secrets (platform-model.md §7 Secrets Delivery).
///
/// Secret plaintext values never appear in the [`Secret`] domain type: they
/// flow only as method arguments (`&[u8]`) and return values (`Vec<u8>`).
/// Implementations MUST encrypt values at rest (AES-256-GCM) and MUST never
/// log or JSON-serialize plaintext values.
///
/// Tenant isolation: every method filters by `tenant_id` in addition to any
/// scope filter, so a secret stored for one tenant is invisible to another.
#[async_trait]
pub trait SecretRepository: Send + Sync {
    /// Create a secret with the given plaintext `value`.
    ///
    /// Fails if a secret with the same `id`, or the same
    /// `(scope, scope_id, name)` within the tenant, already exists.
    async fn create(&self, secret: &Secret, value: &[u8]) -> Result<()>;

    /// Retrieve the decrypted plaintext value of a secret.
    ///
    /// Returns `None` if the secret does not exist in `tenant_id`.
    async fn get_value(&self, id: &Id, tenant_id: &str) -> Result<Option<Vec<u8>>>;

    /// List secret metadata (never values) attached to a scope.
    async fn list_by_scope(
        &self,
        scope: SecretScope,
        scope_id: &str,
        tenant_id: &str,
    ) -> Result<Vec<Secret>>;

    /// Delete a secret. No-op error if absent from `tenant_id`.
    async fn delete(&self, id: &Id, tenant_id: &str) -> Result<()>;

    /// Replace a secret's value with `new_value` and update `last_rotated_at`.
    async fn rotate(&self, id: &Id, new_value: &[u8], tenant_id: &str) -> Result<()>;

    /// Collect the secrets an agent receives, from all applicable scopes
    /// (tenant -> workspace -> repo -> task), as `(name, plaintext value)`
    /// pairs.
    ///
    /// Per platform-model.md §7: a repo-scoped agent gets tenant + workspace +
    /// repo secrets, but not secrets from other repos; `task_id` additionally
    /// contributes task-scoped secrets. When the same name exists at multiple
    /// scopes, the finer (nearer) scope wins, mirroring the budget cascade.
    /// Secrets whose `expires_at` has passed are excluded.
    async fn resolve_for_agent(
        &self,
        tenant_id: &str,
        workspace_id: &str,
        repo_id: &str,
        task_id: Option<&str>,
    ) -> Result<Vec<(String, Vec<u8>)>>;
}
