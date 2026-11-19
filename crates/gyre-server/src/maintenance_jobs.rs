//! Scheduled maintenance jobs (admin-panel.md §Background Jobs; TASK-273):
//!
//! - [`job_history_cleanup`]: self-managing pruning of durable job-run
//!   history past the configured retention.
//! - [`repo_maintenance`]: global repository maintenance — real `git gc`
//!   (prune of unreachable objects + repack) for every stored repository.
//! - [`remote_agent_cleanup`]: kills still-recorded-running agent instances
//!   whose agent is Dead/Stopped or absent, including orphans left by a
//!   server restart.
//! - [`oauth_state_cleanup`]: deletes expired OAuth authorization-request
//!   state rows.
//!
//! Each handler receives the run-scoped [`crate::jobs::JobRunLog`] sink and
//! records per-item outcomes in it, so the admin Jobs tab shows what the
//! run actually did.

use anyhow::Result;
use std::sync::Arc;

use crate::jobs::JobRunLog;
use crate::AppState;

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// `job_history_cleanup` (admin-panel.md: "Job history cleanup
/// (self-managing)"): prune finished job runs older than the configured
/// retention. Running rows are never deleted (the port contract).
///
/// Retention: `GYRE_JOB_HISTORY_RETENTION_DAYS` days (default
/// [`crate::jobs::JOB_HISTORY_RETENTION_DAYS_DEFAULT`]).
pub async fn job_history_cleanup(state: Arc<AppState>, log: JobRunLog) -> Result<()> {
    let retention_days: u64 = std::env::var("GYRE_JOB_HISTORY_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(crate::jobs::JOB_HISTORY_RETENTION_DAYS_DEFAULT);
    let cutoff = now_secs().saturating_sub(retention_days * 86_400);
    let deleted = state.job_runs.delete_older_than(cutoff).await?;
    log.log(format!(
        "pruned {deleted} job runs finished before the {retention_days}-day retention cutoff"
    ))
    .await;
    Ok(())
}

/// `repo_maintenance` (admin-panel.md: "Global repository maintenance"):
/// run `git gc --prune=now` in every stored repository — a real prune of
/// unreachable objects plus a repack, through the server's git execution
/// layer (git CLI, same as git_refs/search indexing). Per-repo outcomes are
/// recorded in the run logs; a failing repo is recorded as failed without
/// aborting the others.
pub async fn repo_maintenance(state: Arc<AppState>, log: JobRunLog) -> Result<()> {
    let repos = state.repos.list().await?;
    log.log(format!("maintaining {} repositories", repos.len()))
        .await;

    let mut failures = 0usize;
    for repo in &repos {
        let path = std::path::Path::new(&repo.path);
        if !path.is_dir() {
            log.log(format!(
                "repo {}: FAILED — path {} does not exist",
                repo.name, repo.path
            ))
            .await;
            failures += 1;
            continue;
        }
        // Bound each repository's gc so one huge repo cannot stall the job
        // forever (10 minutes; local disk work, not HTTP).
        let gc = tokio::time::timeout(
            std::time::Duration::from_secs(600),
            tokio::process::Command::new("git")
                .arg("-C")
                .arg(&repo.path)
                .args(["gc", "--prune=now", "--quiet"])
                .output(),
        )
        .await;
        match gc {
            Ok(Ok(out)) if out.status.success() => {
                log.log(format!("repo {}: ok (git gc --prune=now)", repo.name))
                    .await;
            }
            Ok(Ok(out)) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                log.log(format!(
                    "repo {}: FAILED — git gc exited {:?}: {}",
                    repo.name,
                    out.status.code(),
                    stderr.trim()
                ))
                .await;
                failures += 1;
            }
            Ok(Err(e)) => {
                log.log(format!("repo {}: FAILED — git spawn error: {e}", repo.name))
                    .await;
                failures += 1;
            }
            Err(_) => {
                log.log(format!(
                    "repo {}: FAILED — git gc exceeded the 10-minute timeout",
                    repo.name
                ))
                .await;
                failures += 1;
            }
        }
    }

    if failures > 0 {
        anyhow::bail!(
            "{failures} of {} repositories failed maintenance (see run logs)",
            repos.len()
        );
    }
    Ok(())
}

/// `oauth_state_cleanup` (admin-panel.md): delete expired OAuth
/// authorization-request state rows (Claude Max enrollment flow).
pub async fn oauth_state_cleanup(state: Arc<AppState>, log: JobRunLog) -> Result<()> {
    let deleted = state.oauth_states.delete_expired_before(now_secs()).await?;
    log.log(format!("deleted {deleted} expired OAuth state rows"))
        .await;
    Ok(())
}

