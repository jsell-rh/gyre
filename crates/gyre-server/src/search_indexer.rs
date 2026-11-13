//! Server-level search index coordinator — task-202, search.md §Searchable Entities.
//!
//! The FTS mechanics live in the adapters (SQLite FTS5 / Postgres tsvector /
//! in-memory fallback) behind `SearchPort`. This module is the server-side half
//! that the adapters cannot implement because it needs to read many entity
//! repositories and the git object store:
//!
//! - Document builders for every searchable entity type (spec, task, mr,
//!   commit, agent, persona, activity_event, audit_event, review, file) with
//!   the specced indexed fields and facets.
//! - Write-path indexing helpers called from the API handlers and the git
//!   push path (spec content is extracted from the pushed git blob, commit
//!   messages from `git show`).
//! - File/code-search indexing (`index_repo_files`), registered as a
//!   `JobRegistry` background job and triggered on push — search.md §Index
//!   Updates: code search is eventually consistent, not inline.
//! - `reindex_all`: the real full-reindex coordinator behind
//!   `POST /api/v1/admin/search/reindex` — clears the index, then rebuilds it
//!   from every entity repository and returns the true document count.

use std::collections::HashMap;

use anyhow::Result;
use gyre_domain::{
    Agent, AgentStatus, AnalyticsEvent, AuditEvent, MergeRequest, MrStatus, Persona, PersonaScope,
    Repository, Review, Task, TaskPriority, TaskStatus,
};
use gyre_ports::search::SearchDocument;
use tokio::process::Command;

use crate::AppState;

/// Files larger than this are skipped by code-search indexing (search.md caps
/// indexed blobs; a 1 MiB source file is already far beyond useful search
/// granularity and would bloat the FTS index).
pub const FILE_INDEX_MAX_BYTES: u64 = 1024 * 1024;

/// How many bytes to sniff for NUL bytes when deciding a blob is binary.
const BINARY_SNIFF_BYTES: usize = 8000;

/// Upper bound on events pulled per type during a full reindex.
const REINDEX_EVENT_LIMIT: usize = 100_000;

// ─── Facet value helpers ─────────────────────────────────────────────────────

fn task_status_str(s: &TaskStatus) -> &'static str {
    match s {
        TaskStatus::Backlog => "backlog",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Review => "review",
        TaskStatus::Done => "done",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn task_priority_str(p: &TaskPriority) -> &'static str {
    match p {
        TaskPriority::Low => "low",
        TaskPriority::Medium => "medium",
        TaskPriority::High => "high",
        TaskPriority::Critical => "critical",
    }
}

fn mr_status_str(s: &MrStatus) -> &'static str {
    match s {
        MrStatus::Open => "open",
        MrStatus::Approved => "approved",
        MrStatus::Merged => "merged",
        MrStatus::Closed => "closed",
        MrStatus::Reverted => "reverted",
    }
}

fn agent_status_str(s: &AgentStatus) -> &'static str {
    match s {
        AgentStatus::Active => "active",
        AgentStatus::Idle => "idle",
        AgentStatus::Failed => "failed",
        AgentStatus::Stopped => "stopped",
        AgentStatus::Dead => "dead",
    }
}

fn persona_scope_str(s: &PersonaScope) -> &'static str {
    match s {
        // Tenant-wide personas are the platform's "global" scope
        // (search.md §Searchable Entities: scope global/workspace/repo).
        PersonaScope::Tenant(_) => "global",
        PersonaScope::Workspace(_) => "workspace",
        PersonaScope::Repo(_) => "repo",
    }
}

fn review_decision_str(d: &gyre_domain::ReviewDecision) -> &'static str {
    match d {
        gyre_domain::ReviewDecision::Approved => "approved",
        gyre_domain::ReviewDecision::ChangesRequested => "changes_requested",
    }
}

/// Format a Unix timestamp (seconds) as an ISO `YYYY-MM-DD` date for the
/// `date` facet (date-range filtering per search.md §Searchable Entities).
fn iso_date(secs: u64) -> String {
    // Howard Hinnant's civil_from_days: days since epoch -> (y, m, d).
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 - doe / 36_524 + doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Map a file path to a language facet value for code search.
fn language_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "rb" => "ruby",
        "php" => "php",
        "sh" | "bash" => "shell",
        "sql" => "sql",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        "md" | "markdown" => "markdown",
        "html" => "html",
        "css" | "scss" => "css",
        "svelte" => "svelte",
        _ => "other",
    }
}

fn facets(pairs: Vec<(&str, String)>) -> HashMap<String, String> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

// ─── Document builders (search.md §Searchable Entities) ──────────────────────

/// Task: indexed fields title/description/labels, facets workspace, repo,
/// status, priority, assigned_to.
pub fn task_document(task: &Task) -> SearchDocument {
    let mut body = task.description.clone().unwrap_or_default();
    if !task.labels.is_empty() {
        body.push_str("\nlabels: ");
        body.push_str(&task.labels.join(", "));
    }
    SearchDocument {
        entity_type: "task".to_string(),
        entity_id: task.id.to_string(),
        title: task.title.clone(),
        body,
        workspace_id: Some(task.workspace_id.to_string()),
        repo_id: Some(task.repo_id.to_string()),
        tenant_id: None,
        facets: facets(vec![
            ("status", task_status_str(&task.status).to_string()),
            ("priority", task_priority_str(&task.priority).to_string()),
            (
                "assigned_to",
                task.assigned_to
                    .as_ref()
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
            ),
            ("workspace", task.workspace_id.to_string()),
            ("repo", task.repo_id.to_string()),
        ]),
        // Entity's own update time — the since:/until: facets filter on it.
        updated_at: Some(task.updated_at as i64),
    }
}

/// Merge Request: indexed fields title/source_branch/target_branch/author,
/// facets workspace, repo, status, has_conflicts.
pub fn mr_document(mr: &MergeRequest) -> SearchDocument {
    let author = mr
        .author_agent_id
        .as_ref()
        .map(|id| id.to_string())
        .unwrap_or_default();
    SearchDocument {
        entity_type: "mr".to_string(),
        entity_id: mr.id.to_string(),
        title: mr.title.clone(),
        body: format!(
            "{} -> {} author {}",
            mr.source_branch, mr.target_branch, author
        ),
        workspace_id: Some(mr.workspace_id.to_string()),
        repo_id: Some(mr.repository_id.to_string()),
        tenant_id: None,
        facets: facets(vec![
            ("status", mr_status_str(&mr.status).to_string()),
            (
                "has_conflicts",
                mr.has_conflicts.map(|b| b.to_string()).unwrap_or_default(),
            ),
            ("author", author),
            ("workspace", mr.workspace_id.to_string()),
            ("repo", mr.repository_id.to_string()),
        ]),
        // Entity's own update time — the since:/until: facets filter on it.
        updated_at: Some(mr.updated_at as i64),
    }
}

