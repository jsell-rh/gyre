//! Merge attestation bundle storage adapter (agent-gates.md §Part 3
//! Storage).
//!
//! Implements the `AttestationStorage` port over the two storage
//! targets that record a merge the moment it lands:
//!
//! - **Git note** on the merge commit, under
//!   `refs/notes/merge-attestations` — the commit-keyed target
//!   (`git notes show --ref=... <sha>`), written with the git binary
//!   like every other note path in the codebase.
//! - **Audit event** (`merge_attestation_bundle`) carrying the full
//!   signed bundle JSON in its detail, indexed by MR id
//!   (`resource_type` \"merge_request\", `resource_id` = MR id) — the
//!   queryable target. Retrieval by MR id and the AIBOM bulk
//!   commit-SHA query are served from these records; a missing git
//!   note never makes a stored bundle unreachable.
//!
//! Rekor transparency-log publication is a separate port
//! (`crate::rekor`).

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::audit::{AuditEvent, AuditEventType, AuditOutcome};
use gyre_domain::merge_attestation::{
    MergeAttestationBundle, MERGE_ATTESTATION_AUDIT_TYPE, MERGE_ATTESTATION_NOTES_REF,
};
use gyre_ports::attestation_storage::{AttestationScope, AttestationStorage};
use gyre_ports::audit::AuditQueryFilter;
use gyre_ports::AuditRepository;

/// `AttestationStorage` over git notes + the audit_events table.
pub struct MergeAttestationStorage {
    audit: Arc<dyn AuditRepository>,
}

impl MergeAttestationStorage {
    /// Build the storage over an audit repository (SQLite, Postgres, or
    /// the in-memory store — whichever the server wired).
    pub fn new(audit: Arc<dyn AuditRepository>) -> Self {
        Self { audit }
    }

    /// Every recorded `merge_attestation_bundle` audit event, newest
    /// first (the audit query contract). Complete: the query limit is
    /// derived from `stats_by_type` and grown when events were recorded
    /// between the count and the query, so no stored bundle is silently
    /// unreachable behind an arbitrary cap.
    async fn attestation_events(&self) -> Result<Vec<AuditEvent>> {
        let stats = self.audit.stats_by_type().await?;
        let mut limit = stats
            .iter()
            .find(|(t, _)| t == MERGE_ATTESTATION_AUDIT_TYPE)
            .map(|(_, c)| (*c).max(1) as usize)
            .unwrap_or(0);
        if limit == 0 {
            return Ok(Vec::new());
        }
        loop {
            let events = self
                .audit
                .query(&AuditQueryFilter {
                    event_type: Some(MERGE_ATTESTATION_AUDIT_TYPE.to_string()),
                    limit,
                    ..Default::default()
                })
                .await?;
            if events.len() < limit {
                return Ok(events);
            }
            // More events than the pre-query count — bundles were
            // recorded while querying. Grow and read the rest.
            limit = limit.saturating_mul(2);
        }
    }

    /// The bundle carried by an audit event, when its detail parses.
    fn bundle_of(event: &AuditEvent) -> Option<MergeAttestationBundle> {
        serde_json::from_value(event.detail.clone()).ok()
    }
}

#[async_trait]
impl AttestationStorage for MergeAttestationStorage {
    async fn store(
        &self,
        repo_path: &Path,
        scope: &AttestationScope,
        bundle: &MergeAttestationBundle,
    ) -> Result<()> {
        // 1. Audit event — the durable, queryable record. Recorded
        //    first: if the git note below fails, the bundle is still
        //    queryable and the failure is surfaced by the returned
        //    error, never swallowed.
        let event = AuditEvent::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            AuditEventType::Custom(MERGE_ATTESTATION_AUDIT_TYPE.to_string()),
            None,
            None,
            None,
            Some(scope.workspace_id.clone()),
            Some(scope.repo_id.clone()),
            "merge_request".to_string(),
            Some(bundle.mr_id.clone()),
            AuditOutcome::Success,
            serde_json::to_value(bundle).context("serialize merge attestation bundle")?,
            None,
            None,
            unix_now_secs(),
        );
        self.audit
            .record(&event)
            .await
            .context("record merge attestation bundle audit event")?;

