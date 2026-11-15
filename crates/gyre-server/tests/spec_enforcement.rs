//! Spec enforcement validation (agent-gates.md §Part 2 "What This Prevents").
//!
//! One integration test per attack/drift row in the spec table, each driving
//! the REAL enforcement surfaces end to end: REST API for policy
//! configuration, MR creation and merge-queue enqueueing; the real merge
//! processor (`run_once`, the same cycle the spawned processor loop runs);
//! real on-disk git repositories (real blob SHAs, real merges through the
//! git2 adapter); the real spec-approval ledger; and, for the
//! unauthorized-approver row, the real ABAC middleware over a live HTTP
//! listener.
//!
//! Every test is self-contained: it starts its own server on an ephemeral
//! loopback port with its own state, creates its own repos/policies/MRs,
//! and tears down with its TempDirs. No shared fixtures.
//!
//! Row map:
//! - `code_without_spec_is_rejected` — "Code without a spec"
//! - `nonexistent_spec_reference_is_rejected` — "Code referencing a nonexistent spec"
//! - `unapproved_spec_is_rejected_until_approved` — "Code referencing an unapproved spec"
//! - `stale_spec_warns_or_blocks_by_policy` — "Spec changed after code was written"
//! - `gate_agent_reviews_pinned_sha_and_catches_wrong_version` — "Agent implements wrong spec version"
//! - `retroactive_spec_modification_requires_new_approval` — "Retroactive spec modification"
//! - `unauthorized_approver_is_denied_by_abac` — "Spec approved by unauthorized person"

use gyre_common::Id;
use gyre_domain::{MergeQueueEntry, MergeQueueEntryStatus};
use gyre_server::{abac_middleware, build_router, build_state, merge_processor, AppState};
use sha2::Digest;
use std::sync::Arc;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A live server on an ephemeral loopback port, plus its state handle.
/// `run_once` is invoked directly (deterministic, no 5 s processor tick)
/// — it is the same `process_next` cycle the spawned processor loop runs.
struct TestServer {
    state: Arc<AppState>,
    client: reqwest::Client,
    base_url: String,
    token: String,
}

async fn start_server() -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let base_url = format!("http://127.0.0.1:{port}");
    let token = format!("spec-enf-token-{}", uuid::Uuid::new_v4());

    let state = build_state(&token, &base_url, None);
    abac_middleware::seed_builtin_policies(&state)
        .await
        .expect("seed built-in ABAC policies");

    let app = build_router(state.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server exited");
    });

    TestServer {
        state,
        client: reqwest::Client::new(),
        base_url,
        token,
    }
}

impl TestServer {
    fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }

    async fn get_json(&self, path: &str) -> serde_json::Value {
        let resp = self
            .client
            .get(format!("{}/api/v1/{path}", self.base_url))
            .header("Authorization", self.auth())
            .send()
            .await
            .unwrap();
        assert!(
            resp.status().is_success(),
            "GET {path} failed: {}",
            resp.status()
        );
        resp.json().await.unwrap()
    }

    async fn post_json(&self, path: &str, body: &serde_json::Value) -> reqwest::Response {
        self.client
            .post(format!("{}/api/v1/{path}", self.base_url))
            .header("Authorization", self.auth())
            .json(body)
            .send()
            .await
            .unwrap()
    }

    /// PUT the repo's spec enforcement policy (task-139 config surface).
    async fn set_spec_policy(&self, repo_id: &str, policy: serde_json::Value) {
        let resp = self
            .client
            .put(format!(
                "{}/api/v1/repos/{repo_id}/spec-policy",
                self.base_url
            ))
            .header("Authorization", self.auth())
            .json(&policy)
            .send()
            .await
            .unwrap();
        assert!(
            resp.status().is_success(),
            "PUT spec-policy failed: {}",
            resp.status()
        );
    }

    /// Create an MR via the REST API. Returns (mr_id, queue_entry_id).
    async fn create_mr(
        &self,
        repo_id: &str,
        title: &str,
        branch: &str,
        spec_ref: Option<&str>,
    ) -> (String, String) {
        let mut body = serde_json::json!({
            "repository_id": repo_id,
            "title": title,
            "source_branch": branch,
            "target_branch": "main",
        });
        if let Some(sr) = spec_ref {
            body["spec_ref"] = serde_json::json!(sr);
        }
        let resp = self.post_json("merge-requests", &body).await;
        assert!(
            resp.status().is_success(),
            "create MR failed: {}",
            resp.status()
        );
        let mr: serde_json::Value = resp.json().await.unwrap();
        let mr_id = mr["id"].as_str().unwrap().to_string();

        let resp = self
            .post_json(
                "merge-queue/enqueue",
                &serde_json::json!({ "merge_request_id": mr_id }),
            )
            .await;
        assert!(
            resp.status().is_success(),
            "enqueue failed: {}",
            resp.status()
        );
        let entry: serde_json::Value = resp.json().await.unwrap();
        let entry_id = entry["id"].as_str().unwrap().to_string();
        (mr_id, entry_id)
    }

    /// Terminal queue entry straight from the store (list_queue drops
    /// terminal entries, so the REST list cannot assert on them).
    async fn queue_entry(&self, entry_id: &str) -> MergeQueueEntry {
        self.state
            .merge_queue
            .find_by_id(&Id::new(entry_id))
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("queue entry {entry_id} must persist"))
    }

    async fn mr_status(&self, mr_id: &str) -> String {
        self.get_json(&format!("merge-requests/{mr_id}")).await["status"]
            .as_str()
            .unwrap_or("")
            .to_string()
    }

    /// Run one merge-processor cycle over the queue.
    async fn process_queue(&self) {
        merge_processor::run_once(&self.state)
            .await
            .expect("merge processor cycle");
    }
}

