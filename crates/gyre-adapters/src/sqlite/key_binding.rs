use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::KeyBinding;
use gyre_ports::KeyBindingRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::key_bindings;

#[derive(Queryable, Selectable)]
#[diesel(table_name = key_bindings)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct KeyBindingRow {
    #[allow(dead_code)]
    id: String,
    user_identity: String,
    #[allow(dead_code)]
    tenant_id: String,
    public_key: Vec<u8>,
    issuer: String,
    trust_anchor_id: String,
    issued_at: i64,
    expires_at: i64,
    user_signature: Vec<u8>,
    platform_countersign: Vec<u8>,
    #[allow(dead_code)]
    revoked_at: Option<i64>,
}

impl KeyBindingRow {
    fn into_key_binding(self) -> KeyBinding {
        KeyBinding {
            public_key: self.public_key,
            user_identity: self.user_identity,
            issuer: self.issuer,
            trust_anchor_id: self.trust_anchor_id,
            issued_at: self.issued_at as u64,
            expires_at: self.expires_at as u64,
            user_signature: self.user_signature,
            platform_countersign: self.platform_countersign,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = key_bindings)]
struct NewKeyBindingRow<'a> {
    id: &'a str,
    user_identity: &'a str,
    tenant_id: &'a str,
    public_key: &'a [u8],
    issuer: &'a str,
    trust_anchor_id: &'a str,
    issued_at: i64,
    expires_at: i64,
    user_signature: &'a [u8],
    platform_countersign: &'a [u8],
}

/// Generate a deterministic ID from tenant + public key bytes.
fn binding_id(tenant_id: &str, public_key: &[u8]) -> String {
    format!("{}:{}", tenant_id, hex::encode(public_key))
}

