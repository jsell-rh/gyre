//! Cross-tenant isolation integration test.
//!
//! Spec: specs/system/hierarchy-enforcement.md §3 — "tests/tenant_isolation.rs:
//! creates entities under tenant A, attempts to read them via tenant B's
//! storage adapter, asserts zero leakage." This is the end-to-end proof behind
//! the check-tenant-filter.sh lint: the lint proves the filter text exists;
//! this test proves the filters actually isolate. Two `SqliteStorage` handles
//! share ONE database file and differ only by tenant scope (`with_tenant`) —
//! the exact handle pair two concurrent requests from different tenants ride.
//!
//! Regression targets (each asserted below):
//!   - read methods missing `tenant_id.eq()` return another tenant's rows;
//!   - `find_by_id` on a leaked UUID must still be tenant-filtered;
//!   - create() must stamp the *storage's* tenant, not a fabricated
//!     "default" (both defects lived here before task-160 and are caught by
//!     this file failing).

use gyre_adapters::sqlite::SqliteStorage;
use gyre_common::Id;
use gyre_domain::{Agent, MergeRequest, Repository, Task, Tenant, Workspace};
use gyre_ports::{
    AgentRepository, MergeRequestRepository, RepoRepository, TaskRepository, TenantRepository,
    WorkspaceRepository,
};
use tempfile::NamedTempFile;

/// One DB file, two tenant-scoped handles over the same pool.
fn two_tenants() -> (NamedTempFile, SqliteStorage, SqliteStorage) {
    let tmp = NamedTempFile::new().expect("temp db file");
    let a =
        SqliteStorage::new_for_tenant(tmp.path().to_str().unwrap(), "tenant-a").expect("open A");
    let b = a.with_tenant("tenant-b");
    (tmp, a, b)
}

/// Seed one workspace + repo + task + agent + MR under each tenant.
/// Returns the ids created under A and under B respectively.
struct TenantIds {
    ws: Id,
    repo: Id,
    task: Id,
    agent: Id,
    mr: Id,
}

async fn seed(s: &SqliteStorage, tag: &str, tenant: &Id) -> TenantIds {
    let ids = TenantIds {
        ws: Id::new(format!("ws-{tag}")),
        repo: Id::new(format!("repo-{tag}")),
        task: Id::new(format!("task-{tag}")),
        agent: Id::new(format!("agent-{tag}")),
        mr: Id::new(format!("mr-{tag}")),
    };

    TenantRepository::create(s, &Tenant::new(tenant.clone(), tag, tag, 1000))
        .await
        .unwrap();

    let ws = Workspace::new(ids.ws.clone(), tenant.clone(), format!("ws {tag}"), tag, 1000);
    WorkspaceRepository::create(s, &ws).await.unwrap();

    let repo = Repository::new(
        ids.repo.clone(),
        ids.ws.clone(),
        format!("repo {tag}"),
        format!("/srv/git/{tag}/repo"),
        1000,
    );
    RepoRepository::create(s, &repo).await.unwrap();

    let mut task = Task::new(ids.task.clone(), format!("task {tag}"), 1000);
    task.workspace_id = ids.ws.clone();
    task.repo_id = ids.repo.clone();
    TaskRepository::create(s, &task).await.unwrap();

    let mut agent = Agent::new(ids.agent.clone(), format!("agent {tag}"), 1000);
    agent.workspace_id = ids.ws.clone();
    AgentRepository::create(s, &agent).await.unwrap();

    let mut mr = MergeRequest::new(
        ids.mr.clone(),
        ids.repo.clone(),
        format!("mr {tag}"),
        format!("feature/{tag}"),
        "main",
        1000,
    );
    mr.workspace_id = ids.ws.clone();
    MergeRequestRepository::create(s, &mr).await.unwrap();

    ids
}

