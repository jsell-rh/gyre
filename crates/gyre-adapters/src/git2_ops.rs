use anyhow::{Context, Result};
use async_trait::async_trait;
use git2::{BranchType, Repository};
use gyre_domain::{BranchInfo, CommitInfo, DiffResult, FileDiff, MergeResult};
use gyre_ports::GitOpsPort;

/// Recursively insert a blob into a tree at a nested path.
///
/// For "specs/system/vision.md", this builds:
///   root tree → specs/ subtree → system/ subtree → vision.md blob
fn insert_blob_at_path(
    repo: &Repository,
    base_tree: &git2::Tree,
    path: &str,
    blob_oid: git2::Oid,
) -> Result<git2::Oid> {
    let parts: Vec<&str> = path.splitn(2, '/').collect();
    if parts.len() == 1 {
        // Leaf: insert/replace the blob directly in this tree
        let mut builder = repo.treebuilder(Some(base_tree))?;
        builder.insert(parts[0], blob_oid, 0o100644)?;
        let tree_oid = builder.write()?;
        Ok(tree_oid)
    } else {
        let dir_name = parts[0];
        let rest = parts[1];

        // Get existing subtree or create empty one
        let sub_tree = match base_tree.get_name(dir_name) {
            Some(entry) => repo.find_tree(entry.id())?,
            None => {
                let empty_builder = repo.treebuilder(None)?;
                let empty_oid = empty_builder.write()?;
                repo.find_tree(empty_oid)?
            }
        };

        let new_sub_oid = insert_blob_at_path(repo, &sub_tree, rest, blob_oid)?;

        let mut builder = repo.treebuilder(Some(base_tree))?;
        builder.insert(dir_name, new_sub_oid, 0o040000)?;
        let tree_oid = builder.write()?;
        Ok(tree_oid)
    }
}

pub struct Git2OpsAdapter;

