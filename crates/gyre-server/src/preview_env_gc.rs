//! TTL expiry and liveness reconciliation for preview environments
//! (activity-dashboard.md §"Try it" links — lifecycle: provisioning →
//! active → expired/failed).
//!
//! Registered on the jobs.rs scheduler as `preview_env_gc`; the executing
//! loop is spawned from main.rs like every sibling job. Each pass:
//!
//! 1. **Expiry**: live environments past their TTL deadline are torn down
//!    (process killed, mesh peer removed, slot released) and marked
//!    `expired` with a `preview.expired` analytics event.
//! 2. **Liveness**: `active` environments whose process the server can no
//!    longer observe (exited, or the handle was lost to a restart) are
//!    torn down and marked `failed` with a `preview.failed` event. A probe
//!    that ERRS says nothing about the process (transient daemon failure)
//!    and leaves the environment alone, exactly like the agent exit
//!    monitor's three-state probe.

use crate::api::preview_envs::teardown_preview_environment;
use crate::api::preview_envs::TeardownOutcome;
use crate::AppState;

use gyre_ports::ComputeTarget;

pub const JOB_NAME: &str = "preview_env_gc";

/// How often the executing loop runs (seconds).
pub const INTERVAL_SECS: u64 = 30;

pub async fn run_once(state: &AppState) -> anyhow::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // 1. TTL expiry.
    let expired = state.preview_environments.find_live_expired_before(now).await?;
    for env in expired {
        tracing::info!(env_id = %env.id, "preview environment TTL expired; tearing down");
        teardown_preview_environment(state, env, TeardownOutcome::Expired).await;
    }

    // 2. Liveness reconciliation over still-live environments. The port has
    //    no unbounded "find_live" query by design (every consumer pages a
    //    bounded window); i64::MAX as the cutoff selects every live row.
    let live = state
        .preview_environments
        .find_live_expired_before(i64::MAX as u64)
        .await?;
    for env in live {
        // Provisioning envs get a grace window: the dispatch may still be
        // in flight and a missing handle would be a false negative. Past
        // the TTL they are handled by the expiry pass above.
        if env.status != gyre_domain::PreviewEnvironmentStatus::Active {
            continue;
        }
        if environment_process_alive(state, &env).await {
            continue;
        }
        tracing::info!(env_id = %env.id, "preview environment process is gone; marking failed");
        teardown_preview_environment(state, env, TeardownOutcome::Failed).await;
    }

    Ok(())
}

/// Three-state liveness probe, mirroring the agent exit monitor:
/// `true` = confirmed alive; `false` = confirmed gone (backend answered
/// not-alive, or the server holds no handle — an exited process or a
/// restart orphan); probe errors keep the environment monitored.
async fn environment_process_alive(state: &AppState, env: &gyre_domain::PreviewEnvironment) -> bool {
    let agent_key = env.agent_id.to_string();
    let handle = state.process_registry.lock().await.get(&agent_key).cloned();
    let Some(handle) = handle else {
        // No registered handle: after a restart the server cannot manage
        // (or reach) the process anymore — the environment is dead to the
        // platform even if an orphan lingers.
        return false;
    };
    let backend = state.spawned_backends.lock().await.get(&agent_key).cloned();
    let Some(backend) = backend else {
        return false;
    };
    match ComputeTarget::is_alive(backend.as_ref(), &handle).await {
        Ok(alive) => alive,
        Err(e) => {
            tracing::warn!(
                env_id = %env.id,
                "preview liveness probe failed; environment stays monitored: {e:#}"
            );
            true
        }
    }
}