/// Run a git command in `dir`, asserting success; returns trimmed stdout.
fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A real on-disk git repository (non-bare, branch `main` with an initial
/// commit) registered in the server's store under its own workspace.
struct Fixture {
    dir: TempDir,
    repo: gyre_domain::Repository,
}

impl Fixture {
    fn path(&self) -> std::path::PathBuf {
        std::path::PathBuf::from(&self.repo.path)
    }

    /// Checkout `branch`; feature branches are always cut from the current
    /// `main` (every test creates each branch exactly once, after the spec
    /// commits it wants `main` to carry).
    fn branch(&self, name: &str) {
        let p = self.path();
        if name == "main" {
            git(&p, &["checkout", "main"]);
        } else {
            git(&p, &["checkout", "-b", name, "main"]);
        }
    }

    fn checkout_main(&self) {
        git(&self.path(), &["checkout", "main"]);
    }

    /// Write `rel` on the current branch and commit. Returns the file's
    /// blob SHA at the new HEAD.
    fn commit_file(&self, rel: &str, content: &str, msg: &str) -> String {
        let p = self.path();
        let file = p.join(rel);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file, content).unwrap();
        git(&p, &["add", "."]);
        git(&p, &["commit", "-m", msg]);
        git(&p, &["rev-parse", &format!("HEAD:{rel}")])
    }
}

/// Seed a workspace + repo row backed by a real git repo on disk.
async fn seed_repo(state: &AppState, slug: &str) -> Fixture {
    let dir = TempDir::new().unwrap();
    let repo_path = dir.path().join(slug);
    std::fs::create_dir_all(&repo_path).unwrap();
    git(&repo_path, &["init", "-b", "main"]);
    git(&repo_path, &["config", "user.email", "spec-enf@test.local"]);
    git(&repo_path, &["config", "user.name", "Spec Enforcement"]);
    std::fs::write(repo_path.join("README.md"), format!("# {slug}\n")).unwrap();
    git(&repo_path, &["add", "."]);
    git(&repo_path, &["commit", "-m", "chore: initial commit"]);

    let ws_id = format!("ws-{slug}");
    if state
        .workspaces
        .find_by_id(&Id::new(&ws_id))
        .await
        .unwrap()
        .is_none()
    {
        let ws = gyre_domain::Workspace::new(Id::new(&ws_id), Id::new("t1"), &ws_id, &ws_id, 0);
        state.workspaces.create(&ws).await.unwrap();
    }
    let repo = gyre_domain::Repository::new(
        Id::new(format!("repo-{slug}")),
        Id::new(&ws_id),
        slug,
        repo_path.to_str().unwrap(),
        0,
    );
    state.repos.create(&repo).await.unwrap();

    Fixture { dir, repo }
}

/// Seed an approved spec-approval ledger row for (path, sha).
async fn approve_spec_version(state: &AppState, ledger_id: &str, path: &str, sha: &str) {
    let apr = gyre_domain::SpecApproval::new(Id::new(ledger_id), path, sha, "user:human-approver");
    state.spec_approvals.create(&apr).await.unwrap();
    state
        .spec_approvals
        .approve(&apr.id, 1700000100)
        .await
        .unwrap()
        .expect("ledger entry exists");
}

