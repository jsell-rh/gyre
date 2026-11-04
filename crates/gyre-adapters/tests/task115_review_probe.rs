//! TEMPORARY review probe for task-115 (delete after running).
//! Proves whether TaskRepository::create persists workspace_id in the
//! SQLite adapter (candidate d50dca86 removed it from NewTaskRow).
use gyre_common::Id;
use gyre_domain::{Task, TaskType};
use gyre_ports::TaskRepository;
use gyre_adapters::sqlite::SqliteStorage;

#[tokio::test]
async fn create_persists_workspace_id_sqlite() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let storage = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();

    let mut t = Task::new(Id::new("t-1"), "probe task", 1000);
    t.workspace_id = Id::new("ws-real");
    t.repo_id = Id::new("r-1");
    t.task_type = Some(TaskType::Implementation);
    storage.create(&t).await.unwrap();

    // Read back through the port.
    let back = storage.find_by_id(&t.id).await.unwrap().expect("task exists");
    assert_eq!(
        back.workspace_id,
        Id::new("ws-real"),
        "REGRESSION: create() persisted workspace_id='{}' (column default) instead of 'ws-real'",
        back.workspace_id
    );

    // And the workspace-scoped listing must find it.
    let listed = storage.list_by_workspace(&Id::new("ws-real")).await.unwrap();
    assert!(
        listed.iter().any(|x| x.id == t.id),
        "REGRESSION: list_by_workspace('ws-real') does not see the task"
    );
}
