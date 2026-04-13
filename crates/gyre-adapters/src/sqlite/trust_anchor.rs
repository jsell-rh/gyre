use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::attestation::{OutputConstraint, TrustAnchor, TrustAnchorType};
use gyre_ports::TrustAnchorRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::trust_anchors;

#[derive(Queryable, Selectable)]
#[diesel(table_name = trust_anchors)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct TrustAnchorRow {
    id: String,
    #[allow(dead_code)]
    tenant_id: String,
    issuer: String,
    jwks_uri: String,
    anchor_type: String,
    constraints_json: String,
    #[allow(dead_code)]
    created_at: i64,
}

impl TrustAnchorRow {
    fn into_trust_anchor(self) -> Result<TrustAnchor> {
        let anchor_type = parse_anchor_type(&self.anchor_type)?;
        let constraints: Vec<OutputConstraint> =
            serde_json::from_str(&self.constraints_json).unwrap_or_default();
        Ok(TrustAnchor {
            id: self.id,
            issuer: self.issuer,
            jwks_uri: self.jwks_uri,
            anchor_type,
            constraints,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = trust_anchors)]
struct NewTrustAnchorRow<'a> {
    id: &'a str,
    tenant_id: &'a str,
    issuer: &'a str,
    jwks_uri: &'a str,
    anchor_type: &'a str,
    constraints_json: &'a str,
    created_at: i64,
}

fn anchor_type_to_str(at: &TrustAnchorType) -> &'static str {
    match at {
        TrustAnchorType::User => "user",
        TrustAnchorType::Agent => "agent",
        TrustAnchorType::Addon => "addon",
    }
}

fn parse_anchor_type(s: &str) -> Result<TrustAnchorType> {
    match s {
        "user" => Ok(TrustAnchorType::User),
        "agent" => Ok(TrustAnchorType::Agent),
        "addon" => Ok(TrustAnchorType::Addon),
        other => anyhow::bail!("unknown trust anchor type: {other}"),
    }
}