fn all_false_policy() -> serde_json::Value {
    serde_json::json!({
        "require_spec_ref": false,
        "require_approved_spec": false,
        "warn_stale_spec": false,
        "require_current_spec": false,
        "require_signed_approval": false,
        "enforce_manifest": false,
    })
}

// ---------------------------------------------------------------------------
// Row 1 — "Code without a spec": MR rejected, no spec_ref
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn code_without_spec_is_rejected() {
    let server = start_server().await;
    let fix = seed_repo(&server.state, "spec-ref").await;
    let repo_id = fix.repo.id.to_string();

    // Configure require_spec_ref via the task-139 policy surface.
    let mut policy = all_false_policy();
    policy["require_spec_ref"] = serde_json::json!(true);
    server.set_spec_policy(&repo_id, policy).await;

    // MR with NO spec_ref — the attack.
    fix.branch("feat/no-spec");
    fix.commit_file(
        "feature.txt",
        "sneaky code without a spec\n",
        "feat: no spec",
    );
    fix.checkout_main();
    let (mr_id, entry_id) = server
        .create_mr(&repo_id, "No spec MR", "feat/no-spec", None)
        .await;

    server.process_queue().await;

    let entry = server.queue_entry(&entry_id).await;
    assert_eq!(
        entry.status,
        MergeQueueEntryStatus::Failed,
        "MR without spec_ref must be rejected under require_spec_ref"
    );
    let err = entry.error_message.as_deref().unwrap_or("");
    assert!(
        err.contains("spec_ref"),
        "rejection reason must tell the author WHY (missing spec_ref), got: {err}"
    );

    // The MR was not merged: still open, and the feature commit is NOT on main.
    assert_eq!(server.mr_status(&mr_id).await, "open");
    let show = std::process::Command::new("git")
        .args(["show", "main:feature.txt"])
        .current_dir(fix.path())
        .output()
        .unwrap();
    assert!(
        !show.status.success(),
        "the feature commit must not have landed on main"
    );

    // Control: the same repo merges an MR that carries a spec_ref.
    fix.branch("feat/with-spec");
    fix.commit_file("feature2.txt", "code bound to a spec\n", "feat: with spec");
    fix.checkout_main();
    let (mr2_id, entry2_id) = server
        .create_mr(
            &repo_id,
            "With spec MR",
            "feat/with-spec",
            Some(&format!("system/anything.md@{}", "a".repeat(40))),
        )
        .await;

    server.process_queue().await;

    let entry2 = server.queue_entry(&entry2_id).await;
    assert_eq!(
        entry2.status,
        MergeQueueEntryStatus::Merged,
        "MR with a spec_ref must merge (error: {:?})",
        entry2.error_message
    );
    assert_eq!(server.mr_status(&mr2_id).await, "merged");
}

// ---------------------------------------------------------------------------
// Row 2 — "Code referencing a nonexistent spec": MR rejected, SHA not found
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn nonexistent_spec_reference_is_rejected() {
    let server = start_server().await;
    let fix = seed_repo(&server.state, "ghost-spec").await;
    let repo_id = fix.repo.id.to_string();

    // The merge-time enforcement for an unresolvable spec_ref is the
    // approval-ledger check: no approval can exist for a SHA that was
    // never in the repo, so require_approved_spec must reject it.
    let mut policy = all_false_policy();
    policy["require_approved_spec"] = serde_json::json!(true);
    server.set_spec_policy(&repo_id, policy).await;

    fix.branch("feat/ghost");
    fix.commit_file(
        "feature.txt",
        "code referencing a ghost spec\n",
        "feat: ghost",
    );
    fix.checkout_main();
    let ghost_sha = "deadbeef".repeat(5); // well-formed 40-hex, resolves to nothing
    let spec_ref = format!("specs/nonexistent.md@{ghost_sha}");
    let (mr_id, entry_id) = server
        .create_mr(&repo_id, "Ghost spec MR", "feat/ghost", Some(&spec_ref))
        .await;

    server.process_queue().await;

    let entry = server.queue_entry(&entry_id).await;
    assert_eq!(
        entry.status,
        MergeQueueEntryStatus::Failed,
        "MR referencing a nonexistent spec SHA must be rejected"
    );
    let err = entry.error_message.as_deref().unwrap_or("");
    assert!(
        err.contains("no active approval") && err.contains("specs/nonexistent.md"),
        "rejection reason must name the spec and the missing approval, got: {err}"
    );
    assert_eq!(server.mr_status(&mr_id).await, "open");

    // The pinned SHA is genuinely unreachable in the repo: the MR spec
    // endpoint (the gate agent's spec channel) 404s for it.
    let resp = server
        .client
        .get(format!(
            "{}/api/v1/merge-requests/{mr_id}/spec",
            server.base_url
        ))
        .header("Authorization", server.auth())
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::NOT_FOUND,
        "pinned spec SHA that exists nowhere must not resolve"
    );
}

