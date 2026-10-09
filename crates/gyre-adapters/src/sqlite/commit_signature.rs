//! SQLite adapter for `CommitSignatureRepository` (identity-security.md §Layer 3).
//!
//! Persists the jj-squash commit signature records so they survive a server
//! restart (review task-107 F9). Records are keyed by `(repo_id, commit_sha)`:
//! a lookup through repo A's API path never returns a record stored for repo B.

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_ports::commit_signature_repo::{CommitSignature, CommitSignatureRepository, SigstoreMode};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::commit_signatures;

#[derive(Queryable, Selectable)]
#[diesel(table_name = commit_signatures)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct CommitSignatureRow {
    repo_id: String,
    commit_sha: String,
    signer_id: String,
    task_id: String,
    spawned_by: String,
    algorithm: String,
    signature: String,
    signing_key_id: String,
    signed_at: i64,
    sigstore_mode: String,
    oidc_subject: String,
    oidc_issuer: String,
    certificate_pem: Option<String>,
    certificate_chain_pem: Option<String>,
    rekor_entry_id: Option<String>,
    #[allow(dead_code)]
    tenant_id: String,
}

impl CommitSignatureRow {
    fn into_record(self) -> Result<CommitSignature> {
        let sigstore_mode = match self.sigstore_mode.as_str() {
            "local" => SigstoreMode::Local,
            "fulcio" => SigstoreMode::Fulcio,
            other => anyhow::bail!("unknown sigstore_mode in stored record: {other}"),
        };
        Ok(CommitSignature {
            repo_id: self.repo_id,
            commit_sha: self.commit_sha,
            signer_id: self.signer_id,
            task_id: self.task_id,
            spawned_by: self.spawned_by,
            algorithm: self.algorithm,
            signature: self.signature,
            signing_key_id: self.signing_key_id,
            signed_at: self.signed_at as u64,
            sigstore_mode,
            oidc_subject: self.oidc_subject,
            oidc_issuer: self.oidc_issuer,
            certificate_pem: self.certificate_pem,
            certificate_chain_pem: self.certificate_chain_pem,
            rekor_entry_id: self.rekor_entry_id,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = commit_signatures)]
struct NewCommitSignatureRow<'a> {
    repo_id: &'a str,
    commit_sha: &'a str,
    signer_id: &'a str,
    task_id: &'a str,
    spawned_by: &'a str,
    algorithm: &'a str,
    signature: &'a str,
    signing_key_id: &'a str,
    signed_at: i64,
    sigstore_mode: &'a str,
    oidc_subject: &'a str,
    oidc_issuer: &'a str,
    certificate_pem: Option<&'a str>,
    certificate_chain_pem: Option<&'a str>,
    rekor_entry_id: Option<&'a str>,
    tenant_id: &'a str,
}

fn mode_to_str(mode: SigstoreMode) -> &'static str {
    match mode {
        SigstoreMode::Local => "local",
        SigstoreMode::Fulcio => "fulcio",
    }
}