/// Agent: indexed fields name/persona/status, facets workspace, repo,
/// status. The specced `persona` indexed field is the agent's attached
/// persona id (Agent.persona_id) — it is folded into the body so a
/// full-text query for the persona returns the agent (search.md's table
/// distinguishes Indexed Fields (searchable) from Facets (filterable);
/// both body and facet carry it).
pub fn agent_document(agent: &Agent) -> SearchDocument {
    let persona = agent
        .persona_id
        .as_ref()
        .map(|id| id.to_string())
        .unwrap_or_default();
    let mut body = format!(
        "agent {} status {}",
        agent.name,
        agent_status_str(&agent.status)
    );
    if !persona.is_empty() {
        body.push_str("\npersona ");
        body.push_str(&persona);
    }
    SearchDocument {
        entity_type: "agent".to_string(),
        entity_id: agent.id.to_string(),
        title: agent.name.clone(),
        body,
        workspace_id: Some(agent.workspace_id.to_string()),
        repo_id: agent.repo_id.as_ref().map(|id| id.to_string()),
        tenant_id: None,
        facets: facets(vec![
            ("status", agent_status_str(&agent.status).to_string()),
            ("persona", persona),
            ("workspace", agent.workspace_id.to_string()),
            (
                "repo",
                agent
                    .repo_id
                    .as_ref()
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
            ),
        ]),
        // Entity's own time — last heartbeat, falling back to spawn time —
        // the since:/until: facets filter on it.
        updated_at: Some(agent.last_heartbeat.unwrap_or(agent.spawned_at) as i64),
    }
}

/// Persona: indexed fields name/prompt content, facet scope.
pub fn persona_document(persona: &Persona) -> SearchDocument {
    SearchDocument {
        entity_type: "persona".to_string(),
        entity_id: persona.id.to_string(),
        title: persona.name.clone(),
        body: persona.system_prompt.clone(),
        workspace_id: match &persona.scope {
            PersonaScope::Tenant(t) => Some(t.to_string()),
            PersonaScope::Workspace(w) => Some(w.to_string()),
            PersonaScope::Repo(_) => None,
        },
        repo_id: match &persona.scope {
            PersonaScope::Repo(r) => Some(r.to_string()),
            _ => None,
        },
        tenant_id: None,
        facets: facets(vec![
            ("scope", persona_scope_str(&persona.scope).to_string()),
            ("slug", persona.slug.clone()),
        ]),
        // Entity's own time — the since:/until: facets filter on it.
        updated_at: Some(persona.created_at as i64),
    }
}

/// Review: indexed fields body/decision/reviewer, facets workspace, repo, MR.
/// The decision and reviewer are Indexed Fields (searchable), so they are
/// folded into the body, not just carried as filter facets.
pub fn review_document(review: &Review, mr: &MergeRequest) -> SearchDocument {
    let decision = review_decision_str(&review.decision);
    let reviewer = review.reviewer_agent_id.clone();
    let mut body = review.body.clone().unwrap_or_default();
    body.push_str("\ndecision ");
    body.push_str(decision);
    if !reviewer.is_empty() {
        body.push_str("\nreviewer ");
        body.push_str(&reviewer);
    }
    SearchDocument {
        entity_type: "review".to_string(),
        entity_id: review.id.to_string(),
        title: format!("Review of {}", mr.title),
        body,
        workspace_id: Some(mr.workspace_id.to_string()),
        repo_id: Some(mr.repository_id.to_string()),
        tenant_id: None,
        facets: facets(vec![
            ("decision", decision.to_string()),
            ("reviewer", reviewer),
            ("mr", mr.id.to_string()),
            ("workspace", mr.workspace_id.to_string()),
            ("repo", mr.repository_id.to_string()),
        ]),
        // Entity's own time — the since:/until: facets filter on it.
        updated_at: Some(review.created_at as i64),
    }
}

/// Activity Event: indexed fields description/event_type/agent_id, facets
/// workspace, repo, event_type, date.
pub fn activity_event_document(event: &AnalyticsEvent) -> SearchDocument {
    let props = serde_json::to_string(&event.properties).unwrap_or_default();
    // Prefer a human-readable description from properties when present.
    let description = event
        .properties
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| props.clone());
    let body = if description.is_empty() {
        event.event_name.clone()
    } else {
        format!("{}\n{}", event.event_name, description)
    };
    // agent_id is an Indexed Field for activity events (searchable), not
    // just a filter facet — fold it into the body.
    let agent_id = event.agent_id.clone().unwrap_or_default();
    let body = if agent_id.is_empty() {
        body
    } else {
        format!("{body}\nagent {agent_id}")
    };
    let workspace = event
        .properties
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let repo = event
        .properties
        .get("repo_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    SearchDocument {
        entity_type: "activity_event".to_string(),
        entity_id: event.id.to_string(),
        title: event.event_name.clone(),
        body,
        workspace_id: if workspace.is_empty() {
            None
        } else {
            Some(workspace.clone())
        },
        repo_id: if repo.is_empty() {
            None
        } else {
            Some(repo.clone())
        },
        tenant_id: None,
        facets: facets(vec![
            ("event_type", event.event_name.clone()),
            ("agent_id", agent_id),
            ("workspace", workspace),
            ("repo", repo),
            ("date", iso_date(event.timestamp)),
        ]),
        // Event's own time — the since:/until: facets filter on it.
        updated_at: Some(event.timestamp as i64),
    }
}

