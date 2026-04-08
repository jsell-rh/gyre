//! HTTP middleware: request tracing, rate limiting, and last-seen tracking.
//!
//! For every request:
//! - Generates a UUID request ID and attaches it to the tracing span.
//! - Records method, path, status_code, duration_ms on the span.
//! - Adds `X-Request-Id` response header.
//! - Updates Prometheus request counter and duration histogram.
//! - Enforces per-second rate limit, returning 429 when exceeded.
//!
//! `last_seen_middleware` additionally:
//! - Tracks per-user, per-workspace last-seen timestamps (HSI §1).
//! - Debounced to at most one upsert per 60 seconds per (user, workspace) pair.
//! - Fires upserts as fire-and-forget `tokio::spawn` tasks.
//! - Sets `X-Gyre-Last-Seen` response header with the current epoch seconds.

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::Instrument;

use crate::AppState;

pub async fn request_tracing(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    let method = req.method().to_string();
    // Normalise path to avoid high cardinality in metrics (strip query string).
    let path = req.uri().path().to_string();

    let span = tracing::info_span!(
        "http_request",
        method = %method,
        path = %path,
        request_id = %request_id,
        status_code = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    );

    async move {
        let start = std::time::Instant::now();
        let mut response = next.run(req).await;
        let elapsed = start.elapsed();
        let duration_secs = elapsed.as_secs_f64();
        let status = response.status().as_u16().to_string();

        tracing::Span::current().record("status_code", &status as &str);
        tracing::Span::current().record("duration_ms", elapsed.as_millis());

        // Update Prometheus metrics.
        state
            .metrics
            .http_requests_total
            .with_label_values(&[&method, &path, &status])
            .inc();
        state
            .metrics
            .http_request_duration_seconds
            .with_label_values(&[&method, &path])
            .observe(duration_secs);

        // Attach request ID to the response.
        if let Ok(val) = request_id.parse() {
            response.headers_mut().insert("x-request-id", val);
        }

        response
    }
    .instrument(span)
    .await
}