// ---------------------------------------------------------------------------
// Row 3 — "Code referencing an unapproved spec": MR rejected, no approval
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn unapproved_spec_is_rejected_until_approved() {
    let server = start_server().await;
    let fix = seed_repo(&server.state, "unapproved-spec").await;
    let repo_id = fix.repo.id.to_string();

    // A real spec file committed on main — a REAL blob SHA, not a constant.
    fix.branch("main");
    let blob = fix.commit_file(
        "specs/system/identity.md",
        "# Identity\n\nThe system must know who is calling.\n",
        "docs: add identity spec",
    );
    fix.branch("feat/unapproved");
    fix.commit_file(
        "feature.txt",
        "implementation of the identity spec\n",
        "feat: identity",
    );
    fix.checkout_main();

    // Ledger row for the referenced version exists but was never approved
    // (Pending) — a started approval authorizes nothing.
    let pending = gyre_domain::SpecApproval::new(
        Id::new("apr-unapproved-pending"),
        "system/identity.md",
        &blob,
        "user:approver",
    );
    server.state.spec_approvals.create(&pending).await.unwrap();

    let mut policy = all_false_policy();
    policy["require_approved_spec"] = serde_json::json!(true);
    server.set_spec_policy(&repo_id, policy).await;

    let spec_ref = format!("system/identity.md@{blob}");
    let (mr_id, entry_id) = server
        .create_mr(
            &repo_id,
            "Unapproved spec MR",
            "feat/unapproved",
            Some(&spec_ref),
        )
        .await;

    server.process_queue().await;

    let entry = server.queue_entry(&entry_id).await;
    assert_eq!(
        entry.status,
        MergeQueueEntryStatus::Failed,
        "MR referencing an unapproved spec version must be rejected"
    );
    let err = entry.error_message.as_deref().unwrap_or("");
    assert!(
        err.contains("no active approval") && err.contains("system/identity.md"),
        "rejection reason must name the spec and the missing approval, got: {err}"
    );
    assert_eq!(server.mr_status(&mr_id).await, "open");

    // Approving that exact (path, sha) version releases the block.
    server
        .state
        .spec_approvals
        .approve(&pending.id, 1700000200)
        .await
        .unwrap()
        .expect("ledger entry exists");

    fix.branch("feat/approved");
    fix.commit_file("feature2.txt", "second attempt\n", "feat: identity v2");
    fix.checkout_main();
    let (mr2_id, entry2_id) = server
        .create_mr(
            &repo_id,
            "Approved spec MR",
            "feat/approved",
            Some(&spec_ref),
        )
        .await;

    server.process_queue().await;

    let entry2 = server.queue_entry(&entry2_id).await;
    assert_eq!(
        entry2.status,
        MergeQueueEntryStatus::Merged,
        "approved spec version must merge (error: {:?})",
        entry2.error_message
    );
    assert_eq!(server.mr_status(&mr2_id).await, "merged");
}

