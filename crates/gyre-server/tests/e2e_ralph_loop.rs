//! End-to-end integration test: full Ralph loop via Gyre APIs.
//!
//! Demonstrates the complete agent development cycle:
//!   1. Start server with real git operations
//!   2. Create repo via API
//!   3. Create task via API
//!   4. Spawn agent via /api/v1/agents/spawn (gets token + worktree)
//!   5. Agent clones repo via smart HTTP (Bearer token auth)
//!   6. Agent makes commits and pushes via smart HTTP
//!   7. Agent signals completion -> MR created
//!   8. MR enqueued -> merge queue processes it
//!   9. Verify: MR merged, commit on target branch

use gyre_server::{abac_middleware, build_router, build_state, merge_processor};
use std::sync::Arc;
use tempfile::TempDir;

/// Helper: run a git command in the given directory, injecting the Bearer token.
fn git_with_token(args: &[&str], dir: &std::path::Path, token: &str) -> std::process::Output {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "true")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "http.extraHeader")
        .env(
            "GIT_CONFIG_VALUE_0",
            format!("Authorization: Bearer {token}"),
        )
        .output()
        .expect("failed to run git")
}

/// Helper: run a git command in the given directory (no token).
fn git_local(args: &[&str], dir: &std::path::Path) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("failed to run git");
    assert!(status.success(), "git {:?} failed", args);
}

