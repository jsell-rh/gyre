//! TEMPORARY review probe 2 for task-115 (candidate e54facd4) — DELETE AFTER REVIEW.
//! Two concurrent scheduler_run_once cycles both read the same Backlog
//! snapshot; the Backlog→InProgress claim is a local transition + blind
//! write, not an atomic compare-and-swap. Expected per the code comment:
//! "concurrent cycles ... cannot double-process".
use gyre_common::Id;
use gyre_domain::{Repository, Task, TaskStatus, TaskType, Workspace};

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_cycles_do_not_double_process() {
    let state = gyre_server::build_state("t", "http://x", None);
    let ws = Workspace::new(Id::new("ws-1"), Id::new("t1"), "Ws", "ws", 0);
    state.workspaces.create(&ws).await.unwrap();
    let repo = Repository::new(
        Id::new("r-1"),
        Id::new("ws-1"),
        "r-1",
        "/tmp/nonexistent-gyre-probe2",
        0,
    );
    state.repos.create(&repo).await.unwrap();

    let mut task = Task::new(Id::new("d-1"), "Delegation probe", 0);
    task.task_type = Some(TaskType::Delegation);
    task.repo_id = Id::new("r-1");
    task.workspace_id = Id::new("ws-1");
    task.spec_path = Some("specs/x.md@deadbeef".to_string());
    state.tasks.create(&task).await.unwrap();

    // Two concurrent cycles (30 s loop + POST /admin/jobs/task_scheduler/run).
    let s2 = state.clone();
    let a = tokio::spawn(async move { gyre_server::signal_chain::scheduler_run_once(&s2).await });
    let b = tokio::spawn(async move { gyre_server::signal_chain::scheduler_run_once(&state).await });
    let (ra, rb) = (a.await.unwrap(), b.await.unwrap());
    eprintln!("PROBE cycle results: {:?} {:?}", ra.is_ok(), rb.is_ok());

    let tasks = state.tasks.list().await.unwrap();
    let subs: Vec<_> = tasks
        .iter()
        .filter(|t| {
            t.parent_task_id.as_ref() == Some(&Id::new("d-1"))
                && t.task_type == Some(TaskType::Implementation)
        })
        .collect();
    eprintln!("PROBE implementation sub-tasks created: {}", subs.len());
    let d = state.tasks.find_by_id(&Id::new("d-1")).await.unwrap().unwrap();
    eprintln!("PROBE delegation status: {:?}", d.status);
    assert_eq!(
        subs.len(),
        1,
        "concurrent cycles must not double-decompose; got {} sub-tasks (claim is a local transition + blind write, both cycles read the same Backlog snapshot)",
        subs.len()
    );
    assert_eq!(d.status, TaskStatus::Done);
}
