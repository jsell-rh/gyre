//! Port traits for the DB-backed meta-spec registry (agent-runtime spec §2).

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::meta_spec::{
    MetaSpec, MetaSpecBinding, MetaSpecKind, MetaSpecScope, MetaSpecVersion,
};

// ---------------------------------------------------------------------------
// Filter
// ---------------------------------------------------------------------------

/// Filter for listing meta-specs.
#[derive(Clone, Debug, Default)]
pub struct MetaSpecFilter {
    pub scope: Option<MetaSpecScope>,
    pub scope_id: Option<String>,
    pub kind: Option<MetaSpecKind>,
    pub required: Option<bool>,
}

// ---------------------------------------------------------------------------
// MetaSpecRepository
// ---------------------------------------------------------------------------

#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait MetaSpecRepository: Send + Sync {
    /// Create a new meta-spec. Returns the created entity.
    async fn create(&self, meta_spec: &MetaSpec) -> Result<()>;

    /// Retrieve a meta-spec by ID.
    async fn get_by_id(&self, id: &Id) -> Result<Option<MetaSpec>>;

    /// List meta-specs, optionally filtered.
    async fn list(&self, filter: &MetaSpecFilter) -> Result<Vec<MetaSpec>>;

    /// Update an existing meta-spec. Archives a version-history entry when
    /// (and only when) the content version changed — a same-version update
    /// (e.g. approving unchanged content) must not create duplicate history
    /// rows for the same version.
    async fn update(&self, meta_spec: &MetaSpec) -> Result<()>;

    /// Delete a meta-spec by ID. Returns an error if bindings reference it.
    async fn delete(&self, id: &Id) -> Result<()>;

    /// List all historical versions for a meta-spec.
    async fn list_versions(&self, meta_spec_id: &Id) -> Result<Vec<MetaSpecVersion>>;

    /// Get a specific version of a meta-spec.
    async fn get_version(&self, meta_spec_id: &Id, version: u32)
        -> Result<Option<MetaSpecVersion>>;
}

// ---------------------------------------------------------------------------
// MetaSpecBindingRepository
// ---------------------------------------------------------------------------

#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait MetaSpecBindingRepository: Send + Sync {
    /// Create a binding linking a spec to a meta-spec at a pinned version.
    async fn create(&self, binding: &MetaSpecBinding) -> Result<()>;

    /// List all bindings for a given spec ID.
    async fn list_by_spec_id(&self, spec_id: &str) -> Result<Vec<MetaSpecBinding>>;

    /// List every binding (stale-pin detection scans the full table).
    async fn list_all(&self) -> Result<Vec<MetaSpecBinding>>;

    /// Delete a binding by ID.
    async fn delete(&self, id: &Id) -> Result<()>;

    /// Check if any bindings reference the given meta_spec_id.
    async fn has_bindings_for(&self, meta_spec_id: &Id) -> Result<bool>;
}