/// Audit Event: indexed fields event_type/details/path, facets workspace, repo,
/// agent_id, event_type, date.
pub fn audit_event_document(event: &AuditEvent) -> SearchDocument {
    let detail = serde_json::to_string(&event.detail).unwrap_or_default();
    let path = event
        .detail
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let body = if path.is_empty() {
        detail
    } else {
        format!("{path}\n{detail}")
    };
    // agent_id is searchable text for audit events too (the review finding:
    // facet-only values are neither searchable nor client-filterable via
    // GET /api/v1/search, which has no facet filter params).
    let agent_id = event
        .agent_id
        .as_ref()
        .map(|id| id.to_string())
        .unwrap_or_default();
    let body = if agent_id.is_empty() {
        body
    } else {
        format!("{body}\nagent {agent_id}")
    };
    SearchDocument {
        entity_type: "audit_event".to_string(),
        entity_id: event.id.to_string(),
        title: event.event_type.as_str(),
        body,
        workspace_id: event.workspace_id.as_ref().map(|id| id.to_string()),
        repo_id: event.repo_id.as_ref().map(|id| id.to_string()),
        tenant_id: None,
        facets: facets(vec![
            ("event_type", event.event_type.as_str()),
            ("agent_id", agent_id),
            ("path", path),
            (
                "workspace",
                event
                    .workspace_id
                    .as_ref()
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
            ),
            (
                "repo",
                event
                    .repo_id
                    .as_ref()
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
            ),
            ("date", iso_date(event.timestamp)),
        ]),
        // Event's own time — the since:/until: facets filter on it.
        updated_at: Some(event.timestamp as i64),
    }
}

/// Commit: indexed fields message/SHA/author/branch, facets workspace, repo,
/// agent_id, date.
#[allow(clippy::too_many_arguments)]
pub fn commit_document(
    sha: &str,
    author: &str,
    timestamp: u64,
    message: &str,
    branch: &str,
    agent_id: &str,
    workspace_id: &str,
    repo_id: &str,
) -> SearchDocument {
    let title = message.lines().next().unwrap_or_default().to_string();
    // Indexed Fields are searchable text, not just filter facets (MR-author
    // pattern): the author and branch are folded into the body so a
    // full-text query for either matches the commit.
    let mut body = message.to_string();
    if !author.is_empty() {
        body.push_str("\nauthor ");
        body.push_str(author);
    }
    if !branch.is_empty() {
        body.push_str("\nbranch ");
        body.push_str(branch);
    }
    SearchDocument {
        entity_type: "commit".to_string(),
        entity_id: sha.to_string(),
        title: if title.is_empty() {
            sha.to_string()
        } else {
            title
        },
        body,
        workspace_id: if workspace_id.is_empty() {
            None
        } else {
            Some(workspace_id.to_string())
        },
        repo_id: if repo_id.is_empty() {
            None
        } else {
            Some(repo_id.to_string())
        },
        tenant_id: None,
        facets: facets(vec![
            ("author", author.to_string()),
            ("branch", branch.to_string()),
            ("agent_id", agent_id.to_string()),
            ("workspace", workspace_id.to_string()),
            ("repo", repo_id.to_string()),
            ("date", iso_date(timestamp)),
        ]),
        // Commit's own author time — the since:/until: facets filter on it.
        updated_at: Some(timestamp as i64),
    }
}

/// Spec: indexed fields title/path/full content/owner/links, facets
/// workspace, repo, approval_status, scope.
#[allow(clippy::too_many_arguments)] // one parameter per specced indexed field
pub fn spec_document(
    path: &str,
    title: &str,
    owner: &str,
    approval_status: &str,
    content: &str,
    linked_tasks: &[String],
    linked_mrs: &[String],
    workspace_id: Option<&str>,
    repo_id: Option<&str>,
    // Ledger entry's update time — the since:/until: facets filter on it.
    // `None` (no ledger entry) lets the adapter stamp index time.
    updated_at: Option<u64>,
) -> SearchDocument {
    // The spec's scope facet is the section it lives under (the first path
    // segment, e.g. "system" for "system/search.md").
    let scope = path.split('/').next().unwrap_or_default().to_string();
    // Links (the ledger's linked task/MR ids) are indexed in the body, the
    // same pattern as task labels.
    let mut body = if content.is_empty() {
        title.to_string()
    } else {
        content.to_string()
    };
    if !linked_tasks.is_empty() {
        body.push_str("\nlinked tasks: ");
        body.push_str(&linked_tasks.join(", "));
    }
    if !linked_mrs.is_empty() {
        body.push_str("\nlinked MRs: ");
        body.push_str(&linked_mrs.join(", "));
    }
    // The owner is an Indexed Field (searchable), not just a filter facet —
    // fold it into the body so `?q=<owner>` matches the spec.
    if !owner.is_empty() {
        body.push_str("\nowner ");
        body.push_str(owner);
    }
    SearchDocument {
        entity_type: "spec".to_string(),
        entity_id: path.to_string(),
        title: if title.is_empty() {
            path.to_string()
        } else {
            title.to_string()
        },
        body,
        workspace_id: workspace_id.map(str::to_string),
        repo_id: repo_id.map(str::to_string),
        tenant_id: None,
        facets: facets(vec![
            ("approval_status", approval_status.to_string()),
            ("owner", owner.to_string()),
            ("scope", scope),
            ("workspace", workspace_id.unwrap_or_default().to_string()),
            ("repo", repo_id.unwrap_or_default().to_string()),
        ]),
        updated_at: updated_at.map(|t| t as i64),
    }
}

/// File (code search): indexed fields path/content, facets workspace, repo,
/// language.
pub fn file_document(
    path: &str,
    content: &str,
    language: &str,
    workspace_id: &str,
    repo_id: &str,
) -> SearchDocument {
    SearchDocument {
        entity_type: "file".to_string(),
        entity_id: path.to_string(),
        title: path.to_string(),
        body: content.to_string(),
        workspace_id: if workspace_id.is_empty() {
            None
        } else {
            Some(workspace_id.to_string())
        },
        repo_id: if repo_id.is_empty() {
            None
        } else {
            Some(repo_id.to_string())
        },
        tenant_id: None,
        facets: facets(vec![
            ("language", language.to_string()),
            ("workspace", workspace_id.to_string()),
            ("repo", repo_id.to_string()),
        ]),
        // No single entity time for a git blob — the adapter stamps index
        // time (the since:/until: facets filter on it).
        updated_at: None,
    }
}

// ─── Index / de-index helpers (best-effort, never fail the write path) ───────

/// Index one document. Best-effort: a search-index failure must not fail the
/// entity write that triggered it — it is logged and surfaced by the next
/// full reindex.
pub async fn index(state: &AppState, doc: SearchDocument) {
    if let Err(e) = state.search.index(doc).await {
        tracing::warn!(error = %e, "search-index: failed to index document");
    }
}