/// Rate-limiting middleware. Returns 429 when the token bucket is exhausted.
pub async fn rate_limit_middleware(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if state.rate_limiter.try_acquire() {
        next.run(req).await
    } else {
        (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response()
    }
}

/// Last-seen tracking middleware (HSI §1).
///
/// Runs after `require_auth_middleware` (request is already authenticated).
/// Extracts user_id via `AuthenticatedAgent` extractor and workspace_id from
/// the request path (`/api/v1/workspaces/:id/...`). When both are present:
/// - Debounces to at most one upsert per 60 seconds per (user_id, workspace_id) pair.
/// - Fires the upsert as a fire-and-forget `tokio::spawn` task (non-blocking).
/// - Sets `X-Gyre-Last-Seen: <epoch_seconds>` on the response.
///
/// Silently skips when user_id or workspace_id cannot be determined (system tokens,
/// agent JWTs, non-workspace routes).
pub async fn last_seen_middleware(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    use crate::auth::AuthenticatedAgent;
    use axum::extract::FromRequestParts;

    // Extract workspace_id from path: /api/v1/workspaces/<id>/...
    let workspace_id = extract_workspace_id_from_path(req.uri().path());

    // Extract user_id from auth context. Split+reassemble to use the extractor.
    let (mut parts, body) = req.into_parts();
    let user_id = match AuthenticatedAgent::from_request_parts(&mut parts, &state).await {
        Ok(auth) => auth.user_id.map(|id| id.as_str().to_owned()),
        Err(_) => None,
    };
    let req = Request::from_parts(parts, body);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let tracking = match (&user_id, &workspace_id) {
        (Some(uid), Some(wid)) => {
            let key = (uid.clone(), wid.clone());
            let should = {
                let mut cache = state
                    .last_seen_debounce
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let should = cache
                    .get(&key)
                    .map(|t| t.elapsed().as_secs() >= 60)
                    .unwrap_or(true);
                if should {
                    // Evict stale entries when cache grows large (>10_000 pairs).
                    // Simple full-clear: all entries are at most 60s old, so the
                    // cost is at most 10_000 extra upserts in the next minute.
                    if cache.len() >= 10_000 {
                        cache.retain(|_, t| t.elapsed().as_secs() < 60);
                    }
                    cache.insert(key, std::time::Instant::now());
                }
                should
            };
            if should {
                let uid = uid.clone();
                let wid = wid.clone();
                let repo = state.user_workspace_state.clone();
                tokio::spawn(async move {
                    if let Err(e) = repo.upsert_last_seen(&uid, &wid, now).await {
                        tracing::warn!("last_seen upsert failed: {e}");
                    }
                });
            }
            should
        }
        _ => false,
    };

    let mut response = next.run(req).await;

    if tracking {
        if let Ok(val) = now.to_string().parse() {
            response.headers_mut().insert("x-gyre-last-seen", val);
        }
    }

    response
}

/// Extract workspace_id from paths like `/api/v1/workspaces/<id>/...`.
fn extract_workspace_id_from_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/api/v1/workspaces/")?;
    let id = rest.split('/').next()?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_owned())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use std::sync::Arc;
    use tower::ServiceExt;

    fn app() -> Router {
        let state = test_state();
        crate::build_router(state)
    }

    /// Build router with a very low rate limit for testing.
    fn rate_limited_app(rate: u64) -> Router {
        use crate::AppState;
        let base = test_state();
        let state = Arc::new(AppState {
            rate_limiter: crate::rate_limit::RateLimiter::new(rate),
            auth_token: base.auth_token.clone(),
            base_url: base.base_url.clone(),
            repos: base.repos.clone(),
            agents: base.agents.clone(),
            tasks: base.tasks.clone(),
            merge_requests: base.merge_requests.clone(),
            reviews: base.reviews.clone(),
            merge_queue: base.merge_queue.clone(),
            git_ops: base.git_ops.clone(),
            jj_ops: base.jj_ops.clone(),
            agent_commits: base.agent_commits.clone(),
            worktrees: base.worktrees.clone(),
            telemetry_buffer: base.telemetry_buffer.clone(),
            message_broadcast_tx: base.message_broadcast_tx.clone(),
            kv_store: base.kv_store.clone(),
            agent_signing_key: base.agent_signing_key.clone(),
            agent_jwt_ttl_secs: base.agent_jwt_ttl_secs,
            users: base.users.clone(),
            api_keys: base.api_keys.clone(),
            jwt_config: base.jwt_config.clone(),
            http_client: base.http_client.clone(),
            metrics: base.metrics.clone(),
            started_at_secs: base.started_at_secs,
            compose_sessions: base.compose_sessions.clone(),
            retention_store: base.retention_store.clone(),
            job_registry: base.job_registry.clone(),
            analytics: base.analytics.clone(),
            costs: base.costs.clone(),
            audit: base.audit.clone(),
            siem_store: base.siem_store.clone(),
            audit_broadcast_tx: base.audit_broadcast_tx.clone(),
            network_peers: base.network_peers.clone(),
            dependencies: base.dependencies.clone(),
            process_registry: base.process_registry.clone(),
            agent_logs: base.agent_logs.clone(),
            agent_log_tx: base.agent_log_tx.clone(),
            quality_gates: base.quality_gates.clone(),
            gate_results: base.gate_results.clone(),
            push_gate_registry: base.push_gate_registry.clone(),
            repo_push_gates: base.repo_push_gates.clone(),
            speculative_results: base.speculative_results.clone(),
            spawn_log: base.spawn_log.clone(),
            db_storage: base.db_storage.clone(),
            spec_approvals: base.spec_approvals.clone(),
            spec_policies: base.spec_policies.clone(),
            attestation_store: base.attestation_store.clone(),
            chain_attestations: base.chain_attestations.clone(),
            key_bindings: base.key_bindings.clone(),
            trust_anchors: base.trust_anchors.clone(),
            trusted_issuers: base.trusted_issuers.clone(),
            remote_jwks_cache: base.remote_jwks_cache.clone(),
            commit_signatures: base.commit_signatures.clone(),
            sigstore_mode: base.sigstore_mode.clone(),
            tunnel_store: base.tunnel_store.clone(),
            container_audits: base.container_audits.clone(),
            spec_ledger: base.spec_ledger.clone(),
            spec_approval_history: base.spec_approval_history.clone(),
            spec_links_store: base.spec_links_store.clone(),
            budget_configs: base.budget_configs.clone(),
            budget_usages: base.budget_usages.clone(),
            search: base.search.clone(),
            tenants: base.tenants.clone(),
            workspaces: base.workspaces.clone(),
            personas: base.personas.clone(),
            policies: base.policies.clone(),
            workspace_memberships: base.workspace_memberships.clone(),
            teams: base.teams.clone(),
            notifications: base.notifications.clone(),
            graph_store: base.graph_store.clone(),
            saved_views: base.saved_views.clone(),
            wg_config: base.wg_config.clone(),
            meta_spec_sets: base.meta_spec_sets.clone(),
            messages: base.messages.clone(),
            message_dispatch_tx: base.message_dispatch_tx.clone(),
            agent_inbox_max: base.agent_inbox_max,
            user_workspace_state: base.user_workspace_state.clone(),
            last_seen_debounce: base.last_seen_debounce.clone(),
            llm_rate_limiter: base.llm_rate_limiter.clone(),
            llm_configs: base.llm_configs.clone(),
            presence: base.presence.clone(),
            ws_connections: base.ws_connections.clone(),
            ws_connection_counter: base.ws_connection_counter.clone(),
            ws_connection_workspaces: base.ws_connection_workspaces.clone(),
            traces: base.traces.clone(),
            otlp_config: base.otlp_config.clone(),
            conversations: base.conversations.clone(),
            repos_root: base.repos_root.clone(),
            prompt_templates: base.prompt_templates.clone(),
            compute_targets: base.compute_targets.clone(),
            llm: base.llm.clone(),
            user_notification_prefs: base.user_notification_prefs.clone(),
            user_tokens: base.user_tokens.clone(),
            judgment_ledger: base.judgment_ledger.clone(),
            ws_tickets: base.ws_tickets.clone(),
            meta_specs: base.meta_specs.clone(),
            meta_spec_bindings: base.meta_spec_bindings.clone(),
        });
        crate::build_router(state)
    }

    #[tokio::test]
    async fn request_id_header_present() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            resp.headers().contains_key("x-request-id"),
            "missing X-Request-Id header"
        );
    }

    #[tokio::test]
    async fn request_id_is_uuid_format() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let id = resp
            .headers()
            .get("x-request-id")
            .expect("X-Request-Id must be present")
            .to_str()
            .expect("X-Request-Id must be ASCII");
        // UUID format: 8-4-4-4-12 hex digits.
        assert_eq!(id.len(), 36, "request id length: {id}");
        assert!(id.contains('-'), "request id should be UUID: {id}");
    }

    #[tokio::test]
    async fn request_id_unique_per_request() {
        let app = app();
        let r1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let r2 = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let id1 = r1.headers().get("x-request-id").unwrap().to_str().unwrap();
        let id2 = r2.headers().get("x-request-id").unwrap().to_str().unwrap();
        assert_ne!(id1, id2, "request IDs should be unique per request");
    }

    #[tokio::test]
    async fn metrics_endpoint_returns_200() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn metrics_endpoint_is_prometheus_format() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = std::str::from_utf8(&body).unwrap();
        assert!(
            text.contains("# HELP") || text.contains("# TYPE"),
            "metrics response is not Prometheus format: {text}"
        );
    }

    // ── Production hardening tests ──────────────────────────────────────────

    #[tokio::test]
    async fn rate_limit_allows_within_limit() {
        let app = rate_limited_app(10);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rate_limit_returns_429_when_exceeded() {
        // Rate limit of 0 → every request is rejected.
        let app = rate_limited_app(0);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn rate_limit_exhaustion_returns_429() {
        // Rate of 1: first succeeds, second gets 429.
        let app = rate_limited_app(1);
        let resp1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);

        let resp2 = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn cors_preflight_returns_200() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/v1/version")
                    .header("Origin", "http://example.com")
                    .header("Access-Control-Request-Method", "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // CORS preflight should succeed (2xx)
        assert!(
            resp.status().is_success() || resp.status() == StatusCode::NO_CONTENT,
            "CORS preflight returned: {}",
            resp.status()
        );
    }

    #[tokio::test]
    async fn cors_response_has_allow_origin_header() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header("Origin", "http://localhost:3000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            resp.headers().contains_key("access-control-allow-origin"),
            "missing access-control-allow-origin header"
        );
    }

    #[tokio::test]
    async fn body_size_limit_rejects_oversized_request() {
        // The default body limit in tests comes from build_router's env-var logic.
        // We set GYRE_MAX_BODY_SIZE=100 and send > 100 bytes.
        // Since env vars affect the whole process, we test using a very large body
        // against the default 10MB limit (should succeed), and a known-oversized
        // body with a custom router that has a tiny limit.
        use axum::extract::DefaultBodyLimit;
        use tower_http::catch_panic::CatchPanicLayer;

        let base = test_state();
        let small_limit_router = crate::api::api_router()
            .layer(axum::middleware::from_fn_with_state(
                base.clone(),
                crate::middleware::rate_limit_middleware,
            ))
            .layer(axum::middleware::from_fn_with_state(
                base.clone(),
                crate::middleware::request_tracing,
            ))
            .layer(DefaultBodyLimit::max(10)) // 10 bytes max
            .layer(CatchPanicLayer::new())
            .with_state(base);

        let large_body = vec![b'x'; 1000]; // 1000 bytes >> 10 limit
        let resp = small_limit_router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents")
                    .header("content-type", "application/json")
                    .body(Body::from(large_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn panic_handler_returns_500() {
        use axum::{routing::get, Router};
        use tower_http::catch_panic::CatchPanicLayer;

        // Build a minimal router with a panicking handler + CatchPanicLayer
        async fn panicking_handler() -> axum::http::StatusCode {
            panic!("test panic!")
        }
        let panic_router: Router = Router::new()
            .route("/panic", get(panicking_handler))
            .layer(CatchPanicLayer::new());

        let resp = panic_router
            .oneshot(
                Request::builder()
                    .uri("/panic")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
