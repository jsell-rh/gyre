//! SQLite adapter for the knowledge graph (GraphPort).
//!
//! Persists graph_nodes, graph_edges, and graph_deltas to SQLite via Diesel.
//! Replaces the in-memory MemGraphStore so graph data survives server restarts.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::{
    graph::{
        ArchitecturalDelta, EdgeType, GraphEdge, GraphNode, NodeType, SpecConfidence, Visibility,
    },
    Id,
};
use gyre_ports::GraphPort;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::{graph_deltas, graph_edges, graph_nodes};

// ── Conversion helpers ────────────────────────────────────────────────────────

fn node_type_to_str(nt: &NodeType) -> &'static str {
    match nt {
        NodeType::Package => "package",
        NodeType::Module => "module",
        NodeType::Type => "type",
        NodeType::Interface => "interface",
        NodeType::Function => "function",
        NodeType::Endpoint => "endpoint",
        NodeType::Component => "component",
        NodeType::Table => "table",
        NodeType::Constant => "constant",
        NodeType::Field => "field",
        NodeType::Spec => "spec",
    }
}

fn str_to_node_type(s: &str) -> Result<NodeType> {
    match s {
        "package" => Ok(NodeType::Package),
        "module" => Ok(NodeType::Module),
        "type" => Ok(NodeType::Type),
        "interface" => Ok(NodeType::Interface),
        "function" => Ok(NodeType::Function),
        "endpoint" => Ok(NodeType::Endpoint),
        "component" => Ok(NodeType::Component),
        "table" => Ok(NodeType::Table),
        "constant" => Ok(NodeType::Constant),
        "field" => Ok(NodeType::Field),
        "spec" => Ok(NodeType::Spec),
        other => Err(anyhow!("unknown node type: {other}")),
    }
}

fn edge_type_to_str(et: &EdgeType) -> &'static str {
    match et {
        EdgeType::Contains => "contains",
        EdgeType::Implements => "implements",
        EdgeType::DependsOn => "depends_on",
        EdgeType::Calls => "calls",
        EdgeType::FieldOf => "field_of",
        EdgeType::Returns => "returns",
        EdgeType::RoutesTo => "routes_to",
        EdgeType::Renders => "renders",
        EdgeType::PersistsTo => "persists_to",
        EdgeType::GovernedBy => "governed_by",
        EdgeType::ProducedBy => "produced_by",
    }
}

fn str_to_edge_type(s: &str) -> Result<EdgeType> {
    match s {
        "contains" => Ok(EdgeType::Contains),
        "implements" => Ok(EdgeType::Implements),
        "depends_on" => Ok(EdgeType::DependsOn),
        "calls" => Ok(EdgeType::Calls),
        "field_of" => Ok(EdgeType::FieldOf),
        "returns" => Ok(EdgeType::Returns),
        "routes_to" => Ok(EdgeType::RoutesTo),
        "renders" => Ok(EdgeType::Renders),
        "persists_to" => Ok(EdgeType::PersistsTo),
        "governed_by" => Ok(EdgeType::GovernedBy),
        "produced_by" => Ok(EdgeType::ProducedBy),
        other => Err(anyhow!("unknown edge type: {other}")),
    }
}

fn visibility_to_str(v: &Visibility) -> &'static str {
    match v {
        Visibility::Public => "public",
        Visibility::PubCrate => "pub_crate",
        Visibility::Private => "private",
    }
}

fn str_to_visibility(s: &str) -> Result<Visibility> {
    match s {
        "public" => Ok(Visibility::Public),
        "pub_crate" => Ok(Visibility::PubCrate),
        "private" => Ok(Visibility::Private),
        other => Err(anyhow!("unknown visibility: {other}")),
    }
}

fn spec_confidence_to_str(sc: &SpecConfidence) -> &'static str {
    match sc {
        SpecConfidence::High => "high",
        SpecConfidence::Medium => "medium",
        SpecConfidence::Low => "low",
        SpecConfidence::None => "none",
    }
}

fn str_to_spec_confidence(s: &str) -> Result<SpecConfidence> {
    match s {
        "high" => Ok(SpecConfidence::High),
        "medium" => Ok(SpecConfidence::Medium),
        "low" => Ok(SpecConfidence::Low),
        "none" => Ok(SpecConfidence::None),
        other => Err(anyhow!("unknown spec confidence: {other}")),
    }
}