/// Remove one document from the index (entity delete paths).
pub async fn deindex(state: &AppState, entity_type: &str, entity_id: &str) {
    if let Err(e) = state.search.delete(entity_type, entity_id).await {
        tracing::warn!(entity_type, entity_id, error = %e, "search-index: failed to de-index");
    }
}

// ─── Git object-store helpers ────────────────────────────────────────────────

fn git_bin() -> String {
    std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string())
}

/// Run `git -C repo_path <args>` and return stdout on success.
async fn git_bytes(repo_path: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new(git_bin())
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .output()
        .await
        .ok()?;
    if out.status.success() {
        Some(out.stdout)
    } else {
        None
    }
}

/// Read a blob by SHA. Returns None for missing objects or spawn failures.
async fn read_blob(repo_path: &str, blob_sha: &str) -> Option<Vec<u8>> {
    if blob_sha.is_empty() {
        return None;
    }
    git_bytes(repo_path, &["cat-file", "blob", blob_sha]).await
}

/// Commit metadata for a SHA: (author, timestamp, full message).
async fn commit_metadata(repo_path: &str, sha: &str) -> Option<(String, u64, String)> {
    // %an author name, %at author timestamp, %B raw body — \x1f-separated.
    let fmt = "--format=%an%x1f%at%x1f%B";
    let out = git_bytes(repo_path, &["show", "-s", fmt, sha]).await?;
    let text = String::from_utf8(out).ok()?;
    let mut parts = text.splitn(3, '\x1f');
    let author = parts.next()?.trim().to_string();
    let ts: u64 = parts.next()?.trim().parse().ok()?;
    let message = parts.next().unwrap_or_default().trim_end().to_string();
    Some((author, ts, message))
}

/// True when the content looks binary (NUL byte in the sniff window).
fn looks_binary(content: &[u8]) -> bool {
    let window = &content[..content.len().min(BINARY_SNIFF_BYTES)];
    window.contains(&0)
}

// ─── Write-path indexing (search.md §Index Updates) ──────────────────────────

/// Index every spec registered in the manifest at `head_sha` — content is
/// extracted from the pushed git blob. Called on push to the default branch.
/// Returns the number of specs indexed.
pub async fn index_repo_specs(
    state: &AppState,
    repo_id: &str,
    workspace_id: &str,
    repo_path: &str,
    head_sha: &str,
) -> u64 {
    let manifest_paths = crate::spec_registry::read_manifest_paths(repo_path, head_sha).await;
    let mut count = 0u64;
    for path in manifest_paths {
        let file_path = format!("specs/{path}");
        // Content extracted from the pushed git blob (search.md §Index Updates).
        let content = git_bytes(repo_path, &["show", &format!("{head_sha}:{file_path}")])
            .await
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();
        let (title, owner, approval_status, linked_tasks, linked_mrs, updated_at) =
            match state.spec_ledger.find_by_path(&path).await {
                Ok(Some(entry)) => (
                    entry.title,
                    entry.owner,
                    entry.approval_status.to_string(),
                    entry.linked_tasks,
                    entry.linked_mrs,
                    Some(entry.updated_at),
                ),
                _ => (
                    String::new(),
                    String::new(),
                    "pending".to_string(),
                    Vec::new(),
                    Vec::new(),
                    None,
                ),
            };
        index(
            state,
            spec_document(
                &path,
                &title,
                &owner,
                &approval_status,
                &content,
                &linked_tasks,
                &linked_mrs,
                Some(workspace_id),
                Some(repo_id),
                updated_at,
            ),
        )
        .await;
        count += 1;
    }
    count
}

/// Index one pushed commit — the message is read from the git object store.
pub async fn index_pushed_commit(
    state: &AppState,
    repo_id: &str,
    workspace_id: &str,
    repo_path: &str,
    sha: &str,
    refname: &str,
    agent_id: &str,
) {
    let branch = refname.trim_start_matches("refs/heads/");
    let (author, timestamp, message) = match commit_metadata(repo_path, sha).await {
        Some((a, t, m)) => (a, t, m),
        None => (String::new(), crate::api::now_secs(), String::new()),
    };
    index(
        state,
        commit_document(
            sha,
            &author,
            timestamp,
            &message,
            branch,
            agent_id,
            workspace_id,
            repo_id,
        ),
    )
    .await;
}

/// Index all files in `repo`'s default branch for code search. Walks the git
/// tree (path, blob SHA, size via `git ls-tree -r -l`), skips binaries and
/// oversized blobs, and indexes real content.
pub async fn index_repo_files(state: &AppState, repo: &Repository) -> u64 {
    let out = match git_bytes(&repo.path, &["ls-tree", "-r", "-l", &repo.default_branch]).await {
        Some(o) => o,
        None => {
            tracing::debug!(
                repo_id = %repo.id,
                branch = %repo.default_branch,
                "search-index: no tree to walk (missing branch or repo path)"
            );
            return 0;
        }
    };
    let text = String::from_utf8_lossy(&out);
    let workspace_id = repo.workspace_id.to_string();
    let repo_id = repo.id.to_string();
    let mut count = 0u64;
    for line in text.lines() {
        // Long format: "<mode> SP <type> SP <sha> SP <size> TAB <path>".
        let Some((meta, path)) = line.split_once('\t') else {
            continue;
        };
        let mut fields = meta.split_whitespace();
        let _mode = fields.next();
        let obj_type = fields.next().unwrap_or("");
        let blob_sha = fields.next().unwrap_or("");
        let size: u64 = fields.next().unwrap_or("").parse().unwrap_or(u64::MAX);
        if obj_type != "blob" || path.is_empty() {
            continue;
        }
        if size > FILE_INDEX_MAX_BYTES {
            continue;
        }
        let Some(content) = read_blob(&repo.path, blob_sha).await else {
            continue;
        };
        if looks_binary(&content) {
            continue;
        }
        let Some(body) = String::from_utf8(content).ok() else {
            continue;
        };
        index(
            state,
            file_document(path, &body, language_for(path), &workspace_id, &repo_id),
        )
        .await;
        count += 1;
    }
    count
}

/// Background job entry: refresh file content for every repository. Registered
/// in the `JobRegistry` as `search_code_index` and triggered on push — code
/// search is eventually consistent (search.md §Index Updates).
pub async fn run_code_index_job(state: &AppState) -> Result<()> {
    let repos = state.repos.list().await?;
    for repo in &repos {
        let indexed = index_repo_files(state, repo).await;
        tracing::info!(
            repo_id = %repo.id,
            files_indexed = indexed,
            "search-index: code search refreshed"
        );
    }
    Ok(())
}

