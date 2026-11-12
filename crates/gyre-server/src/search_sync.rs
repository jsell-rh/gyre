//! Search-index synchronization decorators (search.md §Index Updates).
//!
//! Every repository-backed searchable entity type — agent, task, merge
//! request, review, persona, activity event, audit event — MUST keep the
//! search index in sync on write. Chasing individual write callsites proved
//! fragile: agents are created and transitioned by REST handlers, the spawn
//! path, orchestrators, compose, the MCP tools, the stale-agent detector,
//! spec lifecycle automation, and the merge processor, and tasks/MRs/reviews
//! have the same fan-out. Any new path that forgets a manual
//! `search_indexer::index` call silently freezes the entity's search facets
//! until the next admin reindex.
//!
//! Instead of per-callsite discipline, the entity repositories on
//! [`AppState`](crate::AppState) are wrapped in the decorators below at
//! construction time. Every write through the port — from any handler, job,
//! or background task — re-indexes (or de-indexes) the affected document
//! synchronously, best-effort: a search-index failure is logged and left to
//! the next full reindex, never failing the entity write that triggered it.
//!
//! Reads pass straight through, so the full-reindex coordinator
//! (`search_indexer::reindex_all`) can list entities without side effects.
//! Git-backed documents (specs, commits, files) are indexed on the git push
//! path, not here — they have no repository write to hook.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{
    Agent, AgentStatus, AnalyticsEvent, AuditEvent, MergeRequest, MrStatus, Persona, PersonaScope,
    Review, ReviewComment, Task, TaskStatus,
};
use gyre_ports::search::SearchPort;
use gyre_ports::{
    AgentRepository, AnalyticsRepository, AuditRepository, MergeRequestRepository,
    PersonaRepository, ReviewRepository, TaskRepository,
};

use crate::search_indexer::{
    activity_event_document, agent_document, audit_event_document, mr_document, persona_document,
    review_document, task_document,
};

/// Index one document best-effort; never fail the write that triggered it.
async fn index_doc(search: &Arc<dyn SearchPort>, doc: gyre_ports::search::SearchDocument) {
    if let Err(e) = search.index(doc).await {
        tracing::warn!(error = %e, "search-sync: failed to index document");
    }
}

/// De-index one document best-effort (entity delete paths).
async fn deindex_doc(search: &Arc<dyn SearchPort>, entity_type: &str, entity_id: &str) {
    if let Err(e) = search.delete(entity_type, entity_id).await {
        tracing::warn!(entity_type, entity_id, error = %e, "search-sync: failed to de-index");
    }
}

/// Wrap the agent repository so every create/update/status change re-indexes
/// the agent document (search.md: "Agent registered/status changed -> index
/// updated"). Covers the REST spawn/register handlers, `spawn_agent_core`,
/// orchestrator/compose registration, MCP heartbeat/complete, the stale-agent
/// detector, and admin kill.
pub fn wrap_agents(
    inner: Arc<dyn AgentRepository>,
    search: Arc<dyn SearchPort>,
) -> Arc<dyn AgentRepository> {
    Arc::new(IndexingAgentRepository { inner, search })
}

/// Wrap the task repository so every create/update re-indexes the task
/// document (search.md: "Task created/updated -> index updated"). Covers the
/// REST handlers, the MCP `gyre_update_task` tool, spec-lifecycle task
/// cancellation, and task creation in the merge processor, dependency
/// staleness detector, git push automation, and spec registry.
pub fn wrap_tasks(
    inner: Arc<dyn TaskRepository>,
    search: Arc<dyn SearchPort>,
) -> Arc<dyn TaskRepository> {
    Arc::new(IndexingTaskRepository { inner, search })
}

/// Wrap the merge-request repository so every create/update re-indexes the
/// MR document (search.md: "MR created/status changed -> index updated").
/// Covers the REST handlers plus MR status transitions in the merge
/// processor, merge dependency stacking, spec rejection, and release
/// automation.
pub fn wrap_mrs(
    inner: Arc<dyn MergeRequestRepository>,
    search: Arc<dyn SearchPort>,
) -> Arc<dyn MergeRequestRepository> {
    Arc::new(IndexingMrRepository { inner, search })
}

