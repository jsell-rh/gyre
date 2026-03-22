//! Port trait for ABAC policy storage (M22.6).

use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::policy::{Policy, PolicyDecision, PolicyScope};

#[async_trait]
pub trait PolicyRepository: Send + Sync {
    /// Persist a new policy.
    async fn create(&self, policy: &Policy) -> Result<()>;

    /// Retrieve a policy by its ID.
    async fn find_by_id(&self, id: &str) -> Result<Option<Policy>>;

    /// List all policies (optionally filtered by scope + scope_id).
    async fn list(&self) -> Result<Vec<Policy>>;

    /// List policies that apply to the given scope and scope_id.
    /// `scope_id = None` returns policies with no scope_id restriction.
    async fn list_by_scope(
        &self,
        scope: &PolicyScope,
        scope_id: Option<&str>,
    ) -> Result<Vec<Policy>>;

    /// Update an existing policy.
    async fn update(&self, policy: &Policy) -> Result<()>;

    /// Delete a policy. Returns an error if the policy is built-in.
    async fn delete(&self, id: &str) -> Result<()>;

    /// Append a policy decision to the audit log.
    async fn record_decision(&self, decision: &PolicyDecision) -> Result<()>;

    /// Query the decision audit log. Empty filters = return all (up to limit).
    async fn list_decisions(
        &self,
        subject_id: Option<&str>,
        resource_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PolicyDecision>>;
}
