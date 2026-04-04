//! Executable spec assertion endpoint (system-explorer spec S9).
//!
//! POST /api/v1/repos/:id/spec-assertions/check
//!
//! Parses `<!-- gyre:assert ... -->` comments from spec markdown content,
//! evaluates them against the repo's knowledge graph, and returns results.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::spec_assertions;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::error::ApiError;
use crate::AppState;

/// Request body for checking spec assertions.
#[derive(Deserialize)]
pub struct CheckAssertionsRequest {
    /// Path to the spec file (for context/logging; not required for evaluation).
    pub spec_path: String,
    /// Markdown content of the spec containing `<!-- gyre:assert ... -->` comments.
    pub content: String,
}

/// A single assertion result in the response.
#[derive(Serialize)]
pub struct AssertionResultResponse {
    /// 1-based line number where the assertion was found.
    pub line: usize,
    /// The raw assertion text.
    pub assertion_text: String,
    /// Whether the assertion passed.
    pub passed: bool,
    /// Human-readable explanation of the result.
    pub explanation: String,
}

/// Response body for the spec assertions check endpoint.
#[derive(Serialize)]
pub struct CheckAssertionsResponse {
    pub assertions: Vec<AssertionResultResponse>,
}

/// POST /api/v1/repos/:id/spec-assertions/check
///
/// Parses all `<!-- gyre:assert ... -->` comments from the provided spec content,
/// evaluates each assertion against the repo's knowledge graph (nodes + edges),
/// and returns pass/fail results with explanations.
pub async fn check_spec_assertions(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<CheckAssertionsRequest>,
) -> Result<(StatusCode, Json<CheckAssertionsResponse>), ApiError> {
    // Verify repo exists.
    state
        .repos
        .find_by_id(&Id::new(&id))
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;

    let repo_id = Id::new(&id);

    // Load the knowledge graph for this repo.
    let nodes = state
        .graph_store
        .list_nodes(&repo_id, None)
        .await
        .map_err(ApiError::Internal)?;
    let edges = state
        .graph_store
        .list_edges(&repo_id, None)
        .await
        .map_err(ApiError::Internal)?;

    // Parse and evaluate assertions.
    let parsed = spec_assertions::parse_assertions(&req.content);
    let results = spec_assertions::evaluate_assertions(&parsed, &nodes, &edges);

    let assertions = results
        .into_iter()
        .map(|r| AssertionResultResponse {
            line: r.line,
            assertion_text: r.assertion_text,
            passed: r.passed,
            explanation: r.explanation,
        })
        .collect();

    Ok((StatusCode::OK, Json(CheckAssertionsResponse { assertions })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use axum::body::Body;
    use gyre_common::graph::{GraphNode, NodeType, SpecConfidence, Visibility};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn auth() -> &'static str {
        "Bearer test-token"
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn make_node(id: &str, name: &str, node_type: NodeType, repo_id: &str) -> GraphNode {
        GraphNode {
            id: Id::new(id),
            repo_id: Id::new(repo_id),
            node_type,
            name: name.to_string(),
            qualified_name: name.to_string(),
            file_path: "src/lib.rs".to_string(),
            line_start: 1,
            line_end: 10,
            visibility: Visibility::Public,
            doc_comment: None,
            spec_path: None,
            spec_confidence: SpecConfidence::None,
            last_modified_sha: "abc".to_string(),
            last_modified_by: None,
            last_modified_at: 0,
            created_sha: "abc".to_string(),
            created_at: 0,
            complexity: None,
            churn_count_30d: 0,
            test_coverage: None,
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
            test_node: false,
        }
    }

    #[tokio::test]
    async fn check_assertions_returns_results() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        // Create a repo via HTTP (shares the same state).
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "name": "assert-repo",
                            "workspace_id": "ws-1",
                            "tenant_id": "tenant-1",
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(repo_resp.status(), StatusCode::CREATED);
        let repo_json = body_json(repo_resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        // Seed graph nodes via the shared state.
        let domain_node = make_node("nd", "gyre-domain", NodeType::Module, &repo_id);
        let adapters_node = make_node("na", "gyre-adapters", NodeType::Module, &repo_id);

        state.graph_store.create_node(domain_node).await.unwrap();
        state.graph_store.create_node(adapters_node).await.unwrap();

        // Check assertions.
        let check_body = serde_json::json!({
            "spec_path": "specs/system/architecture.md",
            "content": "# Architecture\n<!-- gyre:assert module(\"gyre-domain\") NOT depends_on(\"gyre-adapters\") -->\n"
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/spec-assertions/check"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&check_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let assertions = json["assertions"].as_array().unwrap();
        assert_eq!(assertions.len(), 1);
        assert!(assertions[0]["passed"].as_bool().unwrap());
        assert_eq!(assertions[0]["line"].as_u64().unwrap(), 2);
    }

    #[tokio::test]
    async fn check_assertions_repo_not_found() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state);

        let check_body = serde_json::json!({
            "spec_path": "specs/test.md",
            "content": "<!-- gyre:assert module(\"x\") NOT depends_on(\"y\") -->"
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/nonexistent/spec-assertions/check")
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&check_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn check_assertions_no_assertions_returns_empty() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state);

        // Create a repo.
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "name": "empty-assert-repo",
                            "workspace_id": "ws-1",
                            "tenant_id": "tenant-1",
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let repo_json = body_json(repo_resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        let check_body = serde_json::json!({
            "spec_path": "specs/test.md",
            "content": "# Just a normal spec\nNo assertions here."
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/spec-assertions/check"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&check_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let assertions = json["assertions"].as_array().unwrap();
        assert!(assertions.is_empty());
    }
}