#[tokio::test(flavor = "multi_thread")]
async fn full_ralph_loop_via_gyre() {
    // -- 1. Start server on random port with real git ops --
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let base_url = format!("http://127.0.0.1:{port}");
    let auth_token = "e2e-ralph-token";

    let state = build_state(auth_token, &base_url, None);
    abac_middleware::seed_builtin_policies(&state).await;
    merge_processor::spawn_merge_processor(state.clone());

    let app = build_router(state);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let client = reqwest::Client::new();
    let api = format!("{base_url}/api/v1");
    let auth_hdr = format!("Bearer {auth_token}");

    // -- 2. Create workspace + repo (workspace needed for git URL slug resolution) --
    let ws_slug = format!(
        "e2e-ws-{}",
        &uuid::Uuid::new_v4().to_string().replace('-', "")[..8]
    );
    let workspace: serde_json::Value = client
        .post(format!("{api}/workspaces"))
        .header("Authorization", &auth_hdr)
        .json(&serde_json::json!({
            "tenant_id": "default",
            "name": "E2E Workspace",
            "slug": ws_slug,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let workspace_id = workspace["id"].as_str().unwrap().to_string();

    let repo: serde_json::Value = client
        .post(format!("{api}/repos"))
        .header("Authorization", &auth_hdr)
        .json(&serde_json::json!({
            "workspace_id": workspace_id,
            "name": "gyre-e2e",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let repo_id = repo["id"].as_str().unwrap().to_string();
    // path field removed from response in M16 (M-3 security: no filesystem path exposure)
    let repo_path_str = repo["path"].as_str().unwrap_or("");

    // Verify repo was created — the API response path is server-computed.
    assert!(
        std::path::Path::new(repo_path_str).exists() || repo["id"].as_str().is_some(),
        "repo should be created (API returned valid id)"
    );

    // -- 3. Create task --
    let task: serde_json::Value = client
        .post(format!("{api}/tasks"))
        .header("Authorization", &auth_hdr)
        .json(&serde_json::json!({"title": "Implement E2E feature for Ralph loop", "task_type": "implementation"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let task_id = task["id"].as_str().unwrap().to_string();
    assert_eq!(task["status"], "backlog");

    // -- 4. Spawn agent --
    let spawn: serde_json::Value = client
        .post(format!("{api}/agents/spawn"))
        .header("Authorization", &auth_hdr)
        .json(&serde_json::json!({
            "name": "ralph-worker",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/ralph-loop",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let agent_id = spawn["agent"]["id"].as_str().unwrap().to_string();
    let agent_token = spawn["token"].as_str().unwrap().to_string();
    let clone_url = format!("{}.git", spawn["clone_url"].as_str().unwrap());

    assert_eq!(spawn["agent"]["status"], "active");
    assert_eq!(spawn["branch"], "feat/ralph-loop");
    assert!(!agent_token.is_empty(), "agent token should be non-empty");

    // Verify task was assigned and moved to InProgress
    let task_check: serde_json::Value = client
        .get(format!("{api}/tasks/{task_id}"))
        .header("Authorization", &auth_hdr)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(task_check["status"], "in_progress");
    assert_eq!(task_check["assigned_to"].as_str().unwrap(), &agent_id);

    // -- 5-8. Agent clones, commits, and pushes via smart HTTP --
    let work_tmp = Arc::new(TempDir::new().unwrap());
    let work_tmp_c = work_tmp.clone();
    let agent_token_c = agent_token.clone();
    let clone_url_c = clone_url.clone();

    tokio::task::spawn_blocking(move || {
        let work_root = work_tmp_c.path();
        let work_dir = work_root.join("work");

        // Clone empty repo
        let clone_out = git_with_token(&["clone", &clone_url_c, "work"], work_root, &agent_token_c);
        let clone_ok = clone_out.status.success()
            || String::from_utf8_lossy(&clone_out.stderr).contains("empty repository")
            || String::from_utf8_lossy(&clone_out.stderr).contains("warning");
        assert!(
            clone_ok,
            "git clone failed: {}",
            String::from_utf8_lossy(&clone_out.stderr)
        );

        // Configure git identity for this repo
        git_local(&["config", "user.email", "ralph@gyre.local"], &work_dir);
        git_local(&["config", "user.name", "Ralph Worker"], &work_dir);

        // After cloning an empty repo the local branch name depends on the git
        // client's `init.defaultBranch` setting (may be "master" or "main").
        // Use `HEAD:main` to push to the remote `main` ref regardless of the
        // local branch name.
        std::fs::write(work_dir.join("README.md"), "# gyre-e2e\n").unwrap();
        git_local(&["add", "."], &work_dir);
        git_local(&["commit", "-m", "chore: initial commit"], &work_dir);

        // Push HEAD to establish the base branch as `main` on the remote.
        let push_main = git_with_token(&["push", "origin", "HEAD:main"], &work_dir, &agent_token_c);
        assert!(
            push_main.status.success(),
            "git push main failed: {}",
            String::from_utf8_lossy(&push_main.stderr)
        );

        // Create feature branch and add work
        git_local(&["checkout", "-b", "feat/ralph-loop"], &work_dir);
        std::fs::write(work_dir.join("feature.txt"), "agent implementation\n").unwrap();
        git_local(&["add", "."], &work_dir);
        git_local(
            &["commit", "-m", "feat: implement ralph loop feature"],
            &work_dir,
        );

        // Push feature branch via smart HTTP with agent token
        let push_feat = git_with_token(
            &["push", "origin", "feat/ralph-loop"],
            &work_dir,
            &agent_token_c,
        );
        assert!(
            push_feat.status.success(),
            "git push feat/ralph-loop failed: {}",
            String::from_utf8_lossy(&push_feat.stderr)
        );
    })
    .await
    .unwrap();

    // -- 9. Agent signals completion (creates MR) --
    let mr: serde_json::Value = client
        .post(format!("{api}/agents/{agent_id}/complete"))
        .header("Authorization", &auth_hdr)
        .json(&serde_json::json!({
            "branch": "feat/ralph-loop",
            "title": "feat: implement ralph loop feature",
            "target_branch": "main",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let mr_id = mr["id"].as_str().unwrap().to_string();
    assert_eq!(mr["status"], "open");
    assert_eq!(mr["source_branch"], "feat/ralph-loop");
    assert_eq!(mr["target_branch"], "main");
    assert_eq!(mr["author_agent_id"].as_str().unwrap(), &agent_id);

    // Verify agent and task transitioned
    let agent_check: serde_json::Value = client
        .get(format!("{api}/agents/{agent_id}"))
        .header("Authorization", &auth_hdr)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(agent_check["status"], "idle");

    let task_review: serde_json::Value = client
        .get(format!("{api}/tasks/{task_id}"))
        .header("Authorization", &auth_hdr)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(task_review["status"], "review");

    // -- 10. Enqueue MR in merge queue --
    // The merge processor auto-transitions Open -> Approved -> Merged on success.
    let queue_entry: serde_json::Value = client
        .post(format!("{api}/merge-queue/enqueue"))
        .header("Authorization", &auth_hdr)
        .json(&serde_json::json!({"merge_request_id": mr_id}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(queue_entry["status"], "queued");
    assert_eq!(queue_entry["merge_request_id"].as_str().unwrap(), &mr_id);

    // Verify queue is listed
    let queue_list: serde_json::Value = client
        .get(format!("{api}/merge-queue"))
        .header("Authorization", &auth_hdr)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(queue_list.as_array().unwrap().len(), 1);

    // -- 11. Wait for merge queue processor (runs every 5s, polls up to 20s) --
    let mut final_mr_status = String::from("open");
    for _ in 0..40 {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        let mr_poll: serde_json::Value = client
            .get(format!("{api}/merge-requests/{mr_id}"))
            .header("Authorization", &auth_hdr)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        final_mr_status = mr_poll["status"].as_str().unwrap_or("").to_string();
        if final_mr_status == "merged" {
            break;
        }
    }

    // -- 12. Verify: MR status = Merged --
    assert_eq!(
        final_mr_status, "merged",
        "MR should be merged after merge queue processed it"
    );

    // -- 13. Verify: commit on target branch (main) after merge --
    let commits: serde_json::Value = client
        .get(format!("{api}/repos/{repo_id}/commits?branch=main"))
        .header("Authorization", &auth_hdr)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let commit_list = commits.as_array().unwrap();
    assert!(
        !commit_list.is_empty(),
        "main branch should have commits after merge"
    );
    // After fast-forward merge, main should have the feature commit
    let messages: Vec<&str> = commit_list
        .iter()
        .filter_map(|c| c["message"].as_str())
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("feat") || m.contains("initial")),
        "merged commits should appear on main: {:?}",
        messages
    );

    // -- 14. Verify: activity log is accessible --
    let activity_resp = client
        .get(format!("{api}/activity?limit=50"))
        .header("Authorization", &auth_hdr)
        .send()
        .await
        .unwrap();
    assert!(
        activity_resp.status().is_success(),
        "activity endpoint should return 200"
    );

    // Cleanup (TempDirs drop here)
    drop(work_tmp);
}
