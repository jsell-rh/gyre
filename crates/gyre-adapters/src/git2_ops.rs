use anyhow::{Context, Result};
use async_trait::async_trait;
use git2::{BranchType, Repository};
use gyre_domain::{BranchInfo, CommitInfo, DiffResult, FileDiff};
use gyre_ports::GitOpsPort;

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
            Repository::init_bare(&path).context("failed to init bare repository")?;
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

            // Resolve branch to commit OID
            let branch_ref = repo
                .find_branch(&branch, BranchType::Local)
                .context("branch not found")?;
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
                let content = std::str::from_utf8(line.content())
                    .unwrap_or("")
                    .to_string();
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
}
