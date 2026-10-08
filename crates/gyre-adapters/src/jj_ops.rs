use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_ports::{JjChange, JjOpsPort, JjRebaseOutcome};
use std::env;
use tokio::process::Command;

/// Adapter that shells out to the `jj` CLI binary.
///
/// Configure the binary path via `GYRE_JJ_PATH` env var (default: `jj`).
pub struct JjOpsAdapter {
    jj_path: String,
}

impl JjOpsAdapter {
    pub fn new() -> Self {
        let jj_path = env::var("GYRE_JJ_PATH").unwrap_or_else(|_| "jj".to_string());
        Self { jj_path }
    }

    async fn run_jj(&self, repo_path: &str, args: &[&str]) -> Result<String> {
        let output = Command::new(&self.jj_path)
            .current_dir(repo_path)
            .args(args)
            .output()
            .await
            .with_context(|| format!("failed to run jj (path: {})", self.jj_path))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("jj command failed: {stderr}");
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Run jj, returning (status, stdout, stderr) without failing on
    /// non-zero exit. jj writes human-facing progress ("Rebased N commits",
    /// "New conflicts appeared") to stderr and machine-readable listings to
    /// stdout — callers need both streams separately to interpret exit-code
    /// contracts correctly.
    async fn run_jj_raw(
        &self,
        repo_path: &str,
        args: &[&str],
    ) -> Result<(std::process::ExitStatus, String, String)> {
        let output = Command::new(&self.jj_path)
            .current_dir(repo_path)
            .args(args)
            .output()
            .await
            .with_context(|| format!("failed to run jj (path: {})", self.jj_path))?;
        Ok((
            output.status,
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}


/// Parse the rebased-commit count from jj rebase's stderr line
/// "Rebased N commits to destination" (jj 0.39.0 writes progress to stderr;
/// stdout is empty). Returns 0 when the line is absent (no-op rebase).
fn parse_rebased_count(stderr: &str) -> usize {
    for line in stderr.lines() {
        if let Some(rest) = line.strip_prefix("Rebased ") {
            if let Some(n) = rest
                .split_whitespace()
                .next()
                .and_then(|tok| tok.parse::<usize>().ok())
            {
                return n;
            }
        }
    }
    0
}

impl Default for JjOpsAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Field separator unlikely to appear in commit messages or author names.
const SEP: char = '\x1f';

#[async_trait]
impl JjOpsPort for JjOpsAdapter {
    async fn jj_init(&self, repo_path: &str) -> Result<()> {
        self.run_jj(repo_path, &["git", "init", "--colocate"])
            .await?;
        Ok(())
    }

    async fn jj_new(&self, repo_path: &str, description: &str) -> Result<String> {
        self.run_jj(repo_path, &["new", "-m", description]).await?;
        // Read the current working-copy change ID
        let out = self
            .run_jj(
                repo_path,
                &[
                    "log",
                    "--no-graph",
                    "--color",
                    "never",
                    "--limit",
                    "1",
                    "-T",
                    "change_id",
                ],
            )
            .await?;
        Ok(out.trim().to_string())
    }

    async fn jj_describe(&self, repo_path: &str, change_id: &str, description: &str) -> Result<()> {
        self.run_jj(repo_path, &["describe", change_id, "-m", description])
            .await?;
        Ok(())
    }

    async fn jj_log(&self, repo_path: &str, limit: usize) -> Result<Vec<JjChange>> {
        let limit_str = limit.to_string();
        let template = "change_id ++ \"\x1f\" ++ commit_id ++ \"\x1f\" ++ description.first_line() ++ \"\x1f\" ++ author.name() ++ \"\\n\"";
        let out = self
            .run_jj(
                repo_path,
                &[
                    "log",
                    "--no-graph",
                    "--color",
                    "never",
                    "--limit",
                    &limit_str,
                    "-T",
                    template,
                ],
            )
            .await?;

        let mut changes = Vec::new();
        for line in out.lines() {
            let parts: Vec<&str> = line.splitn(4, SEP).collect();
            if parts.len() == 4 {
                changes.push(JjChange {
                    change_id: parts[0].trim().to_string(),
                    commit_id: parts[1].trim().to_string(),
                    description: parts[2].to_string(),
                    author: parts[3].to_string(),
                    timestamp: 0,
                    bookmarks: vec![],
                });
            }
        }
        Ok(changes)
    }

    async fn jj_squash(&self, repo_path: &str) -> Result<String> {
        self.run_jj(repo_path, &["squash"]).await?;
        // After squash the parent becomes the working copy — get its commit SHA.
        let sha = self
            .run_jj(
                repo_path,
                &[
                    "log",
                    "--no-graph",
                    "--color",
                    "never",
                    "--limit",
                    "1",
                    "-T",
                    "commit_id",
                ],
            )
            .await?;
        Ok(sha.trim().to_string())
    }

    async fn jj_bookmark_create(&self, repo_path: &str, name: &str, change_id: &str) -> Result<()> {
        self.run_jj(repo_path, &["bookmark", "create", name, "-r", change_id])
            .await?;
        Ok(())
    }

    async fn jj_undo(&self, repo_path: &str) -> Result<()> {
        self.run_jj(repo_path, &["op", "undo"]).await?;
        Ok(())
    }

    async fn jj_rebase(
        &self,
        workspace_path: &str,
        revision: &str,
        destination: &str,
    ) -> Result<JjRebaseOutcome> {
        // Import refs from the backing git repo first: the destination (the
        // target branch tip after a merge) is named by branch, and the git
        // ref moves outside jj's view.
        self.run_jj(workspace_path, &["git", "import"])
            .await
            .context("jj git import before rebase")?;

        // -b rebases the branch (revision + ancestors) containing the working
        // copy — the whole in-flight stack, per source-control.md §4.
        // Empirically (jj 0.39.0): exit 0 on success AND on conflicts —
        // conflicts are state, not errors. "Rebased N commits to
        // destination" arrives on stderr; stdout is empty.
        let (status, _stdout, stderr) = self
            .run_jj_raw(workspace_path, &["rebase", "-b", revision, "-d", destination])
            .await?;
        if !status.success() {
            anyhow::bail!("jj rebase failed: {stderr}");
        }
        let rebased_count = parse_rebased_count(&stderr);

        // Conflict detection via the exit-code contract (stable across jj
        // output formatting): `jj resolve --list -r <rev>` exits 0 and lists
        // the conflicted paths on stdout when conflicted; exits 2 with
        // "No conflicts found" when clean; exits 1 on an invalid revset.
        let (rl_status, rl_stdout, rl_stderr) = self
            .run_jj_raw(workspace_path, &["resolve", "--list", "-r", revision])
            .await?;
        if rl_status.success() {
            let files = rl_stdout
                .lines()
                .filter_map(|l| l.split_whitespace().next())
                .filter(|p| !p.is_empty())
                .map(|p| p.to_string())
                .collect::<Vec<_>>();
            if files.is_empty() {
                return Ok(JjRebaseOutcome::Success { rebased_count });
            }
            return Ok(JjRebaseOutcome::Conflict {
                rebased_count,
                files,
            });
        }
        match rl_status.code() {
            // 2 = "No conflicts found at this revision" — clean rebase.
            Some(2) => Ok(JjRebaseOutcome::Success { rebased_count }),
            _ => anyhow::bail!("jj resolve --list failed: {rl_stderr}"),
        }
    }

    async fn jj_main_checkout_init(
        &self,
        main_checkout_path: &str,
        git_repo_path: &str,
    ) -> Result<()> {
        // Idempotent: an existing checkout is left untouched (its store
        // points at the git repo by absolute path recorded at init time).
        if std::path::Path::new(main_checkout_path).join(".jj").exists() {
            return Ok(());
        }
        std::fs::create_dir_all(main_checkout_path)
            .with_context(|| format!("create jj main checkout dir {main_checkout_path}"))?;
        // Standalone (non-colocated) checkout backed by the bare git repo.
        // jj resolves --git-repo against the command's cwd, so absolute
        // paths are required.
        let (status, _stdout, stderr) = self
            .run_jj_raw(
                main_checkout_path,
                &["git", "init", "--git-repo", git_repo_path],
            )
            .await?;
        if !status.success() {
            anyhow::bail!("jj git init --git-repo failed: {stderr}");
        }
        Ok(())
    }

    async fn jj_workspace_add(
        &self,
        main_checkout_path: &str,
        workspace_path: &str,
        name: &str,
        revision: &str,
        description: &str,
    ) -> Result<()> {
        // Refresh jj's view of the git repo before resolving `revision` by
        // name: branches created after the main checkout was initialized are
        // invisible until `jj git import` runs there.
        self.run_jj(main_checkout_path, &["git", "import"])
            .await
            .context("jj git import before workspace add")?;

        let (status, _stdout, stderr) = self
            .run_jj_raw(
                main_checkout_path,
                &[
                    "workspace",
                    "add",
                    "--name",
                    name,
                    workspace_path,
                    "-r",
                    revision,
                    "-m",
                    description,
                ],
            )
            .await?;
        if !status.success() {
            // A failed `workspace add` can leave a half-created workspace:
            // the directory exists with `.jj/` but no files checked out,
            // still registered in the repo. Forget it so the caller's
            // fallback (git worktree) is not defeated by a bare existence
            // check on the path.
            let _ = self
                .run_jj(main_checkout_path, &["workspace", "forget", name])
                .await;
            let _ = std::fs::remove_dir_all(workspace_path);
            anyhow::bail!("jj workspace add failed: {stderr}");
        }
        Ok(())
    }

    async fn jj_workspace_forget(
        &self,
        main_checkout_path: &str,
        name: &str,
    ) -> Result<()> {
        // Forget by name from the shared checkout. jj exits non-zero when
        // the workspace is already absent — that is success for an
        // idempotent forget.
        match self
            .run_jj_raw(main_checkout_path, &["workspace", "forget", name])
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    async fn jj_git_export(&self, workspace_path: &str) -> Result<()> {
        // Push jj's bookmarks/commits into the backing git repo so
        // git-backed consumers (diff, can_merge, merge_branches) see the
        // agent's work. Idempotent ("Nothing changed." on second run).
        self.run_jj(workspace_path, &["git", "export"]).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize tests that mutate GYRE_JJ_PATH — env vars are process-global
    // and parallel test threads cause intermittent failures without this lock.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn jj_available() -> bool {
        std::process::Command::new("jj")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Unit test: JjOpsAdapter is constructible and uses GYRE_JJ_PATH.
    #[test]
    fn adapter_respects_env_path() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("GYRE_JJ_PATH", "/custom/jj") };
        let adapter = JjOpsAdapter::new();
        unsafe { std::env::remove_var("GYRE_JJ_PATH") };
        assert_eq!(adapter.jj_path, "/custom/jj");
    }

    /// Unit test: default adapter uses "jj".
    #[test]
    fn adapter_default_path() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::remove_var("GYRE_JJ_PATH") };
        let adapter = JjOpsAdapter::default();
        assert_eq!(adapter.jj_path, "jj");
    }

    /// Integration: requires jj binary. Skipped if not installed.
    #[tokio::test]
    #[ignore = "requires jj binary on PATH"]
    async fn jj_init_in_git_repo() {
        if !jj_available() {
            return;
        }
        let dir = tempfile::TempDir::new().unwrap();
        // Init a bare git repo first
        std::process::Command::new("git")
            .args(["init", dir.path().to_str().unwrap()])
            .output()
            .unwrap();

        let adapter = JjOpsAdapter::new();
        adapter
            .jj_init(dir.path().to_str().unwrap())
            .await
            .expect("jj init should succeed");
    }

    /// Integration: jj new + log. Requires jj binary.
    #[tokio::test]
    #[ignore = "requires jj binary on PATH"]
    async fn jj_new_and_log() {
        if !jj_available() {
            return;
        }
        let dir = tempfile::TempDir::new().unwrap();
        std::process::Command::new("git")
            .args(["init", dir.path().to_str().unwrap()])
            .output()
            .unwrap();

        let adapter = JjOpsAdapter::new();
        adapter.jj_init(dir.path().to_str().unwrap()).await.unwrap();

        let change_id = adapter
            .jj_new(dir.path().to_str().unwrap(), "test change")
            .await
            .expect("jj new should succeed");
        assert!(!change_id.is_empty());

        let log = adapter
            .jj_log(dir.path().to_str().unwrap(), 5)
            .await
            .expect("jj log should succeed");
        assert!(!log.is_empty());
    }

    /// Integration: jj describe. Requires jj binary.
    #[tokio::test]
    #[ignore = "requires jj binary on PATH"]
    async fn jj_describe_change() {
        if !jj_available() {
            return;
        }
        let dir = tempfile::TempDir::new().unwrap();
        std::process::Command::new("git")
            .args(["init", dir.path().to_str().unwrap()])
            .output()
            .unwrap();

        let adapter = JjOpsAdapter::new();
        adapter.jj_init(dir.path().to_str().unwrap()).await.unwrap();
        let change_id = adapter
            .jj_new(dir.path().to_str().unwrap(), "initial")
            .await
            .unwrap();
        adapter
            .jj_describe(dir.path().to_str().unwrap(), &change_id, "updated desc")
            .await
            .expect("jj describe should succeed");
    }

    /// Integration: jj undo. Requires jj binary.
    #[tokio::test]
    #[ignore = "requires jj binary on PATH"]
    async fn jj_undo_last_op() {
        if !jj_available() {
            return;
        }
        let dir = tempfile::TempDir::new().unwrap();
        std::process::Command::new("git")
            .args(["init", dir.path().to_str().unwrap()])
            .output()
            .unwrap();

        let adapter = JjOpsAdapter::new();
        adapter.jj_init(dir.path().to_str().unwrap()).await.unwrap();
        adapter
            .jj_new(dir.path().to_str().unwrap(), "to be undone")
            .await
            .unwrap();
        adapter
            .jj_undo(dir.path().to_str().unwrap())
            .await
            .expect("jj undo should succeed");
    }

    /// Integration: jj bookmark create. Requires jj binary.
    #[tokio::test]
    #[ignore = "requires jj binary on PATH"]
    async fn jj_bookmark_create() {
        if !jj_available() {
            return;
        }
        let dir = tempfile::TempDir::new().unwrap();
        std::process::Command::new("git")
            .args(["init", dir.path().to_str().unwrap()])
            .output()
            .unwrap();

        let adapter = JjOpsAdapter::new();
        adapter.jj_init(dir.path().to_str().unwrap()).await.unwrap();
        let change_id = adapter
            .jj_new(dir.path().to_str().unwrap(), "bookmark target")
            .await
            .unwrap();
        adapter
            .jj_bookmark_create(dir.path().to_str().unwrap(), "my-feature", &change_id)
            .await
            .expect("jj bookmark create should succeed");
    }

    /// Integration: jj squash. Requires jj binary.
    #[tokio::test]
    #[ignore = "requires jj binary on PATH"]
    async fn jj_squash_into_parent() {
        if !jj_available() {
            return;
        }
        let dir = tempfile::TempDir::new().unwrap();
        std::process::Command::new("git")
            .args(["init", dir.path().to_str().unwrap()])
            .output()
            .unwrap();

        let adapter = JjOpsAdapter::new();
        adapter.jj_init(dir.path().to_str().unwrap()).await.unwrap();
        // Create two changes so squash has a parent
        adapter
            .jj_new(dir.path().to_str().unwrap(), "parent change")
            .await
            .unwrap();
        adapter
            .jj_new(dir.path().to_str().unwrap(), "child change")
            .await
            .unwrap();
        // squash child into parent
        adapter
            .jj_squash(dir.path().to_str().unwrap())
            .await
            .expect("jj squash should succeed");
    }
}