/// Wrap the review repository so every submitted review is indexed
/// (search.md §Searchable Entities: Review — body, decision, reviewer).
/// Covers the REST review endpoint and gate-executor reviews.
pub fn wrap_reviews(
    inner: Arc<dyn ReviewRepository>,
    mrs: Arc<dyn MergeRequestRepository>,
    search: Arc<dyn SearchPort>,
) -> Arc<dyn ReviewRepository> {
    Arc::new(IndexingReviewRepository { inner, mrs, search })
}

/// Wrap the persona repository so every create/update re-indexes and every
/// delete de-indexes the persona document.
pub fn wrap_personas(
    inner: Arc<dyn PersonaRepository>,
    search: Arc<dyn SearchPort>,
) -> Arc<dyn PersonaRepository> {
    Arc::new(IndexingPersonaRepository { inner, search })
}

/// Wrap the analytics repository so every recorded activity event is indexed
/// (search.md §Searchable Entities: Activity Event). Covers every analytics
/// emit path, including the merge processor's.
pub fn wrap_analytics(
    inner: Arc<dyn AnalyticsRepository>,
    search: Arc<dyn SearchPort>,
) -> Arc<dyn AnalyticsRepository> {
    Arc::new(IndexingAnalyticsRepository { inner, search })
}

/// Wrap the audit repository so every recorded audit event is indexed
/// (search.md §Searchable Entities: Audit Event). Covers the audit REST
/// endpoint, the container auditor, and the procfs monitor.
pub fn wrap_audit(
    inner: Arc<dyn AuditRepository>,
    search: Arc<dyn SearchPort>,
) -> Arc<dyn AuditRepository> {
    Arc::new(IndexingAuditRepository { inner, search })
}

// ─── Agent ──────────────────────────────────────────────────────────────────

struct IndexingAgentRepository {
    inner: Arc<dyn AgentRepository>,
    search: Arc<dyn SearchPort>,
}

