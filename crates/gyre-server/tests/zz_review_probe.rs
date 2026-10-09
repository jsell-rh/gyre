//! Temporary review probe (task-156): does an identical re-PUT of a
//! meta-spec set (same entries, same order in the request) change the
//! stored set SHA — i.e., would the conformance sweep false-drift?

use std::sync::Arc;

#[tokio::test(flavor = "multi_thread")]
async fn repro_put_reput_sha_flip() {
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    let state = crate::mem::test_state();
    let app: Router = crate::api::api_router().with_state(state.clone());

    let ws_body = serde_json::json!({"name": "ws-probe", "slug": "ws-probe"});
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/workspaces")
                .header("authorization", "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(ws_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let ws_json: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap(),
    )
    .unwrap();
    let ws_id = ws_json["id"].as_str().unwrap().to_string();

    let put = |app: Router| {
        let body = serde_json::json!({
            "personas": {
                "backend": {"path": "backend-developer", "sha": "a1"},
                "frontend": {"path": "frontend-developer", "sha": "b2"},
                "sre": {"path": "sre-operator", "sha": "c3"}
            },
            "principles": [], "standards": [], "process": []
        });
        async move {
            app.oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-spec-set"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };

    let r1 = put(app.clone()).await;
    assert_eq!(r1.status(), StatusCode::OK);
    let sha1 =
        crate::compute_meta_spec_set_sha(state.meta_spec_sets.as_ref(), &gyre_common::Id::new(&ws_id))
            .await;

    let r2 = put(app).await;
    assert_eq!(r2.status(), StatusCode::OK);
    let sha2 =
        crate::compute_meta_spec_set_sha(state.meta_spec_sets.as_ref(), &gyre_common::Id::new(&ws_id))
            .await;

    println!("sha after first PUT:  {sha1}");
    println!("sha after re-PUT:     {sha2}");
    println!("identical re-PUT changed the stored set SHA: {}", sha1 != sha2);
    println!(
        "reconciliation tasks after identical re-PUT: {}",
        state
            .tasks
            .list()
            .await
            .unwrap()
            .iter()
            .filter(|t| t.labels.iter().any(|l| l == "meta-spec-reconciliation"))
            .count()
    );
}