#[async_trait]
impl KeyBindingRepository for SqliteStorage {
    async fn store(&self, tenant_id: &str, binding: &KeyBinding) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = tenant_id.to_string();
        let binding = binding.clone();
        let id = binding_id(&tenant_id, &binding.public_key);
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewKeyBindingRow {
                id: &id,
                user_identity: &binding.user_identity,
                tenant_id: &tenant_id,
                public_key: &binding.public_key,
                issuer: &binding.issuer,
                trust_anchor_id: &binding.trust_anchor_id,
                issued_at: binding.issued_at as i64,
                expires_at: binding.expires_at as i64,
                user_signature: &binding.user_signature,
                platform_countersign: &binding.platform_countersign,
            };
            diesel::insert_into(key_bindings::table)
                .values(&row)
                .on_conflict(key_bindings::id)
                .do_update()
                .set((
                    key_bindings::user_identity.eq(&binding.user_identity),
                    key_bindings::issuer.eq(&binding.issuer),
                    key_bindings::trust_anchor_id.eq(&binding.trust_anchor_id),
                    key_bindings::issued_at.eq(binding.issued_at as i64),
                    key_bindings::expires_at.eq(binding.expires_at as i64),
                    key_bindings::user_signature.eq(&binding.user_signature as &[u8]),
                    key_bindings::platform_countersign.eq(&binding.platform_countersign as &[u8]),
                    key_bindings::revoked_at.eq(None::<i64>),
                ))
                .execute(&mut *conn)
                .context("upsert key binding")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_public_key(
        &self,
        tenant_id: &str,
        public_key: &[u8],
    ) -> Result<Option<KeyBinding>> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = tenant_id.to_string();
        let public_key = public_key.to_vec();
        tokio::task::spawn_blocking(move || -> Result<Option<KeyBinding>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = key_bindings::table
                .filter(key_bindings::tenant_id.eq(&tenant_id))
                .filter(key_bindings::public_key.eq(&public_key))
                .filter(key_bindings::revoked_at.is_null())
                .first::<KeyBindingRow>(&mut *conn)
                .optional()
                .context("find key binding by public key")?;
            Ok(row.map(KeyBindingRow::into_key_binding))
        })
        .await?
    }

    async fn find_active_by_identity(
        &self,
        tenant_id: &str,
        user_identity: &str,
    ) -> Result<Vec<KeyBinding>> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = tenant_id.to_string();
        let user_identity = user_identity.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<KeyBinding>> {
            let mut conn = pool.get().context("get db connection")?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            let rows = key_bindings::table
                .filter(key_bindings::tenant_id.eq(&tenant_id))
                .filter(key_bindings::user_identity.eq(&user_identity))
                .filter(key_bindings::revoked_at.is_null())
                .filter(key_bindings::expires_at.gt(now))
                .load::<KeyBindingRow>(&mut *conn)
                .context("find active key bindings by identity")?;
            Ok(rows
                .into_iter()
                .map(KeyBindingRow::into_key_binding)
                .collect())
        })
        .await?
    }

    async fn invalidate(&self, tenant_id: &str, public_key: &[u8]) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = tenant_id.to_string();
        let public_key = public_key.to_vec();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            diesel::update(
                key_bindings::table
                    .filter(key_bindings::tenant_id.eq(&tenant_id))
                    .filter(key_bindings::public_key.eq(&public_key))
                    .filter(key_bindings::revoked_at.is_null()),
            )
            .set(key_bindings::revoked_at.eq(Some(now)))
            .execute(&mut *conn)
            .context("invalidate key binding")?;
            Ok(())
        })
        .await?
    }

    async fn invalidate_all_for_identity(
        &self,
        tenant_id: &str,
        user_identity: &str,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = tenant_id.to_string();
        let user_identity = user_identity.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            diesel::update(
                key_bindings::table
                    .filter(key_bindings::tenant_id.eq(&tenant_id))
                    .filter(key_bindings::user_identity.eq(&user_identity))
                    .filter(key_bindings::revoked_at.is_null()),
            )
            .set(key_bindings::revoked_at.eq(Some(now)))
            .execute(&mut *conn)
            .context("invalidate all key bindings for identity")?;
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

    fn sample_binding() -> KeyBinding {
        KeyBinding {
            public_key: vec![1, 2, 3, 4, 5, 6, 7, 8],
            user_identity: "user:jsell".to_string(),
            issuer: "https://keycloak.example.com".to_string(),
            trust_anchor_id: "tenant-keycloak".to_string(),
            issued_at: 1_700_000_000,
            expires_at: 4_102_444_800, // 2100-01-01 — far future so tests don't expire
            user_signature: vec![10, 20, 30, 40],
            platform_countersign: vec![50, 60, 70, 80],
        }
    }

    #[tokio::test]
    async fn store_and_find_by_public_key() {
        let (_tmp, storage) = tmp_storage();
        let binding = sample_binding();
        storage.store("t1", &binding).await.unwrap();
        let found = storage
            .find_by_public_key("t1", &binding.public_key)
            .await
            .unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.user_identity, "user:jsell");
        assert_eq!(found.public_key, binding.public_key);
        assert_eq!(found.issued_at, 1_700_000_000);
        assert_eq!(found.expires_at, 4_102_444_800);
    }

    #[tokio::test]
    async fn find_missing_returns_none() {
        let (_tmp, storage) = tmp_storage();
        let found = storage
            .find_by_public_key("t1", &[99, 99, 99])
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn find_active_by_identity() {
        let (_tmp, storage) = tmp_storage();
        let b1 = sample_binding();
        let mut b2 = sample_binding();
        b2.public_key = vec![9, 8, 7, 6];
        storage.store("t1", &b1).await.unwrap();
        storage.store("t1", &b2).await.unwrap();
        let active = storage
            .find_active_by_identity("t1", "user:jsell")
            .await
            .unwrap();
        assert_eq!(active.len(), 2);
    }

    #[tokio::test]
    async fn invalidate_single() {
        let (_tmp, storage) = tmp_storage();
        let binding = sample_binding();
        storage.store("t1", &binding).await.unwrap();
        storage.invalidate("t1", &binding.public_key).await.unwrap();
        let found = storage
            .find_by_public_key("t1", &binding.public_key)
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn invalidate_all_for_identity() {
        let (_tmp, storage) = tmp_storage();
        let b1 = sample_binding();
        let mut b2 = sample_binding();
        b2.public_key = vec![9, 8, 7, 6];
        storage.store("t1", &b1).await.unwrap();
        storage.store("t1", &b2).await.unwrap();
        storage
            .invalidate_all_for_identity("t1", "user:jsell")
            .await
            .unwrap();
        let active = storage
            .find_active_by_identity("t1", "user:jsell")
            .await
            .unwrap();
        assert_eq!(active.len(), 0);
    }

    #[tokio::test]
    async fn tenant_isolation() {
        let (_tmp, storage) = tmp_storage();
        let binding = sample_binding();
        storage.store("t1", &binding).await.unwrap();
        let found = storage
            .find_by_public_key("t2", &binding.public_key)
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn invalidate_does_not_affect_other_tenant() {
        let (_tmp, storage) = tmp_storage();
        let binding = sample_binding();
        storage.store("t1", &binding).await.unwrap();
        storage.store("t2", &binding).await.unwrap();
        storage.invalidate("t1", &binding.public_key).await.unwrap();
        // t2 should still have it
        let found = storage
            .find_by_public_key("t2", &binding.public_key)
            .await
            .unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn find_active_excludes_expired_bindings() {
        let (_tmp, storage) = tmp_storage();
        // Store a binding that expired in the past (not revoked, but expired)
        let mut expired = sample_binding();
        expired.expires_at = 1_000_000_000; // well in the past
        storage.store("t1", &expired).await.unwrap();
        // Store a binding that is still valid
        let mut active = sample_binding();
        active.public_key = vec![20, 21, 22, 23];
        storage.store("t1", &active).await.unwrap();
        let results = storage
            .find_active_by_identity("t1", "user:jsell")
            .await
            .unwrap();
        // Only the non-expired binding should be returned
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].public_key, vec![20, 21, 22, 23]);
    }

    #[tokio::test]
    async fn binding_roundtrip_preserves_fields() {
        let (_tmp, storage) = tmp_storage();
        let binding = sample_binding();
        storage.store("t1", &binding).await.unwrap();
        let found = storage
            .find_by_public_key("t1", &binding.public_key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.issuer, binding.issuer);
        assert_eq!(found.trust_anchor_id, binding.trust_anchor_id);
        assert_eq!(found.user_signature, binding.user_signature);
        assert_eq!(found.platform_countersign, binding.platform_countersign);
    }
}
