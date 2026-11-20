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
///
/// "Every stored repository" covers BOTH sources the contract names
/// (RepoRepository + repos_root): the registered rows AND repo
/// directories under `repos_root` without a repos row (a workspace whose
/// row was lost, a mirror cloned outside the registration path, …) — an
/// unregistered directory is maintained too, labeled by path (review
/// task-273: a repo directory under repos_root without a repos row is
/// never otherwise maintained).
pub async fn repo_maintenance(state: Arc<AppState>, log: JobRunLog) -> Result<()> {
    let repos = state.repos.list().await?;
    log.log(format!("maintaining {} repositories", repos.len()))
        .await;

    let mut failures = 0usize;
    let mut total = 0usize;
    let mut maintained: std::collections::HashSet<std::path::PathBuf> =
        std::collections::HashSet::new();
    for repo in &repos {
        total += 1;
        let path = std::path::PathBuf::from(&repo.path);
        if !maintain_repo(&path, &repo.name, &log).await {
            failures += 1;
        }
        // Canonicalize when the directory exists so the unregistered scan
        // cannot double-maintain a registered repo reached via a symlinked
        // or differently-spelled path; the raw path is kept when the
        // directory is gone (the row is already recorded as failed).
        let canonical = path.canonicalize().unwrap_or(path);
        maintained.insert(canonical);
    }

    // Unregistered repositories under repos_root: directories that look
    // like git repos (bare: HEAD + objects + refs; or a worktree with a
    // .git child) but have no repos row.
    let mut unregistered = 0usize;
    for dir in scan_git_dirs(std::path::Path::new(&state.repos_root)) {
        let Ok(canonical) = dir.canonicalize() else {
            continue;
        };
        if !maintained.insert(canonical) {
            continue;
        }
        total += 1;
        unregistered += 1;
        let label = dir.display().to_string();
        if !maintain_repo(&dir, &label, &log).await {
            failures += 1;
        }
    }
    if unregistered > 0 {
        log.log(format!(
            "maintained {unregistered} unregistered repo directories under repos_root"
        ))
        .await;
    }

    if failures > 0 {
        anyhow::bail!("{failures} of {total} repositories failed maintenance (see run logs)");
    }
    Ok(())
}

/// Run `git gc --prune=now` in one repository, recording the per-repo
/// outcome in the run logs. Returns whether it succeeded.
async fn maintain_repo(path: &std::path::Path, label: &str, log: &JobRunLog) -> bool {
    if !path.is_dir() {
        log.log(format!(
            "repo {label}: FAILED — path {} does not exist",
            path.display()
        ))
        .await;
        return false;
    }
    // Bound each repository's gc so one huge repo cannot stall the job
    // forever (10 minutes; local disk work, not HTTP).
    let gc = tokio::time::timeout(
        std::time::Duration::from_secs(600),
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(path)
            .args(["gc", "--prune=now", "--quiet"])
            .output(),
    )
    .await;
    match gc {
        Ok(Ok(out)) if out.status.success() => {
            log.log(format!("repo {label}: ok (git gc --prune=now)"))
                .await;
            true
        }
        Ok(Ok(out)) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            log.log(format!(
                "repo {label}: FAILED — git gc exited {:?}: {}",
                out.status.code(),
                stderr.trim()
            ))
            .await;
            false
        }
        Ok(Err(e)) => {
            log.log(format!("repo {label}: FAILED — git spawn error: {e}"))
                .await;
            false
        }
        Err(_) => {
            log.log(format!(
                "repo {label}: FAILED — git gc exceeded the 10-minute timeout"
            ))
            .await;
            false
        }
    }
}

