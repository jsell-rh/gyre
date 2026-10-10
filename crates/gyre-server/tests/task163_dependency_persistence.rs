//! task-163: AppState wiring regression tests for persistent breaking-change
//! and dependency-policy storage.
//!
//! `build_state` reads `GYRE_DATABASE_URL` at construction, so these tests set
//! it before building state. The env var is process-global and `build_state`
//! may be called from either test while the other holds a stale value, so the
//! two tests serialize on a shared lock. Each integration test file runs in
//! its own process — no cross-file contamination.
//!
//! Fails if `breaking_changes` / `dependency_policies` in `AppState` are wired
//! to `MemBreakingChangeRepository` / `MemDependencyPolicyRepository` (mem
//! fallback only): a fresh `build_state` over the same DB file would observe
//! an empty store.

use gyre_common::Id;
use gyre_domain::{BreakingChange, BreakingChangeBehavior, DependencyPolicy};
use gyre_server::{build_state, AppState};
use std::sync::Arc;
use tempfile::TempDir;

/// `GYRE_DATABASE_URL` is process-global; these tests must not interleave.
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn sqlite_state() -> (TempDir, Arc<AppState>) {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("task163.db");
    unsafe {
        std::env::set_var(
            "GYRE_DATABASE_URL",
            format!("sqlite://{}", db_path.display()),
        );
    }
    (dir, build_state("task163-token", "http://localhost:0", None))
}

fn rebuild_state(dir: &TempDir) -> Arc<AppState> {
    let db_path = dir.path().join("task163.db");
    unsafe {
        std::env::set_var(
            "GYRE_DATABASE_URL",
            format!("sqlite://{}", db_path.display()),
        );
    }
    build_state("task163-token", "http://localhost:0", None)
}

#[tokio::test]
async fn breaking_changes_persist_across_state_rebuilds() {
    let _env = ENV_LOCK.lock().await;
    let (dir, state) = sqlite_state();

    let bc = BreakingChange::new(
        Id::new("bc-wire-1"),
        Id::new("edge-wire-1"),
        Id::new("repo-wire-b"),
        "deadbeefdeadbeef",
        "removed public API X",
        1000,
    );
    state.breaking_changes.create(&bc).await.unwrap();

    // A second AppState over the same database file must observe the record.
    let state2 = rebuild_state(&dir);
    let found = state2
        .breaking_changes
        .find_by_id(&Id::new("bc-wire-1"))
        .await
        .unwrap()
        .expect("breaking change must survive an AppState rebuild");
    assert_eq!(found.commit_sha, "deadbeefdeadbeef");
    assert_eq!(found.description, "removed public API X");

    let unacked = state2.breaking_changes.list_unacknowledged().await.unwrap();
    assert_eq!(unacked.len(), 1);
    assert_eq!(unacked[0].id.as_str(), "bc-wire-1");

    // Acknowledgment through the second instance is durable in the DB file.
    assert!(
        state2
            .breaking_changes
            .acknowledge(&Id::new("bc-wire-1"), "user-1", 2000)
            .await
            .unwrap()
    );

    let state3 = rebuild_state(&dir);
    assert!(
        state3
            .breaking_changes
            .list_unacknowledged()
            .await
            .unwrap()
            .is_empty(),
        "acknowledged change must not reappear as unacknowledged after rebuild"
    );

    std::env::remove_var("GYRE_DATABASE_URL");
}

#[tokio::test]
async fn dependency_policies_persist_across_state_rebuilds() {
    let _env = ENV_LOCK.lock().await;
    let (dir, state) = sqlite_state();

    let policy = DependencyPolicy {
        breaking_change_behavior: BreakingChangeBehavior::Block,
        max_version_drift: 7,
        stale_dependency_alert_days: 45,
        require_cascade_tests: true,
        auto_create_update_tasks: false,
    };
    state
        .dependency_policies
        .set_for_workspace(&Id::new("ws-wire-1"), &policy)
        .await
        .unwrap();

    let state2 = rebuild_state(&dir);
    let found = state2
        .dependency_policies
        .get_for_workspace(&Id::new("ws-wire-1"))
        .await
        .unwrap();
    assert_eq!(
        found.breaking_change_behavior,
        BreakingChangeBehavior::Block
    );
    assert_eq!(found.max_version_drift, 7);
    assert_eq!(found.stale_dependency_alert_days, 45);
    assert!(found.require_cascade_tests);
    assert!(!found.auto_create_update_tasks);

    // An unset workspace still yields the domain default.
    let fresh = state2
        .dependency_policies
        .get_for_workspace(&Id::new("ws-wire-none"))
        .await
        .unwrap();
    assert_eq!(fresh.breaking_change_behavior, BreakingChangeBehavior::Warn);

    std::env::remove_var("GYRE_DATABASE_URL");
}
