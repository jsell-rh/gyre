use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_domain::AttestationBundle;
use gyre_ports::AttestationRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::attestation_bundles;

#[derive(Queryable, Selectable)]
#[diesel(table_name = attestation_bundles)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct AttestationRow {
    #[allow(dead_code)]
    mr_id: String,
    attestation: String,
    signature: String,
    signing_key_id: String,
}

impl AttestationRow {
    fn into_bundle(self) -> Option<AttestationBundle> {
        let attestation = serde_json::from_str(&self.attestation).ok()?;
        Some(AttestationBundle {
            attestation,
            signature: self.signature,
            signing_key_id: self.signing_key_id,
            deprecation_notice: Some(
                "This format is deprecated. Use GET /api/v1/repos/{id}/attestations/{commit_sha}/verification for chain attestation.".to_string()
            ),
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = attestation_bundles)]
struct NewAttestationRow<'a> {
    mr_id: &'a str,
    attestation: &'a str,
    signature: &'a str,
    signing_key_id: &'a str,
}

#[async_trait]
impl AttestationRepository for SqliteStorage {
    async fn find_by_mr_id(&self, mr_id: &str) -> Result<Option<AttestationBundle>> {
        let pool = Arc::clone(&self.pool);
        let mr_id = mr_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<AttestationBundle>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = attestation_bundles::table
                .find(&mr_id)
                .first::<AttestationRow>(&mut *conn)
                .optional()
                .context("find attestation by mr_id")?;
            Ok(row.and_then(AttestationRow::into_bundle))
        })
        .await?
    }

    async fn save(&self, mr_id: &str, bundle: &AttestationBundle) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let mr_id = mr_id.to_string();
        let attestation_json =
            serde_json::to_string(&bundle.attestation).context("serialize attestation")?;
        let signature = bundle.signature.clone();
        let signing_key_id = bundle.signing_key_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewAttestationRow {
                mr_id: &mr_id,
                attestation: &attestation_json,
                signature: &signature,
                signing_key_id: &signing_key_id,
            };
            diesel::insert_into(attestation_bundles::table)
                .values(&row)
                .on_conflict(attestation_bundles::mr_id)
                .do_update()
                .set((
                    attestation_bundles::attestation.eq(row.attestation),
                    attestation_bundles::signature.eq(row.signature),
                    attestation_bundles::signing_key_id.eq(row.signing_key_id),
                ))
                .execute(&mut *conn)
                .context("upsert attestation bundle")?;
            Ok(())
        })
        .await?
    }
}