/// Recursively collect git-repository directories under `root` (bounded
/// depth — the repos_root layout is `{ws}/{name}.git` and
/// `{ws}/mirrors/{forge}/{owner}/{repo}.git`). A directory qualifies as a
/// git repo when it is bare (has `HEAD`, `objects/` and `refs/` children)
/// or carries a `.git` child directory; scanning does NOT descend into a
/// qualifying repo (its internals are not separate repos).
fn scan_git_dirs(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    fn looks_like_git_repo(dir: &std::path::Path) -> bool {
        (dir.join("HEAD").is_file() && dir.join("objects").is_dir() && dir.join("refs").is_dir())
            || dir.join(".git").is_dir()
    }

    fn walk(dir: &std::path::Path, depth: usize, out: &mut Vec<std::path::PathBuf>) {
        if depth > 6 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let path = entry.path();
            if looks_like_git_repo(&path) {
                out.push(path);
            } else {
                walk(&path, depth + 1, out);
            }
        }
    }

    let mut out = Vec::new();
    walk(root, 0, &mut out);
    out.sort();
    out
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
        // a default-constructed target; local kills are gated on pid
        // identity verification (a recycled pid must not be SIGTERMed —
        // review task-273); SSH-bare and unreconstructable descriptors
        // are reported and eventually retired, not silently skipped.
        let outcome: Result<()> = match backend {
            Some(b) => kill_and_mark(&state, b.as_ref(), &instance, "remote_agent_cleanup").await,
            None => {
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
                        // Restart-orphan killed by recorded pid: verify the
                        // pid's identity first. The record was written by a
                        // previous process; if the agent process died and
                        // the kernel recycled its pid, SIGTERMing the new
                        // occupant would kill an unrelated process (review
                        // task-273).
                        match instance.pid {
                            Some(pid) => match local_pid_matches_record(pid, instance.spawned_at) {
                                PidCheck::Matches => {
                                    let target = gyre_adapters::compute::LocalTarget;
                                    kill_and_mark(
                                        &state,
                                        &target,
                                        &instance,
                                        "remote_agent_cleanup",
                                    )
                                    .await
                                }
                                PidCheck::Gone => {
                                    // Already exited: nothing to kill —
                                    // the record just needs the terminal
                                    // mark and the fast-path registries
                                    // cleaned.
                                    match mark_gone(&state, &instance, "remote_agent_cleanup").await
                                    {
                                        Ok(()) => {
                                            reaped += 1;
                                            log.log(format!(
                                                "agent {}: instance {} already exited \
                                                 ({why}); marked terminated",
                                                instance.agent_id, instance.handle_id
                                            ))
                                            .await;
                                        }
                                        Err(e) => {
                                            log.log(format!(
                                                "agent {}: FAILED to mark exited instance \
                                                 {} terminated ({why}): {e}",
                                                instance.agent_id, instance.handle_id
                                            ))
                                            .await;
                                        }
                                    }
                                    continue;
                                }
                                PidCheck::Mismatch => {
                                    log.log(format!(
                                        "agent {}: NOT REAPED — recorded pid {pid} does not \
                                         match the instance's spawn time (recycled pid \
                                         suspected; {why}); left for manual verification",
                                        instance.agent_id
                                    ))
                                    .await;
                                    continue;
                                }
                                PidCheck::Unverifiable => {
                                    log.log(format!(
                                        "agent {}: NOT REAPED — pid {pid} identity cannot \
                                         be verified on this platform ({why})",
                                        instance.agent_id
                                    ))
                                    .await;
                                    continue;
                                }
                            },
                            None => {
                                let target = gyre_adapters::compute::LocalTarget;
                                kill_and_mark(&state, &target, &instance, "remote_agent_cleanup")
                                    .await
                            }
                        }
                    }
                    other => {
                        // Backend whose connection details are not persisted
                        // in the instance record (e.g. SSH targets): the
                        // kill cannot be reconstructed. Retried while the
                        // record is younger than UNREAPABLE_RETENTION_SECS,
                        // then RETIRED — marked terminated with an
                        // escalation so the sweep does not re-emit the
                        // same dead entry every cycle forever (review
                        // task-273: unbounded log noise, structurally
                        // unfulfillable cleanup).
                        let age = now_secs().saturating_sub(instance.spawned_at);
                        if age > UNREAPABLE_RETENTION_SECS {
                            match state
                                .agent_instances
                                .mark_terminated(&instance.agent_id, "retired_unreapable")
                                .await
                            {
                                Ok(()) => {
                                    log.log(format!(
                                        "agent {}: RETIRED — instance {} on backend \
                                         {other:?} could not be reaped for {} days \
                                         (connection details are not persisted); recorded \
                                         as terminated. ESCALATION: verify and clean up \
                                         the target manually",
                                        instance.agent_id,
                                        instance.handle_id,
                                        age / 86_400
                                    ))
                                    .await;
                                }
                                Err(e) => {
                                    log.log(format!(
                                        "agent {}: FAILED to retire unreapable instance \
                                         {} ({why}): {e}",
                                        instance.agent_id, instance.handle_id
                                    ))
                                    .await;
                                }
                            }
                        } else {
                            log.log(format!(
                                "agent {}: NOT REAPED — backend {other:?} requires connection \
                                 details not persisted in the instance record ({why}); \
                                 retries for another {} days, then retires",
                                instance.agent_id,
                                (UNREAPABLE_RETENTION_SECS - age) / 86_400
                            ))
                            .await;
                        }
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

/// How long an unreapable instance (a backend whose connection details
/// are not persisted in the record, e.g. SSH targets) is retried before it
/// is retired: marked terminated with an escalation instead of re-logging
/// the same dead entry on every sweep forever (review task-273).
const UNREAPABLE_RETENTION_SECS: u64 = 7 * 86_400;

/// Outcome of verifying a recorded local pid against the instance record.
#[derive(Debug, PartialEq, Eq)]
enum PidCheck {
    /// The process behind `pid` started at the recorded spawn time — it is
    /// the instance's process (safe to kill).
    Matches,
    /// The process behind `pid` started at a different time — a recycled
    /// pid (NOT safe to kill).
    Mismatch,
    /// No process exists behind `pid` (already exited).
    Gone,
    /// The platform exposes no way to verify (/proc unavailable).
    Unverifiable,
}

/// Tolerance between the pid's kernel start time and the recorded
/// `spawned_at` (covers recording delay and clock skew; a recycled pid's
/// start time is arbitrarily later than the original's death, which this
/// still catches in practice because Linux allocates pids sequentially
/// over a large space).
const PID_START_TOLERANCE_SECS: u64 = 3600;

/// Verify a restart-orphan's recorded pid: read the process's kernel start
/// time from `/proc/<pid>/stat` (field 22, clock ticks since boot from
/// `/proc/stat` btime) and compare it with the instance's `spawned_at`.
/// A bare recorded pid must never be trusted across process lifetimes —
/// the kernel may have recycled it onto an unrelated process (review
/// task-273).
fn local_pid_matches_record(pid: u32, spawned_at: u64) -> PidCheck {
    let Some(boot_secs) = boot_time_secs() else {
        return PidCheck::Unverifiable;
    };
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return PidCheck::Gone;
    };
    // comm (field 2) can contain spaces and parentheses — parse after the
    // LAST ')'. The remaining fields start at state (field 3), so field 22
    // (starttime) is index 19.
    let Some((_, after)) = stat.rsplit_once(')') else {
        return PidCheck::Unverifiable;
    };
    let fields: Vec<&str> = after.split_whitespace().collect();
    let Some(starttime_ticks) = fields.get(19).and_then(|f| f.parse::<u64>().ok()) else {
        return PidCheck::Unverifiable;
    };
    // USER_HZ is 100 on every mainstream Linux deployment.
    let start_secs = boot_secs + starttime_ticks / 100;
    if start_secs + PID_START_TOLERANCE_SECS >= spawned_at
        && start_secs <= spawned_at + PID_START_TOLERANCE_SECS
    {
        PidCheck::Matches
    } else {
        PidCheck::Mismatch
    }
}

/// Kernel boot time (Unix epoch seconds) from `/proc/stat` `btime`.
fn boot_time_secs() -> Option<u64> {
    let proc_stat = std::fs::read_to_string("/proc/stat").ok()?;
    for line in proc_stat.lines() {
        if let Some(rest) = line.strip_prefix("btime ") {
            return rest.trim().parse().ok();
        }
    }
    None
}

/// Mark an already-exited instance terminated and clean the in-memory
/// fast-path registries (the kill path of [`kill_and_mark`] without a
/// kill: there is no process left to signal).
async fn mark_gone(
    state: &AppState,
    instance: &gyre_ports::AgentInstanceRecord,
    reason: &str,
) -> Result<()> {
    state
        .agent_instances
        .mark_terminated(&instance.agent_id, reason)
        .await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::Id;
    use gyre_domain::{AgentStatus, Repository};
    use gyre_ports::{AgentInstanceRecord, JobRunRecord, OAuthStateRecord};
    use std::path::Path;
    use std::process::Command;
    use std::time::{Duration, Instant};

    fn run_git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git is available");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn git_stdout(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git is available");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// A real git repo with one commit.
    fn init_repo(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        run_git(dir, &["init", "-q"]);
        run_git(dir, &["config", "user.email", "maint@test"]);
        run_git(dir, &["config", "user.name", "Maint Test"]);
        std::fs::write(dir.join("README.md"), "seed content\n").unwrap();
        run_git(dir, &["add", "."]);
        run_git(dir, &["commit", "-q", "-m", "init"]);
    }

    /// Loose-object count from `git count-objects -v` (the `count:` line).
    fn loose_objects(dir: &Path) -> u64 {
        for line in git_stdout(dir, &["count-objects", "-v"]).lines() {
            if let Some(n) = line.strip_prefix("count: ") {
                return n.trim().parse().unwrap();
            }
        }
        panic!("count-objects produced no count line");
    }

    fn repo_record(id: &str, name: &str, path: &Path) -> Repository {
        Repository::new(
            Id::new(id),
            Id::new("ws-maint"),
            name,
            path.to_str().unwrap(),
            now_secs(),
        )
    }

    // ── repo_maintenance ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn repo_maintenance_prunes_unreachable_objects_and_isolates_failures() {
        let state = crate::mem::test_state();
        let tmp = tempfile::tempdir().unwrap();

        // Repo A: healthy, seeded with an unreachable loose object that a
        // real gc --prune=now must reclaim.
        let repo_a = tmp.path().join("repo-a");
        init_repo(&repo_a);
        let unreachable = Command::new("git")
            .arg("-C")
            .arg(&repo_a)
            .args(["hash-object", "-w", "--stdin"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(b"unreachable blob that gc must prune")
                    .unwrap();
                child.wait_with_output()
            })
            .unwrap();
        assert!(unreachable.status.success());
        let before_a = loose_objects(&repo_a);
        assert!(before_a >= 1, "the unreachable object must exist first");

        // Repo B: healthy control (nothing to reclaim, must still be ok).
        let repo_b = tmp.path().join("repo-b");
        init_repo(&repo_b);

        // Repo C: deliberately corrupted (invalid git config) — its
        // maintenance must fail WITHOUT aborting the others.
        let repo_c = tmp.path().join("repo-c");
        init_repo(&repo_c);
        std::fs::write(
            repo_c.join(".git").join("config"),
            "not a valid config [[[\n",
        )
        .unwrap();

        for (id, name, path) in [
            ("repo-a", "repo-a", &repo_a),
            ("repo-b", "repo-b", &repo_b),
            ("repo-c", "repo-c", &repo_c),
        ] {
            state
                .repos
                .create(&repo_record(id, name, path))
                .await
                .unwrap();
        }

        let log = JobRunLog::new();
        let result = repo_maintenance(state, log.clone()).await;

        // One failed repo ⇒ the run is recorded as failed overall...
        let err = result.expect_err("the corrupted repo must fail the run");
        assert!(
            err.to_string().contains("1 of 3"),
            "error must name the failure count: {err}"
        );

        // ...but the healthy repos were still maintained: the unreachable
        // object is gone (count-objects differs).
        let after_a = loose_objects(&repo_a);
        assert!(
            after_a < before_a,
            "git gc must reclaim the unreachable object (before {before_a}, after {after_a})"
        );

        // Per-repo outcomes are recorded in the run logs, including the
        // failure for the corrupted repo.
        let lines = log.take().await;
        assert!(lines
            .iter()
            .any(|l| l.contains("repo-a") && l.contains("ok")));
        assert!(lines
            .iter()
            .any(|l| l.contains("repo-b") && l.contains("ok")));
        assert!(lines
            .iter()
            .any(|l| l.contains("repo-c") && l.contains("FAILED")));
    }

    /// Review task-273: "every stored repository" covers BOTH sources the
    /// contract names — a repo directory under repos_root WITHOUT a repos
    /// row is maintained too (labeled by path), and never
    /// double-maintained when the same directory is also registered.
    #[tokio::test]
    async fn repo_maintenance_also_maintains_unregistered_repos_under_repos_root() {
        let state = crate::mem::test_state();
        let tmp = tempfile::tempdir().unwrap();
        // Point repos_root at the tempdir (uniquely owned state: safe to
        // mutate before any clone).
        let state = {
            let mut s = state;
            let inner = Arc::get_mut(&mut s).expect("test state is uniquely owned");
            inner.repos_root = tmp.path().to_string_lossy().into_owned();
            s
        };

        // A REGISTERED repo (row + directory).
        let registered = tmp.path().join("ws-1").join("registered.git");
        init_repo(&registered);
        state
            .repos
            .create(&repo_record("registered", "registered", &registered))
            .await
            .unwrap();

        // An UNREGISTERED repo directory with an unreachable loose object
        // that a real gc --prune=now must reclaim — no repos row at all.
        let orphan = tmp.path().join("ws-1").join("orphan.git");
        init_repo(&orphan);
        let unreachable = Command::new("git")
            .arg("-C")
            .arg(&orphan)
            .args(["hash-object", "-w", "--stdin"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(b"unreachable blob in the unregistered repo")
                    .unwrap();
                child.wait_with_output()
            })
            .unwrap();
        assert!(unreachable.status.success());
        let before = loose_objects(&orphan);
        assert!(before >= 1, "the unreachable object must exist first");

        let log = JobRunLog::new();
        repo_maintenance(state, log.clone()).await.unwrap();

        // The unregistered repo was really maintained: the unreachable
        // object is gone (count-objects differs).
        let after = loose_objects(&orphan);
        assert!(
            after < before,
            "git gc must reclaim the unregistered repo's unreachable object \
             (before {before}, after {after})"
        );

        // Per-repo outcomes: both the registered row and the unregistered
        // directory appear in the run logs (the unregistered one labeled
        // by path), each exactly once — no double maintenance.
        let lines = log.take().await;
        let orphan_lines = lines
            .iter()
            .filter(|l| l.contains("orphan.git") && l.contains("ok"))
            .count();
        assert_eq!(
            orphan_lines, 1,
            "the unregistered repo must be maintained exactly once, got: {lines:?}"
        );
        let registered_lines = lines
            .iter()
            .filter(|l| l.contains("registered") && l.contains("ok"))
            .count();
        assert_eq!(
            registered_lines, 1,
            "the registered repo must be maintained exactly once, got: {lines:?}"
        );
    }

    #[tokio::test]
    async fn repo_maintenance_reports_missing_repo_path_without_aborting() {
        let state = crate::mem::test_state();
        let tmp = tempfile::tempdir().unwrap();
        let repo_a = tmp.path().join("repo-a");
        init_repo(&repo_a);
        state
            .repos
            .create(&repo_record("repo-a", "repo-a", &repo_a))
            .await
            .unwrap();
        // Stored path does not exist on disk.
        state
            .repos
            .create(&repo_record(
                "repo-ghost",
                "repo-ghost",
                &tmp.path().join("nope"),
            ))
            .await
            .unwrap();

        let log = JobRunLog::new();
        let result = repo_maintenance(state, log.clone()).await;
        assert!(result.is_err(), "the missing repo must fail the run");
        let lines = log.take().await;
        assert!(lines
            .iter()
            .any(|l| l.contains("repo-ghost") && l.contains("does not exist")));
        assert!(lines
            .iter()
            .any(|l| l.contains("repo-a") && l.contains("ok")));
    }

    // ── job_history_cleanup ──────────────────────────────────────────────────

    #[tokio::test]
    async fn job_history_cleanup_prunes_only_old_finished_runs() {
        let state = crate::mem::test_state();
        let now = now_secs();
        let day = 86_400u64;

        let mk = |id: &str, started: u64, finished: Option<u64>| JobRunRecord {
            id: id.to_string(),
            job_name: "some_job".to_string(),
            started_at: started,
            finished_at: finished,
            status: match finished {
                Some(_) => "success".to_string(),
                None => "running".to_string(),
            },
            error: None,
            logs: vec![format!("log line for {id}")],
        };
        // Old finished run (31 days ago) → pruned by the default 30-day
        // retention.
        state
            .job_runs
            .record(&mk("run-old", now - 31 * day, Some(now - 31 * day)))
            .await
            .unwrap();
        // Old but UNFINISHED run (crashed scheduler cycle) → never deleted
        // (port contract: only finished rows are pruned).
        state
            .job_runs
            .record(&mk("run-stuck", now - 40 * day, None))
            .await
            .unwrap();
        // Recent finished run → kept.
        state
            .job_runs
            .record(&mk("run-new", now - 60, Some(now - 30)))
            .await
            .unwrap();

        let log = JobRunLog::new();
        job_history_cleanup(state.clone(), log.clone())
            .await
            .unwrap();

        let remaining: Vec<String> = state
            .job_runs
            .list_recent(50)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert!(
            !remaining.contains(&"run-old".to_string()),
            "old finished run must be pruned"
        );
        assert!(
            remaining.contains(&"run-stuck".to_string()),
            "unfinished run must never be pruned"
        );
        assert!(
            remaining.contains(&"run-new".to_string()),
            "recent run must be kept"
        );

        let lines = log.take().await;
        assert!(lines.iter().any(|l| l.contains("pruned 1")));
    }

    // ── oauth_state_cleanup ──────────────────────────────────────────────────

    fn state_row(state: &str, expires_at: u64) -> OAuthStateRecord {
        OAuthStateRecord {
            state: state.to_string(),
            tenant_id: "default".to_string(),
            workspace_id: None,
            initiated_by: "user-admin".to_string(),
            pkce_verifier: "verifier".to_string(),
            created_at: expires_at - 600,
            expires_at,
            consumed: false,
            provider_config: "{}".to_string(),
        }
    }

    #[tokio::test]
    async fn oauth_state_cleanup_deletes_only_expired_rows() {
        let state = crate::mem::test_state();
        let now = now_secs();
        state
            .oauth_states
            .create(&state_row("st-expired", now - 10))
            .await
            .unwrap();
        state
            .oauth_states
            .create(&state_row("st-valid", now + 600))
            .await
            .unwrap();
        // An expired but already-consumed row is still just an expired row:
        // cleanup deletes it too.
        let mut consumed = state_row("st-consumed-expired", now - 100);
        consumed.consumed = true;
        state.oauth_states.create(&consumed).await.unwrap();

        let log = JobRunLog::new();
        oauth_state_cleanup(state.clone(), log.clone())
            .await
            .unwrap();

        assert!(
            state
                .oauth_states
                .find_by_state("st-expired")
                .await
                .unwrap()
                .is_none(),
            "expired state must be deleted"
        );
        assert!(
            state
                .oauth_states
                .find_by_state("st-consumed-expired")
                .await
                .unwrap()
                .is_none(),
            "expired consumed state must be deleted"
        );
        assert!(
            state
                .oauth_states
                .find_by_state("st-valid")
                .await
                .unwrap()
                .is_some(),
            "unexpired state must survive"
        );
        let lines = log.take().await;
        assert!(lines.iter().any(|l| l.contains("deleted 2")));
    }

    // ── remote_agent_cleanup ─────────────────────────────────────────────────

    /// Wait until `child` exits; panic after 10 s if it has not (the
    /// cleanup's SIGTERM must have reached it).
    async fn await_exit(child: &mut std::process::Child, what: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if child.try_wait().unwrap().is_some() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the {what} instance was not killed within 10 s"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    #[tokio::test]
    async fn remote_agent_cleanup_kills_orphaned_local_instance() {
        let state = crate::mem::test_state();
        // A real process on the local compute target, recorded as a
        // spawned instance whose agent row does not exist — the shape a
        // server restart leaves behind (in-memory registries lost, durable
        // record still running).
        let mut child = Command::new("sleep").arg("300").spawn().unwrap();
        let pid = child.id();
        state
            .agent_instances
            .record_spawned(&AgentInstanceRecord {
                agent_id: "agent-orphan".to_string(),
                backend: "local".to_string(),
                target_type: "local".to_string(),
                handle_id: pid.to_string(),
                pid: Some(pid),
                spawned_at: now_secs(),
                terminated_at: None,
                terminated_reason: None,
            })
            .await
            .unwrap();

        let log = JobRunLog::new();
        remote_agent_cleanup(state.clone(), log.clone())
            .await
            .unwrap();

        // The process is actually gone (killed via ComputeTarget::kill_process).
        await_exit(&mut child, "orphaned").await;

        // The durable record is marked terminated by the sweep.
        let record = state
            .agent_instances
            .find_by_agent("agent-orphan")
            .await
            .unwrap()
            .expect("record must survive");
        assert!(record.terminated_at.is_some());
        assert_eq!(
            record.terminated_reason.as_deref(),
            Some("remote_agent_cleanup")
        );

        // The run logs name the reaped instance.
        let lines = log.take().await;
        assert!(
            lines
                .iter()
                .any(|l| l.contains("agent-orphan") && l.contains("killed")),
            "run logs must name the reaped instance, got: {lines:?}"
        );
    }

    #[tokio::test]
    async fn remote_agent_cleanup_leaves_live_agents_untouched() {
        let state = crate::mem::test_state();
        // A live agent with a running instance: the sweep must not touch it.
        let mut agent = gyre_domain::Agent::new(Id::new("agent-live"), "live", now_secs());
        agent.status = AgentStatus::Active;
        state.agents.create(&agent).await.unwrap();

        let mut child = Command::new("sleep").arg("300").spawn().unwrap();
        let pid = child.id();
        state
            .agent_instances
            .record_spawned(&AgentInstanceRecord {
                agent_id: "agent-live".to_string(),
                backend: "local".to_string(),
                target_type: "local".to_string(),
                handle_id: pid.to_string(),
                pid: Some(pid),
                spawned_at: now_secs(),
                terminated_at: None,
                terminated_reason: None,
            })
            .await
            .unwrap();

        let log = JobRunLog::new();
        remote_agent_cleanup(state.clone(), log.clone())
            .await
            .unwrap();

        // Process still running, record still marked running.
        assert!(
            child.try_wait().unwrap().is_none(),
            "a live agent's instance must not be killed"
        );
        let record = state
            .agent_instances
            .find_by_agent("agent-live")
            .await
            .unwrap()
            .expect("record must survive");
        assert!(
            record.terminated_at.is_none(),
            "live agent's record must stay running"
        );

        // Cleanup: kill the leftover test process.
        child.kill().unwrap();
        let _ = child.wait();

        let lines = log.take().await;
        assert!(
            lines
                .iter()
                .any(|l| l.contains("0 recorded-running") || l.contains("reaped 0")),
            "no instance may be reaped for a live agent, got: {lines:?}"
        );
    }

    #[tokio::test]
    async fn remote_agent_cleanup_kills_dead_agents_instance() {
        let state = crate::mem::test_state();
        let mut agent = gyre_domain::Agent::new(Id::new("agent-dead"), "dead", now_secs());
        agent.status = AgentStatus::Dead;
        state.agents.create(&agent).await.unwrap();

        let mut child = Command::new("sleep").arg("300").spawn().unwrap();
        let pid = child.id();
        state
            .agent_instances
            .record_spawned(&AgentInstanceRecord {
                agent_id: "agent-dead".to_string(),
                backend: "local".to_string(),
                target_type: "local".to_string(),
                handle_id: pid.to_string(),
                pid: Some(pid),
                spawned_at: now_secs(),
                terminated_at: None,
                terminated_reason: None,
            })
            .await
            .unwrap();

        let log = JobRunLog::new();
        remote_agent_cleanup(state.clone(), log.clone())
            .await
            .unwrap();

        await_exit(&mut child, "dead agent's").await;
        let record = state
            .agent_instances
            .find_by_agent("agent-dead")
            .await
            .unwrap()
            .expect("record must survive");
        assert!(record.terminated_at.is_some());
        let lines = log.take().await;
        assert!(lines
            .iter()
            .any(|l| l.contains("agent-dead") && l.contains("agent status Dead")));
    }

    /// Review task-273: a restart-orphan killed by recorded pid must be
    /// identity-verified first — a pid whose kernel start time does not
    /// match the recorded spawn time (a recycled pid) is NOT killed and
    /// the record is left for manual verification.
    #[tokio::test]
    async fn remote_agent_cleanup_refuses_recycled_pid() {
        let state = crate::mem::test_state();
        // A REAL live process, but the durable record claims it was
        // spawned 10 days ago — the pid cannot be the recorded instance's
        // process (recycled pid suspect).
        let mut child = Command::new("sleep").arg("300").spawn().unwrap();
        let pid = child.id();
        state
            .agent_instances
            .record_spawned(&AgentInstanceRecord {
                agent_id: "agent-recycled".to_string(),
                backend: "local".to_string(),
                target_type: "local".to_string(),
                handle_id: pid.to_string(),
                pid: Some(pid),
                spawned_at: now_secs().saturating_sub(10 * 86_400),
                terminated_at: None,
                terminated_reason: None,
            })
            .await
            .unwrap();

        let log = JobRunLog::new();
        remote_agent_cleanup(state.clone(), log.clone())
            .await
            .unwrap();

        // The unrelated process was NOT killed...
        assert!(
            child.try_wait().unwrap().is_none(),
            "a pid that does not match the recorded spawn time must not be killed"
        );
        child.kill().unwrap();
        let _ = child.wait();

        // ...the record stays running (left for manual verification), and
        // the run log says why.
        let record = state
            .agent_instances
            .find_by_agent("agent-recycled")
            .await
            .unwrap()
            .expect("record must survive");
        assert!(record.terminated_at.is_none());
        let lines = log.take().await;
        assert!(
            lines.iter().any(|l| l.contains("agent-recycled")
                && l.contains("NOT REAPED")
                && l.contains("recycled pid")),
            "run log must name the refused kill and why, got: {lines:?}"
        );
    }

    /// Review task-273: an orphan whose process already exited is marked
    /// terminated (nothing to kill), not left recorded-running forever.
    #[tokio::test]
    async fn remote_agent_cleanup_marks_already_exited_orphan() {
        let state = crate::mem::test_state();
        // A short-lived process: record its pid AFTER it exited.
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        let status = child.wait().unwrap();
        assert!(status.success());
        state
            .agent_instances
            .record_spawned(&AgentInstanceRecord {
                agent_id: "agent-exited".to_string(),
                backend: "local".to_string(),
                target_type: "local".to_string(),
                handle_id: pid.to_string(),
                pid: Some(pid),
                spawned_at: now_secs(),
                terminated_at: None,
                terminated_reason: None,
            })
            .await
            .unwrap();

        let log = JobRunLog::new();
        remote_agent_cleanup(state.clone(), log.clone())
            .await
            .unwrap();

        let record = state
            .agent_instances
            .find_by_agent("agent-exited")
            .await
            .unwrap()
            .expect("record must survive");
        assert!(
            record.terminated_at.is_some(),
            "an already-exited orphan must be marked terminated"
        );
        assert_eq!(
            record.terminated_reason.as_deref(),
            Some("remote_agent_cleanup")
        );
        let lines = log.take().await;
        assert!(lines
            .iter()
            .any(|l| l.contains("agent-exited") && l.contains("already exited")));
    }

    /// Review task-273: an unreapable backend (SSH — connection details
    /// are not persisted) is retried while young, then RETIRED with an
    /// escalation once older than the retention — the sweep must not
    /// re-emit the same dead entry forever.
    #[tokio::test]
    async fn remote_agent_cleanup_retires_aged_unreapable_instance() {
        let state = crate::mem::test_state();
        // Young SSH orphan: reported, kept running (retry window open).
        state
            .agent_instances
            .record_spawned(&AgentInstanceRecord {
                agent_id: "agent-ssh-young".to_string(),
                backend: "ssh".to_string(),
                target_type: "ssh".to_string(),
                handle_id: "ssh-handle".to_string(),
                pid: None,
                spawned_at: now_secs(),
                terminated_at: None,
                terminated_reason: None,
            })
            .await
            .unwrap();
        // Aged SSH orphan: past the retention — retired.
        state
            .agent_instances
            .record_spawned(&AgentInstanceRecord {
                agent_id: "agent-ssh-old".to_string(),
                backend: "ssh".to_string(),
                target_type: "ssh".to_string(),
                handle_id: "ssh-handle-old".to_string(),
                pid: None,
                spawned_at: now_secs().saturating_sub(UNREAPABLE_RETENTION_SECS + 86_400),
                terminated_at: None,
                terminated_reason: None,
            })
            .await
            .unwrap();

        let log = JobRunLog::new();
        remote_agent_cleanup(state.clone(), log.clone())
            .await
            .unwrap();

        // Young: still recorded-running, NOT REAPED with the retry window.
        let young = state
            .agent_instances
            .find_by_agent("agent-ssh-young")
            .await
            .unwrap()
            .unwrap();
        assert!(young.terminated_at.is_none());
        // Old: retired — recorded as terminated with the escalation
        // reason, so the sweep stops re-emitting it.
        let old = state
            .agent_instances
            .find_by_agent("agent-ssh-old")
            .await
            .unwrap()
            .unwrap();
        assert!(
            old.terminated_at.is_some(),
            "an unreapable instance past the retention must be retired"
        );
        assert_eq!(old.terminated_reason.as_deref(), Some("retired_unreapable"));

        let lines = log.take().await;
        assert!(lines.iter().any(|l| l.contains("agent-ssh-old")
            && l.contains("RETIRED")
            && l.contains("ESCALATION")));
        assert!(lines.iter().any(|l| l.contains("agent-ssh-young")
            && l.contains("NOT REAPED")
            && l.contains("retries")));

        // The next sweep no longer emits the retired entry (it is no
        // longer in the running set).
        let log2 = JobRunLog::new();
        remote_agent_cleanup(state.clone(), log2.clone())
            .await
            .unwrap();
        let lines2 = log2.take().await;
        assert!(
            !lines2.iter().any(|l| l.contains("agent-ssh-old")),
            "a retired instance must not be re-emitted, got: {lines2:?}"
        );
    }
}
