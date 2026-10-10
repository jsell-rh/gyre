//! TEMPORARY review probe for task-115 (candidate e54facd4) — DELETE AFTER REVIEW.
//! Exercises the scheduler's failure-release path: a claimed InProgress
//! Delegation task whose repo orchestrator run fails must return to Backlog
//! for retry. Probe 1: budget-exhausted spawn → run_repo_orchestrator errors.
//! Probe 2: what state is the task left in?
use gyre_common::Id;
use gyre_domain::spec_ledger::ApprovalStatus;
use gyre_domain::{Repository, Task, TaskType, Workspace};

async fn probe_state() -> std::sync::Arc<gyre_server::AppState> {
    let state = gyre_server::build_state("t", "http://x", None);
    let ws = Workspace::new(Id::new("ws-1"), Id::new("t1"), "Ws", "ws", 0);
    state.workspaces.create(&ws).await.unwrap();
    let repo = Repository::new(Id::new("r-1"), Id::new("ws-1"), "r-1", "/tmp/nonexistent-gyre-probe", 0);
    state.repos.create(&repo).await.unwrap();
    state
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_repo_orchestrator_run_releases_claim() {
    let state = probe_state().await;

    // Exhaust the workspace budget so the repo-orchestrator spawn inside
    // run_repo_orchestrator fails → run_repo_orchestrator returns Err →
    // the scheduler's failure path runs: `transition_status(Backlog)`.
    state
        .budget_configs
        .set_config("workspace:ws-1", &gyre_domain::BudgetConfig {
            max_concurrent_agents: Some(0),
            ..Default::default()
        })
        .await
        .unwrap();

    let mut task = Task::new(Id::new("d-1"), "Delegation probe", 0);
    task.task_type = Some(TaskType::Delegation);
    task.repo_id = Id::new("r-1");
    task.workspace_id = Id::new("ws-1");
    task.spec_path = Some("specs/x.md@deadbeef".to_string());
    state.tasks.create(&task).await.unwrap();

    let res = gyre_server::signal_chain::scheduler_run_once(&state).await;
    // The cycle itself may return Ok (per-task failures are logged inside);
    // or Err. Either way inspect the task state afterwards.
    let after = state.tasks.find_by_id(&Id::new("d-1")).await.unwrap().unwrap();
    eprintln!("PROBE scheduler_run_once result: {:?}", res.as_ref().map(|_| "Ok").map_err(|e| e.to_string()));
    eprintln!("PROBE task status after failure path: {:?}", after.status);
    eprintln!("PROBE EXPECTED per code intent (comment: 'Release the claim so a later cycle retries'): Backlog");
    eprintln!("PROBE ACTUAL: {:?}", after.status);
    assert_eq!(
        after.status,
        gyre_domain::TaskStatus::Backlog,
        "failure path must release the claim back to Backlog; got {:?} — InProgress→Backlog is not in the TaskStatus transition matrix, so transition_status(Backlog) is a silent no-op and the task is wedged InProgress forever (no cycle ever picks it up again: list_by_status(Backlog) skips it)",
        after.status
    );
}