#[tokio::test]
async fn tenant_reads_never_see_another_tenants_rows() {
    let (_tmp, sa, sb) = two_tenants();
    let a = seed(&sa, "a", &Id::new("tenant-a")).await;
    let b = seed(&sb, "b", &Id::new("tenant-b")).await;

    // ── workspaces ────────────────────────────────────────────────────────
    let wa = WorkspaceRepository::list(&sa).await.unwrap();
    assert_eq!(
        wa.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
        vec!["ws-a"],
        "tenant A's workspace list leaked another tenant's rows"
    );
    assert!(
        WorkspaceRepository::find_by_id(&sa, &b.ws)
            .await
            .unwrap()
            .is_none(),
        "find_by_id leaked tenant B's workspace id to tenant A"
    );
    assert!(
        WorkspaceRepository::find_by_id(&sb, &a.ws)
            .await
            .unwrap()
            .is_none(),
        "find_by_id leaked tenant A's workspace id to tenant B"
    );
    // positive control: each tenant still sees its own row
    assert!(WorkspaceRepository::find_by_id(&sa, &a.ws).await.unwrap().is_some());

    // ── repositories ──────────────────────────────────────────────────────
    let ra = RepoRepository::list(&sa).await.unwrap();
    assert_eq!(
        ra.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        vec!["repo-a"],
        "tenant A's repo list leaked another tenant's repos (create() tenant stamp broken?)"
    );
    assert!(RepoRepository::find_by_id(&sa, &b.repo).await.unwrap().is_none());
    assert!(RepoRepository::find_by_id(&sb, &a.repo).await.unwrap().is_none());
    assert!(RepoRepository::find_by_id(&sb, &b.repo).await.unwrap().is_some());

    // ── tasks ─────────────────────────────────────────────────────────────
    let ta = TaskRepository::list(&sa).await.unwrap();
    assert_eq!(
        ta.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        vec!["task-a"],
        "tenant A's task list leaked another tenant's tasks"
    );
    assert!(TaskRepository::find_by_id(&sa, &b.task).await.unwrap().is_none());
    assert!(TaskRepository::find_by_id(&sb, &a.task).await.unwrap().is_none());
    let tb_task = TaskRepository::find_by_id(&sb, &b.task).await.unwrap().unwrap();
    assert_eq!(
        tb_task.workspace_id, b.ws,
        "task row lost its hierarchy edge across the storage boundary"
    );

    // ── agents ────────────────────────────────────────────────────────────
    let aa = AgentRepository::list(&sa).await.unwrap();
    assert_eq!(
        aa.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(),
        vec!["agent-a"],
        "tenant A's agent list leaked another tenant's agents"
    );
    assert!(AgentRepository::find_by_id(&sa, &b.agent).await.unwrap().is_none());
    assert!(AgentRepository::find_by_id(&sb, &a.agent).await.unwrap().is_none());

    // ── merge requests ────────────────────────────────────────────────────
    let ma = MergeRequestRepository::list(&sa).await.unwrap();
    assert_eq!(
        ma.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        vec!["mr-a"],
        "tenant A's MR list leaked another tenant's MRs (create() tenant stamp broken?)"
    );
    assert!(
        MergeRequestRepository::find_by_id(&sa, &b.mr)
            .await
            .unwrap()
            .is_none(),
        "find_by_id leaked tenant B's MR id to tenant A (unfiltered find_by_id)"
    );
    assert!(MergeRequestRepository::find_by_id(&sb, &a.mr).await.unwrap().is_none());
    assert!(MergeRequestRepository::find_by_id(&sb, &b.mr).await.unwrap().is_some());
}

#[tokio::test]
async fn create_under_tenant_a_is_invisible_to_a_default_scoped_handle() {
    // The fabricated-"default" create() stamp class: a tenant-scoped handle
    // must persist rows under ITS tenant, so a second handle scoped to the
    // same tenant finds them and a "default"-scoped handle does not.
    let (_tmp, base) = {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        let storage = SqliteStorage::new_for_tenant(&path, "default").unwrap();
        (tmp, storage)
    };
    let sa = base.with_tenant("tenant-a");

    let ws = Workspace::new(Id::new("ws-x"), Id::new("tenant-a"), "x", "x", 1000);
    WorkspaceRepository::create(&sa, &ws).await.unwrap();
    let repo = Repository::new(Id::new("repo-x"), Id::new("ws-x"), "x", "/x", 1000);
    RepoRepository::create(&sa, &repo).await.unwrap();
    let mr = MergeRequest::new(Id::new("mr-x"), Id::new("repo-x"), "x", "b", "main", 1000);
    MergeRequestRepository::create(&sa, &mr).await.unwrap();

    // Same tenant via a fresh handle: must see it.
    let sa2 = base.with_tenant("tenant-a");
    assert!(RepoRepository::find_by_id(&sa2, &Id::new("repo-x")).await.unwrap().is_some());
    assert!(MergeRequestRepository::find_by_id(&sa2, &Id::new("mr-x")).await.unwrap().is_some());

    // A different scope ("default") must NOT see it.
    assert!(RepoRepository::find_by_id(&base, &Id::new("repo-x")).await.unwrap().is_none());
    assert!(
        MergeRequestRepository::find_by_id(&base, &Id::new("mr-x")).await.unwrap().is_none(),
        "MR create() stamped a fabricated tenant, not the storage's tenant"
    );
}
