//! Port trait for spec-link graph persistence (spec-links.md §Forge-Maintained
//! Spec Graph).
//!
//! The forge's tenant-wide directed graph of spec links is a persistent SQL
//! table. All link mutations (manifest sync, staleness transitions, repo
//! deletion) write through this port so the table stays authoritative across
//! restarts; the in-memory `SpecLinksStore` is a hot cache rebuilt from
//! `list_all` at boot.

use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::spec_links::SpecLinkEntry;

#[async_trait]
pub trait SpecLinkRepository: Send + Sync {
    /// All persisted links, tenant-wide.
    async fn list_all(&self) -> Result<Vec<SpecLinkEntry>>;

    /// Atomically replace the link set originating from one source spec:
    /// delete existing rows for `(source_repo_id, source_path)` and insert
    /// `links`. Mirrors `sync_spec_ledger`, which recomputes a source spec's
    /// links on each push.
    ///
    /// `source_repo_id` may be `None`-origin (legacy same-repo links are
    /// stored with an empty repo id); callers pass the same scoping they used
    /// when building the entries.
    async fn replace_for_source(
        &self,
        source_repo_id: &str,
        source_path: &str,
        links: &[SpecLinkEntry],
    ) -> Result<()>;

    /// Upsert a single link by `id` (used by staleness/patrol status mutations).
    async fn save(&self, entry: &SpecLinkEntry) -> Result<()>;

    /// Delete all links originating from a repo (repo removal cleanup).
    async fn delete_by_source_repo(&self, source_repo_id: &str) -> Result<()>;
}