        // 2. Git note on the merge commit.
        write_note(repo_path, bundle).await?;
        Ok(())
    }

    async fn retrieve_by_commit(
        &self,
        repo_path: &Path,
        commit_sha: &str,
    ) -> Result<Option<MergeAttestationBundle>> {
        // The git note is the commit-keyed target — try it first.
        match read_note(repo_path, commit_sha).await {
            Ok(Some(bundle)) => return Ok(Some(bundle)),
            Ok(None) => {}
            Err(e) => {
                tracing::debug!(
                    commit_sha,
                    error = %e,
                    "merge attestation note unreadable — falling back to the audit record"
                );
            }
        }
        // No (or unreadable) note: the audit-event record.
        let events = self.attestation_events().await?;
        for event in events {
            if let Some(bundle) = Self::bundle_of(&event) {
                if bundle.merge_commit_sha == commit_sha {
                    return Ok(Some(bundle));
                }
            }
        }
        Ok(None)
    }

    async fn retrieve_by_mr(&self, mr_id: &Id) -> Result<Option<MergeAttestationBundle>> {
        // Events are newest-first, so the first match is the newest
        // bundle recorded for the MR.
        for event in self.attestation_events().await? {
            if let Some(bundle) = Self::bundle_of(&event) {
                if bundle.mr_id == mr_id.as_str() {
                    return Ok(Some(bundle));
                }
            }
        }
        Ok(None)
    }

    async fn retrieve_by_commits(
        &self,
        commit_shas: &[String],
    ) -> Result<Vec<MergeAttestationBundle>> {
        let wanted: HashSet<&str> = commit_shas.iter().map(|s| s.as_str()).collect();
        let mut bundles = Vec::new();
        for event in self.attestation_events().await? {
            if let Some(bundle) = Self::bundle_of(&event) {
                if wanted.contains(bundle.merge_commit_sha.as_str()) {
                    bundles.push(bundle);
                }
            }
        }
        Ok(bundles)
    }
}

