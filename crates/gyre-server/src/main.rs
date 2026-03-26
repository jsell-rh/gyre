use anyhow::Result;
use gyre_server::{
    abac_middleware, audit_simulator, build_router, build_state, jobs, merge_processor,
    procfs_monitor, register_default_compute_target, siem, spawn_budget_daily_reset,
    spawn_llm_rate_limiter_cleanup, spawn_presence_eviction, spawn_stale_agent_detector,
    spawn_stale_peer_detector, telemetry, JwtConfig,
};
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize OTel tracing + structured logging.
    // Guard is held until end of main so spans are flushed on shutdown.
    let _telemetry_guard = telemetry::init_telemetry();

    info!("gyre-server starting");

    let port: u16 = std::env::var("GYRE_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()?;

    let auth_token =
        std::env::var("GYRE_AUTH_TOKEN").unwrap_or_else(|_| "gyre-dev-token".to_string());

    let base_url =
        std::env::var("GYRE_BASE_URL").unwrap_or_else(|_| format!("http://localhost:{port}"));

    let jwt_config = std::env::var("GYRE_OIDC_ISSUER").ok().map(|issuer| {
        let audience = std::env::var("GYRE_OIDC_AUDIENCE").ok();
        Arc::new(JwtConfig::new(issuer, audience))
    });

    // Ensure the git repos root directory exists on startup.
    let repos_dir = std::env::var("GYRE_REPOS_PATH").unwrap_or_else(|_| "./repos".to_string());
    if let Err(e) = std::fs::create_dir_all(&repos_dir) {
        tracing::warn!("failed to create repos directory '{repos_dir}': {e}");
    }

    let state = build_state(&auth_token, &base_url, jwt_config);

    // Initialise ABAC resource resolver and seed built-in policies (M34 Slice 4).
    abac_middleware::init_resolver();
    abac_middleware::seed_builtin_policies(&state).await;

    // M25: Auto-register default container compute target if Docker/Podman is available.
    register_default_compute_target(&state).await;

    // Register jobs into the admin job registry so GET/POST /admin/jobs work.
    jobs::start_job_registry(state.clone()).await;

    // Background tasks.
    spawn_stale_agent_detector(state.clone());
    spawn_stale_peer_detector(state.clone());
    spawn_presence_eviction(state.clone());
    merge_processor::spawn_merge_processor(state.clone());
    siem::spawn_siem_forwarder(state.clone());
    // Real procfs-based agent monitoring (replaces the synthetic simulator).
    // Simulator still available for testing via GYRE_AUDIT_SIMULATE=true.
    procfs_monitor::spawn_procfs_monitor(state.clone());
    audit_simulator::spawn_audit_simulator(state.clone());
    spawn_budget_daily_reset(state.clone());
    spawn_llm_rate_limiter_cleanup(state.clone());

    let app = build_router(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    info!(%addr, "listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("gyre-server stopped");
    // _telemetry_guard drops here, flushing buffered OTel spans.
    Ok(())
}

/// Wait for SIGINT or SIGTERM.
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to listen for ctrl-c");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("shutdown signal received");
}
