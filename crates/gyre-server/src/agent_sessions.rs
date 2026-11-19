//! Agent session lifecycle: persistence and termination-time observation of
//! the `Session` entity (specs/system/source-control.md §Agent-to-Commit
//! Tracking).
//!
//! A session is one context window of an agent: created at spawn and at each
//! Ralph-loop respawn with fresh context, ended on every termination path
//! (complete_agent, fail_agent, and the stale-agent Dead transition). At
//! termination the session records the worktree's real current branch and
//! dirty state — queried against the real VCS, never copied forward from the
//! spawn-time values.

use gyre_common::Id;
use gyre_domain::{Agent, AgentWorktree, Session};
use std::sync::Arc;
use tracing::warn;

use crate::AppState;

/// Resolve the git binary the same way the git-http surface does.
fn git_bin() -> String {
    std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string())
}

/// Start a new open session for an agent (spawn or Ralph-loop respawn —
/// one session per context window). Best-effort: a failure to persist is
/// logged, never fatal to the spawn (sessions are tracking metadata).
pub async fn start_session(
    state: &Arc<AppState>,
    agent_id: &Id,
    repository_id: &Id,
    initial_branch: &str,
) {
    let session = Session::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        agent_id.clone(),
        repository_id.clone(),
        crate::api::now_secs(),
        initial_branch,
    );
    if let Err(e) = state.agent_sessions.create(&session).await {
        warn!(
            agent_id = %agent_id,
            error = %e,
            "failed to persist agent session (tracking degraded, spawn continues)"
        );
    }
}

/// Termination-path bookkeeping: resolve the agent's currently-open session,
/// observe the worktree's real state (current branch, uncommitted work), and
/// persist `ended_at` plus the observations.
///
/// The current branch is resolved with a real lookup against the agent's
/// worktree (never the spawn-time value copied forward). The dirty state is
/// a real `git status --porcelain` query in the worktree; a missing or
/// inaccessible worktree records an explicit unknown state (`dirty = None`
/// plus `dirty_detail` naming why) — never a silent clean.
///
/// Best-effort: a failure to persist never fails the termination path.
pub async fn end_session(state: &Arc<AppState>, agent: &Agent) {
    let mut session = match state.agent_sessions.find_open_by_agent(&agent.id).await {
        Ok(Some(s)) => s,
        Ok(None) => return, // no open session (e.g. already terminated)
        Err(e) => {
            warn!(
                agent_id = %agent.id,
                error = %e,
                "session termination: cannot resolve open session — skipping"
            );
            return;
        }
    };

    let worktree = state
        .worktrees
        .find_by_agent(&agent.id)
        .await
        .ok()
        .and_then(|wts| wts.into_iter().next());

    // Current branch at session end — real worktree lookup.
    session.final_branch = match &worktree {
        Some(wt) => current_branch_in(&wt.path).await,
        None => None,
    };

    // Dirty state — real `git status --porcelain` in the worktree.
    match &worktree {
        Some(wt) => match status_porcelain(&wt.path).await {
            Ok(paths) => {
                session.dirty = Some(!paths.is_empty());
                session.uncommitted_paths = paths;
                session.dirty_detail = None;
            }
            Err(e) => {
                // Inaccessible worktree: explicit unknown state.
                session.dirty = None;
                session.uncommitted_paths = Vec::new();
                session.dirty_detail = Some(format!("worktree status query failed: {e}"));
            }
        },
        None => {
            // No worktree record (e.g. already cleaned up, or an agent that
            // never had one): explicit unknown state, not a silent clean.
            session.dirty = None;
            session.uncommitted_paths = Vec::new();
            session.dirty_detail = Some("no worktree record for agent".to_string());
        }
    }

    session.ended_at = Some(crate::api::now_secs());
    if let Err(e) = state.agent_sessions.update(&session).await {
        warn!(
            agent_id = %agent.id,
            session_id = %session.id,
            error = %e,
            "session termination: failed to persist ended session"
        );
    }
}