#[async_trait]
impl TrustAnchorRepository for SqliteStorage {
    async fn create(&self, tenant_id: &str, anchor: &TrustAnchor) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = tenant_id.to_string();
        let anchor = anchor.clone();
        let constraints_json =
            serde_json::to_string(&anchor.constraints).context("serialize constraints")?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            let row = NewTrustAnchorRow {
                id: &anchor.id,
                tenant_id: &tenant_id,
                issuer: &anchor.issuer,
                jwks_uri: &anchor.jwks_uri,
                anchor_type: anchor_type_to_str(&anchor.anchor_type),
                constraints_json: &constraints_json,
                created_at: now,
            };
            diesel::insert_into(trust_anchors::table)
                .values(&row)
                .execute(&mut *conn)
                .context("insert trust anchor")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, tenant_id: &str, anchor_id: &str) -> Result<Option<TrustAnchor>> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = tenant_id.to_string();
        let anchor_id = anchor_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<TrustAnchor>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = trust_anchors::table
                .filter(trust_anchors::tenant_id.eq(&tenant_id))
                .filter(trust_anchors::id.eq(&anchor_id))
                .first::<TrustAnchorRow>(&mut *conn)
                .optional()
                .context("find trust anchor by id")?;
            row.map(TrustAnchorRow::into_trust_anchor).transpose()
        })
        .await?
    }

    async fn list_by_tenant(&self, tenant_id: &str) -> Result<Vec<TrustAnchor>> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = tenant_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<TrustAnchor>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = trust_anchors::table
                .filter(trust_anchors::tenant_id.eq(&tenant_id))
                .load::<TrustAnchorRow>(&mut *conn)
                .context("list trust anchors")?;
            rows.into_iter()
                .map(TrustAnchorRow::into_trust_anchor)
                .collect()
        })
        .await?
    }

    async fn update(&self, tenant_id: &str, anchor: &TrustAnchor) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = tenant_id.to_string();
        let anchor = anchor.clone();
        let constraints_json =
            serde_json::to_string(&anchor.constraints).context("serialize constraints")?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let updated = diesel::update(
                trust_anchors::table
                    .filter(trust_anchors::tenant_id.eq(&tenant_id))
                    .filter(trust_anchors::id.eq(&anchor.id)),
            )
            .set((
                trust_anchors::issuer.eq(&anchor.issuer),
                trust_anchors::jwks_uri.eq(&anchor.jwks_uri),
                trust_anchors::anchor_type.eq(anchor_type_to_str(&anchor.anchor_type)),
                trust_anchors::constraints_json.eq(&constraints_json),
            ))
            .execute(&mut *conn)
            .context("update trust anchor")?;
            if updated == 0 {
                anyhow::bail!(
                    "trust anchor not found: tenant={tenant_id}, id={}",
                    anchor.id
                );
            }
            Ok(())
        })
        .await?
    }

    async fn delete(&self, tenant_id: &str, anchor_id: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = tenant_id.to_string();
        let anchor_id = anchor_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let deleted = diesel::delete(
                trust_anchors::table
                    .filter(trust_anchors::tenant_id.eq(&tenant_id))
                    .filter(trust_anchors::id.eq(&anchor_id)),
            )
            .execute(&mut *conn)
            .context("delete trust anchor")?;
            if deleted == 0 {
                anyhow::bail!("trust anchor not found: tenant={tenant_id}, id={anchor_id}");
            }
            Ok(())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use tempfile::NamedTempFile;

    fn tmp_storage() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, storage)
    }

    fn sample_anchor() -> TrustAnchor {
        TrustAnchor {
            id: "tenant-keycloak".to_string(),
            issuer: "https://keycloak.example.com".to_string(),
            jwks_uri: "https://keycloak.example.com/.well-known/jwks.json".to_string(),
            anchor_type: TrustAnchorType::User,
            constraints: vec![],
        }
    }

    #[tokio::test]
    async fn create_and_find() {
        let (_tmp, storage) = tmp_storage();
        let anchor = sample_anchor();
        storage.create("t1", &anchor).await.unwrap();
        let found = storage.find_by_id("t1", "tenant-keycloak").await.unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.id, "tenant-keycloak");
        assert_eq!(found.issuer, anchor.issuer);
        assert_eq!(found.anchor_type, TrustAnchorType::User);
    }

    #[tokio::test]
    async fn find_missing_returns_none() {
        let (_tmp, storage) = tmp_storage();
        let found = storage.find_by_id("t1", "nonexistent").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn list_by_tenant() {
        let (_tmp, storage) = tmp_storage();
        let a1 = sample_anchor();
        let mut a2 = sample_anchor();
        a2.id = "gyre-oidc".to_string();
        a2.anchor_type = TrustAnchorType::Agent;
        storage.create("t1", &a1).await.unwrap();
        storage.create("t1", &a2).await.unwrap();
        storage.create("t2", &a1).await.unwrap();
        let list = storage.list_by_tenant("t1").await.unwrap();
        assert_eq!(list.len(), 2);
        let list_t2 = storage.list_by_tenant("t2").await.unwrap();
        assert_eq!(list_t2.len(), 1);
    }

    #[tokio::test]
    async fn update_anchor() {
        let (_tmp, storage) = tmp_storage();
        let anchor = sample_anchor();
        storage.create("t1", &anchor).await.unwrap();
        let mut updated = anchor.clone();
        updated.jwks_uri = "https://keycloak.example.com/v2/jwks.json".to_string();
        updated.constraints = vec![OutputConstraint {
            name: "test".to_string(),
            expression: "true".to_string(),
        }];
        storage.update("t1", &updated).await.unwrap();
        let found = storage
            .find_by_id("t1", "tenant-keycloak")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.jwks_uri, updated.jwks_uri);
        assert_eq!(found.constraints.len(), 1);
        assert_eq!(found.constraints[0].name, "test");
    }

    #[tokio::test]
    async fn delete_anchor() {
        let (_tmp, storage) = tmp_storage();
        let anchor = sample_anchor();
        storage.create("t1", &anchor).await.unwrap();
        storage.delete("t1", "tenant-keycloak").await.unwrap();
        let found = storage.find_by_id("t1", "tenant-keycloak").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn delete_missing_errors() {
        let (_tmp, storage) = tmp_storage();
        let result = storage.delete("t1", "nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn update_missing_errors() {
        let (_tmp, storage) = tmp_storage();
        let anchor = sample_anchor();
        let result = storage.update("t1", &anchor).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn tenant_isolation() {
        let (_tmp, storage) = tmp_storage();
        let anchor = sample_anchor();
        storage.create("t1", &anchor).await.unwrap();
        let found = storage.find_by_id("t2", "tenant-keycloak").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn anchor_with_constraints_roundtrip() {
        let (_tmp, storage) = tmp_storage();
        let mut anchor = sample_anchor();
        anchor.constraints = vec![
            OutputConstraint {
                name: "scope check".to_string(),
                expression: "output.files.all(f, f.startsWith(\"src/\"))".to_string(),
            },
            OutputConstraint {
                name: "no secrets".to_string(),
                expression: "!output.files.exists(f, f.endsWith(\".env\"))".to_string(),
            },
        ];
        storage.create("t1", &anchor).await.unwrap();
        let found = storage
            .find_by_id("t1", "tenant-keycloak")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.constraints.len(), 2);
        assert_eq!(found.constraints[0].name, "scope check");
        assert_eq!(found.constraints[1].name, "no secrets");
    }
}