impl Git2OpsAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Git2OpsAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GitOpsPort for Git2OpsAdapter {
    async fn init_bare(&self, path: &str) -> Result<()> {
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            if let Some(parent) = std::path::Path::new(&path).parent() {
                std::fs::create_dir_all(parent)
                    .context("failed to create parent directories for bare repo")?;
            }
            let repo = Repository::init_bare(&path).context("failed to init bare repository")?;
            // Allow pushes to branches checked out in linked worktrees. Without this,
            // git push to a branch that an agent has checked out via `git worktree add`
            // fails with "refusing to update checked out branch".
            let mut config = repo.config()?;
            config.set_str("receive.denyCurrentBranch", "ignore")?;
            Ok(())
        })
        .await?
    }

    async fn list_branches(&self, repo_path: &str) -> Result<Vec<BranchInfo>> {
        let repo_path = repo_path.to_string();
        tokio::task::spawn_blocking(move || {
            let repo = Repository::open(&repo_path).context("failed to open repository")?;

            let default_branch = repo
                .head()
                .ok()
                .and_then(|h| h.shorthand().map(str::to_string));

            let mut branches = Vec::new();
            let iter = repo.branches(Some(BranchType::Local))?;
            for item in iter {
                let (branch, _) = item?;
                let name = branch.name()?.unwrap_or("<invalid>").to_string();
                let head_sha = branch
                    .get()
                    .peel_to_commit()
                    .map(|c| c.id().to_string())
                    .unwrap_or_default();
                let is_default = default_branch.as_deref() == Some(&name);
                branches.push(BranchInfo {
                    name,
                    head_sha,
                    is_default,
                });
            }
            Ok(branches)
        })
        .await?
    }

    async fn commit_log(
        &self,
        repo_path: &str,
        branch: &str,
        limit: usize,
    ) -> Result<Vec<CommitInfo>> {
        let repo_path = repo_path.to_string();
        let branch = branch.to_string();
        tokio::task::spawn_blocking(move || {
            let repo = Repository::open(&repo_path).context("failed to open repository")?;

            // Resolve branch to commit OID — return empty if branch doesn't exist
            let branch_ref = match repo.find_branch(&branch, BranchType::Local) {
                Ok(b) => b,
                Err(_) => return Ok(Vec::new()),
            };
            let head_commit = branch_ref.get().peel_to_commit()?;

            let mut walk = repo.revwalk()?;
            walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
            walk.push(head_commit.id())?;

            let mut commits = Vec::new();
            for oid in walk.take(limit) {
                let oid = oid?;
                let commit = repo.find_commit(oid)?;
                let author = commit.author();
                commits.push(CommitInfo {
                    sha: oid.to_string(),
                    message: commit.message().unwrap_or("").trim().to_string(),
                    author: author.name().unwrap_or("").to_string(),
                    timestamp: commit.time().seconds() as u64,
                });
            }
            Ok(commits)
        })
        .await?
    }

    async fn diff(&self, repo_path: &str, from: &str, to: &str) -> Result<DiffResult> {
        let repo_path = repo_path.to_string();
        let from = from.to_string();
        let to = to.to_string();
        tokio::task::spawn_blocking(move || {
            let repo = Repository::open(&repo_path).context("failed to open repository")?;

            let from_obj = repo
                .revparse_single(&from)
                .context("failed to resolve 'from' ref")?;
            let to_obj = repo
                .revparse_single(&to)
                .context("failed to resolve 'to' ref")?;

            let from_tree = from_obj.peel_to_tree()?;
            let to_tree = to_obj.peel_to_tree()?;

            let diff = repo.diff_tree_to_tree(Some(&from_tree), Some(&to_tree), None)?;
            let stats = diff.stats()?;

            let mut patches = Vec::new();
            diff.print(git2::DiffFormat::Patch, |delta, _hunk, line| {
                let path = delta
                    .new_file()
                    .path()
                    .or_else(|| delta.old_file().path())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                let status = format!("{:?}", delta.status());
                let raw = std::str::from_utf8(line.content())
                    .unwrap_or("")
                    .to_string();
                // Prepend origin char (+/-/space/header markers) so the patch
                // text is valid unified diff that parse_patch_to_hunks can classify.
                let origin = line.origin();
                let content = match origin {
                    '+' | '-' | ' ' => format!("{}{}", origin, raw),
                    // Header / file-marker lines: keep as-is
                    _ => raw,
                };
                if let Some(existing) = patches.iter_mut().find(|p: &&mut FileDiff| p.path == path)
                {
                    if let Some(ref mut patch) = existing.patch {
                        patch.push_str(&content);
                    }
                } else {
                    patches.push(FileDiff {
                        path,
                        status,
                        patch: Some(content),
                    });
                }
                true
            })?;

            Ok(DiffResult {
                files_changed: stats.files_changed(),
                insertions: stats.insertions(),
                deletions: stats.deletions(),
                patches,
            })
        })
        .await?
    }

    async fn is_repo(&self, path: &str) -> Result<bool> {
        let path = path.to_string();
        tokio::task::spawn_blocking(move || Ok(Repository::open(&path).is_ok())).await?
    }

    async fn can_merge(&self, repo_path: &str, source: &str, target: &str) -> Result<bool> {
        let repo_path = repo_path.to_string();
        let source = source.to_string();
        let target = target.to_string();
        tokio::task::spawn_blocking(move || {
            let repo = Repository::open(&repo_path).context("failed to open repository")?;
            let source_commit = repo
                .revparse_single(&source)
                .context("failed to resolve source ref")?
                .peel_to_commit()
                .context("source is not a commit")?;
            let target_commit = repo
                .revparse_single(&target)
                .context("failed to resolve target ref")?
                .peel_to_commit()
                .context("target is not a commit")?;
            // merge_commits performs an in-memory merge; no working tree is modified
            let merge_index = repo
                .merge_commits(&target_commit, &source_commit, None)
                .context("merge_commits failed")?;
            Ok(!merge_index.has_conflicts())
        })
        .await?
    }

    async fn merge_branches(
        &self,
        repo_path: &str,
        source: &str,
        target: &str,
    ) -> Result<MergeResult> {
        let repo_path = repo_path.to_string();
        let source = source.to_string();
        let target = target.to_string();
        tokio::task::spawn_blocking(move || {
            let repo = Repository::open(&repo_path).context("failed to open repository")?;

            let source_branch = repo
                .find_branch(&source, BranchType::Local)
                .context("source branch not found")?;
            let source_commit = source_branch.get().peel_to_commit()?;

            let target_branch = repo
                .find_branch(&target, BranchType::Local)
                .context("target branch not found")?;
            let target_commit = target_branch.get().peel_to_commit()?;

            // Fast-forward check: if source is already an ancestor of target, nothing to do.
            if repo.merge_base(source_commit.id(), target_commit.id())? == source_commit.id() {
                return Ok(MergeResult::Success {
                    merge_commit_sha: target_commit.id().to_string(),
                });
            }

            // Fast-forward: if target is ancestor of source, just advance target ref.
            if repo.merge_base(source_commit.id(), target_commit.id())? == target_commit.id() {
                let refname = format!("refs/heads/{}", target);
                let mut target_ref = repo.find_reference(&refname)?;
                target_ref.set_target(
                    source_commit.id(),
                    &format!("merge: fast-forward {} into {}", source, target),
                )?;
                return Ok(MergeResult::Success {
                    merge_commit_sha: source_commit.id().to_string(),
                });
            }

            // Three-way merge using trees (works for bare and non-bare repos).
            let merge_base_oid = repo.merge_base(source_commit.id(), target_commit.id())?;
            let merge_base_commit = repo.find_commit(merge_base_oid)?;
            let ancestor_tree = merge_base_commit.tree()?;
            let our_tree = target_commit.tree()?;
            let their_tree = source_commit.tree()?;

            let mut index = repo.merge_trees(&ancestor_tree, &our_tree, &their_tree, None)?;

            if index.has_conflicts() {
                return Ok(MergeResult::Conflict {
                    message: "merge conflict detected".to_string(),
                });
            }

            let tree_id = index.write_tree_to(&repo)?;
            let tree = repo.find_tree(tree_id)?;
            let sig = git2::Signature::now("Gyre", "gyre@local")?;
            let message = format!("Merge branch '{}' into '{}'", source, target);
            let merge_commit_id = repo.commit(
                Some(&format!("refs/heads/{}", target)),
                &sig,
                &sig,
                &message,
                &tree,
                &[&target_commit, &source_commit],
            )?;

            Ok(MergeResult::Success {
                merge_commit_sha: merge_commit_id.to_string(),
            })
        })
        .await?
    }

    async fn create_worktree(
        &self,
        repo_path: &str,
        worktree_path: &str,
        branch: &str,
    ) -> Result<()> {
        let repo_path = repo_path.to_string();
        let worktree_path = worktree_path.to_string();
        let branch = branch.to_string();
        tokio::task::spawn_blocking(move || {
            // Try checking out existing branch first.
            let output = std::process::Command::new("git")
                .args([
                    "-C",
                    &repo_path,
                    "worktree",
                    "add",
                    "--checkout",
                    &worktree_path,
                    &branch,
                ])
                .output()
                .context("failed to run git worktree add")?;
            if output.status.success() {
                return Ok(());
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If branch doesn't exist, create it from HEAD.
            if stderr.contains("invalid reference") || stderr.contains("not a valid object name") {
                let output2 = std::process::Command::new("git")
                    .args([
                        "-C",
                        &repo_path,
                        "worktree",
                        "add",
                        "-b",
                        &branch,
                        &worktree_path,
                        "HEAD",
                    ])
                    .output()
                    .context("failed to run git worktree add -b")?;
                if !output2.status.success() {
                    let stderr2 = String::from_utf8_lossy(&output2.stderr);
                    anyhow::bail!("git worktree add -b failed: {stderr2}");
                }
                return Ok(());
            }
            anyhow::bail!("git worktree add failed: {stderr}");
        })
        .await?
    }

    async fn remove_worktree(&self, repo_path: &str, worktree_path: &str) -> Result<()> {
        let repo_path = repo_path.to_string();
        let worktree_path = worktree_path.to_string();
        tokio::task::spawn_blocking(move || {
            let output = std::process::Command::new("git")
                .args(["-C", &repo_path, "worktree", "remove", &worktree_path])
                .output()
                .context("failed to run git worktree remove")?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("git worktree remove failed: {stderr}");
            }
            Ok(())
        })
        .await?
    }

    async fn list_worktrees(&self, repo_path: &str) -> Result<Vec<String>> {
        let repo_path = repo_path.to_string();
        tokio::task::spawn_blocking(move || {
            let output = std::process::Command::new("git")
                .args(["-C", &repo_path, "worktree", "list", "--porcelain"])
                .output()
                .context("failed to run git worktree list")?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("git worktree list failed: {stderr}");
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Porcelain output: blocks separated by blank lines, each starting with "worktree <path>"
            let paths = stdout
                .lines()
                .filter_map(|line| line.strip_prefix("worktree "))
                .map(str::to_string)
                .collect();
            Ok(paths)
        })
        .await?
    }

    async fn create_initial_commit(&self, repo_path: &str, branch: &str) -> Result<String> {
        let repo_path = repo_path.to_string();
        let branch = branch.to_string();
        tokio::task::spawn_blocking(move || {
            let repo =
                Repository::open_bare(&repo_path).context("failed to open bare repository")?;
            let sig = git2::Signature::now("Gyre", "gyre@local")?;
            let builder = repo.treebuilder(None)?;
            let tree_oid = builder.write()?;
            let tree = repo.find_tree(tree_oid)?;
            let refname = format!("refs/heads/{branch}");
            let commit_oid =
                repo.commit(Some(&refname), &sig, &sig, "Initial commit", &tree, &[])?;
            repo.set_head(&refname)?;
            Ok(commit_oid.to_string())
        })
        .await?
    }

    async fn clone_mirror(&self, url: &str, path: &str) -> Result<()> {
        let url = url.to_string();
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            if let Some(parent) = std::path::Path::new(&path).parent() {
                std::fs::create_dir_all(parent)
                    .context("failed to create parent directories for mirror")?;
            }
            let output = std::process::Command::new("git")
                .args(["clone", "--mirror", &url, &path])
                .output()
                .context("failed to run git clone --mirror")?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("git clone --mirror failed: {stderr}");
            }
            Ok(())
        })
        .await?
    }

    async fn fetch_mirror(&self, path: &str) -> Result<()> {
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let output = std::process::Command::new("git")
                .args(["-C", &path, "fetch", "--all"])
                .output()
                .context("failed to run git fetch --all")?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("git fetch --all failed: {stderr}");
            }
            Ok(())
        })
        .await?
    }

    async fn branch_exists(&self, repo_path: &str, branch_name: &str) -> Result<bool> {
        let repo_path = repo_path.to_string();
        let branch_name = branch_name.to_string();
        tokio::task::spawn_blocking(move || {
            let repo = Repository::open(&repo_path).context("failed to open repository")?;
            let exists = repo.find_branch(&branch_name, BranchType::Local).is_ok();
            Ok(exists)
        })
        .await?
    }

    async fn create_branch(
        &self,
        repo_path: &str,
        branch_name: &str,
        from_ref: &str,
    ) -> Result<()> {
        let repo_path = repo_path.to_string();
        let branch_name = branch_name.to_string();
        let from_ref = from_ref.to_string();
        tokio::task::spawn_blocking(move || {
            let repo = Repository::open(&repo_path).context("failed to open repository")?;
            let commit = repo
                .revparse_single(&from_ref)
                .context("failed to resolve from_ref")?
                .peel_to_commit()
                .context("from_ref is not a commit")?;
            repo.branch(&branch_name, &commit, false)
                .context("failed to create branch")?;
            Ok(())
        })
        .await?
    }

    async fn write_file(
        &self,
        repo_path: &str,
        branch: &str,
        file_path: &str,
        content: &[u8],
        message: &str,
    ) -> Result<String> {
        let repo_path = repo_path.to_string();
        let branch = branch.to_string();
        let file_path = file_path.to_string();
        let content = content.to_vec();
        let message = message.to_string();
        tokio::task::spawn_blocking(move || {
            let repo = Repository::open(&repo_path).context("failed to open repository")?;

            // Resolve branch tip commit
            let branch_ref = repo
                .find_branch(&branch, BranchType::Local)
                .context("branch not found")?;
            let parent_commit = branch_ref.get().peel_to_commit()?;
            let parent_tree = parent_commit.tree()?;

            // Write blob
            let blob_oid = repo.blob(&content)?;

            // Build new tree by inserting/replacing the file.
            // For nested paths (e.g. "specs/system/vision.md"), we need to
            // recursively build sub-trees.
            let new_tree_oid = insert_blob_at_path(&repo, &parent_tree, &file_path, blob_oid)?;
            let new_tree = repo.find_tree(new_tree_oid)?;

            let sig = git2::Signature::now("Gyre", "gyre@local")?;
            let refname = format!("refs/heads/{}", branch);
            let commit_oid = repo.commit(
                Some(&refname),
                &sig,
                &sig,
                &message,
                &new_tree,
                &[&parent_commit],
            )?;

            Ok(commit_oid.to_string())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    fn make_commit(repo: &Repository, msg: &str) -> git2::Oid {
        let sig = git2::Signature::now("Test User", "test@example.com").unwrap();
        let tree_id = {
            let mut index = repo.index().unwrap();
            index.write_tree().unwrap()
        };
        let tree = repo.find_tree(tree_id).unwrap();
        let parents: Vec<git2::Commit> = match repo.head() {
            Ok(head) => vec![head.peel_to_commit().unwrap()],
            Err(_) => vec![],
        };
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parent_refs)
            .unwrap()
    }

    fn create_branch(repo: &Repository, name: &str) {
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch(name, &head, false).unwrap();
    }

    #[tokio::test]
    async fn test_init_bare_creates_valid_repo() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.git");
        let adapter = Git2OpsAdapter::new();

        adapter.init_bare(path.to_str().unwrap()).await.unwrap();

        assert!(Path::new(&path).exists());
        assert!(Repository::open_bare(&path).is_ok());
    }

    #[tokio::test]
    async fn test_init_bare_creates_parent_directories() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested").join("deep").join("repo.git");
        let adapter = Git2OpsAdapter::new();

        adapter.init_bare(path.to_str().unwrap()).await.unwrap();

        assert!(Path::new(&path).exists());
        assert!(Repository::open_bare(&path).is_ok());
    }

    #[tokio::test]
    async fn test_create_initial_commit_sets_branch_and_head() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bare.git");
        let adapter = Git2OpsAdapter::new();

        adapter.init_bare(path.to_str().unwrap()).await.unwrap();
        let sha = adapter
            .create_initial_commit(path.to_str().unwrap(), "main")
            .await
            .unwrap();

        assert!(!sha.is_empty());
        let repo = Repository::open_bare(&path).unwrap();
        let branch = repo.find_branch("main", git2::BranchType::Local).unwrap();
        assert_eq!(branch.get().peel_to_commit().unwrap().id().to_string(), sha);
        let head = repo.head().unwrap();
        assert_eq!(head.shorthand().unwrap(), "main");
    }

    #[tokio::test]
    async fn test_create_initial_commit_repo_shows_branch_in_list() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bare2.git");
        let adapter = Git2OpsAdapter::new();

        adapter.init_bare(path.to_str().unwrap()).await.unwrap();
        adapter
            .create_initial_commit(path.to_str().unwrap(), "main")
            .await
            .unwrap();

        let branches = adapter.list_branches(path.to_str().unwrap()).await.unwrap();
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].name, "main");
        assert!(branches[0].is_default);
        assert!(!branches[0].head_sha.is_empty());
    }

    #[tokio::test]
    async fn test_is_repo_returns_true_for_valid_repo() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let adapter = Git2OpsAdapter::new();

        let result = adapter
            .is_repo(repo.workdir().unwrap().to_str().unwrap())
            .await
            .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn test_is_repo_returns_false_for_non_repo() {
        let dir = TempDir::new().unwrap();
        let adapter = Git2OpsAdapter::new();

        let result = adapter.is_repo(dir.path().to_str().unwrap()).await.unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn test_list_branches_empty_repo() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        // No commits — HEAD is unborn, no branches
        let _ = repo;
        let adapter = Git2OpsAdapter::new();
        let branches = adapter
            .list_branches(dir.path().to_str().unwrap())
            .await
            .unwrap();
        assert!(branches.is_empty());
    }

    #[tokio::test]
    async fn test_list_branches_with_commits() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        make_commit(&repo, "initial commit");
        create_branch(&repo, "feature");

        let adapter = Git2OpsAdapter::new();
        let branches = adapter
            .list_branches(dir.path().to_str().unwrap())
            .await
            .unwrap();

        let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"master") || names.contains(&"main"));
        assert!(names.contains(&"feature"));
        // head_sha should be non-empty
        for b in &branches {
            assert!(!b.head_sha.is_empty());
        }
    }

    #[tokio::test]
    async fn test_commit_log_order_and_data() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let oid1 = make_commit(&repo, "first commit");
        make_commit(&repo, "second commit");
        let oid3 = make_commit(&repo, "third commit");

        // Determine default branch name
        let branch_name = repo.head().unwrap().shorthand().unwrap().to_string();

        let adapter = Git2OpsAdapter::new();
        let log = adapter
            .commit_log(dir.path().to_str().unwrap(), &branch_name, 10)
            .await
            .unwrap();

        assert_eq!(log.len(), 3);
        // Topological walk from HEAD — most recent (oid3) must be first, oldest (oid1) must be last.
        assert_eq!(log[0].sha, oid3.to_string());
        assert_eq!(log[2].sha, oid1.to_string());
        // All messages present
        let messages: Vec<&str> = log.iter().map(|c| c.message.as_str()).collect();
        assert!(messages.contains(&"first commit"));
        assert!(messages.contains(&"second commit"));
        assert!(messages.contains(&"third commit"));
        // Authors populated
        assert_eq!(log[0].author, "Test User");
    }

    #[tokio::test]
    async fn test_commit_log_limit() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        for i in 0..10 {
            make_commit(&repo, &format!("commit {i}"));
        }
        let branch_name = repo.head().unwrap().shorthand().unwrap().to_string();

        let adapter = Git2OpsAdapter::new();
        let log = adapter
            .commit_log(dir.path().to_str().unwrap(), &branch_name, 3)
            .await
            .unwrap();

        assert_eq!(log.len(), 3);
    }

    fn make_file_commit(repo: &Repository, filename: &str, content: &str, msg: &str) -> git2::Oid {
        let sig = git2::Signature::now("Test User", "test@example.com").unwrap();
        let mut index = repo.index().unwrap();
        let path = repo.workdir().unwrap().join(filename);
        std::fs::write(&path, content).unwrap();
        index.add_path(std::path::Path::new(filename)).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let parents: Vec<git2::Commit> = match repo.head() {
            Ok(head) => vec![head.peel_to_commit().unwrap()],
            Err(_) => vec![],
        };
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parent_refs)
            .unwrap()
    }

    #[tokio::test]
    async fn test_merge_branches_fast_forward() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        make_commit(&repo, "initial");
        let branch_name = repo.head().unwrap().shorthand().unwrap().to_string();

        // Create feature branch from main
        create_branch(&repo, "feature");
        // Checkout feature and add a commit
        repo.set_head("refs/heads/feature").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
            .unwrap();
        make_file_commit(&repo, "feat.txt", "feature content", "feat: add feature");

        let adapter = Git2OpsAdapter::new();
        let result = adapter
            .merge_branches(dir.path().to_str().unwrap(), "feature", &branch_name)
            .await
            .unwrap();

        assert!(matches!(result, MergeResult::Success { .. }));
    }

    #[tokio::test]
    async fn test_merge_branches_three_way_no_conflict() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        make_file_commit(&repo, "base.txt", "base", "initial");
        let branch_name = repo.head().unwrap().shorthand().unwrap().to_string();

        // Create feature branch
        create_branch(&repo, "feature");

        // Add commit on main
        make_file_commit(&repo, "main_change.txt", "main work", "main: add file");

        // Switch to feature and add different file
        repo.set_head("refs/heads/feature").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
            .unwrap();
        make_file_commit(
            &repo,
            "feature_change.txt",
            "feature work",
            "feat: add file",
        );

        // Switch back to main
        repo.set_head(&format!("refs/heads/{}", branch_name))
            .unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
            .unwrap();

        let adapter = Git2OpsAdapter::new();
        let result = adapter
            .merge_branches(dir.path().to_str().unwrap(), "feature", &branch_name)
            .await
            .unwrap();

        assert!(matches!(result, MergeResult::Success { .. }));
    }

    #[tokio::test]
    async fn test_merge_branches_conflict() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        make_file_commit(&repo, "conflict.txt", "original content", "initial");
        let branch_name = repo.head().unwrap().shorthand().unwrap().to_string();

        // Create feature branch
        create_branch(&repo, "feature");

        // Modify same file on main
        make_file_commit(&repo, "conflict.txt", "main version", "main: modify file");

        // Switch to feature and modify same file differently
        repo.set_head("refs/heads/feature").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
            .unwrap();
        make_file_commit(
            &repo,
            "conflict.txt",
            "feature version",
            "feat: modify file",
        );

        // Switch back to main
        repo.set_head(&format!("refs/heads/{}", branch_name))
            .unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
            .unwrap();

        let adapter = Git2OpsAdapter::new();
        let result = adapter
            .merge_branches(dir.path().to_str().unwrap(), "feature", &branch_name)
            .await
            .unwrap();

        assert!(matches!(result, MergeResult::Conflict { .. }));
    }

    #[tokio::test]
    async fn test_diff_between_commits() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let oid1 = make_commit(&repo, "first commit");
        let oid2 = make_commit(&repo, "second commit");

        let adapter = Git2OpsAdapter::new();
        let diff = adapter
            .diff(
                dir.path().to_str().unwrap(),
                &oid1.to_string(),
                &oid2.to_string(),
            )
            .await
            .unwrap();

        // No file changes between two commits on empty tree
        assert_eq!(diff.files_changed, 0);
    }

    #[tokio::test]
    async fn test_create_worktree_new_branch_from_head() {
        // A bare repo with commits but no feature branch — create_worktree
        // should fall back to `git worktree add -b <branch> <path> HEAD`.
        let repo_dir = TempDir::new().unwrap();
        let wt_dir = TempDir::new().unwrap();

        // Init a bare repo and give it an initial commit via a temporary clone.
        let non_bare = TempDir::new().unwrap();
        std::process::Command::new("git")
            .args(["init", non_bare.path().to_str().unwrap()])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "-C",
                non_bare.path().to_str().unwrap(),
                "config",
                "user.email",
                "t@t.com",
            ])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "-C",
                non_bare.path().to_str().unwrap(),
                "config",
                "user.name",
                "T",
            ])
            .output()
            .unwrap();
        std::fs::write(non_bare.path().join("README"), "hello").unwrap();
        std::process::Command::new("git")
            .args(["-C", non_bare.path().to_str().unwrap(), "add", "."])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "-C",
                non_bare.path().to_str().unwrap(),
                "commit",
                "-m",
                "init",
            ])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "clone",
                "--bare",
                non_bare.path().to_str().unwrap(),
                repo_dir.path().to_str().unwrap(),
            ])
            .output()
            .unwrap();

        let worktree_path = wt_dir.path().join("feat-new").to_str().unwrap().to_string();
        let adapter = Git2OpsAdapter::new();

        // Branch "feat/new" does not exist — should create it from HEAD.
        let result = adapter
            .create_worktree(
                repo_dir.path().to_str().unwrap(),
                &worktree_path,
                "feat/new",
            )
            .await;

        assert!(
            result.is_ok(),
            "create_worktree should succeed for new branch: {result:?}"
        );
        assert!(
            std::path::Path::new(&worktree_path).exists(),
            "worktree directory should exist"
        );
    }

    #[tokio::test]
    async fn test_create_worktree_existing_branch() {
        // A bare repo with an existing feature branch — checkout should succeed directly.
        let repo_dir = TempDir::new().unwrap();
        let wt_dir = TempDir::new().unwrap();

        let non_bare = TempDir::new().unwrap();
        std::process::Command::new("git")
            .args(["init", non_bare.path().to_str().unwrap()])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "-C",
                non_bare.path().to_str().unwrap(),
                "config",
                "user.email",
                "t@t.com",
            ])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "-C",
                non_bare.path().to_str().unwrap(),
                "config",
                "user.name",
                "T",
            ])
            .output()
            .unwrap();
        std::fs::write(non_bare.path().join("README"), "hello").unwrap();
        std::process::Command::new("git")
            .args(["-C", non_bare.path().to_str().unwrap(), "add", "."])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "-C",
                non_bare.path().to_str().unwrap(),
                "commit",
                "-m",
                "init",
            ])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "-C",
                non_bare.path().to_str().unwrap(),
                "checkout",
                "-b",
                "feat/existing",
            ])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "clone",
                "--bare",
                non_bare.path().to_str().unwrap(),
                repo_dir.path().to_str().unwrap(),
            ])
            .output()
            .unwrap();

        let worktree_path = wt_dir
            .path()
            .join("feat-existing")
            .to_str()
            .unwrap()
            .to_string();
        let adapter = Git2OpsAdapter::new();

        let result = adapter
            .create_worktree(
                repo_dir.path().to_str().unwrap(),
                &worktree_path,
                "feat/existing",
            )
            .await;

        assert!(
            result.is_ok(),
            "create_worktree should succeed for existing branch: {result:?}"
        );
        assert!(
            std::path::Path::new(&worktree_path).exists(),
            "worktree directory should exist"
        );
    }

    // ── branch_exists tests ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_branch_exists_true_for_existing_branch() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bare.git");
        let adapter = Git2OpsAdapter::new();
        adapter.init_bare(path.to_str().unwrap()).await.unwrap();
        adapter
            .create_initial_commit(path.to_str().unwrap(), "main")
            .await
            .unwrap();

        assert!(adapter
            .branch_exists(path.to_str().unwrap(), "main")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_branch_exists_false_for_missing_branch() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bare.git");
        let adapter = Git2OpsAdapter::new();
        adapter.init_bare(path.to_str().unwrap()).await.unwrap();
        adapter
            .create_initial_commit(path.to_str().unwrap(), "main")
            .await
            .unwrap();

        assert!(!adapter
            .branch_exists(path.to_str().unwrap(), "no-such-branch")
            .await
            .unwrap());
    }

    // ── create_branch tests ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_create_branch_from_existing_ref() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bare.git");
        let adapter = Git2OpsAdapter::new();
        adapter.init_bare(path.to_str().unwrap()).await.unwrap();
        adapter
            .create_initial_commit(path.to_str().unwrap(), "main")
            .await
            .unwrap();

        adapter
            .create_branch(path.to_str().unwrap(), "feature-x", "main")
            .await
            .unwrap();

        assert!(adapter
            .branch_exists(path.to_str().unwrap(), "feature-x")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_create_branch_from_invalid_ref_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bare.git");
        let adapter = Git2OpsAdapter::new();
        adapter.init_bare(path.to_str().unwrap()).await.unwrap();
        adapter
            .create_initial_commit(path.to_str().unwrap(), "main")
            .await
            .unwrap();

        let result = adapter
            .create_branch(path.to_str().unwrap(), "feature-y", "nonexistent")
            .await;
        assert!(result.is_err());
    }

    // ── write_file tests ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_write_file_creates_commit_on_branch() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bare.git");
        let adapter = Git2OpsAdapter::new();
        adapter.init_bare(path.to_str().unwrap()).await.unwrap();
        adapter
            .create_initial_commit(path.to_str().unwrap(), "main")
            .await
            .unwrap();

        let sha = adapter
            .write_file(
                path.to_str().unwrap(),
                "main",
                "README.md",
                b"# Hello",
                "Add README",
            )
            .await
            .unwrap();

        assert!(!sha.is_empty());
        assert_eq!(sha.len(), 40);

        // Verify the commit log shows the new commit
        let log = adapter
            .commit_log(path.to_str().unwrap(), "main", 1)
            .await
            .unwrap();
        assert_eq!(log[0].sha, sha);
        assert_eq!(log[0].message, "Add README");
    }

    #[tokio::test]
    async fn test_write_file_nested_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bare.git");
        let adapter = Git2OpsAdapter::new();
        adapter.init_bare(path.to_str().unwrap()).await.unwrap();
        adapter
            .create_initial_commit(path.to_str().unwrap(), "main")
            .await
            .unwrap();

        let sha = adapter
            .write_file(
                path.to_str().unwrap(),
                "main",
                "specs/system/vision.md",
                b"# Vision\n\nContent here.",
                "Add vision spec",
            )
            .await
            .unwrap();

        assert!(!sha.is_empty());

        // Verify blob content by reading the tree
        let repo = Repository::open_bare(&path).unwrap();
        let commit = repo
            .find_commit(git2::Oid::from_str(&sha).unwrap())
            .unwrap();
        let tree = commit.tree().unwrap();
        let entry = tree
            .get_path(std::path::Path::new("specs/system/vision.md"))
            .unwrap();
        let blob = repo.find_blob(entry.id()).unwrap();
        assert_eq!(blob.content(), b"# Vision\n\nContent here.");
    }

    #[tokio::test]
    async fn test_write_file_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bare.git");
        let adapter = Git2OpsAdapter::new();
        adapter.init_bare(path.to_str().unwrap()).await.unwrap();
        adapter
            .create_initial_commit(path.to_str().unwrap(), "main")
            .await
            .unwrap();

        // Write first version
        adapter
            .write_file(path.to_str().unwrap(), "main", "doc.md", b"version 1", "v1")
            .await
            .unwrap();

        // Overwrite with second version
        let sha2 = adapter
            .write_file(path.to_str().unwrap(), "main", "doc.md", b"version 2", "v2")
            .await
            .unwrap();

        // Verify latest content
        let repo = Repository::open_bare(&path).unwrap();
        let commit = repo
            .find_commit(git2::Oid::from_str(&sha2).unwrap())
            .unwrap();
        let tree = commit.tree().unwrap();
        let entry = tree.get_path(std::path::Path::new("doc.md")).unwrap();
        let blob = repo.find_blob(entry.id()).unwrap();
        assert_eq!(blob.content(), b"version 2");

        // Two commits total (initial + v1 + v2)
        let log = adapter
            .commit_log(path.to_str().unwrap(), "main", 10)
            .await
            .unwrap();
        assert_eq!(log.len(), 3);
    }

    #[tokio::test]
    async fn test_write_file_on_feature_branch() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bare.git");
        let adapter = Git2OpsAdapter::new();
        adapter.init_bare(path.to_str().unwrap()).await.unwrap();
        adapter
            .create_initial_commit(path.to_str().unwrap(), "main")
            .await
            .unwrap();

        // Create a feature branch and write to it
        adapter
            .create_branch(path.to_str().unwrap(), "spec-edit/vision-a1b2", "main")
            .await
            .unwrap();

        let sha = adapter
            .write_file(
                path.to_str().unwrap(),
                "spec-edit/vision-a1b2",
                "specs/system/vision.md",
                b"# New Vision",
                "Edit vision spec",
            )
            .await
            .unwrap();

        // Feature branch should have the commit
        let feature_log = adapter
            .commit_log(path.to_str().unwrap(), "spec-edit/vision-a1b2", 10)
            .await
            .unwrap();
        assert_eq!(feature_log[0].sha, sha);
        assert_eq!(feature_log[0].message, "Edit vision spec");

        // Main branch should NOT have the commit
        let main_log = adapter
            .commit_log(path.to_str().unwrap(), "main", 10)
            .await
            .unwrap();
        assert_eq!(main_log.len(), 1); // only initial commit
    }
}