/// The branch checked out in `path`, via `git rev-parse --abbrev-ref HEAD`.
/// `None` when the worktree is missing, unreadable, or in a detached state.
pub(crate) async fn current_branch_in(path: &str) -> Option<String> {
    let out = tokio::process::Command::new(git_bin())
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        warn!(
            path,
            stderr = %String::from_utf8_lossy(&out.stderr),
            "session observation: rev-parse HEAD failed"
        );
        return None;
    }
    let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        // Detached HEAD has no branch name to record.
        None
    } else {
        Some(branch)
    }
}

/// Uncommitted paths in `path`, via `git status --porcelain`.
/// Errors when the worktree is missing or git fails — the caller records an
/// explicit unknown state.
async fn status_porcelain(path: &str) -> anyhow::Result<Vec<String>> {
    let out = tokio::process::Command::new(git_bin())
        .arg("-C")
        .arg(path)
        .args(["status", "--porcelain"])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("git status failed to run: {e}"))?;
    if !out.status.success() {
        anyhow::bail!(
            "git status failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            // Porcelain v1: "XY <path>" (rename: "XY <old> -> <new>").
            // `get(3..)` is char-boundary safe (returns None otherwise).
            line.get(3..).map(|rest| match rest.split_once(" -> ") {
                Some((_, new)) => new.to_string(),
                None => rest.to_string(),
            })
        })
        .collect())
}

/// Files changed by commit `sha` in the repo at `repo_path`, via
/// `git diff-tree --no-commit-id --name-only -r --root <sha>`.
/// Empty when the query fails (the caller records the commit regardless).
pub(crate) async fn files_changed_for_commit(repo_path: &str, sha: &str) -> Vec<String> {
    // Only a 40-char hex SHA may reach the command line — anything else is
    // either garbage or an injection attempt (same guard as git_refs).
    if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        warn!(sha, "files_changed: invalid SHA rejected");
        return Vec::new();
    }
    let out = match tokio::process::Command::new(git_bin())
        .arg("-C")
        .arg(repo_path)
        .args([
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "--root",
            sha,
        ])
        .output()
        .await
    {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            warn!(
                sha,
                stderr = %String::from_utf8_lossy(&o.stderr),
                "files_changed: git diff-tree failed"
            );
            return Vec::new();
        }
        Err(e) => {
            warn!(sha, "files_changed: git failed to run: {e}");
            return Vec::new();
        }
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

/// One commit observed during a session's active window (time-travel view).
#[derive(Debug, Clone, serde::Serialize)]
pub struct WindowCommit {
    pub sha: String,
    pub timestamp: u64,
    pub author: String,
    pub subject: String,
}

/// The completion-point snapshot for a session (time-travel view): the
/// newest `refs/agents/{agent-id}/snapshots/{n}` ref whose commit existed
/// by the session's end.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompletionSnapshot {
    pub ref_name: String,
    pub commit_sha: String,
    pub committed_at: u64,
    pub tree_sha: String,
    pub files: Vec<String>,
}

