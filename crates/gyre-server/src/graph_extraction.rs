//! Push-triggered knowledge graph extraction (M30b).
//!
//! After each successful git push, extracts architectural knowledge from the
//! repository source tree and persists nodes, edges, and an architectural
//! delta to the graph store.  Runs as a background task and never blocks the
//! push response.

use gyre_common::{graph::ArchitecturalDelta, Id};
use gyre_domain::{LanguageExtractor, RustExtractor};
use gyre_ports::GraphPort;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tracing::{info, warn};
use uuid::Uuid;

/// Extract and persist the knowledge graph for a repo at a specific commit.
///
/// Uses `git archive` to snapshot the tree, runs the Rust extractor, clears
/// stale graph data for the repo, then persists the new nodes, edges, and an
/// [`ArchitecturalDelta`] record.
///
/// All errors are logged and swallowed — extraction must never fail a push.
pub async fn extract_and_store_graph(
    repo_path: &str,
    repo_id: &str,
    new_sha: &str,
    graph_store: &dyn GraphPort,
    git_bin: &str,
) {
    if let Err(e) = do_extract(repo_path, repo_id, new_sha, graph_store, git_bin).await {
        warn!(%repo_id, %new_sha, "knowledge graph extraction failed: {e}");
    }
}

async fn do_extract(
    repo_path: &str,
    repo_id: &str,
    new_sha: &str,
    graph_store: &dyn GraphPort,
    git_bin: &str,
) -> anyhow::Result<()> {
    // --- Step 1: snapshot the commit tree into a temp directory ---------------

    let tmp = tempfile::TempDir::new()?;

    // `git archive --format=tar <sha>` on a bare repo outputs a tar stream.
    let archive = Command::new(git_bin)
        .args(["-C", repo_path, "archive", "--format=tar", new_sha])
        .output()
        .await?;

    if !archive.status.success() {
        anyhow::bail!(
            "git archive failed: {}",
            String::from_utf8_lossy(&archive.stderr)
        );
    }

    // Write tar bytes to a file then extract with the system tar binary.
    let archive_path = tmp.path().join("archive.tar");
    tokio::fs::write(&archive_path, &archive.stdout).await?;

    let tar_status = Command::new("tar")
        .args([
            "-x",
            "-C",
            tmp.path().to_str().unwrap_or(""),
            "-f",
            archive_path.to_str().unwrap_or(""),
        ])
        .status()
        .await?;

    if !tar_status.success() {
        anyhow::bail!("tar extraction failed (exit code: {:?})", tar_status.code());
    }

    // --- Step 2: run the Rust extractor (blocking, CPU-intensive) -------------

    let tmp_path = tmp.path().to_path_buf();
    let sha_str = new_sha.to_string();
    let repo_id_str = repo_id.to_string();

    let (nodes, edges) = tokio::task::spawn_blocking(move || {
        let extractor = RustExtractor;
        if !extractor.detect(&tmp_path) {
            // Not a Rust repository — nothing to extract.
            return (vec![], vec![]);
        }

        let result = extractor.extract(&tmp_path, &sha_str);
        for err in &result.errors {
            tracing::warn!(
                file = %err.file_path,
                "extraction warning: {}",
                err.message
            );
        }

        let repo_id = Id::new(repo_id_str);
        let mut nodes = result.nodes;
        let mut edges = result.edges;

        // Fix the placeholder repo_id on every emitted node and edge.
        for n in &mut nodes {
            n.repo_id = repo_id.clone();
        }
        for e in &mut edges {
            e.repo_id = repo_id.clone();
        }

        (nodes, edges)
    })
    .await?;

    if nodes.is_empty() && edges.is_empty() {
        info!(
            %repo_id,
            %new_sha,
            "graph extraction: no nodes found (non-Rust or empty repo)"
        );
        return Ok(());
    }

    let node_count = nodes.len();
    let edge_count = edges.len();
    let repo_id_parsed = Id::new(repo_id.to_string());

    // --- Step 3: clear stale data, then persist the new snapshot --------------

    graph_store.delete_nodes_by_repo(&repo_id_parsed).await?;
    graph_store.delete_edges_by_repo(&repo_id_parsed).await?;

    for node in nodes {
        graph_store.create_node(node).await?;
    }
    for edge in edges {
        graph_store.create_edge(edge).await?;
    }

    // --- Step 4: record an architectural delta --------------------------------

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let delta = ArchitecturalDelta {
        id: Id::new(Uuid::new_v4().to_string()),
        repo_id: repo_id_parsed,
        commit_sha: new_sha.to_string(),
        timestamp: now,
        agent_id: None,
        spec_ref: None,
        delta_json: serde_json::json!({
            "nodes_extracted": node_count,
            "edges_extracted": edge_count,
        })
        .to_string(),
    };
    graph_store.record_delta(delta).await?;

    info!(
        %repo_id,
        %new_sha,
        nodes = node_count,
        edges = edge_count,
        "knowledge graph extraction complete"
    );
    Ok(())
}