// ─── Full reindex coordinator ────────────────────────────────────────────────

/// Rebuild the entire search index from the domain repositories: clear the
/// index, then index every spec, task, MR, review, commit, agent, persona,
/// activity event, audit event, and file. Returns the true number of
/// documents indexed (NOT a count of pre-existing docs).
pub async fn reindex_all(state: &AppState) -> Result<u64> {
    // Adapter contract: clear the index first so the rebuild cannot leave
    // stale documents behind.
    state.search.reindex_all().await?;

    let mut count = 0u64;
    let repos = state.repos.list().await?;
    let repo_by_id: HashMap<String, Repository> = repos
        .iter()
        .map(|r| (r.id.to_string(), r.clone()))
        .collect();

    // Specs — content extracted from the git blob pinned in the spec ledger.
    for entry in state.spec_ledger.list_all().await? {
        let repo_path = entry
            .repo_id
            .as_ref()
            .and_then(|rid| repo_by_id.get(rid))
            .map(|r| r.path.clone());
        let content = match repo_path {
            Some(path) => read_blob(&path, &entry.current_sha)
                .await
                .and_then(|b| String::from_utf8(b).ok())
                .unwrap_or_default(),
            None => String::new(),
        };
        index(
            state,
            spec_document(
                &entry.path,
                &entry.title,
                &entry.owner,
                &entry.approval_status.to_string(),
                &content,
                &entry.linked_tasks,
                &entry.linked_mrs,
                entry.workspace_id.as_deref(),
                entry.repo_id.as_deref(),
                Some(entry.updated_at),
            ),
        )
        .await;
        count += 1;
    }

    // Tasks.
    for task in state.tasks.list().await? {
        index(state, task_document(&task)).await;
        count += 1;
    }

    // MRs + their reviews.
    for mr in state.merge_requests.list().await? {
        index(state, mr_document(&mr)).await;
        count += 1;
        for review in state.reviews.list_reviews(&mr.id).await? {
            index(state, review_document(&review, &mr)).await;
            count += 1;
        }
    }

    // Agents.
    for agent in state.agents.list().await? {
        index(state, agent_document(&agent)).await;
        count += 1;
    }

    // Personas.
    for persona in state.personas.list().await? {
        index(state, persona_document(&persona)).await;
        count += 1;
    }

    // Activity events (analytics).
    for event in state
        .analytics
        .query(None, None, REINDEX_EVENT_LIMIT)
        .await?
    {
        index(state, activity_event_document(&event)).await;
        count += 1;
    }

    // Audit events.
    let audit_filter = gyre_ports::audit::AuditQueryFilter {
        agent_id: None,
        event_type: None,
        workspace_id: None,
        user_id: None,
        resource_type: None,
        outcome: None,
        since: None,
        until: None,
        limit: REINDEX_EVENT_LIMIT,
    };
    for event in state.audit.query(&audit_filter).await? {
        index(state, audit_event_document(&event)).await;
        count += 1;
    }

    // Commits — agent-commit mappings; the message is read from git.
    for repo in &repos {
        for mapping in state.agent_commits.find_by_repo(&repo.id).await? {
            let (author, timestamp, message) =
                match commit_metadata(&repo.path, &mapping.commit_sha).await {
                    Some((a, t, m)) => (a, t, m),
                    None => (String::new(), mapping.timestamp, String::new()),
                };
            // Same normalization as index_pushed_commit: the branch facet
            // carries the branch name ("main"), not the raw refname — the
            // reindexed document must be identical to the push-path one.
            let branch = mapping.branch.trim_start_matches("refs/heads/");
            index(
                state,
                commit_document(
                    &mapping.commit_sha,
                    &author,
                    timestamp,
                    &message,
                    branch,
                    &mapping.agent_id.to_string(),
                    &repo.workspace_id.to_string(),
                    &repo.id.to_string(),
                ),
            )
            .await;
            count += 1;
        }
    }

    // Files (code search) — real content from the default-branch tree.
    for repo in &repos {
        count += index_repo_files(state, repo).await;
    }

    Ok(count)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::Id;
    use gyre_domain::{
        AgentCommit, ApprovalStatus, AuditEventType, AuditOutcome, PersonaScope, Review,
        ReviewDecision, SpecLedgerEntry,
    };
    use gyre_ports::search::SearchQuery;
    use gyre_ports::SearchPort;
    use std::sync::Arc;

    /// Distinctive terms used to probe the index — chosen so no other seeded
    /// document can accidentally match them.
    const SPEC_TERM: &str = "quokkafederation";
    const COMMIT_TERM: &str = "wombatintegration";
    const TASK_TERM: &str = "axolotlplanning";
    const FILE_TERM: &str = "pangolinlauncher";
    const GHOST_TERM: &str = "zzzghoststaledoc";

    /// Build a temp git repo with a spec (registered in the manifest), a Rust
    /// source file, and one commit carrying a distinctive message.
    /// Returns (tempdir, head_sha, spec_blob_sha).
    async fn make_git_repo() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().to_str().unwrap().to_string();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&path)
                .output()
                .expect("git spawn failed");
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Testcommitter"]);

        let write = |rel: &str, content: &str| {
            let full = dir.path().join(rel);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, content).unwrap();
        };
        write(
            "specs/manifest.yaml",
            "version: 1\nspecs:\n  - path: system/search.md\n    title: Search\n    owner: user:test\n",
        );
        write(
            "specs/system/search.md",
            &format!("# Search\n\nThe {SPEC_TERM} protocol governs index rebuilds."),
        );
        write(
            "src/main.rs",
            &format!("fn main() {{\n    println!(\"{FILE_TERM}\");\n}}\n"),
        );
        run(&["add", "."]);
        run(&["commit", "-m", &format!("Add {COMMIT_TERM} scaffolding")]);
        let head = run(&["rev-parse", "HEAD"]);
        let blob = run(&["rev-parse", "HEAD:specs/system/search.md"]);
        (dir, head, blob)
    }

    /// A test AppState whose search port is a real SQLite FTS5 backend.
    fn state_with_sqlite_search() -> (tempfile::NamedTempFile, Arc<AppState>) {
        let file = tempfile::NamedTempFile::new().unwrap();
        let storage = gyre_adapters::SqliteStorage::new(file.path().to_str().unwrap()).unwrap();
        (
            file,
            crate::mem::test_state_with_search(Arc::new(storage) as Arc<dyn SearchPort>),
        )
    }

    async fn search(
        state: &AppState,
        q: &str,
        entity_type: Option<&str>,
    ) -> Vec<gyre_ports::search::SearchResult> {
        // Unscoped-within-tenant query (Admin shape). The seeded entities'
        // workspaces have no row in the test state's workspace repo, so their
        // tenant resolves to the empty string (never a fabricated scope) —
        // the query matches that boundary.
        state
            .search
            .search(SearchQuery {
                query: q.to_string(),
                parsed: gyre_ports::search::parse_query(q),
                entity_type: entity_type.map(str::to_string),
                workspace_id: None,
                limit: 50,
                tenant_id: String::new(),
                accessible_workspace_ids: None,
                accessible_repo_ids: None,
            })
            .await
            .unwrap()
    }

    /// The full coordinator test required by task-202's acceptance criteria:
    /// seed entities of every type, poison the index with a stale doc, run the
    /// reindex coordinator against a real SQLite FTS backend, and assert each
    /// type is retrievable. Fails if any type is skipped or if reindex counts
    /// stale docs instead of rebuilding.
    #[tokio::test]
    async fn full_reindex_rebuilds_all_entity_types_from_real_storage() {
        let (git_dir, head_sha, spec_blob_sha) = make_git_repo().await;
        let (_db, state) = state_with_sqlite_search();
        let now = 1_800_000_000u64;

        // A real repository record pointing at the git repo on disk.
        let repo = Repository::new(
            Id::new("repo-1"),
            Id::new("ws-1"),
            "gyre",
            git_dir.path().to_str().unwrap(),
            now,
        );
        state.repos.create(&repo).await.unwrap();

        // Spec: ledger entry pinning the pushed blob.
        state
            .spec_ledger
            .save(&SpecLedgerEntry {
                path: "system/search.md".to_string(),
                title: "Search".to_string(),
                owner: "user:test".to_string(),
                kind: None,
                current_sha: spec_blob_sha,
                approval_mode: "human_and_agent".to_string(),
                approval_status: ApprovalStatus::Pending,
                linked_tasks: vec!["task-9x9".to_string()],
                linked_mrs: vec!["mr-4y4".to_string()],
                drift_status: "clean".to_string(),
                created_at: now,
                updated_at: now,
                repo_id: Some("repo-1".to_string()),
                workspace_id: Some("ws-1".to_string()),
            })
            .await
            .unwrap();

        // Task.
        let mut task = Task::new(Id::new("task-1"), format!("{TASK_TERM} roadmap"), now);
        task.description = Some("Plan the search rollout".to_string());
        task.workspace_id = Id::new("ws-1");
        task.repo_id = Id::new("repo-1");
        state.tasks.create(&task).await.unwrap();

        // MR + review.
        let mut mr = MergeRequest::new(
            Id::new("mr-1"),
            Id::new("repo-1"),
            "Search reindex",
            "feat/search",
            "main",
            now,
        );
        mr.workspace_id = Id::new("ws-1");
        state.merge_requests.create(&mr).await.unwrap();
        let review = Review::new(
            Id::new("review-1"),
            Id::new("mr-1"),
            "agent-reviewer-9",
            ReviewDecision::Approved,
            now,
        );
        // Review body with a term findable via the MR join. The body must NOT
        // contain the decision or reviewer values — the decision/reviewer
        // assertions below prove those Indexed Fields are searchable on
        // their own (review finding: facet-only values are unmatchable).
        let mut review = review;
        review.body = Some("narwhalreview commentary".to_string());
        state.reviews.submit_review(&review).await.unwrap();

        // Agent — with a persona attached, so the specced `persona` indexed
        // field is exercised (review finding: it was never indexed).
        let mut agent = Agent::new(Id::new("agent-1"), "agent-capybara", now);
        agent.workspace_id = Id::new("ws-1");
        agent.persona_id = Some(Id::new("persona-1"));
        state.agents.create(&agent).await.unwrap();

        // Persona.
        let persona = Persona::new(
            Id::new("persona-1"),
            "Ibex",
            "ibex",
            PersonaScope::Workspace(Id::new("ws-1")),
            "You are the ibexprompt strategist.",
            now,
        );
        state.personas.create(&persona).await.unwrap();

        // Activity event.
        let activity = AnalyticsEvent::new(
            Id::new("ev-1"),
            "task.status_changed",
            Some("agent-1".to_string()),
            serde_json::json!({ "description": "dolphinactivity transition" }),
            now,
        );
        state.analytics.record(&activity).await.unwrap();

        // Audit event.
        let audit = AuditEvent::new(
            Id::new("aud-1"),
            AuditEventType::ProcessExec,
            Some(Id::new("agent-1")),
            None,
            None,
            Some(Id::new("ws-1")),
            Some(Id::new("repo-1")),
            "agent".to_string(),
            Some("agent-1".to_string()),
            AuditOutcome::Success,
            serde_json::json!({ "path": "/bin/echidnaaudit" }),
            None,
            None,
            now,
        );
        state.audit.record(&audit).await.unwrap();

        // Commit mapping for the pushed commit.
        let commit_mapping = AgentCommit::new(
            Id::new("ac-1"),
            Id::new("agent-1"),
            Id::new("repo-1"),
            head_sha.clone(),
            "refs/heads/main",
            now,
        );
        state.agent_commits.record(&commit_mapping).await.unwrap();

        // Poison the index with a stale doc that matches no entity: the
        // coordinator must rebuild (drop it), not count pre-existing docs.
        state
            .search
            .index(SearchDocument {
                entity_type: "task".to_string(),
                entity_id: "ghost".to_string(),
                title: "ghost".to_string(),
                body: GHOST_TERM.to_string(),
                workspace_id: None,
                repo_id: None,
                tenant_id: None,
                facets: HashMap::new(),
                updated_at: None,
            })
            .await
            .unwrap();

        // Expected documents: spec, task, mr, review, agent, persona,
        // activity event, audit event, commit + 3 files (manifest.yaml,
        // search.md, main.rs).
        let count = reindex_all(&state).await.unwrap();
        assert_eq!(
            count, 12,
            "reindex must return the true document count for all 10 entity types"
        );

        // A second run must be idempotent (no duplicate docs).
        let count2 = reindex_all(&state).await.unwrap();
        assert_eq!(count2, 12, "reindex must be idempotent");

        // The stale doc must be gone — proof of rebuild, not count.
        assert!(
            search(&state, GHOST_TERM, None).await.is_empty(),
            "stale document survived the reindex"
        );

        // Each entity type is retrievable, including spec-by-body-content and
        // commit-by-message.
        let specs = search(&state, SPEC_TERM, Some("spec")).await;
        assert_eq!(specs.len(), 1, "spec must be findable by body content");
        assert_eq!(specs[0].entity_id, "system/search.md");
        assert_eq!(
            specs[0].facets.get("approval_status").map(String::as_str),
            Some("pending")
        );
        assert_eq!(
            specs[0].facets.get("scope").map(String::as_str),
            Some("system")
        );

        // The spec's links (ledger linked_tasks/linked_mrs) are indexed —
        // the spec is findable by a linked entity id (review finding).
        let spec_by_link = search(&state, "task-9x9", Some("spec")).await;
        assert_eq!(
            spec_by_link.len(),
            1,
            "spec must be findable by linked task id"
        );
        assert_eq!(spec_by_link[0].entity_id, "system/search.md");
        let spec_by_mr_link = search(&state, "mr-4y4", Some("spec")).await;
        assert_eq!(
            spec_by_mr_link.len(),
            1,
            "spec must be findable by linked MR id"
        );
        assert_eq!(spec_by_mr_link[0].entity_id, "system/search.md");
        // The owner is an Indexed Field (searchable) — the spec content and
        // links contain neither "user" nor "test", so this only matches if
        // the owner is folded into the body (review finding).
        let spec_by_owner = search(&state, "user:test", Some("spec")).await;
        assert_eq!(spec_by_owner.len(), 1, "spec must be findable by owner");
        assert_eq!(spec_by_owner[0].entity_id, "system/search.md");

        let commits = search(&state, COMMIT_TERM, Some("commit")).await;
        assert_eq!(commits.len(), 1, "commit must be findable by message");
        assert_eq!(commits[0].entity_id, head_sha);
        assert_eq!(
            commits[0].facets.get("branch").map(String::as_str),
            Some("main")
        );
        // Author and branch are Indexed Fields (searchable), not just filter
        // facets — the commit message contains neither, so these queries only
        // match if they are folded into the body (review finding).
        let commits_by_author = search(&state, "Testcommitter", Some("commit")).await;
        assert_eq!(
            commits_by_author.len(),
            1,
            "commit must be findable by author"
        );
        assert_eq!(commits_by_author[0].entity_id, head_sha);
        let commits_by_branch = search(&state, "main", Some("commit")).await;
        assert_eq!(
            commits_by_branch.len(),
            1,
            "commit must be findable by branch"
        );
        assert_eq!(commits_by_branch[0].entity_id, head_sha);

        let tasks = search(&state, TASK_TERM, Some("task")).await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].entity_id, "task-1");
        assert_eq!(
            tasks[0].facets.get("status").map(String::as_str),
            Some("backlog")
        );

        let mrs = search(&state, "reindex", Some("mr")).await;
        assert_eq!(mrs.len(), 1, "mr must be findable");
        assert_eq!(mrs[0].entity_id, "mr-1");

        let reviews = search(&state, "narwhalreview", Some("review")).await;
        assert_eq!(reviews.len(), 1, "review must be findable by body");
        assert_eq!(reviews[0].entity_id, "review-1");
        assert_eq!(
            reviews[0].facets.get("decision").map(String::as_str),
            Some("approved")
        );
        // Decision and reviewer are Indexed Fields (searchable), not just
        // filter facets — the seeded body deliberately contains neither
        // value, so these queries only match if they are folded into the
        // document body (review finding).
        let reviews_by_decision = search(&state, "approved", Some("review")).await;
        assert_eq!(
            reviews_by_decision.len(),
            1,
            "review must be findable by decision"
        );
        assert_eq!(reviews_by_decision[0].entity_id, "review-1");
        let reviews_by_reviewer = search(&state, "agent-reviewer-9", Some("review")).await;
        assert_eq!(
            reviews_by_reviewer.len(),
            1,
            "review must be findable by reviewer"
        );
        assert_eq!(reviews_by_reviewer[0].entity_id, "review-1");

        let agents = search(&state, "capybara", Some("agent")).await;
        assert_eq!(agents.len(), 1, "agent must be findable by name");
        assert_eq!(agents[0].entity_id, "agent-1");
        // The specced `persona` indexed field: an agent with a persona is
        // findable by that persona id (review finding: it was never indexed).
        let agents_by_persona = search(&state, "persona-1", Some("agent")).await;
        assert_eq!(
            agents_by_persona.len(),
            1,
            "agent must be findable by its attached persona"
        );
        assert_eq!(agents_by_persona[0].entity_id, "agent-1");
        assert_eq!(
            agents_by_persona[0]
                .facets
                .get("persona")
                .map(String::as_str),
            Some("persona-1")
        );

        let personas = search(&state, "ibexprompt", Some("persona")).await;
        assert_eq!(
            personas.len(),
            1,
            "persona must be findable by prompt content"
        );
        assert_eq!(personas[0].entity_id, "persona-1");
        assert_eq!(
            personas[0].facets.get("scope").map(String::as_str),
            Some("workspace")
        );

        let activity_results = search(&state, "dolphinactivity", Some("activity_event")).await;
        assert_eq!(
            activity_results.len(),
            1,
            "activity event must be findable by description"
        );
        assert_eq!(activity_results[0].entity_id, "ev-1");
        // agent_id is an Indexed Field for activity events (searchable) —
        // the description contains neither "agent" nor "1", so this only
        // matches if agent_id is folded into the body (review finding).
        let activity_by_agent = search(&state, "agent-1", Some("activity_event")).await;
        assert_eq!(
            activity_by_agent.len(),
            1,
            "activity event must be findable by agent_id"
        );
        assert_eq!(activity_by_agent[0].entity_id, "ev-1");

        let audits = search(&state, "echidnaaudit", Some("audit_event")).await;
        assert_eq!(audits.len(), 1, "audit event must be findable by path");
        assert_eq!(audits[0].entity_id, "aud-1");
        assert_eq!(
            audits[0].facets.get("event_type").map(String::as_str),
            Some("process_exec")
        );
        // agent_id is searchable for audit events too (review finding:
        // facet-only values are unmatchable — GET /api/v1/search has no
        // facet filter params). The path/detail contain neither "agent"
        // nor "1".
        let audits_by_agent = search(&state, "agent-1", Some("audit_event")).await;
        assert_eq!(
            audits_by_agent.len(),
            1,
            "audit event must be findable by agent_id"
        );
        assert_eq!(audits_by_agent[0].entity_id, "aud-1");

        let files = search(&state, FILE_TERM, Some("file")).await;
        assert_eq!(files.len(), 1, "file must be findable by content");
        assert_eq!(files[0].entity_id, "src/main.rs");
        assert_eq!(
            files[0].facets.get("language").map(String::as_str),
            Some("rust")
        );
    }

    /// The push-path spec indexer extracts content from the git blob at the
    /// pushed commit — not from a stub.
    #[tokio::test]
    async fn index_repo_specs_extracts_blob_content_from_pushed_commit() {
        let (git_dir, head_sha, _blob) = make_git_repo().await;
        let (_db, state) = state_with_sqlite_search();
        let repo = Repository::new(
            Id::new("repo-1"),
            Id::new("ws-1"),
            "gyre",
            git_dir.path().to_str().unwrap(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        let indexed = index_repo_specs(&state, "repo-1", "ws-1", &repo.path, &head_sha).await;
        assert_eq!(indexed, 1, "manifest registers exactly one spec");

        let results = search(&state, SPEC_TERM, Some("spec")).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity_id, "system/search.md");
        // Unregistered-in-ledger specs index as pending until sync runs.
        assert_eq!(
            results[0].facets.get("approval_status").map(String::as_str),
            Some("pending")
        );
    }

    /// The push-path spec indexer includes the ledger's linked task/MR ids —
    /// the spec's "links" indexed field (review finding: it was never
    /// indexed, so `?q=<linked id>` returned no spec).
    #[tokio::test]
    async fn index_repo_specs_indexes_linked_ids_from_ledger() {
        let (git_dir, head_sha, spec_blob_sha) = make_git_repo().await;
        let (_db, state) = state_with_sqlite_search();
        let repo = Repository::new(
            Id::new("repo-1"),
            Id::new("ws-1"),
            "gyre",
            git_dir.path().to_str().unwrap(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        let now = 1_800_000_000u64;
        state
            .spec_ledger
            .save(&SpecLedgerEntry {
                path: "system/search.md".to_string(),
                title: "Search".to_string(),
                owner: "user:test".to_string(),
                kind: None,
                current_sha: spec_blob_sha,
                approval_mode: "human_and_agent".to_string(),
                approval_status: ApprovalStatus::Approved,
                linked_tasks: vec!["task-9x9".to_string()],
                linked_mrs: vec!["mr-4y4".to_string()],
                drift_status: "clean".to_string(),
                created_at: now,
                updated_at: now,
                repo_id: Some("repo-1".to_string()),
                workspace_id: Some("ws-1".to_string()),
            })
            .await
            .unwrap();

        let indexed = index_repo_specs(&state, "repo-1", "ws-1", &repo.path, &head_sha).await;
        assert_eq!(indexed, 1);

        let by_task_link = search(&state, "task-9x9", Some("spec")).await;
        assert_eq!(
            by_task_link.len(),
            1,
            "spec must be findable by linked task id on the push path"
        );
        assert_eq!(by_task_link[0].entity_id, "system/search.md");
        let by_mr_link = search(&state, "mr-4y4", Some("spec")).await;
        assert_eq!(
            by_mr_link.len(),
            1,
            "spec must be findable by linked MR id on the push path"
        );
    }

    /// The push-path commit indexer reads the real commit message and author.
    #[tokio::test]
    async fn index_pushed_commit_reads_message_and_author() {
        let (git_dir, head_sha, _blob) = make_git_repo().await;
        let (_db, state) = state_with_sqlite_search();
        let repo = Repository::new(
            Id::new("repo-1"),
            Id::new("ws-1"),
            "gyre",
            git_dir.path().to_str().unwrap(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        index_pushed_commit(
            &state,
            "repo-1",
            "ws-1",
            &repo.path,
            &head_sha,
            "refs/heads/main",
            "agent-7",
        )
        .await;

        let results = search(&state, COMMIT_TERM, Some("commit")).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity_id, head_sha);
        assert_eq!(
            results[0].facets.get("author").map(String::as_str),
            Some("Testcommitter")
        );
        assert_eq!(
            results[0].facets.get("agent_id").map(String::as_str),
            Some("agent-7")
        );
    }

    /// Code-search file indexing skips binaries and oversized blobs but
    /// indexes real content.
    #[tokio::test]
    async fn file_indexing_skips_binary_and_oversized_blobs() {
        let (git_dir, _head, _blob) = make_git_repo().await;
        let (_db, state) = state_with_sqlite_search();

        // Binary blob (NUL bytes).
        std::fs::write(git_dir.path().join("blob.bin"), b"\x00\x01binary\xff").unwrap();
        // Oversized text blob (over the 1 MiB cap).
        let big = "x".repeat(FILE_INDEX_MAX_BYTES as usize + 100);
        std::fs::write(git_dir.path().join("big.txt"), big).unwrap();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(git_dir.path())
                .output()
                .unwrap();
            assert!(out.status.success());
        };
        run(&["add", "."]);
        run(&["commit", "-m", "add binary and big"]);

        let repo = Repository::new(
            Id::new("repo-1"),
            Id::new("ws-1"),
            "gyre",
            git_dir.path().to_str().unwrap(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        let indexed = index_repo_files(&state, &repo).await;
        // manifest.yaml, search.md, main.rs — binary and oversized skipped.
        assert_eq!(indexed, 3, "binary and oversized blobs must be skipped");
        assert!(
            search(&state, FILE_TERM, Some("file")).await.len() == 1,
            "real file content must be indexed"
        );
    }

    #[test]
    fn iso_date_formats_epoch_days() {
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(1_800_000_000), "2027-01-15");
    }

    #[test]
    fn language_for_maps_extensions() {
        assert_eq!(language_for("src/main.rs"), "rust");
        assert_eq!(language_for("a/b/c.ts"), "typescript");
        assert_eq!(language_for("no-extension"), "other");
    }
}
