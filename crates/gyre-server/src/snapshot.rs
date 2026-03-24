//! Snapshot/restore: point-in-time JSON exports of server state.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

use crate::AppState;

const DEFAULT_SNAPSHOT_PATH: &str = "./snapshots";

fn snapshot_dir() -> PathBuf {
    std::env::var("GYRE_SNAPSHOT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_SNAPSHOT_PATH))
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub snapshot_id: String,
    pub path: String,
    pub size_bytes: u64,
    pub created_at: u64,
}

/// The full state snapshot written to disk.
#[derive(Serialize, Deserialize)]
struct StateSnapshot {
    created_at: u64,
    agents: Vec<gyre_domain::Agent>,
    tasks: Vec<gyre_domain::Task>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Create a snapshot of the current state. Returns metadata about the created snapshot.
pub async fn create_snapshot(state: &AppState) -> anyhow::Result<SnapshotMeta> {
    let dir = snapshot_dir();
    tokio::fs::create_dir_all(&dir).await?;

    let created_at = now_secs();
    let snapshot_id = format!("{created_at}");
    let filename = format!("{snapshot_id}.json");
    let path = dir.join(&filename);

    let agents = state.agents.list().await?;
    let tasks = state.tasks.list().await?;

    let snap = StateSnapshot {
        created_at,
        agents,
        tasks,
    };

    let json = serde_json::to_string_pretty(&snap)?;
    tokio::fs::write(&path, &json).await?;

    let size_bytes = tokio::fs::metadata(&path).await?.len();

    let meta = SnapshotMeta {
        snapshot_id,
        path: path.to_string_lossy().to_string(),
        size_bytes,
        created_at,
    };

    info!(snapshot_id = %meta.snapshot_id, path = %meta.path, "snapshot created");
    Ok(meta)
}

/// List all available snapshots.
pub async fn list_snapshots() -> anyhow::Result<Vec<SnapshotMeta>> {
    let dir = snapshot_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut entries = tokio::fs::read_dir(&dir).await?;
    let mut snapshots = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let filename = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        if let Ok(created_at) = filename.parse::<u64>() {
            let size_bytes = tokio::fs::metadata(&path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
            snapshots.push(SnapshotMeta {
                snapshot_id: filename,
                path: path.to_string_lossy().to_string(),
                size_bytes,
                created_at,
            });
        }
    }

    snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(snapshots)
}

/// Delete a snapshot by ID.
pub async fn delete_snapshot(snapshot_id: &str) -> anyhow::Result<()> {
    let dir = snapshot_dir();
    let path = dir.join(format!("{snapshot_id}.json"));

    if !path.exists() {
        anyhow::bail!("snapshot '{}' not found", snapshot_id);
    }

    tokio::fs::remove_file(&path).await?;
    info!(snapshot_id = %snapshot_id, "snapshot deleted");
    Ok(())
}

/// Restore from a snapshot. Loads the snapshot data back into state.
/// Note: only replaces projects/agents/tasks that don't already exist.
/// Returns a warning that server restart may be needed for full effect.
pub async fn restore_snapshot(state: &AppState, snapshot_id: &str) -> anyhow::Result<String> {
    let dir = snapshot_dir();
    let path = dir.join(format!("{snapshot_id}.json"));

    if !path.exists() {
        anyhow::bail!("snapshot '{}' not found", snapshot_id);
    }

    let json = tokio::fs::read_to_string(&path).await?;
    let snap: StateSnapshot = serde_json::from_str(&json)?;

    // Restore agents
    for agent in snap.agents {
        if state
            .agents
            .find_by_id(&agent.id)
            .await
            .ok()
            .flatten()
            .is_none()
        {
            let _ = state.agents.create(&agent).await;
        }
    }

    // Restore tasks
    for task in snap.tasks {
        if state
            .tasks
            .find_by_id(&task.id)
            .await
            .ok()
            .flatten()
            .is_none()
        {
            let _ = state.tasks.create(&task).await;
        }
    }

    info!(snapshot_id = %snapshot_id, "snapshot restored");
    Ok(
        "Snapshot restored. A server restart is recommended to ensure full state consistency."
            .to_string(),
    )
}
