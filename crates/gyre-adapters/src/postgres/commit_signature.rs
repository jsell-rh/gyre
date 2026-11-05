//! PostgreSQL adapter for `CommitSignatureRepository` (identity-security.md §Layer 3).
//!
//! Persists the jj-squash commit signature records so they survive a server
//! restart (review task-107 F9). Records are keyed by `(repo_id, commit_sha)`
//! and scoped to the storage tenant, mirroring the SQLite adapter.

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_ports::commit_signature_repo::{CommitSignature, CommitSignatureRepository, SigstoreMode};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::commit_signatures;

#[derive(Queryable, Selectable)]
#[diesel(table_name = commit_signatures)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
impl CommitSignatureRepository for PgStorage {
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