/// Current Unix time in seconds.
fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Write the serialized bundle as a git note on the merge commit under
/// `refs/notes/merge-attestations` (spec §Storage: \"as a git note on
/// the merge commit\", accessible via `git notes show\").
async fn write_note(repo_path: &Path, bundle: &MergeAttestationBundle) -> Result<()> {
    let note = serde_json::to_string(bundle).context("serialize merge attestation bundle note")?;
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args([
            "notes",
            &format!("--ref={MERGE_ATTESTATION_NOTES_REF}"),
            "add",
            "-f",
            "-m",
            &note,
            &bundle.merge_commit_sha,
        ])
        .output()
        .await
        .context("spawn git notes add")?;
    if !out.status.success() {
        bail!(
            "git notes add failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// Read the bundle note attached to `commit_sha`, if one exists and
/// parses. A commit without a note is `Ok(None)`; any other git failure
/// is an error.
async fn read_note(repo_path: &Path, commit_sha: &str) -> Result<Option<MergeAttestationBundle>> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args([
            "notes",
            &format!("--ref={MERGE_ATTESTATION_NOTES_REF}"),
            "show",
            commit_sha,
        ])
        .output()
        .await
        .context("spawn git notes show")?;
    if !out.status.success() {
        return Ok(None);
    }
    let note = String::from_utf8(out.stdout).context("git notes show output not UTF-8")?;
    serde_json::from_str(&note)
        .map(Some)
        .context("parse merge attestation bundle note")
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_domain::merge_attestation::{
        GateAttestation, MergeAttestationBundle, MERGE_ATTESTATION_VERSION,
    };
    use tempfile::{tempdir, NamedTempFile, TempDir};

    /// Real audit store (SQLite on a temp file) behind the adapter.
    fn audit_store() -> (NamedTempFile, Arc<dyn AuditRepository>) {
        let tmp = NamedTempFile::new().unwrap();
        let s = crate::SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, Arc::new(s))
    }

    fn scope() -> AttestationScope {
        AttestationScope {
            workspace_id: Id::new("ws-1"),
            repo_id: Id::new("repo-1"),
        }
    }

    fn bundle(mr_id: &str, sha: &str) -> MergeAttestationBundle {
        MergeAttestationBundle {
            merge_attestation_version: MERGE_ATTESTATION_VERSION.to_string(),
            mr_id: mr_id.to_string(),
            merge_commit_sha: sha.to_string(),
            merged_at: 1_774_000_000,
            spec_ref: None,
            spec_approval: None,
            author: None,
            gates: vec![GateAttestation {
                name: "tests".to_string(),
                gate_type: "TestCommand".to_string(),
                status: "Passed".to_string(),
                output_hash: "sha256:abcd".to_string(),
                started_at: Some(1_774_000_000),
                finished_at: Some(1_774_000_120),
                reviewer_agent_id: None,
                reviewer_oidc_sub: None,
                reviewer_stack_attestation: None,
                review_decision: None,
                review_body_hash: None,
                signed_by: "forge".to_string(),
                signature: Some("sig".to_string()),
            }],
            bundle_signature: Some("forge-bundle-sig".to_string()),
        }
    }

    /// Real git repository with one commit; returns (tempdir guard,
    /// repo path, commit sha).
    fn git_repo_with_commit() -> (TempDir, std::path::PathBuf, String) {
        let dir = tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let sha = git(&repo, &["init", "--initial-branch=main"]);
        let _ = sha;
        git(&repo, &["config", "user.email", "test@gyre.local"]);
        git(&repo, &["config", "user.name", "Gyre Test"]);
        std::fs::write(repo.join("file.txt"), "base").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "base"]);
        let sha = git(&repo, &["rev-parse", "HEAD"]);
        (dir, repo, sha)
    }

    /// Run git synchronously in `repo`, asserting success.
    fn git(repo: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Add one more commit to `repo`; returns its sha.
    fn commit(repo: &Path, name: &str) -> String {
        std::fs::write(repo.join(name), name).unwrap();
        git(repo, &["add", "."]);
        git(repo, &["commit", "-m", name]);
        git(repo, &["rev-parse", "HEAD"])
    }

    /// Integration: the bundle is stored as a git note on the merge
    /// commit and is retrievable with `git notes show` — the exact
    /// mechanism the spec names. Reverting the note write in `store`
    /// fails this test.
    #[tokio::test]
    async fn bundle_stored_as_git_note_retrievable_via_git_notes_show() {
        let (_tmp_audit, audit) = audit_store();
        let storage = MergeAttestationStorage::new(Arc::clone(&audit));
        let (_dir, repo, sha) = git_repo_with_commit();
        let b = bundle("mr-1", &sha);

        storage
            .store(&repo, &scope(), &b)
            .await
            .expect("store must succeed against a real git repo");

        let shown = git(
            &repo,
            &[
                "notes",
                &format!("--ref={MERGE_ATTESTATION_NOTES_REF}"),
                "show",
                &sha,
            ],
        );
        let from_note: MergeAttestationBundle =
            serde_json::from_str(&shown).expect("note content must be the bundle JSON");
        assert_eq!(from_note.mr_id, "mr-1");
        assert_eq!(from_note.merge_commit_sha, sha);
        assert_eq!(
            from_note.bundle_signature.as_deref(),
            Some("forge-bundle-sig"),
            "the note must carry the full signed bundle"
        );
    }

    /// Integration: the bundle is stored in audit_events, queryable by
    /// MR id — both through the raw audit repository (the record exists
    /// with the full bundle JSON in its detail) and through the port.
    #[tokio::test]
    async fn bundle_stored_in_audit_events_queryable_by_mr_id() {
        let (_tmp_audit, audit) = audit_store();
        let storage = MergeAttestationStorage::new(Arc::clone(&audit));
        let (_dir, repo, sha) = git_repo_with_commit();
        let b = bundle("mr-2", &sha);

        storage.store(&repo, &scope(), &b).await.unwrap();

        // The raw audit record: event type, MR index, full bundle JSON.
        let events = audit
            .query(&AuditQueryFilter {
                event_type: Some(MERGE_ATTESTATION_AUDIT_TYPE.to_string()),
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        let event = events
            .iter()
            .find(|e| e.resource_id.as_deref() == Some("mr-2"))
            .expect("merge attestation audit event must be recorded");
        assert_eq!(event.resource_type, "merge_request");
        assert_eq!(event.workspace_id, Some(Id::new("ws-1")));
        let detail: MergeAttestationBundle = serde_json::from_value(event.detail.clone()).unwrap();
        assert_eq!(detail.merge_commit_sha, sha);

        // The port path.
        let retrieved = storage
            .retrieve_by_mr(&Id::new("mr-2"))
            .await
            .unwrap()
            .expect("bundle must be queryable by MR id");
        assert_eq!(retrieved.mr_id, "mr-2");
        assert_eq!(retrieved.merge_commit_sha, sha);
    }

    /// Unit: retrieval by commit SHA returns the correct bundle (from
    /// the git note on that commit).
    #[tokio::test]
    async fn retrieve_by_commit_returns_correct_bundle() {
        let (_tmp_audit, audit) = audit_store();
        let storage = MergeAttestationStorage::new(Arc::clone(&audit));
        let (_dir, repo, sha1) = git_repo_with_commit();
        let sha2 = commit(&repo, "second.txt");
        storage
            .store(&repo, &scope(), &bundle("mr-a", &sha1))
            .await
            .unwrap();
        storage
            .store(&repo, &scope(), &bundle("mr-b", &sha2))
            .await
            .unwrap();

        let got = storage
            .retrieve_by_commit(&repo, &sha2)
            .await
            .unwrap()
            .expect("bundle for sha2 must be retrievable");
        assert_eq!(
            got.mr_id, "mr-b",
            "retrieval by commit returns that commit's bundle"
        );
        assert_eq!(got.merge_commit_sha, sha2);
    }

    /// Unit: retrieval by MR id returns the correct bundle when several
    /// merges are recorded.
    #[tokio::test]
    async fn retrieve_by_mr_returns_correct_bundle() {
        let (_tmp_audit, audit) = audit_store();
        let storage = MergeAttestationStorage::new(Arc::clone(&audit));
        let (_dir, repo, sha1) = git_repo_with_commit();
        let sha2 = commit(&repo, "second.txt");
        storage
            .store(&repo, &scope(), &bundle("mr-a", &sha1))
            .await
            .unwrap();
        storage
            .store(&repo, &scope(), &bundle("mr-b", &sha2))
            .await
            .unwrap();

        let got = storage
            .retrieve_by_mr(&Id::new("mr-a"))
            .await
            .unwrap()
            .expect("bundle for mr-a must be retrievable");
        assert_eq!(got.mr_id, "mr-a");
        assert_eq!(got.merge_commit_sha, sha1);

        assert!(
            storage
                .retrieve_by_mr(&Id::new("mr-unknown"))
                .await
                .unwrap()
                .is_none(),
            "unknown MR id retrieves nothing"
        );
    }

    /// Retrieval by commit falls back to the audit record when the git
    /// note is absent (e.g. removed or never written) — a stored bundle
    /// is never unreachable.
    #[tokio::test]
    async fn retrieve_by_commit_falls_back_to_audit_record_when_note_absent() {
        let (_tmp_audit, audit) = audit_store();
        let storage = MergeAttestationStorage::new(Arc::clone(&audit));
        let (_dir, repo, sha) = git_repo_with_commit();
        storage
            .store(&repo, &scope(), &bundle("mr-3", &sha))
            .await
            .unwrap();

        git(
            &repo,
            &[
                "notes",
                &format!("--ref={MERGE_ATTESTATION_NOTES_REF}"),
                "remove",
                "--ignore-missing",
                &sha,
            ],
        );

        let got = storage
            .retrieve_by_commit(&repo, &sha)
            .await
            .unwrap()
            .expect("audit record must serve retrieval when the note is gone");
        assert_eq!(got.mr_id, "mr-3");
    }

    /// AIBOM integration point: the bulk query returns every bundle
    /// attached to the given commit SHAs and nothing else.
    #[tokio::test]
    async fn retrieve_by_commits_collects_bundles_for_release_aibom() {
        let (_tmp_audit, audit) = audit_store();
        let storage = MergeAttestationStorage::new(Arc::clone(&audit));
        let (_dir, repo, sha1) = git_repo_with_commit();
        let sha2 = commit(&repo, "second.txt");
        let sha3 = commit(&repo, "third.txt");
        storage
            .store(&repo, &scope(), &bundle("mr-a", &sha1))
            .await
            .unwrap();
        storage
            .store(&repo, &scope(), &bundle("mr-b", &sha2))
            .await
            .unwrap();
        storage
            .store(&repo, &scope(), &bundle("mr-c", &sha3))
            .await
            .unwrap();

        let mut got = storage
            .retrieve_by_commits(&[sha1.clone(), sha2.clone()])
            .await
            .unwrap();
        got.sort_by(|a, b| a.mr_id.cmp(&b.mr_id));
        assert_eq!(got.len(), 2, "only the requested commits' bundles");
        assert_eq!(got[0].mr_id, "mr-a");
        assert_eq!(got[0].merge_commit_sha, sha1);
        assert_eq!(got[1].mr_id, "mr-b");
        assert_eq!(got[1].merge_commit_sha, sha2);
    }

    /// A git-note failure is surfaced by `store` (returned error) but
    /// the audit record — written first — keeps the bundle queryable:
    /// partial storage is visible, never silent.
    #[tokio::test]
    async fn store_surfaces_note_failure_but_keeps_audit_record() {
        let (_tmp_audit, audit) = audit_store();
        let storage = MergeAttestationStorage::new(Arc::clone(&audit));
        let dir = tempdir().unwrap();
        let not_a_repo = dir.path().join("not-a-repo");
        std::fs::create_dir_all(&not_a_repo).unwrap();

        let err = storage
            .store(&not_a_repo, &scope(), &bundle("mr-4", "deadbeef"))
            .await
            .expect_err("storing against a non-repo path must fail");
        assert!(
            err.to_string().contains("git notes add failed"),
            "error must name the failed target: {err}"
        );

        let got = storage
            .retrieve_by_mr(&Id::new("mr-4"))
            .await
            .unwrap()
            .expect("audit record must survive the note failure");
        assert_eq!(got.mr_id, "mr-4");
    }
}
