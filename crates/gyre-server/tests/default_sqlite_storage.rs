//! Default durable storage (specs/development/architecture.md §Storage:
//! "SQLite as the default (simple, vertical scaling, zero ops)").
//!
//! Proves, against real SQLite files in isolated temp dirs, that:
//!   - with `GYRE_DATABASE_URL` unset, `build_state` opens durable SQLite
//!     at the platform default data path — redirected here to a temp XDG
//!     data home so the test never touches the operator's real database —
//!     a workspace created through one state survives a full state rebuild
//!     (the process-restart equivalent), and the database file exists at
//!     the default path with the Diesel migrations having run on it;
//!   - with `GYRE_DATABASE_URL=memory` (the explicit stateless opt-in),
//!     the server behaves exactly as the old unset default did: no durable
//!     backend handle, nothing survives a rebuild, and no database file is
//!     created at the default data path;
//!   - an explicit `sqlite://<path>` URL keeps selecting that exact path.
//!
//! `GYRE_DATABASE_URL` (and `XDG_DATA_HOME`, which redirects the default
//! path on Linux) are process-global: every test holds `ENV_LOCK` for its
//! whole env-poking window. No test in this binary may use
//! `gyre_server::select_in_memory_storage()` — the sticky in-process
//! sentinel would defeat the unset-env test below.

use gyre_common::Id;
use gyre_domain::Workspace;
use gyre_server::{build_state, default_sqlite_db_path};
use std::sync::LazyLock;
use tempfile::TempDir;

const TOKEN: &str = "default-storage-token";

/// Serializes env-var mutation across tests in this binary (own process).
static ENV_LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

fn make_workspace(id: &str) -> Workspace {
    // Tenant "default" matches the tenant scope `build_state`'s SQLite
    // handle is opened with, so the row is visible to the same handle's
    // find_by_id (hierarchy-enforcement.md §3 tenant-checked lookups).
    Workspace::new(
        Id::new(id),
        Id::new("default"),
        "Default Storage WS",
        "default-storage-ws",
        1,
    )
}

/// With `GYRE_DATABASE_URL` unset, a workspace created via the repository
/// survives a full state rebuild and lands in a real SQLite file at the
/// platform default data path. On the old code (in-memory default) the
/// rebuild sees nothing and no file is created — this is the regression
/// probe for the spec's "SQLite as the default" bullet.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn unset_env_boots_durable_sqlite_at_default_path() {
    let _env = ENV_LOCK.lock().await;
    let dir = TempDir::new().unwrap();
    // Redirect the XDG data home so the "default data path" is an isolated
    // temp dir, not the operator's real one.
    let xdg = dir.path().join("xdg-data");
    unsafe {
        std::env::remove_var("GYRE_DATABASE_URL");
        std::env::set_var("XDG_DATA_HOME", &xdg);
    }

    let db_path = default_sqlite_db_path();
    assert_eq!(
        db_path,
        xdg.join("gyre").join("gyre.db").display().to_string(),
        "the default path must follow the XDG data home"
    );

    // Boot 1: unset env must open the durable backend (the health-probe
    // handle is Some exactly when a real storage was constructed).
    let state = build_state(TOKEN, "http://127.0.0.1:1", None);
    assert!(
        state.storage.is_some(),
        "unset GYRE_DATABASE_URL must wire the durable default SQLite backend"
    );

    let ws = make_workspace("ws-default-persist");
    state.workspaces.create(&ws).await.unwrap();
    drop(state);

    // The database file exists at the default path — with the migrations
    // SqliteStorage::new ran on the fresh file.
    assert!(
        std::path::Path::new(&db_path).is_file(),
        "gyre.db must exist at the default data path ({db_path})"
    );

    // Boot 2: a genuinely fresh AppState + pool over the same file — the
    // process-restart equivalent. The in-memory default would 404 here.
    let state2 = build_state(TOKEN, "http://127.0.0.1:1", None);
    let found = state2
        .workspaces
        .find_by_id(&Id::new("ws-default-persist"))
        .await
        .unwrap();
    assert!(
        found.is_some(),
        "a workspace must survive a restart on the default SQLite backend"
    );

    unsafe { std::env::remove_var("XDG_DATA_HOME") };
}

/// `GYRE_DATABASE_URL=memory` is stateless exactly as the old unset
/// default was: no durable backend, nothing survives a rebuild — and (on
/// Linux, where the default path can be redirected) no database file is
/// created at the default data path.
#[tokio::test(flavor = "multi_thread")]
async fn memory_sentinel_is_stateless() {
    let _env = ENV_LOCK.lock().await;
    let dir = TempDir::new().unwrap();
    unsafe {
        std::env::set_var("GYRE_DATABASE_URL", "memory");
        #[cfg(target_os = "linux")]
        std::env::set_var("XDG_DATA_HOME", dir.path().join("xdg-mem"));
    }

    let state = build_state(TOKEN, "http://127.0.0.1:1", None);
    assert!(
        state.storage.is_none(),
        "the memory sentinel must not open any durable backend"
    );

    let ws = make_workspace("ws-memory");
    state.workspaces.create(&ws).await.unwrap();
    drop(state);

    // Rebuild: state is lost, exactly as before the default change.
    let state2 = build_state(TOKEN, "http://127.0.0.1:1", None);
    let found = state2
        .workspaces
        .find_by_id(&Id::new("ws-memory"))
        .await
        .unwrap();
    assert!(
        found.is_none(),
        "the memory sentinel must be stateless across rebuilds"
    );

    // And no database file appeared at the (redirected) default path.
    #[cfg(target_os = "linux")]
    {
        let db_path = default_sqlite_db_path();
        assert!(
            !std::path::Path::new(&db_path).exists(),
            "the memory sentinel must not create a database file at the default \
             data path ({db_path})"
        );
        unsafe { std::env::remove_var("XDG_DATA_HOME") };
    }

    unsafe { std::env::remove_var("GYRE_DATABASE_URL") };
}

/// Explicit `sqlite://<path>` URLs keep selecting that exact path — no
/// regression in backend selection for operators who configure one.
#[tokio::test(flavor = "multi_thread")]
async fn explicit_sqlite_url_selects_that_path() {
    let _env = ENV_LOCK.lock().await;
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("explicit.db");
    let url = format!("sqlite://{}", db_path.display());
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &url) };

    let state = build_state(TOKEN, "http://127.0.0.1:1", None);
    assert!(
        state.storage.is_some(),
        "an explicit sqlite:// URL must open the durable backend"
    );
    let ws = make_workspace("ws-explicit");
    state.workspaces.create(&ws).await.unwrap();
    drop(state);

    assert!(
        db_path.is_file(),
        "the database file must exist at the explicit path"
    );

    // Rebuild over the same URL: durable.
    let state2 = build_state(TOKEN, "http://127.0.0.1:1", None);
    let found = state2
        .workspaces
        .find_by_id(&Id::new("ws-explicit"))
        .await
        .unwrap();
    assert!(found.is_some(), "explicit-path SQLite must be durable");

    unsafe { std::env::remove_var("GYRE_DATABASE_URL") };
}