/// Commits present in the repo during `[started_at, ended_at]` — a real
/// `git log --after/--before` query against the object store (`--all`
/// refs), never current HEAD. Returns an empty list when the repo cannot
/// be queried (the caller surfaces the error state).
pub(crate) async fn commits_in_window(
    repo_path: &str,
    started_at: u64,
    ended_at: u64,
) -> anyhow::Result<Vec<WindowCommit>> {
    let out = tokio::process::Command::new(git_bin())
        .arg("-C")
        .arg(repo_path)
        .args(["log", "--all"])
        .args([
            "--format=%H%x1f%ct%x1f%an%x1f%s".to_string(),
            format!("--after={}", started_at.saturating_sub(1)),
            format!("--before={}", ended_at.saturating_add(1)),
        ])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("git log failed to run: {e}"))?;
    if !out.status.success() {
        anyhow::bail!(
            "git log failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut commits = Vec::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let mut parts = line.splitn(4, '\u{1f}');
        let (Some(sha), Some(ts), Some(author), Some(subject)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        commits.push(WindowCommit {
            sha: sha.to_string(),
            timestamp: ts.parse().unwrap_or(0),
            author: author.to_string(),
            subject: subject.to_string(),
        });
    }
    // Oldest first.
    commits.sort_by_key(|c| c.timestamp);
    Ok(commits)
}

/// Resolve the session's completion snapshot: the newest
/// `refs/agents/{agent-id}/snapshots/{n}` ref whose target commit's
/// committer date is at or before `ended_at`. `None` when the session has
/// not ended, no snapshot qualifies, or the refs cannot be read.
pub(crate) async fn completion_snapshot(
    repo_path: &str,
    agent_id: &str,
    ended_at: u64,
) -> Option<CompletionSnapshot> {
    let prefix = format!("refs/agents/{agent_id}/snapshots/");
    if !crate::git_refs::refname_safe(&prefix) {
        return None;
    }
    let out = tokio::process::Command::new(git_bin())
        .arg("-C")
        .arg(repo_path)
        .args(["for-each-ref", "--format=%(refname) %(objectname)", &prefix])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);

    // Newest snapshot whose commit existed by the session's end.
    let mut best: Option<(u64, String, String)> = None;
    for line in text.lines().filter(|l| !l.is_empty()) {
        let (ref_name, sha) = line.split_once(' ')?;
        if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let committed_at = commit_time(repo_path, sha).await?;
        if committed_at <= ended_at && best.as_ref().is_none_or(|(t, _, _)| committed_at >= *t) {
            best = Some((committed_at, ref_name.to_string(), sha.to_string()));
        }
    }
    let (committed_at, ref_name, commit_sha) = best?;

    let tree_sha = tokio::process::Command::new(git_bin())
        .arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg(format!("{commit_sha}^{{tree}}"))
        .output()
        .await
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;

    let files = list_tree_files(repo_path, &commit_sha).await;

    Some(CompletionSnapshot {
        ref_name,
        commit_sha,
        committed_at,
        tree_sha,
        files,
    })
}

