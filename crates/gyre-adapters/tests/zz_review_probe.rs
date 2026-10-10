// TASK-095 review probe (temporary — removed after review): manual revert
// of a HISTORICAL merge commit, the exact R3-F1 scenario: MR-1 merged,
// then MR-2 merged on top, then MR-1 is reverted via its recorded
// merge_commit_sha.
//
// Question: does Git2OpsAdapter::revert_commit(mr1_merge_sha) undo ONLY
// MR-1's changes, or does it clobber MR-2's changes too?
use git2::Repository;
use gyre_adapters::Git2OpsAdapter;
use gyre_domain::MergeResult;
use gyre_ports::git_ops::GitOpsPort;
use std::path::Path;
use tempfile::TempDir;

fn commit_file(repo: &Repository, path: &str, content: &str, msg: &str) -> git2::Oid {
    let sig = git2::Signature::now("T", "t@t").unwrap();
    let workdir = repo.workdir().unwrap();
    let fpath = workdir.join(path);
    if let Some(parent) = fpath.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&fpath, content).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(path)).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let parents: Vec<git2::Commit> = match repo.head() {
        Ok(h) => vec![h.peel_to_commit().unwrap()],
        Err(_) => vec![],
    };
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parent_refs)
        .unwrap()
}

fn read(repo: &Repository, path: &str) -> Option<String> {
    std::fs::read_to_string(repo.workdir().unwrap().join(path)).ok()
}

fn merge_sha(r: &MergeResult) -> String {
    match r {
        MergeResult::Success { merge_commit_sha } => merge_commit_sha.clone(),
        MergeResult::Conflict { message } => panic!("merge conflict: {message}"),
    }
}

#[tokio::test]
async fn probe_historical_merge_revert_semantics() {
    let dir = TempDir::new().unwrap();
    let workdir = dir.path().join("repo");
    std::fs::create_dir_all(&workdir).unwrap();
    let repo = Repository::init(&workdir).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "T").unwrap();
    cfg.set_str("user.email", "t@t").unwrap();
    repo.set_head("refs/heads/main").unwrap();
    commit_file(&repo, "base.txt", "base\n", "base");

    // MR-1 branch: adds feature-a.txt
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feat-a", &head, false).unwrap();
    repo.set_head("refs/heads/feat-a").unwrap();
    commit_file(&repo, "feature-a.txt", "A\n", "feat A");

    // back to main, merge feat-a (no-FF → true merge commit)
    repo.set_head("refs/heads/main").unwrap();
    let adapter = Git2OpsAdapter::new();
    let mr1 = merge_sha(
        &adapter
            .merge_branches(workdir.to_str().unwrap(), "feat-a", "main")
            .await
            .unwrap(),
    );

    // MR-2 branch from main tip: adds feature-b.txt
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feat-b", &head, false).unwrap();
    repo.set_head("refs/heads/feat-b").unwrap();
    commit_file(&repo, "feature-b.txt", "B\n", "feat B");

    // merge feat-b into main
    repo.set_head("refs/heads/main").unwrap();
    let mr2 = merge_sha(
        &adapter
            .merge_branches(workdir.to_str().unwrap(), "feat-b", "main")
            .await
            .unwrap(),
    );

    println!("mr1_merge: {mr1}");
    println!("mr2_merge: {mr2}");

    // Both features present on main now.
    assert_eq!(read(&repo, "feature-a.txt").as_deref(), Some("A\n"));
    assert_eq!(read(&repo, "feature-b.txt").as_deref(), Some("B\n"));

    // ── The R3-F1 manual revert: revert MR-1's HISTORICAL merge commit ──
    let revert_sha = adapter
        .revert_commit(workdir.to_str().unwrap(), "main", &mr1)
        .await
        .unwrap();
    println!("revert: {revert_sha}");

    // Read main tip's tree content (revert_commit moved the branch ref).
    let repo2 = Repository::open(&workdir).unwrap();
    let tip = repo2
        .find_branch("main", git2::BranchType::Local)
        .unwrap()
        .get()
        .peel_to_commit()
        .unwrap();
    let tree = tip.tree().unwrap();
    let a = tree
        .get_path(Path::new("feature-a.txt"))
        .ok()
        .map(|e| e.to_object(&repo2).unwrap().peel_to_blob().unwrap().content().to_vec());
    let b = tree
        .get_path(Path::new("feature-b.txt"))
        .ok()
        .map(|e| e.to_object(&repo2).unwrap().peel_to_blob().unwrap().content().to_vec());
    println!(
        "after revert of MR-1: feature-a={a:?} feature-b={b:?}",
        a = a.as_deref().map(|v| String::from_utf8_lossy(v).to_string()),
        b = b.as_deref().map(|v| String::from_utf8_lossy(v).to_string().to_string()),
    );

    // Spec §6 "revert commit undoing the merge": MR-1's change gone...
    assert!(a.is_none(), "MR-1's feature-a must be undone by its revert");
    // ...and MR-2's change SURVIVES. If this fails, the manual revert
    // clobbered an unrelated later merge — silent data loss on main.
    assert!(
        b.is_some(),
        "MR-2's feature-b must SURVIVE the revert of MR-1; a tree-reset to \
         parent(0) wipes every later merge that landed on main"
    );
}
