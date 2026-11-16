//! Policy-set cache for the ABAC evaluation engine (§Performance).
//!
//! Policy evaluation happens on EVERY request; the spec requires cached
//! policy sets with invalidation on change and a <1ms evaluation target.
//! [`CachedPolicyRepository`] is a read-through caching decorator around the
//! real [`PolicyRepository`]: `list()` / `list_by_scope()` are served from an
//! in-memory set until any mutating call (`create`, `update`, `delete`,
//! `delete_by_name_prefix`, `delete_by_name_prefix_and_scope_id`) succeeds,
//! which invalidates the cached sets.
//!
//! ## Out-of-band writers (correctness contract)
//!
//! One production path writes policies WITHOUT going through the
//! `PolicyRepository` port: `WorkspaceRepository::apply_trust_transition`
//! (it must write the workspace row and its `trust:` policies in ONE
//! transaction, so it writes the policies table directly). Its two call
//! sites (`api/workspaces.rs` — workspace create and trust-level update)
//! invalidate the shared [`PolicyCache`] handle after a successful
//! transition. Any NEW writer that persists policies outside the
//! `PolicyRepository` port MUST do the same — a stale cache would silently
//! hide a Supervised workspace's `trust:require-human-mr-review` Deny from
//! the merge-time evaluation.
//!
//! ## Scope granularity
//!
//! The spec asks for per-scope caching with invalidation "for that scope".
//! Sets are cached per scope key (full set + `(scope, scope_id)` pairs), but
//! any mutation clears EVERY key: a tenant-scoped policy (scope_id `None`)
//! applies to every scope query, so precise per-scope invalidation would
//! have to clear nearly the whole map anyway — the conservative clear is
//! simpler and cannot serve stale entries. The number of distinct scope
//! keys is bounded by the number of workspaces/repos queried, so the map
//! stays small.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use gyre_domain::policy::{Policy, PolicyDecision, PolicyScope};
use gyre_ports::PolicyRepository;