// ── Diesel row types ──────────────────────────────────────────────────────────

#[derive(Queryable, Selectable)]
#[diesel(table_name = graph_nodes)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct GraphNodeRow {
    id: String,
    repo_id: String,
    node_type: String,
    name: String,
    qualified_name: String,
    file_path: String,
    line_start: i32,
    line_end: i32,
    visibility: String,
    doc_comment: Option<String>,
    spec_path: Option<String>,
    spec_confidence: String,
    last_modified_sha: String,
    last_modified_by: Option<String>,
    last_modified_at: i64,
    created_sha: String,
    created_at: i64,
    complexity: Option<i32>,
    churn_count_30d: i32,
    test_coverage: Option<f64>,
    first_seen_at: i64,
    last_seen_at: i64,
    deleted_at: Option<i64>,
    test_node: bool,
}

impl GraphNodeRow {
    fn into_node(self) -> Result<GraphNode> {
        Ok(GraphNode {
            id: Id::new(self.id),
            repo_id: Id::new(self.repo_id),
            node_type: str_to_node_type(&self.node_type)?,
            name: self.name,
            qualified_name: self.qualified_name,
            file_path: self.file_path,
            line_start: self.line_start as u32,
            line_end: self.line_end as u32,
            visibility: str_to_visibility(&self.visibility)?,
            doc_comment: self.doc_comment,
            spec_path: self.spec_path,
            spec_confidence: str_to_spec_confidence(&self.spec_confidence)?,
            last_modified_sha: self.last_modified_sha,
            last_modified_by: self.last_modified_by.map(Id::new),
            last_modified_at: self.last_modified_at as u64,
            created_sha: self.created_sha,
            created_at: self.created_at as u64,
            complexity: self.complexity.map(|c| c as u32),
            churn_count_30d: self.churn_count_30d as u32,
            test_coverage: self.test_coverage,
            first_seen_at: self.first_seen_at as u64,
            last_seen_at: self.last_seen_at as u64,
            deleted_at: self.deleted_at.map(|t| t as u64),
            test_node: self.test_node,
            spec_approved_at: None,
            milestone_completed_at: None,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = graph_nodes)]
struct NewGraphNodeRow<'a> {
    id: &'a str,
    repo_id: &'a str,
    node_type: &'a str,
    name: &'a str,
    qualified_name: &'a str,
    file_path: &'a str,
    line_start: i32,
    line_end: i32,
    visibility: &'a str,
    doc_comment: Option<&'a str>,
    spec_path: Option<&'a str>,
    spec_confidence: &'a str,
    last_modified_sha: &'a str,
    last_modified_by: Option<&'a str>,
    last_modified_at: i64,
    created_sha: &'a str,
    created_at: i64,
    complexity: Option<i32>,
    churn_count_30d: i32,
    test_coverage: Option<f64>,
    first_seen_at: i64,
    last_seen_at: i64,
    deleted_at: Option<i64>,
    test_node: bool,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = graph_edges)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct GraphEdgeRow {
    id: String,
    repo_id: String,
    source_id: String,
    target_id: String,
    edge_type: String,
    metadata: Option<String>,
    first_seen_at: i64,
    last_seen_at: i64,
    deleted_at: Option<i64>,
}

impl GraphEdgeRow {
    fn into_edge(self) -> Result<GraphEdge> {
        Ok(GraphEdge {
            id: Id::new(self.id),
            repo_id: Id::new(self.repo_id),
            source_id: Id::new(self.source_id),
            target_id: Id::new(self.target_id),
            edge_type: str_to_edge_type(&self.edge_type)?,
            metadata: self.metadata,
            first_seen_at: self.first_seen_at as u64,
            last_seen_at: self.last_seen_at as u64,
            deleted_at: self.deleted_at.map(|t| t as u64),
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = graph_edges)]
struct NewGraphEdgeRow<'a> {
    id: &'a str,
    repo_id: &'a str,
    source_id: &'a str,
    target_id: &'a str,
    edge_type: &'a str,
    metadata: Option<&'a str>,
    first_seen_at: i64,
    last_seen_at: i64,
    deleted_at: Option<i64>,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = graph_deltas)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct GraphDeltaRow {
    id: String,
    repo_id: String,
    commit_sha: String,
    timestamp: i64,
    agent_id: Option<String>,
    spec_ref: Option<String>,
    delta_json: String,
}

impl GraphDeltaRow {
    fn into_delta(self) -> ArchitecturalDelta {
        ArchitecturalDelta {
            id: Id::new(self.id),
            repo_id: Id::new(self.repo_id),
            commit_sha: self.commit_sha,
            timestamp: self.timestamp as u64,
            agent_id: self.agent_id.map(Id::new),
            spec_ref: self.spec_ref,
            delta_json: self.delta_json,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = graph_deltas)]
struct NewGraphDeltaRow<'a> {
    id: &'a str,
    repo_id: &'a str,
    commit_sha: &'a str,
    timestamp: i64,
    agent_id: Option<&'a str>,
    spec_ref: Option<&'a str>,
    delta_json: &'a str,
}

// ── GraphPort implementation ──────────────────────────────────────────────────

#[async_trait]
impl GraphPort for SqliteStorage {
    async fn create_node(&self, node: GraphNode) -> Result<GraphNode> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<GraphNode> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewGraphNodeRow {
                id: node.id.as_str(),
                repo_id: node.repo_id.as_str(),
                node_type: node_type_to_str(&node.node_type),
                name: &node.name,
                qualified_name: &node.qualified_name,
                file_path: &node.file_path,
                line_start: node.line_start as i32,
                line_end: node.line_end as i32,
                visibility: visibility_to_str(&node.visibility),
                doc_comment: node.doc_comment.as_deref(),
                spec_path: node.spec_path.as_deref(),
                spec_confidence: spec_confidence_to_str(&node.spec_confidence),
                last_modified_sha: &node.last_modified_sha,
                last_modified_by: node.last_modified_by.as_ref().map(|id| id.as_str()),
                last_modified_at: node.last_modified_at as i64,
                created_sha: &node.created_sha,
                created_at: node.created_at as i64,
                complexity: node.complexity.map(|c| c as i32),
                churn_count_30d: node.churn_count_30d as i32,
                test_coverage: node.test_coverage,
                first_seen_at: node.first_seen_at as i64,
                last_seen_at: node.last_seen_at as i64,
                deleted_at: node.deleted_at.map(|t| t as i64),
                test_node: node.test_node,
            };
            diesel::insert_into(graph_nodes::table)
                .values(&row)
                .on_conflict(graph_nodes::id)
                .do_update()
                .set((
                    graph_nodes::node_type.eq(row.node_type),
                    graph_nodes::name.eq(row.name),
                    graph_nodes::qualified_name.eq(row.qualified_name),
                    graph_nodes::file_path.eq(row.file_path),
                    graph_nodes::line_start.eq(row.line_start),
                    graph_nodes::line_end.eq(row.line_end),
                    graph_nodes::visibility.eq(row.visibility),
                    graph_nodes::doc_comment.eq(row.doc_comment),
                    graph_nodes::spec_path.eq(row.spec_path),
                    graph_nodes::spec_confidence.eq(row.spec_confidence),
                    graph_nodes::last_modified_sha.eq(row.last_modified_sha),
                    graph_nodes::last_modified_by.eq(row.last_modified_by),
                    graph_nodes::last_modified_at.eq(row.last_modified_at),
                    graph_nodes::complexity.eq(row.complexity),
                    graph_nodes::churn_count_30d.eq(row.churn_count_30d),
                    graph_nodes::test_coverage.eq(row.test_coverage),
                    // first_seen_at is immutable — never updated on conflict.
                    graph_nodes::last_seen_at.eq(row.last_seen_at),
                    // Clear deleted_at when a node reappears after removal.
                    graph_nodes::deleted_at.eq(row.deleted_at),
                    graph_nodes::test_node.eq(row.test_node),
                ))
                .execute(&mut *conn)
                .context("insert graph node")?;
            Ok(node)
        })
        .await?
    }

    async fn get_node(&self, id: &Id) -> Result<Option<GraphNode>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<GraphNode>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = graph_nodes::table
                .find(id.as_str())
                .first::<GraphNodeRow>(&mut *conn)
                .optional()
                .context("get graph node")?;
            result.map(GraphNodeRow::into_node).transpose()
        })
        .await?
    }

    async fn list_nodes(
        &self,
        repo_id: &Id,
        node_type: Option<NodeType>,
    ) -> Result<Vec<GraphNode>> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.clone();
        let nt_str = node_type.as_ref().map(node_type_to_str).map(str::to_owned);
        tokio::task::spawn_blocking(move || -> Result<Vec<GraphNode>> {
            let mut conn = pool.get().context("get db connection")?;
            // Always filter out soft-deleted nodes (deleted_at IS NULL = active).
            let rows = if let Some(nt) = nt_str {
                graph_nodes::table
                    .filter(graph_nodes::repo_id.eq(repo_id.as_str()))
                    .filter(graph_nodes::node_type.eq(&nt))
                    .filter(graph_nodes::deleted_at.is_null())
                    .load::<GraphNodeRow>(&mut *conn)
                    .context("list graph nodes by type")?
            } else {
                graph_nodes::table
                    .filter(graph_nodes::repo_id.eq(repo_id.as_str()))
                    .filter(graph_nodes::deleted_at.is_null())
                    .load::<GraphNodeRow>(&mut *conn)
                    .context("list graph nodes")?
            };
            rows.into_iter().map(GraphNodeRow::into_node).collect()
        })
        .await?
    }

    async fn create_edge(&self, edge: GraphEdge) -> Result<GraphEdge> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<GraphEdge> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewGraphEdgeRow {
                id: edge.id.as_str(),
                repo_id: edge.repo_id.as_str(),
                source_id: edge.source_id.as_str(),
                target_id: edge.target_id.as_str(),
                edge_type: edge_type_to_str(&edge.edge_type),
                metadata: edge.metadata.as_deref(),
                first_seen_at: edge.first_seen_at as i64,
                last_seen_at: edge.last_seen_at as i64,
                deleted_at: edge.deleted_at.map(|t| t as i64),
            };
            diesel::insert_into(graph_edges::table)
                .values(&row)
                .on_conflict(graph_edges::id)
                .do_update()
                .set((
                    graph_edges::edge_type.eq(row.edge_type),
                    graph_edges::metadata.eq(row.metadata),
                    // first_seen_at is immutable — never updated on conflict.
                    graph_edges::last_seen_at.eq(row.last_seen_at),
                    graph_edges::deleted_at.eq(row.deleted_at),
                ))
                .execute(&mut *conn)
                .context("insert graph edge")?;
            Ok(edge)
        })
        .await?
    }

    async fn list_edges(
        &self,
        repo_id: &Id,
        edge_type: Option<EdgeType>,
    ) -> Result<Vec<GraphEdge>> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.clone();
        let et_str = edge_type.as_ref().map(edge_type_to_str).map(str::to_owned);
        tokio::task::spawn_blocking(move || -> Result<Vec<GraphEdge>> {
            let mut conn = pool.get().context("get db connection")?;
            // Always filter out soft-deleted edges.
            let rows = if let Some(et) = et_str {
                graph_edges::table
                    .filter(graph_edges::repo_id.eq(repo_id.as_str()))
                    .filter(graph_edges::edge_type.eq(&et))
                    .filter(graph_edges::deleted_at.is_null())
                    .load::<GraphEdgeRow>(&mut *conn)
                    .context("list graph edges by type")?
            } else {
                graph_edges::table
                    .filter(graph_edges::repo_id.eq(repo_id.as_str()))
                    .filter(graph_edges::deleted_at.is_null())
                    .load::<GraphEdgeRow>(&mut *conn)
                    .context("list graph edges")?
            };
            rows.into_iter().map(GraphEdgeRow::into_edge).collect()
        })
        .await?
    }

    async fn delete_node(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            diesel::update(graph_nodes::table.find(id.as_str()))
                .set(graph_nodes::deleted_at.eq(now))
                .execute(&mut *conn)
                .context("soft-delete graph node")?;
            Ok(())
        })
        .await?
    }

    async fn delete_edge(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            diesel::update(graph_edges::table.find(id.as_str()))
                .set(graph_edges::deleted_at.eq(now))
                .execute(&mut *conn)
                .context("soft-delete graph edge")?;
            Ok(())
        })
        .await?
    }

    async fn delete_nodes_by_repo(&self, repo_id: &Id) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.clone();
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let count = diesel::delete(
                graph_nodes::table.filter(graph_nodes::repo_id.eq(repo_id.as_str())),
            )
            .execute(&mut *conn)
            .context("delete graph nodes by repo")?;
            Ok(count as u64)
        })
        .await?
    }

    async fn delete_edges_by_repo(&self, repo_id: &Id) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.clone();
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let count = diesel::delete(
                graph_edges::table.filter(graph_edges::repo_id.eq(repo_id.as_str())),
            )
            .execute(&mut *conn)
            .context("delete graph edges by repo")?;
            Ok(count as u64)
        })
        .await?
    }

    async fn record_delta(&self, delta: ArchitecturalDelta) -> Result<ArchitecturalDelta> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<ArchitecturalDelta> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewGraphDeltaRow {
                id: delta.id.as_str(),
                repo_id: delta.repo_id.as_str(),
                commit_sha: &delta.commit_sha,
                timestamp: delta.timestamp as i64,
                agent_id: delta.agent_id.as_ref().map(|id| id.as_str()),
                spec_ref: delta.spec_ref.as_deref(),
                delta_json: &delta.delta_json,
            };
            diesel::insert_into(graph_deltas::table)
                .values(&row)
                .on_conflict(graph_deltas::id)
                .do_nothing()
                .execute(&mut *conn)
                .context("insert graph delta")?;
            Ok(delta)
        })
        .await?
    }

    async fn list_deltas(
        &self,
        repo_id: &Id,
        since: Option<u64>,
        until: Option<u64>,
    ) -> Result<Vec<ArchitecturalDelta>> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<ArchitecturalDelta>> {
            let mut conn = pool.get().context("get db connection")?;
            let mut query = graph_deltas::table
                .filter(graph_deltas::repo_id.eq(repo_id.as_str()))
                .into_boxed();
            if let Some(s) = since {
                query = query.filter(graph_deltas::timestamp.ge(s as i64));
            }
            if let Some(u) = until {
                query = query.filter(graph_deltas::timestamp.le(u as i64));
            }
            let rows = query
                .order(graph_deltas::timestamp.asc())
                .load::<GraphDeltaRow>(&mut *conn)
                .context("list graph deltas")?;
            Ok(rows.into_iter().map(GraphDeltaRow::into_delta).collect())
        })
        .await?
    }

    async fn get_nodes_by_spec(&self, repo_id: &Id, spec_path: &str) -> Result<Vec<GraphNode>> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.clone();
        let spec_path = spec_path.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<GraphNode>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = graph_nodes::table
                .filter(graph_nodes::repo_id.eq(repo_id.as_str()))
                .filter(graph_nodes::spec_path.eq(&spec_path))
                .filter(graph_nodes::deleted_at.is_null())
                .load::<GraphNodeRow>(&mut *conn)
                .context("get nodes by spec")?;
            rows.into_iter().map(GraphNodeRow::into_node).collect()
        })
        .await?
    }

    async fn link_node_to_spec(
        &self,
        node_id: &Id,
        spec_path: &str,
        confidence: SpecConfidence,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let node_id = node_id.clone();
        let spec_path = spec_path.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(graph_nodes::table.find(node_id.as_str()))
                .set((
                    graph_nodes::spec_path.eq(&spec_path),
                    graph_nodes::spec_confidence.eq(spec_confidence_to_str(&confidence)),
                ))
                .execute(&mut *conn)
                .context("link node to spec")?;
            Ok(())
        })
        .await?
    }

    async fn list_edges_for_node(&self, node_id: &Id) -> Result<Vec<GraphEdge>> {
        let pool = Arc::clone(&self.pool);
        let node_id = node_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<GraphEdge>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = graph_edges::table
                .filter(
                    graph_edges::source_id
                        .eq(node_id.as_str())
                        .or(graph_edges::target_id.eq(node_id.as_str())),
                )
                .filter(graph_edges::deleted_at.is_null())
                .load::<GraphEdgeRow>(&mut *conn)
                .context("list edges for node")?;
            rows.into_iter().map(GraphEdgeRow::into_edge).collect()
        })
        .await?
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::graph::{EdgeType, NodeType, SpecConfidence, Visibility};
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    fn make_node(id: &str, repo_id: &str) -> GraphNode {
        GraphNode {
            id: Id::new(id),
            repo_id: Id::new(repo_id),
            node_type: NodeType::Function,
            name: format!("fn_{id}"),
            qualified_name: format!("crate::fn_{id}"),
            file_path: "src/lib.rs".to_string(),
            line_start: 10,
            line_end: 20,
            visibility: Visibility::Public,
            doc_comment: None,
            spec_path: None,
            spec_confidence: SpecConfidence::None,
            last_modified_sha: "abc123".to_string(),
            last_modified_by: None,
            last_modified_at: 1000,
            created_sha: "abc123".to_string(),
            created_at: 1000,
            complexity: None,
            churn_count_30d: 0,
            test_coverage: None,
            first_seen_at: 1000,
            last_seen_at: 1000,
            deleted_at: None,
            test_node: false,
            spec_approved_at: None,
            milestone_completed_at: None,
        }
    }

    fn make_edge(id: &str, repo_id: &str, source: &str, target: &str) -> GraphEdge {
        GraphEdge {
            id: Id::new(id),
            repo_id: Id::new(repo_id),
            source_id: Id::new(source),
            target_id: Id::new(target),
            edge_type: EdgeType::Calls,
            metadata: None,
            first_seen_at: 1000,
            last_seen_at: 1000,
            deleted_at: None,
        }
    }

    fn make_delta(id: &str, repo_id: &str, ts: u64) -> ArchitecturalDelta {
        ArchitecturalDelta {
            id: Id::new(id),
            repo_id: Id::new(repo_id),
            commit_sha: "deadbeef".to_string(),
            timestamp: ts,
            agent_id: None,
            spec_ref: None,
            delta_json: r#"{"nodes_extracted":1}"#.to_string(),
        }
    }

    #[tokio::test]
    async fn store_and_retrieve_node() {
        let (_tmp, s) = setup();
        let node = make_node("n1", "repo1");
        let created = GraphPort::create_node(&s, node.clone()).await.unwrap();
        assert_eq!(created.id, node.id);

        let found = GraphPort::get_node(&s, &node.id).await.unwrap().unwrap();
        assert_eq!(found.name, "fn_n1");
        assert_eq!(found.qualified_name, "crate::fn_n1");
        assert_eq!(found.node_type, NodeType::Function);
        assert_eq!(found.visibility, Visibility::Public);
    }

    #[tokio::test]
    async fn get_node_missing_returns_none() {
        let (_tmp, s) = setup();
        let result = GraphPort::get_node(&s, &Id::new("nonexistent"))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn store_and_query_nodes() {
        let (_tmp, s) = setup();
        let repo = "repo1";
        GraphPort::create_node(&s, make_node("n1", repo))
            .await
            .unwrap();
        GraphPort::create_node(&s, make_node("n2", repo))
            .await
            .unwrap();
        GraphPort::create_node(&s, {
            let mut n = make_node("n3", repo);
            n.node_type = NodeType::Type;
            n
        })
        .await
        .unwrap();

        let all = GraphPort::list_nodes(&s, &Id::new(repo), None)
            .await
            .unwrap();
        assert_eq!(all.len(), 3);

        let fns = GraphPort::list_nodes(&s, &Id::new(repo), Some(NodeType::Function))
            .await
            .unwrap();
        assert_eq!(fns.len(), 2);

        let types = GraphPort::list_nodes(&s, &Id::new(repo), Some(NodeType::Type))
            .await
            .unwrap();
        assert_eq!(types.len(), 1);
    }

    #[tokio::test]
    async fn store_and_query_edges() {
        let (_tmp, s) = setup();
        let repo = "repo1";
        GraphPort::create_node(&s, make_node("n1", repo))
            .await
            .unwrap();
        GraphPort::create_node(&s, make_node("n2", repo))
            .await
            .unwrap();
        GraphPort::create_node(&s, make_node("n3", repo))
            .await
            .unwrap();

        GraphPort::create_edge(&s, make_edge("e1", repo, "n1", "n2"))
            .await
            .unwrap();
        GraphPort::create_edge(&s, {
            let mut e = make_edge("e2", repo, "n2", "n3");
            e.edge_type = EdgeType::Contains;
            e
        })
        .await
        .unwrap();

        let all = GraphPort::list_edges(&s, &Id::new(repo), None)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);

        let calls = GraphPort::list_edges(&s, &Id::new(repo), Some(EdgeType::Calls))
            .await
            .unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].source_id, Id::new("n1"));
    }

    #[tokio::test]
    async fn store_delta() {
        let (_tmp, s) = setup();
        let repo = "repo1";
        let d = make_delta("d1", repo, 5000);
        let stored = GraphPort::record_delta(&s, d).await.unwrap();
        assert_eq!(stored.id, Id::new("d1"));

        let deltas = GraphPort::list_deltas(&s, &Id::new(repo), None, None)
            .await
            .unwrap();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].commit_sha, "deadbeef");
    }

    #[tokio::test]
    async fn delete_nodes_and_edges_by_repo() {
        let (_tmp, s) = setup();
        let repo = "repo1";
        GraphPort::create_node(&s, make_node("n1", repo))
            .await
            .unwrap();
        GraphPort::create_node(&s, make_node("n2", repo))
            .await
            .unwrap();
        GraphPort::create_edge(&s, make_edge("e1", repo, "n1", "n2"))
            .await
            .unwrap();

        let deleted = GraphPort::delete_nodes_by_repo(&s, &Id::new(repo))
            .await
            .unwrap();
        assert_eq!(deleted, 2);

        let remaining = GraphPort::list_nodes(&s, &Id::new(repo), None)
            .await
            .unwrap();
        assert!(remaining.is_empty());

        let del_edges = GraphPort::delete_edges_by_repo(&s, &Id::new(repo))
            .await
            .unwrap();
        assert_eq!(del_edges, 1);
    }

    #[tokio::test]
    async fn spec_link_and_query() {
        let (_tmp, s) = setup();
        let repo = "repo1";
        let mut node = make_node("n1", repo);
        node.spec_path = Some("specs/foo.md".to_string());
        GraphPort::create_node(&s, node).await.unwrap();
        GraphPort::create_node(&s, make_node("n2", repo))
            .await
            .unwrap();

        let by_spec = GraphPort::get_nodes_by_spec(&s, &Id::new(repo), "specs/foo.md")
            .await
            .unwrap();
        assert_eq!(by_spec.len(), 1);
        assert_eq!(by_spec[0].id, Id::new("n1"));

        GraphPort::link_node_to_spec(&s, &Id::new("n2"), "specs/bar.md", SpecConfidence::High)
            .await
            .unwrap();
        let by_bar = GraphPort::get_nodes_by_spec(&s, &Id::new(repo), "specs/bar.md")
            .await
            .unwrap();
        assert_eq!(by_bar.len(), 1);
        assert_eq!(by_bar[0].spec_confidence, SpecConfidence::High);
    }

    #[tokio::test]
    async fn list_edges_for_node() {
        let (_tmp, s) = setup();
        let repo = "repo1";
        GraphPort::create_node(&s, make_node("n1", repo))
            .await
            .unwrap();
        GraphPort::create_node(&s, make_node("n2", repo))
            .await
            .unwrap();
        GraphPort::create_node(&s, make_node("n3", repo))
            .await
            .unwrap();

        GraphPort::create_edge(&s, make_edge("e1", repo, "n1", "n2"))
            .await
            .unwrap();
        GraphPort::create_edge(&s, make_edge("e2", repo, "n3", "n1"))
            .await
            .unwrap();
        // unrelated edge
        GraphPort::create_edge(&s, make_edge("e3", repo, "n2", "n3"))
            .await
            .unwrap();

        let edges = GraphPort::list_edges_for_node(&s, &Id::new("n1"))
            .await
            .unwrap();
        // n1 is source of e1 and target of e2
        assert_eq!(edges.len(), 2);
        let ids: Vec<String> = edges.iter().map(|e| e.id.as_str().to_string()).collect();
        assert!(ids.contains(&"e1".to_string()));
        assert!(ids.contains(&"e2".to_string()));
    }

    #[tokio::test]
    async fn delete_single_node() {
        let (_tmp, s) = setup();
        let repo = "repo1";
        GraphPort::create_node(&s, make_node("n1", repo))
            .await
            .unwrap();
        GraphPort::create_node(&s, make_node("n2", repo))
            .await
            .unwrap();

        GraphPort::delete_node(&s, &Id::new("n1")).await.unwrap();

        let remaining = GraphPort::list_nodes(&s, &Id::new(repo), None)
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, Id::new("n2"));
    }

    #[tokio::test]
    async fn delete_single_edge() {
        let (_tmp, s) = setup();
        let repo = "repo1";
        GraphPort::create_node(&s, make_node("n1", repo))
            .await
            .unwrap();
        GraphPort::create_node(&s, make_node("n2", repo))
            .await
            .unwrap();
        GraphPort::create_edge(&s, make_edge("e1", repo, "n1", "n2"))
            .await
            .unwrap();
        GraphPort::create_edge(&s, make_edge("e2", repo, "n2", "n1"))
            .await
            .unwrap();

        GraphPort::delete_edge(&s, &Id::new("e1")).await.unwrap();

        let remaining = GraphPort::list_edges(&s, &Id::new(repo), None)
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, Id::new("e2"));
    }

    #[tokio::test]
    async fn delta_time_range_filter() {
        let (_tmp, s) = setup();
        let repo = "repo1";
        GraphPort::record_delta(&s, make_delta("d1", repo, 100))
            .await
            .unwrap();
        GraphPort::record_delta(&s, make_delta("d2", repo, 200))
            .await
            .unwrap();
        GraphPort::record_delta(&s, make_delta("d3", repo, 300))
            .await
            .unwrap();

        let since = GraphPort::list_deltas(&s, &Id::new(repo), Some(150), None)
            .await
            .unwrap();
        assert_eq!(since.len(), 2);

        let until = GraphPort::list_deltas(&s, &Id::new(repo), None, Some(200))
            .await
            .unwrap();
        assert_eq!(until.len(), 2);

        let range = GraphPort::list_deltas(&s, &Id::new(repo), Some(150), Some(250))
            .await
            .unwrap();
        assert_eq!(range.len(), 1);
        assert_eq!(range[0].id, Id::new("d2"));
    }

    #[tokio::test]
    async fn node_with_all_fields() {
        let (_tmp, s) = setup();
        let node = GraphNode {
            id: Id::new("full"),
            repo_id: Id::new("repo1"),
            node_type: NodeType::Type,
            name: "MyStruct".to_string(),
            qualified_name: "crate::MyStruct".to_string(),
            file_path: "src/types.rs".to_string(),
            line_start: 5,
            line_end: 50,
            visibility: Visibility::PubCrate,
            doc_comment: Some("A well-documented struct".to_string()),
            spec_path: Some("specs/types.md".to_string()),
            spec_confidence: SpecConfidence::Medium,
            last_modified_sha: "cafebabe".to_string(),
            last_modified_by: Some(Id::new("agent-42")),
            last_modified_at: 9999,
            created_sha: "deadbeef".to_string(),
            created_at: 1234,
            complexity: Some(7),
            churn_count_30d: 3,
            test_coverage: Some(0.85),
            first_seen_at: 100,
            last_seen_at: 9999,
            deleted_at: None,
            test_node: false,
            spec_approved_at: None,
            milestone_completed_at: None,
        };
        GraphPort::create_node(&s, node.clone()).await.unwrap();
        let found = GraphPort::get_node(&s, &node.id).await.unwrap().unwrap();
        assert_eq!(
            found.doc_comment.as_deref(),
            Some("A well-documented struct")
        );
        assert_eq!(found.spec_confidence, SpecConfidence::Medium);
        assert_eq!(found.complexity, Some(7));
        assert_eq!(found.churn_count_30d, 3);
        assert!((found.test_coverage.unwrap() - 0.85).abs() < 1e-9);
        assert_eq!(found.last_modified_by, Some(Id::new("agent-42")));
    }
}
