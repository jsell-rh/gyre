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

        // jj does not create intermediate directories — `workspace add`
        // fails with "Cannot access <path>" when the workspace's parent
        // (e.g. `{repo}/workspaces/`) does not exist yet. Verified against
        // jj 0.39.0. Same treatment as `jj_main_checkout_init`.
        if let Some(parent) = std::path::Path::new(workspace_path).parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("create workspace parent dir {}", parent.display())
            })?;
        }

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

    // ── TASK-106 recorded-fixture tests ─────────────────────────────────────
    //
    // These run in CI (a `jj_available()` precondition skips them only when
    // the binary is genuinely absent). They pin the external-tool contracts
    // the adapter depends on, against the real jj binary — the flaw class
    // from specs/reviews/task-106.md F1: jj writes rebase progress to
    // stderr (stdout is empty), and conflict state is an exit-code
    // contract, not a string in the output.

    /// Set up a real bare git repo (default branch `main`, one commit on
    /// it) plus a standalone jj main checkout backed by it — the
    /// source-control.md §4 layout: `{repo.path}` bare, `{repo.path}/jj-main`
    /// shared jj checkout, `{repo.path}/workspaces/<name>` workspaces.
    async fn fixture_shared_checkout(dir: &tempfile::TempDir) -> String {
        let repo_path = dir.path().join("repo.git");
        std::fs::create_dir_all(&repo_path).unwrap();
        git(&repo_path, &["init", "--bare", "--initial-branch=main"]);
        git(&repo_path, &["config", "user.email", "test@gyre.local"]);
        git(&repo_path, &["config", "user.name", "Gyre Test"]);

        // Seed `main` with one commit via a scratch clone.
        let scratch = dir.path().join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        git(&scratch, &["init", "--initial-branch=main"]);
        git(&scratch, &["config", "user.email", "test@gyre.local"]);
        git(&scratch, &["config", "user.name", "Gyre Test"]);
        std::fs::write(scratch.join("base.txt"), "base\n").unwrap();
        git(&scratch, &["add", "."]);
        git(&scratch, &["commit", "-m", "base"]);
        git(&scratch, &["push", repo_path.to_str().unwrap(), "main"]);

        // Shared jj main checkout backed by the bare repo (standalone, not
        // colocated — jj refuses colocated repos inside git worktrees).
        let main_checkout = dir.path().join("jj-main");
        let adapter = JjOpsAdapter::new();
        adapter
            .jj_main_checkout_init(
                main_checkout.to_str().unwrap(),
                repo_path.to_str().unwrap(),
            )
            .await
            .expect("jj main checkout init");
        repo_path.to_str().unwrap().to_string()
    }

    fn git(repo: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("git binary");
        assert!(
            out.status.success(),
            "git {args:?} in {repo:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }


    /// Run jj directly (test harness), asserting success.
    async fn jj_direct(cwd: &std::path::Path, args: &[&str]) {
        let out = Command::new("jj")
            .current_dir(cwd)
            .args(args)
            .output()
            .await
            .expect("jj binary");
        assert!(
            out.status.success(),
            "jj {args:?} in {cwd:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// F1 contract, clean path: after the target branch moves in the git
    /// repo, `jj_rebase` succeeds, reports the moved stack, and detects no
    /// conflicts. Pinned against real jj 0.39.0: rebase progress is on
    /// stderr, `jj resolve --list` exits 2 when clean.
    #[tokio::test]
    async fn jj_rebase_clean_after_target_moves() {
        if !jj_available() {
            return;
        }
        let dir = tempfile::TempDir::new().unwrap();
        let repo_path = fixture_shared_checkout(&dir).await;
        let main_checkout = dir.path().join("jj-main");

        // Agent workspace on a branch off main.
        let ws = dir.path().join("workspaces").join("agent-1");
        let adapter = JjOpsAdapter::new();
        adapter
            .jj_workspace_add(
                main_checkout.to_str().unwrap(),
                ws.to_str().unwrap(),
                "agent-1",
                "main",
                "agent work",
            )
            .await
            .expect("workspace add");
        // In-progress work lives in @ (the working-copy change) — exactly
        // the production flow: spawn's jj_new creates the change, the
        // agent edits files, nothing is committed until MR time.
        std::fs::write(ws.join("feature.txt"), "agent change\n").unwrap();

        // Target branch moves in the bare repo (another agent's MR merged):
        // new commit on main, pushed.
        let scratch = dir.path().join("scratch");
        std::fs::write(scratch.join("landed.txt"), "landed\n").unwrap();
        git(&scratch, &["add", "."]);
        git(&scratch, &["commit", "-m", "landed on main"]);
        git(&scratch, &["push", &repo_path, "main"]);

        // The agent's in-flight stack moves onto the new main.
        // The agent's in-flight work moves onto the new main.
        let outcome = adapter
            .jj_rebase(ws.to_str().unwrap(), "@", "main")
            .await
            .expect("rebase must succeed");
        match outcome {
            JjRebaseOutcome::Success { rebased_count } => {
                // The workspace's working-copy change (with the edit in it)
                // was rebased onto the new main.
                assert!(
                    rebased_count >= 1,
                    "expected >=1 rebased commit, got {rebased_count}"
                );
            }
            JjRebaseOutcome::Conflict { .. } => panic!("disjoint files must not conflict"),
        }

        // The rebase really moved the stack onto the new main: @'s parent
        // is the moved main tip, and the in-progress edit survived.
        let out = Command::new("jj")
            .current_dir(&ws)
            .args([
                "log",
                "-r",
                "@-",
                "--no-graph",
                "--color",
                "never",
                "-T",
                "commit_id",
            ])
            .output()
            .await
            .expect("jj binary");
        assert!(
            out.status.success(),
            "jj log failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let parent = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let scratch_head = git_head(&dir);
        assert_eq!(
            parent, scratch_head,
            "@- must be the moved main tip after rebase"
        );
        assert!(
            std::fs::read_to_string(ws.join("feature.txt"))
                .is_ok_and(|c| c == "agent change\n"),
            "in-progress edit must survive the rebase in @"
        );
    }

    /// F1 contract, conflict path: overlapping edits on the same file
    /// produce a Conflict outcome carrying the conflicted file list (jj's
    /// conflict-as-state model — `jj resolve --list` exits 0 and lists
    /// paths on stdout).
    #[tokio::test]
    async fn jj_rebase_conflict_surfaces_files() {
        if !jj_available() {
            return;
        }
        let dir = tempfile::TempDir::new().unwrap();
        let repo_path = fixture_shared_checkout(&dir).await;
        let main_checkout = dir.path().join("jj-main");

        let ws = dir.path().join("workspaces").join("agent-2");
        let adapter = JjOpsAdapter::new();
        adapter
            .jj_workspace_add(
                main_checkout.to_str().unwrap(),
                ws.to_str().unwrap(),
                "agent-2",
                "main",
                "agent work",
            )
            .await
            .expect("workspace add");
        // Same file, same region as the incoming change — work in @.
        std::fs::write(ws.join("base.txt"), "agent edit\n").unwrap();

        // Conflicting movement of main.
        let scratch = dir.path().join("scratch");
        std::fs::write(scratch.join("base.txt"), "main edit\n").unwrap();
        git(&scratch, &["add", "."]);
        git(&scratch, &["commit", "-m", "main edits base.txt"]);
        git(&scratch, &["push", &repo_path, "main"]);

        let outcome = adapter
            .jj_rebase(ws.to_str().unwrap(), "@", "main")
            .await
            .expect("rebase itself must succeed (conflicts are state)");
        match outcome {
            JjRebaseOutcome::Conflict { files, .. } => {
                assert!(
                    files.iter().any(|f| f.ends_with("base.txt")),
                    "conflicted file list must contain base.txt, got {files:?}"
                );
            }
            JjRebaseOutcome::Success { .. } => panic!("same-region edits must conflict"),
        }
    }


    /// F1 exit-code contract pin: `jj resolve --list -r <rev>` exits 2
    /// when the revision is clean and exits 0 listing conflicted paths on
    /// stdout when it is not — the contract `jj_rebase` uses to classify
    /// a completed rebase (conflict-as-state, not an error).
    #[tokio::test]
    async fn jj_resolve_list_exit_code_contract() {
        if !jj_available() {
            return;
        }
        let dir = tempfile::TempDir::new().unwrap();
        let r = dir.path().join("r");
        std::fs::create_dir_all(&r).unwrap();
        jj_direct(&r, &["git", "init", "--colocate"]).await;

        // Clean revision: no conflicts anywhere.
        let out = Command::new("jj")
            .current_dir(&r)
            .args(["resolve", "--list", "-r", "@"])
            .output()
            .await
            .expect("jj binary");
        assert_eq!(
            out.status.code(),
            Some(2),
            "clean rev must exit 2, stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        // Conflicted revision: a merge of two divergent edits to c.txt.
        std::fs::write(r.join("c.txt"), "one\n").unwrap();
        jj_direct(&r, &["commit", "-m", "one"]).await;
        let one = current_commit_id(&r).await;
        jj_direct(&r, &["new", "root()"]).await;
        std::fs::write(r.join("c.txt"), "two\n").unwrap();
        jj_direct(&r, &["commit", "-m", "two"]).await;
        let two = current_commit_id(&r).await;
        // Merge of the two divergent changes: c.txt conflicts.
        jj_direct(&r, &["new", &one, &two]).await;
        let out = Command::new("jj")
            .current_dir(&r)
            .args(["resolve", "--list", "-r", "@"])
            .output()
            .await
            .expect("jj binary");
        assert_eq!(
            out.status.code(),
            Some(0),
            "conflicted rev must exit 0, stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            stdout.lines().any(|l| l.contains("c.txt")),
            "conflicted path c.txt must be listed on stdout, got: {stdout}"
        );
    }

    /// F5 contract pin: `jj rebase -b @` moves the WHOLE branch containing
    /// the working copy — a stacked change below @ moves with it (the
    /// spec's "agent's in-progress work", not just the working-copy commit
    /// that `-r @` would move).
    #[tokio::test]
    async fn jj_rebase_b_moves_whole_stack() {
        if !jj_available() {
            return;
        }
        let dir = tempfile::TempDir::new().unwrap();
        let r = dir.path().join("r");
        std::fs::create_dir_all(&r).unwrap();
        jj_direct(&r, &["git", "init", "--colocate"]).await;
        std::fs::write(r.join("f.txt"), "base\n").unwrap();
        jj_direct(&r, &["commit", "-m", "base"]).await;
        let old_main = current_commit_id(&r).await;

        // Agent stack: two committed changes on top of the old main, then
        // a working-copy change on top of those.
        std::fs::write(r.join("stack.txt"), "one\n").unwrap();
        jj_direct(&r, &["commit", "-m", "stack one"]).await;
        std::fs::write(r.join("top.txt"), "two\n").unwrap();
        jj_direct(&r, &["commit", "-m", "stack two"]).await;
        let stack_tip = current_change_id(&r).await;

        // Another agent's MR lands: sibling commit on the old main.
        jj_direct(&r, &["new", "-r", &old_main, "-m", "landed"]).await;
        std::fs::write(r.join("landed.txt"), "landed\n").unwrap();
        jj_direct(&r, &["commit", "-m", "landed work"]).await;
        let new_main = current_commit_id(&r).await;

        // Agent's working change back on top of their stack.
        jj_direct(&r, &["new", "-r", &stack_tip, "-m", "agent wip"]).await;
        std::fs::write(r.join("wip.txt"), "wip\n").unwrap();

        let outcome = JjOpsAdapter::new()
            .jj_rebase(r.to_str().unwrap(), "@", &new_main)
            .await
            .expect("rebase must succeed");
        assert!(
            matches!(outcome, JjRebaseOutcome::Success { rebased_count } if rebased_count >= 3),
            "stack rebase must move all 3 changes (stack one, stack two, wip), got {outcome:?}"
        );

        // Every change of the stack is now above the new base.
        let out = Command::new("jj")
            .current_dir(&r)
            .args([
                "log",
                "-r",
                &format!("{new_main}..@"),
                "--no-graph",
                "--color",
                "never",
                "-T",
                "description.first_line() ++ \"\\n\"",
            ])
            .output()
            .await
            .expect("jj binary");
        assert!(
            out.status.success(),
            "jj log failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let above: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .collect();
        assert_eq!(
            above,
            vec!["agent wip", "stack two", "stack one"],
            "the whole stack must be above the new base"
        );
    }

    /// Current commit id of @- (the last committed change) — used to
    /// navigate back to a stack tip by commit id.
    async fn current_commit_id(cwd: &std::path::Path) -> String {
        let out = Command::new("jj")
            .current_dir(cwd)
            .args([
                "log",
                "-r",
                "@-",
                "--no-graph",
                "--color",
                "never",
                "-T",
                "commit_id",
            ])
            .output()
            .await
            .expect("jj binary");
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Current change id of @- — stable across rebases, unlike commit ids.
    async fn current_change_id(cwd: &std::path::Path) -> String {
        let out = Command::new("jj")
            .current_dir(cwd)
            .args([
                "log",
                "-r",
                "@-",
                "--no-graph",
                "--color",
                "never",
                "-T",
                "change_id",
            ])
            .output()
            .await
            .expect("jj binary");
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// HEAD of the scratch clone (the pushed main tip).
    fn git_head(dir: &tempfile::TempDir) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path().join("scratch"))
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git binary");
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
}