/// Committer date of `sha` (Unix seconds), via `git show -s --format=%ct`.
async fn commit_time(repo_path: &str, sha: &str) -> Option<u64> {
    let out = tokio::process::Command::new(git_bin())
        .arg("-C")
        .arg(repo_path)
        .args(["show", "-s", "--format=%ct", sha])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Paths in the tree of `sha`, via `git ls-tree -r --name-only`.
async fn list_tree_files(repo_path: &str, sha: &str) -> Vec<String> {
    let out = match tokio::process::Command::new(git_bin())
        .arg("-C")
        .arg(repo_path)
        .args(["ls-tree", "-r", "--name-only", sha])
        .output()
        .await
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

/// Resolve the pushing agent's currently-open session id for commit
/// attribution. `None` when the agent has no open session (recorded as
/// NULL — never a fabricated id) or the lookup fails (also NULL: the
/// commit must still be recorded, attributed at agent granularity).
pub async fn open_session_id(state: &Arc<AppState>, agent_id: &Id) -> Option<String> {
    match state.agent_sessions.find_open_by_agent(agent_id).await {
        Ok(Some(session)) => Some(session.id.to_string()),
        Ok(None) => None,
        Err(e) => {
            warn!(
                agent_id = %agent_id,
                error = %e,
                "commit attribution: cannot resolve open session — recording with NULL session_id"
            );
            None
        }
    }
}

/// The agent's worktree (first record), if any — used by the branch
/// lifecycle view to name the spawn-time worktree.
pub async fn worktree_for_agent(state: &Arc<AppState>, agent_id: &Id) -> Option<AgentWorktree> {
    state
        .worktrees
        .find_by_agent(agent_id)
        .await
        .ok()
        .and_then(|wts| wts.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_domain::{Agent, AgentWorktree};

    fn run_git(dir: &std::path::Path, args: &[&str]) {
        run_git_at(dir, args, None);
    }

    /// Run git with an explicit committer/author date (epoch seconds), so
    /// commit timestamps are deterministic relative to session windows
    /// (wall-clock second granularity would make window membership flaky).
    fn run_git_at(dir: &std::path::Path, args: &[&str], epoch: Option<u64>) {
        let mut cmd = std::process::Command::new("git");
        cmd.arg("-C").arg(dir).args(args);
        cmd.env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t");
        if let Some(t) = epoch {
            let date = format!("{t} +0000");
            cmd.env("GIT_AUTHOR_DATE", &date)
                .env("GIT_COMMITTER_DATE", &date);
        }
        let ok = cmd.status().unwrap().success();
        assert!(ok, "git {:?} failed in {}", args, dir.display());
    }

    /// A real git worktree on disk with one committed file, plus the state
    /// (mem repos) to observe it through.
    async fn real_worktree_state(
        tag: &str,
    ) -> (Arc<AppState>, std::path::PathBuf, tempfile::TempDir) {
        let state = test_state();
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir_all(&main).unwrap();
        run_git(&main, &["init", "-b", "main"]);
        std::fs::write(main.join("src"), "fn main() {}\n").unwrap();
        run_git(&main, &["add", "src"]);
        run_git(&main, &["commit", "-m", "init"]);

        // A real linked worktree on a second branch.
        let wt = dir.path().join("wt");
        run_git(
            &main,
            &["worktree", "add", "-b", "feat/spawn", wt.to_str().unwrap()],
        );

        let agent = Agent::new(Id::new(format!("agent-{tag}")), format!("a-{tag}"), 1000);
        state.agents.create(&agent).await.unwrap();
        let repo = gyre_domain::Repository::new(
            Id::new(format!("repo-{tag}")),
            Id::new("ws-1"),
            format!("r-{tag}"),
            main.to_str().unwrap().to_string(),
            1000,
        );
        state.repos.create(&repo).await.unwrap();
        let wt_record = AgentWorktree::new(
            Id::new(format!("wt-{tag}")),
            agent.id.clone(),
            repo.id.clone(),
            None,
            "feat/spawn",
            wt.to_str().unwrap().to_string(),
            1000,
        );
        state.worktrees.create(&wt_record).await.unwrap();
        start_session(&state, &agent.id, &repo.id, "feat/spawn").await;
        (state, wt, dir)
    }

    #[tokio::test]
    async fn end_session_records_dirty_state_from_real_worktree() {
        let (state, wt, _dir) = real_worktree_state("dirty").await;
        let agent = state
            .agents
            .find_by_id(&Id::new("agent-dirty"))
            .await
            .unwrap()
            .unwrap();

        // Uncommitted modification in the worktree.
        std::fs::write(wt.join("src"), "fn main() { changed }\n").unwrap();
        std::fs::write(wt.join("untracked.txt"), "new\n").unwrap();

        end_session(&state, &agent).await;
        let sessions = state.agent_sessions.find_by_agent(&agent.id).await.unwrap();
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert!(s.ended_at.is_some(), "termination must set ended_at");
        assert_eq!(
            s.dirty,
            Some(true),
            "uncommitted edit must record dirty=true"
        );
        assert!(
            s.uncommitted_paths.contains(&"src".to_string()),
            "modified file must be listed, got {:?}",
            s.uncommitted_paths
        );
        assert!(
            s.uncommitted_paths.contains(&"untracked.txt".to_string()),
            "untracked file must be listed, got {:?}",
            s.uncommitted_paths
        );
        assert!(s.dirty_detail.is_none());
    }

    #[tokio::test]
    async fn end_session_clean_worktree_records_clean() {
        let (state, _wt, _dir) = real_worktree_state("clean").await;
        let agent = state
            .agents
            .find_by_id(&Id::new("agent-clean"))
            .await
            .unwrap()
            .unwrap();

        end_session(&state, &agent).await;
        let sessions = state.agent_sessions.find_by_agent(&agent.id).await.unwrap();
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(
            s.dirty,
            Some(false),
            "clean worktree must record dirty=false"
        );
        assert!(s.uncommitted_paths.is_empty());
    }

    #[tokio::test]
    async fn end_session_missing_worktree_records_unknown_not_clean() {
        let state = test_state();
        let agent = Agent::new(Id::new("agent-nowt"), "nowt", 1000);
        state.agents.create(&agent).await.unwrap();
        let repo = gyre_domain::Repository::new(
            Id::new("repo-nowt"),
            Id::new("ws-1"),
            "nowt",
            "/nonexistent/repo.git",
            1000,
        );
        state.repos.create(&repo).await.unwrap();
        start_session(&state, &agent.id, &repo.id, "feat/x").await;

        end_session(&state, &agent).await;
        let sessions = state.agent_sessions.find_by_agent(&agent.id).await.unwrap();
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        // Missing worktree: explicit unknown state — a silent clean would
        // fabricate an observation that was never made.
        assert_eq!(
            s.dirty, None,
            "missing worktree must record unknown dirty state"
        );
        assert!(s.dirty_detail.is_some(), "unknown state must say why");
        assert!(s.ended_at.is_some(), "termination must still set ended_at");
    }

    #[tokio::test]
    async fn end_session_resolves_moved_branch_not_spawn_branch() {
        let (state, wt, _dir) = real_worktree_state("branch").await;
        let agent = state
            .agents
            .find_by_id(&Id::new("agent-branch"))
            .await
            .unwrap()
            .unwrap();

        // The agent moves to a different branch mid-session.
        run_git(&wt, &["switch", "-c", "feat/moved"]);

        end_session(&state, &agent).await;
        let sessions = state.agent_sessions.find_by_agent(&agent.id).await.unwrap();
        let s = &sessions[0];
        assert_eq!(
            s.final_branch.as_deref(),
            Some("feat/moved"),
            "current branch must be resolved from the worktree at termination"
        );
        assert_eq!(
            s.initial_branch, "feat/spawn",
            "spawn-time branch is preserved"
        );
    }

    #[tokio::test]
    async fn end_session_without_open_session_is_noop() {
        let state = test_state();
        let agent = Agent::new(Id::new("agent-none"), "none", 1000);
        state.agents.create(&agent).await.unwrap();
        // No session was ever started — must not panic or create one.
        end_session(&state, &agent).await;
        let sessions = state.agent_sessions.find_by_agent(&agent.id).await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn files_changed_for_commit_lists_real_paths() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir_all(main.join("src")).unwrap();
        run_git(&main, &["init", "-b", "main"]);
        std::fs::write(main.join("src").join("lib.rs"), "pub fn a() {}\n").unwrap();
        run_git(&main, &["add", "src/lib.rs"]);
        run_git(&main, &["commit", "-m", "c1"]);
        let sha = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .arg("-C")
                .arg(&main)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        let files = files_changed_for_commit(main.to_str().unwrap(), &sha).await;
        assert_eq!(files, vec!["src/lib.rs".to_string()]);

        // Root commit: --root makes diff-tree list its files.
        // Invalid SHA: rejected, empty (never reaches the command line).
        assert!(
            files_changed_for_commit(main.to_str().unwrap(), "not-a-sha")
                .await
                .is_empty()
        );
    }

    /// Same-second session ordering (the mem adapter backing test_state,
    /// same contract as the SQLite/PostgreSQL adapters): started_at has
    /// second granularity, so a terminated session and its respawn
    /// successor can share a started_at. The tie must break by session
    /// window — the ended session predates the still-open one — never by
    /// the random-UUID id, which orders the tie arbitrarily (CI run
    /// 44611030853, ralph_loop::tests::respawn_opens_a_new_session:
    /// "session 1 stays terminated"). The ids here contradict the
    /// window order ("aa-…" is the OPEN session), so a bare id tiebreak
    /// fails this test deterministically.
    #[tokio::test]
    async fn find_by_agent_same_second_tie_orders_by_session_window() {
        let state = test_state();
        let agent = Agent::new(Id::new("agent-tie"), "tie", 1000);
        state.agents.create(&agent).await.unwrap();
        let repo = gyre_domain::Repository::new(
            Id::new("repo-tie"),
            Id::new("ws-1"),
            "tie",
            "/tmp/gyre-agent-sessions-tie",
            1000,
        );
        state.repos.create(&repo).await.unwrap();

        // Both sessions started in the same wall-clock second.
        let same_second = 1_800_000_000u64;
        let mut ended = Session::new(
            Id::new("zz-ended"),
            agent.id.clone(),
            repo.id.clone(),
            same_second,
            "feat/a",
        );
        ended.ended_at = Some(same_second + 1);
        let open = Session::new(
            Id::new("aa-open"),
            agent.id.clone(),
            repo.id.clone(),
            same_second,
            "feat/a",
        );
        // Insertion order opposite to the expected result.
        state.agent_sessions.create(&open).await.unwrap();
        state.agent_sessions.create(&ended).await.unwrap();

        let sessions = state.agent_sessions.find_by_agent(&agent.id).await.unwrap();
        assert_eq!(
            sessions
                .iter()
                .map(|s| s.id.to_string())
                .collect::<Vec<_>>(),
            vec!["zz-ended".to_string(), "aa-open".to_string()],
            "same-second sessions order by session window, not random id"
        );
        assert_eq!(sessions[0].ended_at, Some(same_second + 1));
        assert_eq!(sessions[1].ended_at, None);
    }

    #[tokio::test]
    async fn time_travel_window_and_snapshot_from_real_repo() {
        let (state, _wt, dir) = real_worktree_state("tt").await;
        let repo = state
            .repos
            .find_by_id(&Id::new("repo-tt"))
            .await
            .unwrap()
            .unwrap();
        let agent = state
            .agents
            .find_by_id(&Id::new("agent-tt"))
            .await
            .unwrap()
            .unwrap();
        let main = dir.path().join("main");

        // Deterministic session window: [now-3000, now-2000].
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let started_at = now - 3000;
        let ended_at = now - 2000;
        let mut session = state
            .agent_sessions
            .find_open_by_agent(&agent.id)
            .await
            .unwrap()
            .expect("session started by the fixture");
        session.started_at = started_at;
        session.ended_at = Some(ended_at);
        state.agent_sessions.update(&session).await.unwrap();

        // A commit dated INSIDE the window.
        std::fs::write(main.join("during.txt"), "during\n").unwrap();
        run_git_at(&main, &["add", "during.txt"], Some(started_at + 100));
        run_git_at(
            &main,
            &["commit", "-m", "during session"],
            Some(started_at + 100),
        );
        let during_sha = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .arg("-C")
                .arg(&main)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        // Snapshot ref for this agent, at the session's completion point.
        run_git(
            &main,
            &[
                "update-ref",
                &format!("refs/agents/{}/snapshots/0", agent.id),
                &during_sha,
            ],
        );

        // A commit dated AFTER the session ended — must be excluded from
        // the window and from the completion snapshot.
        std::fs::write(main.join("after.txt"), "after\n").unwrap();
        run_git_at(&main, &["add", "after.txt"], Some(ended_at + 1000));
        run_git_at(
            &main,
            &["commit", "-m", "after session"],
            Some(ended_at + 1000),
        );
        let after_sha = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .arg("-C")
                .arg(&main)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        let window = commits_in_window(&repo.path, session.started_at, ended_at)
            .await
            .unwrap();
        assert!(
            window.iter().any(|c| c.sha == during_sha),
            "commit made during the session must be present: {:?}",
            window.iter().map(|c| c.sha.clone()).collect::<Vec<_>>()
        );
        assert!(
            !window.iter().any(|c| c.sha == after_sha),
            "commit pushed after the session ended must be excluded"
        );

        let snap = completion_snapshot(&repo.path, &agent.id.to_string(), ended_at)
            .await
            .expect("snapshot must resolve");
        assert_eq!(snap.commit_sha, during_sha);
        assert_eq!(
            snap.ref_name,
            format!("refs/agents/{}/snapshots/0", agent.id)
        );
        assert!(
            snap.files.iter().any(|f| f == "during.txt"),
            "snapshot tree must list the session's files, got {:?}",
            snap.files
        );
    }
}
