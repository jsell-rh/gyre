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
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
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

    /// Look up a secret's metadata (never its value) by id.
    ///
    /// Returns `None` if the secret does not exist in `tenant_id`. Used by
    /// the Admin API to resolve `DELETE /secrets/{id}` and
    /// `POST /secrets/{id}/rotate` targets before acting on them.
    async fn find_by_id(&self, id: &Id, tenant_id: &str) -> Result<Option<Secret>>;

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

    /// Agent-scoped resolution carrying the agent's own JWT
    /// (platform-model.md §7 Storage Backend: with the Vault backend,
    /// resolution authenticates to Vault with the agent's JWT via the
    /// JWT auth backend, so Vault policies scope the returned secrets —
    /// the server never applies its own credentials to an agent read).
    ///
    /// The default implementation ignores the JWT and delegates to
    /// [`SecretRepository::resolve_for_agent`] (database-backed backends
    /// apply the scope cascade themselves); the Vault backend overrides
    /// it to perform the agent-authenticated read.
    async fn resolve_for_agent_as(
        &self,
        tenant_id: &str,
        workspace_id: &str,
        repo_id: &str,
        task_id: Option<&str>,
        _agent_jwt: &str,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        self.resolve_for_agent(tenant_id, workspace_id, repo_id, task_id)
            .await
    }
}
