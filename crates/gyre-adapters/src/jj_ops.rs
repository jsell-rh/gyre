use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_ports::{JjChange, JjOpsPort};
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

    async fn jj_squash(&self, repo_path: &str) -> Result<()> {
        self.run_jj(repo_path, &["squash"]).await?;
        Ok(())
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        unsafe { std::env::set_var("GYRE_JJ_PATH", "/custom/jj") };
        let adapter = JjOpsAdapter::new();
        assert_eq!(adapter.jj_path, "/custom/jj");
        unsafe { std::env::remove_var("GYRE_JJ_PATH") };
    }

    /// Unit test: default adapter uses "jj".
    #[test]
    fn adapter_default_path() {
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