#[async_trait]
impl AgentRepository for IndexingAgentRepository {
    async fn create(&self, agent: &Agent) -> Result<()> {
        self.inner.create(agent).await?;
        index_doc(&self.search, agent_document(agent)).await;
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Agent>> {
        self.inner.find_by_id(id).await
    }

    async fn find_by_name(&self, name: &str) -> Result<Option<Agent>> {
        self.inner.find_by_name(name).await
    }

    async fn list(&self) -> Result<Vec<Agent>> {
        self.inner.list().await
    }

    async fn list_by_status(&self, status: &AgentStatus) -> Result<Vec<Agent>> {
        self.inner.list_by_status(status).await
    }

    async fn update(&self, agent: &Agent) -> Result<()> {
        self.inner.update(agent).await?;
        index_doc(&self.search, agent_document(agent)).await;
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.inner.delete(id).await?;
        deindex_doc(&self.search, "agent", &id.to_string()).await;
        Ok(())
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Agent>> {
        self.inner.list_by_workspace(workspace_id).await
    }

    async fn update_status(&self, agent_id: &Id, status: AgentStatus) -> Result<()> {
        self.inner.update_status(agent_id, status).await?;
        // The optimized path carries only the status; re-fetch the agent to
        // rebuild the full document so the `status` facet reflects the change.
        match self.inner.find_by_id(agent_id).await {
            Ok(Some(agent)) => index_doc(&self.search, agent_document(&agent)).await,
            Ok(None) => {
                tracing::warn!(agent_id = %agent_id, "search-sync: agent vanished after update_status")
            }
            Err(e) => {
                tracing::warn!(agent_id = %agent_id, error = %e, "search-sync: failed to re-fetch agent after update_status")
            }
        }
        Ok(())
    }

    async fn record_usage(&self, usage: &gyre_domain::AgentUsage) -> Result<()> {
        // Usage fields are not part of the agent search document.
        self.inner.record_usage(usage).await
    }
}

// ─── Task ───────────────────────────────────────────────────────────────────

struct IndexingTaskRepository {
    inner: Arc<dyn TaskRepository>,
    search: Arc<dyn SearchPort>,
}

#[async_trait]
impl TaskRepository for IndexingTaskRepository {
    async fn create(&self, task: &Task) -> Result<()> {
        self.inner.create(task).await?;
        index_doc(&self.search, task_document(task)).await;
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Task>> {
        self.inner.find_by_id(id).await
    }

    async fn list(&self) -> Result<Vec<Task>> {
        self.inner.list().await
    }

    async fn list_by_status(&self, status: &TaskStatus) -> Result<Vec<Task>> {
        self.inner.list_by_status(status).await
    }

    async fn list_by_assignee(&self, agent_id: &Id) -> Result<Vec<Task>> {
        self.inner.list_by_assignee(agent_id).await
    }

    async fn list_by_parent(&self, parent_task_id: &Id) -> Result<Vec<Task>> {
        self.inner.list_by_parent(parent_task_id).await
    }

    async fn update(&self, task: &Task) -> Result<()> {
        self.inner.update(task).await?;
        index_doc(&self.search, task_document(task)).await;
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.inner.delete(id).await?;
        deindex_doc(&self.search, "task", &id.to_string()).await;
        Ok(())
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Task>> {
        self.inner.list_by_workspace(workspace_id).await
    }

    async fn list_by_spec_path(&self, spec_path: &str) -> Result<Vec<Task>> {
        self.inner.list_by_spec_path(spec_path).await
    }

    async fn list_by_repo(&self, repo_id: &Id) -> Result<Vec<Task>> {
        self.inner.list_by_repo(repo_id).await
    }
}

// ─── Merge Request ──────────────────────────────────────────────────────────

struct IndexingMrRepository {
    inner: Arc<dyn MergeRequestRepository>,
    search: Arc<dyn SearchPort>,
}

#[async_trait]
impl MergeRequestRepository for IndexingMrRepository {
    async fn create(&self, mr: &MergeRequest) -> Result<()> {
        self.inner.create(mr).await?;
        index_doc(&self.search, mr_document(mr)).await;
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<MergeRequest>> {
        self.inner.find_by_id(id).await
    }

    async fn list(&self) -> Result<Vec<MergeRequest>> {
        self.inner.list().await
    }

    async fn list_by_status(&self, status: &MrStatus) -> Result<Vec<MergeRequest>> {
        self.inner.list_by_status(status).await
    }

    async fn list_by_repo(&self, repository_id: &Id) -> Result<Vec<MergeRequest>> {
        self.inner.list_by_repo(repository_id).await
    }

    async fn update(&self, mr: &MergeRequest) -> Result<()> {
        self.inner.update(mr).await?;
        index_doc(&self.search, mr_document(mr)).await;
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.inner.delete(id).await?;
        deindex_doc(&self.search, "mr", &id.to_string()).await;
        Ok(())
    }

    async fn list_dependents(&self, mr_id: &Id) -> Result<Vec<Id>> {
        self.inner.list_dependents(mr_id).await
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<MergeRequest>> {
        self.inner.list_by_workspace(workspace_id).await
    }
}

// ─── Review ─────────────────────────────────────────────────────────────────

struct IndexingReviewRepository {
    inner: Arc<dyn ReviewRepository>,
    mrs: Arc<dyn MergeRequestRepository>,
    search: Arc<dyn SearchPort>,
}

#[async_trait]
impl ReviewRepository for IndexingReviewRepository {
    async fn add_comment(&self, comment: &ReviewComment) -> Result<()> {
        // Comments are not a searchable entity (Review is).
        self.inner.add_comment(comment).await
    }

    async fn list_comments(&self, mr_id: &Id) -> Result<Vec<ReviewComment>> {
        self.inner.list_comments(mr_id).await
    }

    async fn submit_review(&self, review: &Review) -> Result<()> {
        self.inner.submit_review(review).await?;
        // The review document joins the MR for title/workspace/repo facets.
        match self.mrs.find_by_id(&review.merge_request_id).await {
            Ok(Some(mr)) => index_doc(&self.search, review_document(review, &mr)).await,
            Ok(None) => {
                tracing::warn!(mr_id = %review.merge_request_id, "search-sync: MR missing for submitted review")
            }
            Err(e) => {
                tracing::warn!(mr_id = %review.merge_request_id, error = %e, "search-sync: failed to fetch MR for review")
            }
        }
        Ok(())
    }

    async fn list_reviews(&self, mr_id: &Id) -> Result<Vec<Review>> {
        self.inner.list_reviews(mr_id).await
    }

    async fn is_approved(&self, mr_id: &Id) -> Result<bool> {
        self.inner.is_approved(mr_id).await
    }
}

// ─── Persona ────────────────────────────────────────────────────────────────

struct IndexingPersonaRepository {
    inner: Arc<dyn PersonaRepository>,
    search: Arc<dyn SearchPort>,
}

#[async_trait]
impl PersonaRepository for IndexingPersonaRepository {
    async fn create(&self, persona: &Persona) -> Result<()> {
        self.inner.create(persona).await?;
        index_doc(&self.search, persona_document(persona)).await;
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Persona>> {
        self.inner.find_by_id(id).await
    }

    async fn find_by_slug_and_scope(
        &self,
        slug: &str,
        scope: &PersonaScope,
    ) -> Result<Option<Persona>> {
        self.inner.find_by_slug_and_scope(slug, scope).await
    }

    async fn list(&self) -> Result<Vec<Persona>> {
        self.inner.list().await
    }

    async fn list_by_scope(&self, scope: &PersonaScope) -> Result<Vec<Persona>> {
        self.inner.list_by_scope(scope).await
    }

    async fn update(&self, persona: &Persona) -> Result<()> {
        self.inner.update(persona).await?;
        index_doc(&self.search, persona_document(persona)).await;
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.inner.delete(id).await?;
        deindex_doc(&self.search, "persona", &id.to_string()).await;
        Ok(())
    }
}

// ─── Activity Event (analytics) ──────────────────────────────────────────────

struct IndexingAnalyticsRepository {
    inner: Arc<dyn AnalyticsRepository>,
    search: Arc<dyn SearchPort>,
}

#[async_trait]
impl AnalyticsRepository for IndexingAnalyticsRepository {
    async fn record(&self, event: &AnalyticsEvent) -> Result<()> {
        self.inner.record(event).await?;
        index_doc(&self.search, activity_event_document(event)).await;
        Ok(())
    }

    async fn query(
        &self,
        event_name: Option<&str>,
        since: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AnalyticsEvent>> {
        self.inner.query(event_name, since, limit).await
    }

    async fn count(&self, event_name: &str, since: u64, until: u64) -> Result<u64> {
        self.inner.count(event_name, since, until).await
    }

    async fn aggregate_by_day(
        &self,
        event_name: &str,
        since: u64,
        until: u64,
    ) -> Result<Vec<(String, u64)>> {
        self.inner.aggregate_by_day(event_name, since, until).await
    }

    async fn delete_older_than(&self, cutoff_secs: u64) -> Result<u64> {
        self.inner.delete_older_than(cutoff_secs).await
    }
}

// ─── Audit Event ─────────────────────────────────────────────────────────────

struct IndexingAuditRepository {
    inner: Arc<dyn AuditRepository>,
    search: Arc<dyn SearchPort>,
}

#[async_trait]
impl AuditRepository for IndexingAuditRepository {
    async fn record(&self, event: &AuditEvent) -> Result<()> {
        self.inner.record(event).await?;
        index_doc(&self.search, audit_event_document(event)).await;
        Ok(())
    }

    async fn query(&self, filter: &gyre_ports::audit::AuditQueryFilter) -> Result<Vec<AuditEvent>> {
        self.inner.query(filter).await
    }

    async fn count(&self) -> Result<u64> {
        self.inner.count().await
    }

    async fn stats_by_type(&self) -> Result<Vec<(String, u64)>> {
        self.inner.stats_by_type().await
    }

    async fn since_timestamp(&self, since: u64, limit: usize) -> Result<Vec<AuditEvent>> {
        self.inner.since_timestamp(since, limit).await
    }

    async fn delete_older_than(&self, cutoff_secs: u64) -> Result<u64> {
        self.inner.delete_older_than(cutoff_secs).await
    }

    async fn compress_details_before(
        &self,
        cutoff_secs: u64,
        limit: usize,
    ) -> Result<gyre_ports::audit::CompressionStats> {
        // Compression is storage-level maintenance, not an entity write —
        // delegate to the inner repository (no search indexing needed;
        // reads decompress transparently).
        self.inner.compress_details_before(cutoff_secs, limit).await
    }
}