/// `remote_agent_cleanup` (admin-panel.md: "Remote agent cleanup
/// (expired/orphaned instances)"): kill still-recorded-running instances
/// whose agent is Dead/Stopped or absent — including orphans left by a
/// server restart (the durable agent-instance store survives it) — via
/// `ComputeTarget::kill_process`, then mark them terminated.
pub async fn remote_agent_cleanup(state: Arc<AppState>, log: JobRunLog) -> Result<()> {
    use gyre_domain::AgentStatus;

    let running = state.agent_instances.list_running().await?;
    log.log(format!(
        "{} recorded-running agent instances",
        running.len()
    ))
    .await;

    let agents = state.agents.list().await?;
    let mut reaped = 0usize;
    for instance in running {
        let agent = agents.iter().find(|a| a.id.as_str() == instance.agent_id);
        let reap = match agent {
            // Agent is gone (deleted, or orphaned by a server restart that
            // lost the in-memory registry): reap.
            None => true,
            // Agent exists but is dead/stopped: reap.
            Some(a) => matches!(a.status, AgentStatus::Dead | AgentStatus::Stopped),
        };
        if !reap {
            continue;
        }
        let why = match agent {
            None => "agent absent (orphaned instance)".to_string(),
            Some(a) => format!("agent status {:?}", a.status),
        };

        // Fast path: the in-memory spawned_backends map still owns this
        // instance's backend (same process lifetime).
        let backend = {
            let backends = state.spawned_backends.lock().await;
            backends.get(&instance.agent_id).cloned()
        };

        // Reconstruct a backend for restart-orphans based on the durable
        // record's backend descriptor. Container/Kubernetes kills address
        // global resources (docker rm / kubectl delete pod) reachable from
        // a default-constructed target; SSH-bare and unreconstructable
        // descriptors are reported, not silently skipped.
        let outcome: Result<()> = match backend {
            Some(b) => kill_and_mark(&state, b.as_ref(), &instance, "remote_agent_cleanup").await,
            None => {
                let handle = gyre_ports::ProcessHandle {
                    id: instance.handle_id.clone(),
                    target_type: instance.target_type.clone(),
                    pid: instance.pid,
                };
                match instance.backend.as_str() {
                    "container" => {
                        let target = gyre_adapters::compute::ContainerTarget::new("unused");
                        kill_and_mark(&state, &target, &instance, "remote_agent_cleanup").await
                    }
                    "kubernetes" => {
                        let target = gyre_adapters::compute::KubernetesTarget::new("unused");
                        kill_and_mark(&state, &target, &instance, "remote_agent_cleanup").await
                    }
                    "local" => {
                        let target = gyre_adapters::compute::LocalTarget;
                        let _ = handle;
                        kill_and_mark(&state, &target, &instance, "remote_agent_cleanup").await
                    }
                    other => {
                        log.log(format!(
                            "agent {}: NOT REAPED — backend {other:?} requires connection \
                             details not persisted in the instance record ({why})",
                            instance.agent_id
                        ))
                        .await;
                        continue;
                    }
                }
            }
        };

        match outcome {
            Ok(()) => {
                reaped += 1;
                log.log(format!(
                    "agent {}: killed instance {} ({why})",
                    instance.agent_id, instance.handle_id
                ))
                .await;
            }
            Err(e) => {
                log.log(format!(
                    "agent {}: FAILED to kill instance {} ({why}): {e}",
                    instance.agent_id, instance.handle_id
                ))
                .await;
            }
        }
    }
    log.log(format!("reaped {reaped} instances")).await;
    Ok(())
}

/// Kill `instance` via `target` and mark it terminated with `reason`.
async fn kill_and_mark(
    state: &AppState,
    target: &dyn gyre_ports::ComputeTarget,
    instance: &gyre_ports::AgentInstanceRecord,
    reason: &str,
) -> Result<()> {
    let handle = gyre_ports::ProcessHandle {
        id: instance.handle_id.clone(),
        target_type: instance.target_type.clone(),
        pid: instance.pid,
    };
    target.kill_process(&handle).await?;
    state
        .agent_instances
        .mark_terminated(&instance.agent_id, reason)
        .await?;
    // The in-memory fast-path registries (if this process still holds them)
    // must not keep a killed instance alive as a phantom.
    state
        .process_registry
        .lock()
        .await
        .remove(&instance.agent_id);
    state
        .spawned_backends
        .lock()
        .await
        .remove(&instance.agent_id);
    Ok(())
}
