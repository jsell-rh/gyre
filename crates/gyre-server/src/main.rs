use anyhow::Result;
use gyre_server::{
    abac_middleware, audit_simulator, bootstrap_default_tenant, build_router, build_state, jobs,
    merge_processor, procfs_monitor, register_default_compute_target, seed_builtin_meta_specs,
    seed_builtin_personas, siem, signal_chain, spawn_budget_daily_reset,
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
    let repos_dir = gyre_server::configured_repos_path();
    if let Err(e) = std::fs::create_dir_all(&repos_dir) {
        tracing::warn!(
            "failed to create repos directory '{}': {e}",
            repos_dir.display()
        );
    }

    let state = build_state(&auth_token, &base_url, jwt_config);

    // Materialize the default Tenant entity before any tenant-scoped seeding
    // (hierarchy-enforcement.md §1 Bootstrap Behavior).
    bootstrap_default_tenant(&state).await;

    // Initialise ABAC resource resolver and seed built-in policies (M34 Slice 4).
    // Fails closed: if any built-in policy (incl. the immutable
    // builtin:require-human-spec-approval Deny, HSI §2) cannot be persisted,
    // refuse to start rather than serve with a partially seeded policy set.
    abac_middleware::init_resolver();
    abac_middleware::seed_builtin_policies(&state).await?;

    // Seed built-in meta-specs on first startup (agent-runtime spec §2).
    seed_builtin_meta_specs(&state).await;

    // Seed the four built-in personas at tenant level (platform-model.md §2).
    // Idempotent per tenant: existing slug+Tenant-scope personas are kept.
    seed_builtin_personas(&state).await;

    // Load persisted retention policies from KV (or seed defaults on first
    // boot) before the job registry registers the nightly cleanup handler.
    state.retention_store.init(state.kv_store.clone()).await;

    // M25: Auto-register default container compute target if Docker/Podman is available.
    register_default_compute_target(&state).await;

    // Register jobs into the admin job registry so GET/POST /admin/jobs work.
    jobs::start_job_registry(state.clone()).await;

    // Nightly retention cleanup at 02:00 UTC (business-continuity.md §5) —
    // wall-clock scheduled, see jobs::spawn_job.
    jobs::spawn_job(
        Arc::clone(&state.job_registry),
        "retention_cleanup".to_string(),
        Arc::clone(&state),
    );

    // Hourly stale-pin detection: specs pinning old meta-spec versions get
    // priority-6 MetaSpecDrift notifications (agent-runtime.md §2).
    jobs::spawn_job(
        Arc::clone(&state.job_registry),
        "meta_spec_stale_pin_check".to_string(),
        Arc::clone(&state),
    );

    // Secret expiry check every 5 minutes (platform-model.md §7, task-098).
    jobs::spawn_job(
        Arc::clone(&state.job_registry),
        "secret_rotation_check".to_string(),
        Arc::clone(&state),
    );

    // Meta-spec conformance sweep (meta-spec-reconciliation.md §10) — daily by
    // default, configurable via GYRE_META_SPEC_SWEEP_INTERVAL_SECS.
    jobs::spawn_job(
        Arc::clone(&state.job_registry),
        "meta_spec_conformance_sweep".to_string(),
        Arc::clone(&state),
    );

    // Impersonation TTL expiry sweep (identity-security.md §Impersonation) —
    // ends sessions past their TTL, emitting ImpersonationEnded.
    jobs::spawn_job(
        Arc::clone(&state.job_registry),
        "impersonation_expiry".to_string(),
        Arc::clone(&state),
    );

    // Log compression (observability.md §Log Retention & Compression) —
    // daily by default, configurable via GYRE_LOG_COMPRESSION_INTERVAL.
    jobs::spawn_job(
        Arc::clone(&state.job_registry),
        gyre_server::log_compression::JOB_NAME.to_string(),
        Arc::clone(&state),
    );

    // Background tasks.
    // Task-115 (agent-runtime.md §1 Phase 3): detect Delegation/Coordination
    // tasks and run repo orchestrators under per-repo registry locks.
    signal_chain::spawn_task_scheduler(state.clone());
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

    axum::serve(
        listener,
        // Attach the socket peer address to every request so handlers can
        // resolve forensic source IPs from the transport (task-102 F3,
        // CWE-348) instead of trusting caller-controlled forwarded headers.
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
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