#[async_trait]
impl CommitSignatureRepository for SqliteStorage {
    async fn save(&self, record: &CommitSignature) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = self.tenant_id.clone();
        let record = record.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewCommitSignatureRow {
                repo_id: &record.repo_id,
                commit_sha: &record.commit_sha,
                signer_id: &record.signer_id,
                task_id: &record.task_id,
                spawned_by: &record.spawned_by,
                algorithm: &record.algorithm,
                signature: &record.signature,
                signing_key_id: &record.signing_key_id,
                signed_at: record.signed_at as i64,
                sigstore_mode: mode_to_str(record.sigstore_mode),
                oidc_subject: &record.oidc_subject,
                oidc_issuer: &record.oidc_issuer,
                certificate_pem: record.certificate_pem.as_deref(),
                certificate_chain_pem: record.certificate_chain_pem.as_deref(),
                rekor_entry_id: record.rekor_entry_id.as_deref(),
                tenant_id: &tenant_id,
            };
            diesel::insert_into(commit_signatures::table)
                .values(&row)
                .on_conflict((commit_signatures::repo_id, commit_signatures::commit_sha))
                .do_update()
                .set((
                    commit_signatures::signer_id.eq(&record.signer_id),
                    commit_signatures::task_id.eq(&record.task_id),
                    commit_signatures::spawned_by.eq(&record.spawned_by),
                    commit_signatures::algorithm.eq(&record.algorithm),
                    commit_signatures::signature.eq(&record.signature),
                    commit_signatures::signing_key_id.eq(&record.signing_key_id),
                    commit_signatures::signed_at.eq(record.signed_at as i64),
                    commit_signatures::sigstore_mode.eq(mode_to_str(record.sigstore_mode)),
                    commit_signatures::oidc_subject.eq(&record.oidc_subject),
                    commit_signatures::oidc_issuer.eq(&record.oidc_issuer),
                    commit_signatures::certificate_pem.eq(record.certificate_pem.as_deref()),
                    commit_signatures::certificate_chain_pem
                        .eq(record.certificate_chain_pem.as_deref()),
                    commit_signatures::rekor_entry_id.eq(record.rekor_entry_id.as_deref()),
                ))
                .execute(&mut *conn)
                .context("upsert commit signature")?;
            Ok(())
        })
        .await?
    }

    async fn find(&self, repo_id: &str, commit_sha: &str) -> Result<Option<CommitSignature>> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = self.tenant_id.clone();
        let repo_id = repo_id.to_string();
        let commit_sha = commit_sha.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<CommitSignature>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = commit_signatures::table
                .filter(commit_signatures::tenant_id.eq(&tenant_id))
                .filter(commit_signatures::repo_id.eq(&repo_id))
                .filter(commit_signatures::commit_sha.eq(&commit_sha))
                .first::<CommitSignatureRow>(&mut *conn)
                .optional()
                .context("find commit signature")?;
            row.map(CommitSignatureRow::into_record).transpose()
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_ports::commit_signature_repo::ALGORITHM_ED25519;

    fn sample_record(repo_id: &str, sha: &str) -> CommitSignature {
        CommitSignature {
            repo_id: repo_id.to_string(),
            commit_sha: sha.to_string(),
            signer_id: "agent-1".to_string(),
            task_id: "task-9".to_string(),
            spawned_by: "user-1".to_string(),
            algorithm: ALGORITHM_ED25519.to_string(),
            signature: "c2ln".to_string(),
            signing_key_id: "kid-1".to_string(),
            signed_at: 1_700_000_000,
            sigstore_mode: SigstoreMode::Local,
            oidc_subject: "agent-1".to_string(),
            oidc_issuer: "http://localhost:3000".to_string(),
            certificate_pem: None,
            certificate_chain_pem: None,
            rekor_entry_id: None,
        }
    }

    async fn storage() -> SqliteStorage {
        let dir = tempfile::tempdir().unwrap();
        SqliteStorage::new(dir.path().join("test.db").to_str().unwrap()).unwrap()
    }

    #[tokio::test]
    async fn roundtrips_local_record() {
        let s = storage().await;
        let rec = sample_record("repo-1", "abc123");
        s.save(&rec).await.unwrap();
        let got = s.find("repo-1", "abc123").await.unwrap().unwrap();
        assert_eq!(got.commit_sha, "abc123");
        assert_eq!(got.signer_id, "agent-1");
        assert_eq!(got.task_id, "task-9");
        assert_eq!(got.spawned_by, "user-1");
        assert_eq!(got.sigstore_mode, SigstoreMode::Local);
    }

    #[tokio::test]
    async fn roundtrips_fulcio_record_with_optional_fields() {
        let s = storage().await;
        let mut rec = sample_record("repo-1", "def456");
        rec.sigstore_mode = SigstoreMode::Fulcio;
        rec.algorithm = gyre_ports::commit_signature_repo::ALGORITHM_ECDSA_P256.to_string();
        rec.certificate_pem = Some("-----BEGIN CERTIFICATE-----".to_string());
        rec.certificate_chain_pem = Some("chain".to_string());
        rec.rekor_entry_id = Some("0f41e".to_string());
        s.save(&rec).await.unwrap();
        let got = s.find("repo-1", "def456").await.unwrap().unwrap();
        assert_eq!(got.sigstore_mode, SigstoreMode::Fulcio);
        assert_eq!(got.rekor_entry_id.as_deref(), Some("0f41e"));
        assert!(got.certificate_pem.is_some());
    }

    #[tokio::test]
    async fn find_is_scoped_by_repo() {
        let s = storage().await;
        s.save(&sample_record("repo-a", "sha-shared")).await
            .unwrap();
        // Same SHA stored for repo B is a distinct record.
        let mut other = sample_record("repo-b", "sha-shared");
        other.signer_id = "agent-2".to_string();
        s.save(&other).await.unwrap();

        let got_a = s.find("repo-a", "sha-shared").await.unwrap().unwrap();
        let got_b = s.find("repo-b", "sha-shared").await.unwrap().unwrap();
        assert_eq!(got_a.signer_id, "agent-1");
        assert_eq!(got_b.signer_id, "agent-2");
        // Repo C sees nothing.
        assert!(s.find("repo-c", "sha-shared").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn find_unknown_sha_returns_none() {
        let s = storage().await;
        assert!(s.find("repo-1", "missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn save_upserts_on_same_key() {
        let s = storage().await;
        s.save(&sample_record("repo-1", "same-sha")).await.unwrap();
        let mut updated = sample_record("repo-1", "same-sha");
        updated.signature = "newsig".to_string();
        s.save(&updated).await.unwrap();
        let got = s.find("repo-1", "same-sha").await.unwrap().unwrap();
        assert_eq!(got.signature, "newsig");
    }
}