// ---------------------------------------------------------------------------
// Row 4 — "Spec changed after code was written": warn or block, by policy
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stale_spec_warns_or_blocks_by_policy() {
    let server = start_server().await;
    let fix = seed_repo(&server.state, "stale-spec").await;
    let repo_id = fix.repo.id.to_string();

    // Spec v1 lands (SHA-1), then the spec moves to v2 (SHA-2) at HEAD.
    fix.branch("main");
    let v1_blob = fix.commit_file(
        "specs/system/identity.md",
        "# Identity v1\n",
        "docs: identity spec v1",
    );
    let v2_blob = fix.commit_file(
        "specs/system/identity.md",
        "# Identity v2 — EDITED\n",
        "docs: identity spec v2",
    );
    assert_ne!(
        v1_blob, v2_blob,
        "modifying the spec must change its blob SHA"
    );

    // warn_stale_spec: merge proceeds, warning lands as an MR comment.
    let mut warn_policy = all_false_policy();
    warn_policy["warn_stale_spec"] = serde_json::json!(true);
    server.set_spec_policy(&repo_id, warn_policy).await;

    fix.branch("feat/stale-warn");
    fix.commit_file(
        "feature.txt",
        "written against spec v1\n",
        "feat: stale warn",
    );
    fix.checkout_main();
    let (mr_warn_id, entry_warn_id) = server
        .create_mr(
            &repo_id,
            "Stale spec (warn) MR",
            "feat/stale-warn",
            Some(&format!("system/identity.md@{v1_blob}")),
        )
        .await;

    server.process_queue().await;

    let entry_warn = server.queue_entry(&entry_warn_id).await;
    assert_eq!(
        entry_warn.status,
        MergeQueueEntryStatus::Merged,
        "warn_stale_spec must NOT block the merge (error: {:?})",
        entry_warn.error_message
    );
    let comments = server
        .get_json(&format!("merge-requests/{mr_warn_id}/comments"))
        .await;
    let advisory = comments
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["author_agent_id"] == "forge:spec-policy")
        .unwrap_or_else(|| {
            panic!("warn_stale_spec must leave an advisory MR comment, got: {comments}")
        });
    assert!(
        advisory["body"]
            .as_str()
            .unwrap()
            .contains("Stale spec reference"),
        "advisory must describe the drift, got: {}",
        advisory["body"]
    );

    // require_current_spec: the same stale reference is blocked.
    let mut block_policy = all_false_policy();
    block_policy["require_current_spec"] = serde_json::json!(true);
    server.set_spec_policy(&repo_id, block_policy).await;

    fix.branch("feat/stale-block");
    fix.commit_file(
        "feature2.txt",
        "also written against spec v1\n",
        "feat: stale block",
    );
    fix.checkout_main();
    let (mr_block_id, entry_block_id) = server
        .create_mr(
            &repo_id,
            "Stale spec (block) MR",
            "feat/stale-block",
            Some(&format!("system/identity.md@{v1_blob}")),
        )
        .await;

    server.process_queue().await;

    let entry_block = server.queue_entry(&entry_block_id).await;
    assert_eq!(
        entry_block.status,
        MergeQueueEntryStatus::Failed,
        "require_current_spec must block a stale spec_ref"
    );
    let err = entry_block.error_message.as_deref().unwrap_or("");
    assert!(
        err.contains("requires current spec"),
        "rejection reason must name the stale-spec policy, got: {err}"
    );
    assert_eq!(server.mr_status(&mr_block_id).await, "open");
}

// ---------------------------------------------------------------------------
// Row 5 — "Agent implements wrong spec version": gate agent reviews against
// the pinned SHA and catches the mismatch
// ---------------------------------------------------------------------------

/// Reviewer script for the AgentReview gate: compares the implementation in
/// the MR diff against the requirement in the spec at the PINNED SHA
/// (delivered as GYRE_SPEC_FILE — never HEAD), and submits its verdict
/// through the review API with its scoped token. This is the same contract
/// a real gate-agent process runs.
const REVIEWER_SCRIPT: &str = r#"#!/usr/bin/env bash
req="$(grep -o 'ANSWER: [0-9]*' "$GYRE_SPEC_FILE" 2>/dev/null | head -n1 | tr -cd '0-9')"
impl="$(grep -o 'return [0-9]*' "$GYRE_DIFF_FILE" 2>/dev/null | head -n1 | tr -cd '0-9')"
if [ -n "$req" ] && [ -n "$impl" ] && [ "$req" = "$impl" ]; then
  verb="approved"
  body="implementation matches the pinned spec version (answer $impl)"
else
  verb="changes_requested"
  body="implementation answers ${impl:-<nothing>} but the pinned spec version requires ${req:-<nothing>}"
fi
code="$(curl -s -o /dev/null -w '%{http_code}' -X POST \
  -H "Authorization: Bearer $GYRE_REVIEW_TOKEN" \
  -H 'Content-Type: application/json' \
  -d "{\"decision\":\"$verb\",\"reviewer_agent_id\":\"$GYRE_GATE_AGENT_ID\",\"body\":\"$body\"}" \
  "$GYRE_SERVER_URL/api/v1/merge-requests/$GYRE_MR_ID/reviews")"
if [ "$code" != "201" ]; then
  echo "review submission failed with HTTP $code" >&2
  exit 1
fi
"#;