/// Key identifying a cached policy set.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum CacheKey {
    /// The full `list()` set.
    All,
    /// A `list_by_scope(scope, scope_id)` set.
    Scope(ScopeDiscriminant, Option<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ScopeDiscriminant {
    Tenant,
    Workspace,
    Repo,
}

impl From<&PolicyScope> for ScopeDiscriminant {
    fn from(s: &PolicyScope) -> Self {
        match s {
            PolicyScope::Tenant => ScopeDiscriminant::Tenant,
            PolicyScope::Workspace => ScopeDiscriminant::Workspace,
            PolicyScope::Repo => ScopeDiscriminant::Repo,
        }
    }
}

#[derive(Default)]
struct CacheState {
    sets: HashMap<CacheKey, Arc<Vec<Policy>>>,
}

/// Shared cache handle: read-through storage for policy sets plus the
/// invalidation entry point for out-of-band policy writers
/// (`apply_trust_transition`).
#[derive(Default)]
pub struct PolicyCache {
    state: RwLock<CacheState>,
}

impl PolicyCache {
    /// Drop every cached policy set. Called on ANY policy mutation (through
    /// the [`CachedPolicyRepository`] wrapper) and explicitly by handlers
    /// that persist policies outside the `PolicyRepository` port.
    pub fn invalidate(&self) {
        self.state
            .write()
            .expect("policy cache lock poisoned")
            .sets
            .clear();
    }

    fn get(&self, key: &CacheKey) -> Option<Arc<Vec<Policy>>> {
        self.state
            .read()
            .expect("policy cache lock poisoned")
            .sets
            .get(key)
            .cloned()
    }

    fn put(&self, key: CacheKey, set: Arc<Vec<Policy>>) {
        // Bound: distinct scope keys are bounded by the workspace/repo
        // count; no eviction is needed in practice.
        self.state
            .write()
            .expect("policy cache lock poisoned")
            .sets
            .insert(key, set);
    }
}

/// Read-through caching decorator over a real [`PolicyRepository`].
///
/// Reads (`list`, `list_by_scope`) are served from the shared
/// [`PolicyCache`]; the first miss loads through `inner` and populates the
/// cache. Mutations delegate to `inner` and invalidate the cache ONLY on
/// success — a failed write changes nothing, so the cached set stays valid.
/// `find_by_id`, `record_decision`, and `list_decisions` pass through:
/// point lookups and audit appends are cheap and must observe the store
/// directly (seeding's drift detection, the decision log).
pub struct CachedPolicyRepository {
    inner: Arc<dyn PolicyRepository>,
    cache: Arc<PolicyCache>,
}

impl CachedPolicyRepository {
    pub fn new(inner: Arc<dyn PolicyRepository>, cache: Arc<PolicyCache>) -> Self {
        Self { inner, cache }
    }

    /// Read-through `list()`: cached copy when present, else load + populate.
    async fn list_cached(&self) -> anyhow::Result<Arc<Vec<Policy>>> {
        if let Some(cached) = self.cache.get(&CacheKey::All) {
            return Ok(cached);
        }
        let policies = Arc::new(self.inner.list().await?);
        self.cache.put(CacheKey::All, policies.clone());
        Ok(policies)
    }
}

#[async_trait]
impl PolicyRepository for CachedPolicyRepository {
    async fn create(&self, policy: &Policy) -> anyhow::Result<()> {
        self.inner.create(policy).await?;
        self.cache.invalidate();
        Ok(())
    }

    async fn find_by_id(&self, id: &str) -> anyhow::Result<Option<Policy>> {
        self.inner.find_by_id(id).await
    }

    async fn list(&self) -> anyhow::Result<Vec<Policy>> {
        Ok((*self.list_cached().await?).clone())
    }

    async fn list_by_scope(
        &self,
        scope: &PolicyScope,
        scope_id: Option<&str>,
    ) -> anyhow::Result<Vec<Policy>> {
        let key = CacheKey::Scope(scope.into(), scope_id.map(str::to_string));
        if let Some(cached) = self.cache.get(&key) {
            return Ok((*cached).clone());
        }
        let policies = self.inner.list_by_scope(scope, scope_id).await?;
        let set = Arc::new(policies);
        self.cache.put(key, set.clone());
        Ok((*set).clone())
    }

    async fn update(&self, policy: &Policy) -> anyhow::Result<()> {
        self.inner.update(policy).await?;
        self.cache.invalidate();
        Ok(())
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        self.inner.delete(id).await?;
        self.cache.invalidate();
        Ok(())
    }

    async fn delete_by_name_prefix(&self, prefix: &str) -> anyhow::Result<u64> {
        let n = self.inner.delete_by_name_prefix(prefix).await?;
        self.cache.invalidate();
        Ok(n)
    }

    async fn delete_by_name_prefix_and_scope_id(
        &self,
        prefix: &str,
        scope_id: &str,
    ) -> anyhow::Result<u64> {
        let n = self
            .inner
            .delete_by_name_prefix_and_scope_id(prefix, scope_id)
            .await?;
        self.cache.invalidate();
        Ok(n)
    }

    async fn record_decision(&self, decision: &PolicyDecision) -> anyhow::Result<()> {
        self.inner.record_decision(decision).await
    }

    async fn list_decisions(
        &self,
        subject_id: Option<&str>,
        resource_type: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<PolicyDecision>> {
        self.inner
            .list_decisions(subject_id, resource_type, limit)
            .await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::Id;

    /// Policy repository that counts reads, so the tests can prove the
    /// cache actually serves reads and that mutations invalidate it.
    struct CountingRepo {
        policies: tokio::sync::Mutex<Vec<Policy>>,
        lists: std::sync::atomic::AtomicUsize,
        list_by_scopes: std::sync::atomic::AtomicUsize,
    }

    impl CountingRepo {
        fn with(policies: Vec<Policy>) -> Arc<Self> {
            Arc::new(Self {
                policies: tokio::sync::Mutex::new(policies),
                lists: std::sync::atomic::AtomicUsize::new(0),
                list_by_scopes: std::sync::atomic::AtomicUsize::new(0),
            })
        }

        fn lists(&self) -> usize {
            self.lists.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn list_by_scopes(&self) -> usize {
            self.list_by_scopes
                .load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    fn sample_policy(id: &str, priority: u32) -> Policy {
        Policy {
            id: Id::new(id),
            name: format!("policy-{id}"),
            description: String::new(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority,
            effect: gyre_domain::policy::PolicyEffect::Allow,
            conditions: vec![],
            actions: vec!["*".to_string()],
            resource_types: vec!["*".to_string()],
            enabled: true,
            built_in: false,
            immutable: false,
            created_by: Id::new("system"),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[async_trait]
    impl PolicyRepository for CountingRepo {
        async fn create(&self, policy: &Policy) -> anyhow::Result<()> {
            self.policies.lock().await.push(policy.clone());
            Ok(())
        }

        async fn find_by_id(&self, id: &str) -> anyhow::Result<Option<Policy>> {
            Ok(self
                .policies
                .lock()
                .await
                .iter()
                .find(|p| p.id.as_str() == id)
                .cloned())
        }

        async fn list(&self) -> anyhow::Result<Vec<Policy>> {
            self.lists.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(self.policies.lock().await.clone())
        }

        async fn list_by_scope(
            &self,
            scope: &PolicyScope,
            scope_id: Option<&str>,
        ) -> anyhow::Result<Vec<Policy>> {
            self.list_by_scopes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(self
                .policies
                .lock()
                .await
                .iter()
                .filter(|p| {
                    &p.scope == scope && p.scope_id.as_ref().map(|i| i.as_str()) == scope_id
                })
                .cloned()
                .collect())
        }

        async fn update(&self, _policy: &Policy) -> anyhow::Result<()> {
            Ok(())
        }

        async fn delete(&self, _id: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn delete_by_name_prefix(&self, _prefix: &str) -> anyhow::Result<u64> {
            Ok(0)
        }

        async fn delete_by_name_prefix_and_scope_id(
            &self,
            _prefix: &str,
            _scope_id: &str,
        ) -> anyhow::Result<u64> {
            Ok(0)
        }

        async fn record_decision(&self, _decision: &PolicyDecision) -> anyhow::Result<()> {
            Ok(())
        }

        async fn list_decisions(
            &self,
            _subject_id: Option<&str>,
            _resource_type: Option<&str>,
            _limit: usize,
        ) -> anyhow::Result<Vec<PolicyDecision>> {
            Ok(vec![])
        }
    }

    fn wrapped(
        policies: Vec<Policy>,
    ) -> (Arc<CountingRepo>, Arc<PolicyCache>, CachedPolicyRepository) {
        let inner = CountingRepo::with(policies);
        let cache = Arc::new(PolicyCache::default());
        let wrapped = CachedPolicyRepository::new(inner.clone(), cache.clone());
        (inner, cache, wrapped)
    }

    #[tokio::test]
    async fn list_is_served_from_cache_after_first_load() {
        let (inner, _cache, repo) = wrapped(vec![sample_policy("p-1", 10)]);
        let first = repo.list().await.unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(inner.lists(), 1, "first list loads through the store");

        let second = repo.list().await.unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(
            inner.lists(),
            1,
            "subsequent lists must be served from the cache, not the store"
        );
    }

    #[tokio::test]
    async fn list_by_scope_is_cached_per_scope_key() {
        let mut ws_policy = sample_policy("p-ws", 10);
        ws_policy.scope = PolicyScope::Workspace;
        ws_policy.scope_id = Some(Id::new("ws-1"));
        let (inner, _cache, repo) = wrapped(vec![sample_policy("p-1", 10), ws_policy]);

        let scoped = repo
            .list_by_scope(&PolicyScope::Workspace, Some("ws-1"))
            .await
            .unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].id.as_str(), "p-ws");
        assert_eq!(inner.list_by_scopes(), 1);

        // Same key: cached.
        let _ = repo
            .list_by_scope(&PolicyScope::Workspace, Some("ws-1"))
            .await
            .unwrap();
        assert_eq!(inner.list_by_scopes(), 1, "same scope key must be cached");

        // Different key: loads through the store (and does NOT disturb the
        // first key's cache entry).
        let _ = repo
            .list_by_scope(&PolicyScope::Tenant, None)
            .await
            .unwrap();
        assert_eq!(
            inner.list_by_scopes(),
            2,
            "a different scope key must load through the store"
        );
    }

    #[tokio::test]
    async fn mutation_invalidates_the_cached_sets() {
        let (inner, _cache, repo) = wrapped(vec![sample_policy("p-1", 10)]);
        let _ = repo.list().await.unwrap();
        let _ = repo
            .list_by_scope(&PolicyScope::Tenant, None)
            .await
            .unwrap();
        assert_eq!(inner.lists(), 1);
        assert_eq!(inner.list_by_scopes(), 1);

        repo.create(&sample_policy("p-2", 20)).await.unwrap();
        let after = repo.list().await.unwrap();
        assert_eq!(after.len(), 2, "the new policy must be visible");
        assert_eq!(
            inner.lists(),
            2,
            "create must invalidate the cached list set"
        );

        // The scope set was invalidated too (conservative full clear).
        let _ = repo
            .list_by_scope(&PolicyScope::Tenant, None)
            .await
            .unwrap();
        assert_eq!(
            inner.list_by_scopes(),
            2,
            "create must invalidate the cached scope sets"
        );
    }

    #[tokio::test]
    async fn explicit_invalidation_covers_out_of_band_writers() {
        // apply_trust_transition writes the policies table WITHOUT the
        // PolicyRepository port; its handler invalidates the shared handle.
        let (inner, cache, repo) = wrapped(vec![sample_policy("p-1", 10)]);
        let _ = repo.list().await.unwrap();
        assert_eq!(inner.lists(), 1);

        cache.invalidate();
        let _ = repo.list().await.unwrap();
        assert_eq!(
            inner.lists(),
            2,
            "PolicyCache::invalidate must force the next list through the store"
        );
    }

    #[tokio::test]
    async fn find_by_id_passes_through_uncached() {
        let (inner, _cache, repo) = wrapped(vec![sample_policy("p-1", 10)]);
        let found = repo.find_by_id("p-1").await.unwrap();
        assert!(found.is_some());
        // Point lookups read the store directly (seeding drift detection
        // relies on observing the stored row).
        assert_eq!(inner.lists(), 0);
    }
}