#[tokio::test(flavor = "multi_thread")]
async fn gate_agent_reviews_pinned_sha_and_catches_wrong_version() {
    let server = start_server().await;
    let fix = seed_repo(&server.state, "pinned-review").await;
    let state = &server.state;
    let repo_id = fix.repo.id.to_string();

    // Spec v1 (pinned by the MR): the widget MUST answer 42. Then the spec
    // at HEAD moves to v2: answer 43.
    fix.branch("main");
    let v1_blob = fix.commit_file(
        "specs/system/widget.md",
        "# Widget spec v1\n\nThe widget MUST answer: ANSWER: 42\n",
        "docs: widget spec v1",
    );
    let v2_blob = fix.commit_file(
        "specs/system/widget.md",
        "# Widget spec v2\n\nThe widget MUST answer: ANSWER: 43\n",
        "docs: widget spec v2",
    );

    // Reviewer persona the gate resolves nearest-wins.
    let persona = gyre_domain::Persona::new(
        Id::new("persona-spec-reviewer"),
        "Spec reviewer",
        "spec-reviewer",
        gyre_domain::PersonaScope::Workspace(fix.repo.workspace_id.clone()),
        "You review MRs against the spec version pinned in their spec_ref.",
        0,
    );
    state.personas.create(&persona).await.unwrap();

    // The reviewer script, on disk where the gate's command can reach it.
    let script_path = fix.dir.path().join("reviewer.sh");
    std::fs::write(&script_path, REVIEWER_SCRIPT).unwrap();

    // AgentReview gate via the REST API (task-134 gate surface).
    let gate: serde_json::Value = serde_json::json!({
        "name": "pinned-spec-review",
        "gate_type": "agent_review",
        "command": format!("bash {}", script_path.display()),
        "persona": "personas/spec-reviewer.md",
        "timeout_secs": 60,
    });
    let resp = server
        .post_json(&format!("repos/{repo_id}/gates"), &gate)
        .await;
    assert!(
        resp.status().is_success(),
        "create agent_review gate failed: {}",
        resp.status()
    );

    // MR1 pins v1 (answer 42) but implements 43 — the NEW spec version.
    // The gate agent must judge against the PINNED v1 and catch this.
    fix.branch("feat/wrong-version");
    fix.commit_file(
        "src/widget.rs",
        "pub fn answer() -> u32 {\n    return 43;\n}\n",
        "feat: widget per the newer spec",
    );
    fix.checkout_main();
    let (mr_wrong_id, entry_wrong_id) = server
        .create_mr(
            &repo_id,
            "Widget (wrong spec version)",
            "feat/wrong-version",
            Some(&format!("specs/system/widget.md@{v1_blob}")),
        )
        .await;

    // MR2 pins v2 (answer 43) and implements 43 — the correct pairing.
    fix.branch("feat/right-version");
    fix.commit_file(
        "src/widget.rs",
        "pub fn answer() -> u32 {\n    return 43;\n}\n",
        "feat: widget per the current spec",
    );
    fix.checkout_main();
    let (mr_right_id, entry_right_id) = server
        .create_mr(
            &repo_id,
            "Widget (correct spec version)",
            "feat/right-version",
            Some(&format!("specs/system/widget.md@{v2_blob}")),
        )
        .await;

    // Wait for both MRs' gate runs to reach a terminal state (the enqueue
    // triggers them in the background; the reviewer is a real process).
    for (mr_id, label) in [
        (&mr_wrong_id, "wrong-version"),
        (&mr_right_id, "right-version"),
    ] {
        let mut terminal = false;
        for _ in 0..150 {
            let results = server
                .get_json(&format!("merge-requests/{mr_id}/gates"))
                .await;
            let arr = results.as_array().unwrap();
            if !arr.is_empty()
                && arr
                    .iter()
                    .all(|g| g["status"] == "passed" || g["status"] == "failed")
            {
                terminal = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        assert!(
            terminal,
            "gate for the {label} MR never reached a terminal state"
        );
    }

    // The gate agent caught the version mismatch on MR1: verdict recorded,
    // gate Failed.
    let wrong_reviews = server
        .get_json(&format!("merge-requests/{mr_wrong_id}/reviews"))
        .await;
    let verdict = wrong_reviews
        .as_array()
        .unwrap()
        .iter()
        .find(|r| {
            r["reviewer_agent_id"]
                .as_str()
                .unwrap_or("")
                .starts_with("gate-review-")
        })
        .unwrap_or_else(|| panic!("gate agent must submit a verdict, got: {wrong_reviews}"));
    assert_eq!(verdict["decision"], "changes_requested");
    assert!(
        verdict["body"].as_str().unwrap().contains("pinned spec"),
        "verdict must explain the mismatch against the pinned version, got: {}",
        verdict["body"]
    );

    let wrong_gates = server
        .get_json(&format!("merge-requests/{mr_wrong_id}/gates"))
        .await;
    assert!(
        wrong_gates
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["status"] == "failed"),
        "the agent_review gate must fail for the wrong spec version, got: {wrong_gates}"
    );

    // The merge queue honors the failed gate: MR1 is blocked, MR2 merges.
    server.process_queue().await;
    server.process_queue().await;

    let entry_wrong = server.queue_entry(&entry_wrong_id).await;
    assert_eq!(
        entry_wrong.status,
        MergeQueueEntryStatus::Failed,
        "wrong-spec-version MR must be blocked by the failed review gate (error: {:?})",
        entry_wrong.error_message
    );
    assert_eq!(server.mr_status(&mr_wrong_id).await, "open");

    let entry_right = server.queue_entry(&entry_right_id).await;
    assert_eq!(
        entry_right.status,
        MergeQueueEntryStatus::Merged,
        "correct-spec-version MR must merge (error: {:?})",
        entry_right.error_message
    );
    assert_eq!(server.mr_status(&mr_right_id).await, "merged");
}

// ---------------------------------------------------------------------------
// Row 6 — "Retroactive spec modification": the old SHA's approval survives,
// the new SHA has none and is rejected
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn retroactive_spec_modification_requires_new_approval() {
    let server = start_server().await;
    let fix = seed_repo(&server.state, "retro-spec").await;
    let state = &server.state;
    let repo_id = fix.repo.id.to_string();

    fix.branch("main");
    let v1_blob = fix.commit_file(
        "specs/system/identity.md",
        "# Identity v1\n",
        "docs: identity spec v1",
    );
    let v2_blob = fix.commit_file(
        "specs/system/identity.md",
        "# Identity v2 — RETROACTIVELY EDITED\n",
        "docs: identity spec v2",
    );

    // Approve v1 only.
    approve_spec_version(state, "apr-retro-v1", "system/identity.md", &v1_blob).await;

    // The v1 approval is still active; v2 — a different SHA — has none.
    let for_v1 = state
        .spec_approvals
        .find_by_spec_sha("system/identity.md", &v1_blob)
        .await
        .unwrap();
    assert!(
        for_v1.iter().any(|a| a.is_active()),
        "approval for SHA-1 must remain valid after the spec is modified"
    );
    let for_v2 = state
        .spec_approvals
        .find_by_spec_sha("system/identity.md", &v2_blob)
        .await
        .unwrap();
    assert!(
        !for_v2.iter().any(|a| a.is_active()),
        "the modified spec's new SHA must start with no approval"
    );

    // An MR referencing the modified (SHA-2) content is rejected even
    // though the same spec path at SHA-1 is approved.
    let mut policy = all_false_policy();
    policy["require_approved_spec"] = serde_json::json!(true);
    server.set_spec_policy(&repo_id, policy).await;

    fix.branch("feat/retro");
    fix.commit_file(
        "feature.txt",
        "written against the edited spec\n",
        "feat: retro",
    );
    fix.checkout_main();
    let (mr_id, entry_id) = server
        .create_mr(
            &repo_id,
            "Retroactively modified spec MR",
            "feat/retro",
            Some(&format!("system/identity.md@{v2_blob}")),
        )
        .await;

    server.process_queue().await;

    let entry = server.queue_entry(&entry_id).await;
    assert_eq!(
        entry.status,
        MergeQueueEntryStatus::Failed,
        "MR referencing the modified spec SHA must be rejected — changing the spec requires a new approval"
    );
    let err = entry.error_message.as_deref().unwrap_or("");
    assert!(
        err.contains("no active approval") && err.contains(&v2_blob),
        "rejection reason must name the unapproved SHA, got: {err}"
    );
    assert_eq!(server.mr_status(&mr_id).await, "open");
}

// ---------------------------------------------------------------------------
// Row 7 — "Spec approved by unauthorized person": ABAC denies the approval
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn unauthorized_approver_is_denied_by_abac() {
    let server = start_server().await;
    let state = &server.state;

    let spec_path = "system/human-only.md";
    let sha = "d".repeat(40);

    // Ledger entry with a current version, so the ONLY thing standing
    // between the caller and a recorded approval is the ABAC policy.
    state
        .spec_ledger
        .save(&gyre_domain::SpecLedgerEntry {
            path: spec_path.to_string(),
            title: "Human-only spec".to_string(),
            owner: "user:jsell".to_string(),
            kind: None,
            current_sha: sha.clone(),
            approval_mode: "human_only".to_string(),
            approval_status: gyre_domain::ApprovalStatus::Pending,
            linked_tasks: vec![],
            linked_mrs: vec![],
            drift_status: "unknown".to_string(),
            created_at: 1700000000,
            updated_at: 1700000000,
            repo_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    // An agent identity with a valid, registered JWT. Hierarchy claims
    // (task-130 §1 token scoping) are minted from a self-consistent rogue
    // scope — the denial under test is role-based (subject.type != "user"),
    // so any registered agent JWT must be refused.
    let rogue_agent_id = "agent-rogue-approver";
    let agent_token = state
        .agent_signing_key
        .mint(
            rogue_agent_id,
            "task-rogue",
            "forge",
            &server.base_url,
            600,
            "default",
            "ws-rogue",
            "repo-rogue",
            "repo-rogue",
        )
        .unwrap();
    state
        .kv_store
        .kv_set("agent_tokens", rogue_agent_id, agent_token.clone())
        .await
        .unwrap();

    let approve_uri = format!(
        "{}/api/v1/specs/system%2Fhuman-only.md/approve",
        server.base_url
    );
    let body = serde_json::json!({ "sha": sha });

    // The agent attempts the approval — ABAC must deny it
    // (builtin:require-human-spec-approval is an immutable Deny).
    let resp = server
        .client
        .post(&approve_uri)
        .header("Authorization", format!("Bearer {agent_token}"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::FORBIDDEN,
        "non-human approver must be denied by ABAC"
    );

    // The denial is attributable to the spec-approval policy in the audit
    // trail (the approval ledger records WHO approves — and who was
    // refused).
    let denied = state
        .audit
        .query(&gyre_ports::AuditQueryFilter {
            agent_id: Some(rogue_agent_id.to_string()),
            event_type: Some("abac_denied".to_string()),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        denied.iter().any(|e| {
            e.detail.get("policy_id").and_then(|p| p.as_str())
                == Some("builtin-require-human-spec-approval")
        }),
        "the ABAC denial must be recorded against builtin:require-human-spec-approval, got: {:?}",
        denied.iter().map(|e| &e.detail).collect::<Vec<_>>()
    );

    // No approval was recorded for the denied attempt.
    let rows = state
        .spec_approvals
        .find_by_spec_sha(spec_path, &sha)
        .await
        .unwrap();
    assert!(
        !rows.iter().any(|a| a.is_active()),
        "a denied approval attempt must not create an active ledger entry"
    );

    // Control: a human user (API key, Admin role) IS allowed to approve —
    // proving the 403 above is the human-only policy, not default-deny.
    let raw_key = format!("spec-enf-user-key-{}", uuid::Uuid::new_v4());
    let mut user = gyre_domain::User::new(
        Id::new("user-human-approver"),
        "ext-human-approver",
        "Human Approver",
        1700000000,
    );
    user.roles = vec![gyre_domain::UserRole::Admin];
    user.tenant_id = Some(Id::new("t-human"));
    state.users.create(&user).await.unwrap();
    let hashed = sha2::Sha256::digest(raw_key.as_bytes());
    let hashed_hex: String = hashed.iter().map(|b| format!("{b:02x}")).collect();
    state
        .api_keys
        .create(&hashed_hex, &user.id, "spec-enf test key")
        .await
        .unwrap();

    let resp = server
        .client
        .post(&approve_uri)
        .header("Authorization", format!("Bearer {raw_key}"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::CREATED,
        "a human Admin must be allowed to approve the spec"
    );

    let rows = state
        .spec_approvals
        .find_by_spec_sha(spec_path, &sha)
        .await
        .unwrap();
    assert!(
        rows.iter().any(|a| a.is_active()),
        "the human approval must be recorded in the ledger"
    );
    assert!(
        rows.iter().any(|a| a.approver_id == "user-human-approver"),
        "the ledger must record the approver identity, got: {:?}",
        rows.iter().map(|a| &a.approver_id).collect::<Vec<_>>()
    );
    // HSI §12 judgment-ledger attribution key: the durable spec-approvals
    // ledger records the acting human's canonical user Id (the same key
    // GET /users/me/judgments resolves for the caller), NOT the
    // `user:<display name>` KeyBinding label — that label lives on the
    // approval-history event. A display-name key could never match the
    // judgment-ledger reader (writer/reader identity-key mismatch) and
    // misattributes on display-name collisions.
}
