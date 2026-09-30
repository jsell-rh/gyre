use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::{Id, Secret, SecretScope, SecretType};
use gyre_ports::SecretRepository;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};

use super::SqliteStorage;
use crate::schema::secrets;

/// KV namespace + key where the auto-generated encryption key is persisted
/// (only used when GYRE_SECRET_ENCRYPTION_KEY is not set).
const KEY_NAMESPACE: &str = "secrets";
const KEY_KV_KEY: &str = "encryption_key";

#[derive(Queryable, Selectable)]
#[diesel(table_name = secrets)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct SecretRow {
    id: String,
    name: String,
    scope: String,
    scope_id: String,
    secret_type: String,
    // Ciphertext + nonce only; plaintext never touches this struct.
    #[allow(dead_code)] // selected via explicit column tuples elsewhere
    encrypted_value: Vec<u8>,
    #[allow(dead_code)]
    nonce: Vec<u8>,
    created_by: String,
    created_at: i64,
    expires_at: Option<i64>,
    last_rotated_at: Option<i64>,
    tenant_id: String,
}

impl SecretRow {
    fn into_secret(self) -> Result<Secret> {
        let scope = SecretScope::from_str_opt(&self.scope)
            .with_context(|| format!("unknown secret scope {:?}", self.scope))?;
        let secret_type = SecretType::from_str_opt(&self.secret_type)
            .with_context(|| format!("unknown secret type {:?}", self.secret_type))?;
        Ok(Secret {
            id: Id::new(self.id),
            name: self.name,
            scope,
            scope_id: self.scope_id,
            secret_type,
            created_by: self.created_by,
            created_at: self.created_at as u64,
            expires_at: self.expires_at.map(|t| t as u64),
            last_rotated_at: self.last_rotated_at.map(|t| t as u64),
            tenant_id: self.tenant_id,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = secrets)]
struct NewSecretRow<'a> {
    id: &'a str,
    name: &'a str,
    scope: &'a str,
    scope_id: &'a str,
    secret_type: &'a str,
    encrypted_value: Vec<u8>,
    nonce: Vec<u8>,
    created_by: &'a str,
    created_at: i64,
    expires_at: Option<i64>,
    last_rotated_at: Option<i64>,
    tenant_id: &'a str,
}

/// Encrypt `plaintext` with AES-256-GCM under `key`.
/// Returns (ciphertext_with_tag, nonce). The tag is appended to the
/// ciphertext by ring's seal-in-place API.
fn encrypt_value(key: &LessSafeKey, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; ring::aead::NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| anyhow::anyhow!("system RNG failure generating secret nonce"))?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut in_out = plaintext.to_vec();
    let tag = key
        .seal_in_place_separate_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| anyhow::anyhow!("AES-256-GCM encryption failed"))?;
    in_out.extend_from_slice(tag.as_ref());
    Ok((in_out, nonce_bytes.to_vec()))
}

/// Decrypt an AES-256-GCM (ciphertext || tag) with `nonce` under `key`.
fn decrypt_value(key: &LessSafeKey, ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>> {
    if nonce.len() != ring::aead::NONCE_LEN {
        anyhow::bail!("secret nonce has wrong length {}", nonce.len());
    }
    let mut nonce_bytes = [0u8; ring::aead::NONCE_LEN];
    nonce_bytes.copy_from_slice(nonce);
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut in_out = ciphertext.to_vec();
    let plaintext_len = key
        .open_in_place(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| {
            anyhow::anyhow!("AES-256-GCM decryption failed (wrong key or tampered ciphertext)")
        })?
        .len();
    in_out.truncate(plaintext_len);
    Ok(in_out)
}

/// Load the AES-256-GCM key.
///
/// Priority:
/// 1. `GYRE_SECRET_ENCRYPTION_KEY` env var: hex string (64 hex chars) or any
///    other string, which is hashed with SHA-256 to derive 32 key bytes.
/// 2. Auto-generated random key, persisted in the kv_store table so it is
///    stable across server restarts.
fn load_encryption_key(conn: &mut SqliteConnection) -> Result<LessSafeKey> {
    use ring::digest::{digest, SHA256};
    let key_bytes: [u8; 32] = if let Ok(env_key) = std::env::var("GYRE_SECRET_ENCRYPTION_KEY") {
        if env_key.trim().len() == 64 {
            // Hex-encoded 32-byte key.
            let bytes = hex::decode(env_key.trim())
                .map_err(|e| anyhow::anyhow!("GYRE_SECRET_ENCRYPTION_KEY is not valid hex: {e}"))?;
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
        } else {
            // Arbitrary passphrase: derive 32 bytes via SHA-256.
            let d = digest(&SHA256, env_key.as_bytes());
            let mut arr = [0u8; 32];
            arr.copy_from_slice(d.as_ref());
            arr
        }
    } else {
        // Auto-generate and persist so restarts can still decrypt stored secrets.
        let existing = kv_get(conn, KEY_NAMESPACE, KEY_KV_KEY)?;
        if let Some(hex_key) = existing {
            let bytes = hex::decode(&hex_key)
                .map_err(|e| anyhow::anyhow!("persisted secret key is corrupt: {e}"))?;
            if bytes.len() != 32 {
                anyhow::bail!("persisted secret key has wrong length {}", bytes.len());
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
        } else {
            let rng = SystemRandom::new();
            let mut arr = [0u8; 32];
            rng.fill(&mut arr)
                .map_err(|_| anyhow::anyhow!("system RNG failure generating secret key"))?;
            kv_set(conn, KEY_NAMESPACE, KEY_KV_KEY, &hex::encode(arr))?;
            arr
        }
    };

    let unbound = UnboundKey::new(&AES_256_GCM, &key_bytes)
        .map_err(|_| anyhow::anyhow!("invalid AES-256-GCM key length"))?;
    Ok(LessSafeKey::new(unbound))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Minimal kv_store accessors (avoids the async port's spawn_blocking indirection).
fn kv_get(conn: &mut SqliteConnection, namespace: &str, key: &str) -> Result<Option<String>> {
    use crate::schema::kv_store;
    let row = kv_store::table
        .filter(kv_store::namespace.eq(namespace))
        .filter(kv_store::key.eq(key))
        .select(kv_store::value_json)
        .first::<String>(conn)
        .optional()?;
    Ok(row)
}

fn kv_set(conn: &mut SqliteConnection, namespace: &str, key: &str, value: &str) -> Result<()> {
    use crate::schema::kv_store;
    diesel::replace_into(kv_store::table)
        .values((
            kv_store::namespace.eq(namespace),
            kv_store::key.eq(key),
            kv_store::value_json.eq(value),
            kv_store::updated_at.eq(now_secs() as i64),
        ))
        .execute(conn)?;
    Ok(())
}

#[async_trait]
impl SecretRepository for SqliteStorage {
    async fn create(&self, secret: &Secret, value: &[u8]) -> Result<()> {
        let mut conn = self.pool.get()?;
        let key = load_encryption_key(&mut conn)?;
        let (encrypted_value, nonce) = encrypt_value(&key, value)?;
        let row = NewSecretRow {
            id: secret.id.as_str(),
            name: &secret.name,
            scope: secret.scope.as_str(),
            scope_id: &secret.scope_id,
            secret_type: secret.secret_type.as_str(),
            encrypted_value,
            nonce,
            created_by: &secret.created_by,
            created_at: secret.created_at as i64,
            expires_at: secret.expires_at.map(|t| t as i64),
            last_rotated_at: secret.last_rotated_at.map(|t| t as i64),
            tenant_id: &secret.tenant_id,
        };
        diesel::insert_into(secrets::table)
            .values(&row)
            .execute(&mut conn)?;
        Ok(())
    }

    async fn get_value(&self, id: &Id, tenant_id: &str) -> Result<Option<Vec<u8>>> {
        let mut conn = self.pool.get()?;
        let key = load_encryption_key(&mut conn)?;
        let row = secrets::table
            .filter(secrets::id.eq(id.as_str()))
            .filter(secrets::tenant_id.eq(tenant_id))
            .select((secrets::encrypted_value, secrets::nonce))
            .first::<(Vec<u8>, Vec<u8>)>(&mut conn)
            .optional()?;
        match row {
            None => Ok(None),
            Some((encrypted_value, nonce)) => {
                Ok(Some(decrypt_value(&key, &encrypted_value, &nonce)?))
            }
        }
    }

    async fn list_by_scope(
        &self,
        scope: SecretScope,
        scope_id: &str,
        tenant_id: &str,
    ) -> Result<Vec<Secret>> {
        let mut conn = self.pool.get()?;
        let rows = secrets::table
            .filter(secrets::scope.eq(scope.as_str()))
            .filter(secrets::scope_id.eq(scope_id))
            .filter(secrets::tenant_id.eq(tenant_id))
            .order(secrets::name)
            .select(SecretRow::as_select())
            .load(&mut conn)?;
        rows.into_iter().map(SecretRow::into_secret).collect()
    }

    async fn delete(&self, id: &Id, tenant_id: &str) -> Result<()> {
        let mut conn = self.pool.get()?;
        diesel::delete(secrets::table)
            .filter(secrets::id.eq(id.as_str()))
            .filter(secrets::tenant_id.eq(tenant_id))
            .execute(&mut conn)?;
        Ok(())
    }

    async fn rotate(&self, id: &Id, new_value: &[u8], tenant_id: &str) -> Result<()> {
        let mut conn = self.pool.get()?;
        let key = load_encryption_key(&mut conn)?;
        let (encrypted_value, nonce) = encrypt_value(&key, new_value)?;
        let rows = diesel::update(
            secrets::table
                .filter(secrets::id.eq(id.as_str()))
                .filter(secrets::tenant_id.eq(tenant_id)),
        )
        .set((
            secrets::encrypted_value.eq(encrypted_value),
            secrets::nonce.eq(nonce),
            secrets::last_rotated_at.eq(now_secs() as i64),
        ))
        .execute(&mut conn)?;
        if rows == 0 {
            anyhow::bail!("secret {} not found in tenant {}", id, tenant_id);
        }
        Ok(())
    }

    async fn resolve_for_agent(
        &self,
        tenant_id: &str,
        workspace_id: &str,
        repo_id: &str,
        task_id: Option<&str>,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        let mut conn = self.pool.get()?;
        let key = load_encryption_key(&mut conn)?;

        // Collect candidate rows from coarse to fine scope. Later (finer)
        // scopes overwrite same-name entries from coarser scopes, so the
        // nearest scope wins, mirroring the budget cascade.
        let mut scope_targets: Vec<(String, String)> = vec![
            (
                SecretScope::Tenant.as_str().to_string(),
                tenant_id.to_string(),
            ),
            (
                SecretScope::Workspace.as_str().to_string(),
                workspace_id.to_string(),
            ),
            (SecretScope::Repo.as_str().to_string(), repo_id.to_string()),
        ];
        if let Some(tid) = task_id {
            scope_targets.push((SecretScope::Task.as_str().to_string(), tid.to_string()));
        }

        let now = now_secs() as i64;
        let mut by_name: std::collections::HashMap<String, (Vec<u8>, Vec<u8>)> =
            std::collections::HashMap::new();
        for (scope, sid) in &scope_targets {
            let rows = secrets::table
                .filter(secrets::scope.eq(scope))
                .filter(secrets::scope_id.eq(sid))
                .filter(secrets::tenant_id.eq(tenant_id))
                // Expired secrets are never delivered.
                .filter(
                    secrets::expires_at
                        .is_null()
                        .or(secrets::expires_at.gt(now)),
                )
                .select((secrets::name, secrets::encrypted_value, secrets::nonce))
                .load::<(String, Vec<u8>, Vec<u8>)>(&mut conn)?;
            for (name, encrypted_value, nonce) in rows {
                by_name.insert(name, (encrypted_value, nonce));
            }
        }

        let mut resolved: Vec<(String, Vec<u8>)> = Vec::with_capacity(by_name.len());
        for (name, (encrypted_value, nonce)) in by_name {
            resolved.push((name, decrypt_value(&key, &encrypted_value, &nonce)?));
        }
        // Deterministic order for callers and tests.
        resolved.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(resolved)
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

    fn sample_secret(name: &str, scope: SecretScope, scope_id: &str, tenant_id: &str) -> Secret {
        Secret {
            id: Id::new(format!("secret-{name}-{scope_id}")),
            name: name.to_string(),
            scope,
            scope_id: scope_id.to_string(),
            secret_type: SecretType::Static,
            created_by: "user:admin".to_string(),
            created_at: 1_700_000_000,
            expires_at: None,
            last_rotated_at: None,
            tenant_id: tenant_id.to_string(),
        }
    }

    fn decrypt_row(storage: &SqliteStorage, secret_id: &str) -> (Vec<u8>, Vec<u8>) {
        let mut conn = storage.pool.get().unwrap();
        secrets::table
            .filter(secrets::id.eq(secret_id))
            .select((secrets::encrypted_value, secrets::nonce))
            .first::<(Vec<u8>, Vec<u8>)>(&mut conn)
            .unwrap()
    }

    #[tokio::test]
    async fn create_get_value_roundtrip() {
        let (_tmp, storage) = tmp_storage();
        let secret = sample_secret("DATABASE_URL", SecretScope::Repo, "repo-1", "t1");
        storage
            .create(&secret, b"postgres://localhost/gyre")
            .await
            .unwrap();
        let value = storage
            .get_value(&secret.id, "t1")
            .await
            .unwrap()
            .expect("secret should exist");
        assert_eq!(value, b"postgres://localhost/gyre".to_vec());
    }

    #[tokio::test]
    async fn stored_value_is_encrypted_at_rest() {
        let (_tmp, storage) = tmp_storage();
        let plaintext = b"postgres://localhost/gyre".to_vec();
        let secret = sample_secret("DATABASE_URL", SecretScope::Repo, "repo-1", "t1");
        storage.create(&secret, &plaintext).await.unwrap();
        let (encrypted_value, nonce) = decrypt_row(&storage, secret.id.as_str());
        assert_ne!(encrypted_value, plaintext);
        // GCM appends a 16-byte tag to the ciphertext.
        assert_eq!(encrypted_value.len(), plaintext.len() + 16);
        assert_eq!(nonce.len(), ring::aead::NONCE_LEN);
        // Plaintext bytes must not appear anywhere in the stored ciphertext.
        assert!(!encrypted_value
            .windows(plaintext.len().min(encrypted_value.len()))
            .any(|w| w == plaintext));
    }

    #[tokio::test]
    async fn get_value_missing_returns_none() {
        let (_tmp, storage) = tmp_storage();
        let found = storage.get_value(&Id::new("nope"), "t1").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn get_value_is_tenant_isolated() {
        let (_tmp, storage) = tmp_storage();
        let secret = sample_secret("DATABASE_URL", SecretScope::Repo, "repo-1", "t1");
        storage.create(&secret, b"value-t1").await.unwrap();
        let found = storage.get_value(&secret.id, "t2").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn list_by_scope_returns_metadata_only_matching_scope() {
        let (_tmp, storage) = tmp_storage();
        let s1 = sample_secret("A", SecretScope::Repo, "repo-1", "t1");
        let s2 = sample_secret("B", SecretScope::Repo, "repo-1", "t1");
        let s3 = sample_secret("C", SecretScope::Repo, "repo-2", "t1");
        let s4 = sample_secret("D", SecretScope::Repo, "repo-1", "t2");
        for s in [&s1, &s2, &s3, &s4] {
            storage.create(s, b"v").await.unwrap();
        }
        let listed = storage
            .list_by_scope(SecretScope::Repo, "repo-1", "t1")
            .await
            .unwrap();
        let names: Vec<&str> = listed.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["A", "B"]);
        // Metadata round-trips.
        assert_eq!(listed[0].scope, SecretScope::Repo);
        assert_eq!(listed[0].scope_id, "repo-1");
        assert_eq!(listed[0].tenant_id, "t1");
    }

    #[tokio::test]
    async fn delete_removes_secret() {
        let (_tmp, storage) = tmp_storage();
        let secret = sample_secret("A", SecretScope::Repo, "repo-1", "t1");
        storage.create(&secret, b"v").await.unwrap();
        storage.delete(&secret.id, "t1").await.unwrap();
        let found = storage.get_value(&secret.id, "t1").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn rotate_replaces_value_and_updates_timestamp() {
        let (_tmp, storage) = tmp_storage();
        let secret = sample_secret("TOKEN", SecretScope::Workspace, "ws-1", "t1");
        storage.create(&secret, b"old-token").await.unwrap();
        storage
            .rotate(&secret.id, b"new-token", "t1")
            .await
            .unwrap();
        let value = storage.get_value(&secret.id, "t1").await.unwrap().unwrap();
        assert_eq!(value, b"new-token".to_vec());
        let listed = storage
            .list_by_scope(SecretScope::Workspace, "ws-1", "t1")
            .await
            .unwrap();
        assert!(listed[0].last_rotated_at.is_some());
    }

    #[tokio::test]
    async fn rotate_missing_secret_fails() {
        let (_tmp, storage) = tmp_storage();
        let err = storage
            .rotate(&Id::new("nope"), b"v", "t1")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn resolve_for_agent_collects_all_scopes() {
        let (_tmp, storage) = tmp_storage();
        storage
            .create(
                &sample_secret("TENANT_KEY", SecretScope::Tenant, "t1", "t1"),
                b"tv",
            )
            .await
            .unwrap();
        storage
            .create(
                &sample_secret("WS_KEY", SecretScope::Workspace, "ws-1", "t1"),
                b"wv",
            )
            .await
            .unwrap();
        storage
            .create(
                &sample_secret("REPO_KEY", SecretScope::Repo, "repo-1", "t1"),
                b"rv",
            )
            .await
            .unwrap();
        storage
            .create(
                &sample_secret("TASK_KEY", SecretScope::Task, "task-9", "t1"),
                b"kv",
            )
            .await
            .unwrap();

        let resolved = storage
            .resolve_for_agent("t1", "ws-1", "repo-1", Some("task-9"))
            .await
            .unwrap();
        let map: std::collections::HashMap<String, Vec<u8>> = resolved.into_iter().collect();
        assert_eq!(map.get("TENANT_KEY").unwrap(), b"tv");
        assert_eq!(map.get("WS_KEY").unwrap(), b"wv");
        assert_eq!(map.get("REPO_KEY").unwrap(), b"rv");
        assert_eq!(map.get("TASK_KEY").unwrap(), b"kv");
        assert_eq!(map.len(), 4);
    }

    #[tokio::test]
    async fn resolve_for_agent_without_task_omits_task_secrets() {
        let (_tmp, storage) = tmp_storage();
        storage
            .create(
                &sample_secret("TASK_KEY", SecretScope::Task, "task-9", "t1"),
                b"kv",
            )
            .await
            .unwrap();
        storage
            .create(
                &sample_secret("REPO_KEY", SecretScope::Repo, "repo-1", "t1"),
                b"rv",
            )
            .await
            .unwrap();
        let resolved = storage
            .resolve_for_agent("t1", "ws-1", "repo-1", None)
            .await
            .unwrap();
        let names: Vec<&str> = resolved.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["REPO_KEY"]);
    }

    #[tokio::test]
    async fn resolve_for_agent_excludes_other_repos_and_tenants() {
        let (_tmp, storage) = tmp_storage();
        storage
            .create(
                &sample_secret("OTHER_REPO", SecretScope::Repo, "repo-2", "t1"),
                b"v",
            )
            .await
            .unwrap();
        storage
            .create(
                &sample_secret("OTHER_TENANT", SecretScope::Tenant, "t2", "t2"),
                b"v",
            )
            .await
            .unwrap();
        let resolved = storage
            .resolve_for_agent("t1", "ws-1", "repo-1", None)
            .await
            .unwrap();
        assert!(resolved.is_empty());
    }

    #[tokio::test]
    async fn resolve_for_agent_finer_scope_overrides_same_name() {
        let (_tmp, storage) = tmp_storage();
        storage
            .create(
                &sample_secret("SHARED", SecretScope::Tenant, "t1", "t1"),
                b"tenant-value",
            )
            .await
            .unwrap();
        storage
            .create(
                &sample_secret("SHARED", SecretScope::Repo, "repo-1", "t1"),
                b"repo-value",
            )
            .await
            .unwrap();
        let resolved = storage
            .resolve_for_agent("t1", "ws-1", "repo-1", None)
            .await
            .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].0, "SHARED");
        assert_eq!(resolved[0].1, b"repo-value".to_vec());
    }

    #[tokio::test]
    async fn resolve_for_agent_excludes_expired_secrets() {
        let (_tmp, storage) = tmp_storage();
        let mut expired = sample_secret("OLD", SecretScope::Repo, "repo-1", "t1");
        expired.expires_at = Some(1_000_000_000); // long past
        let mut fresh = sample_secret("NEW", SecretScope::Repo, "repo-1", "t1");
        fresh.expires_at = Some(4_102_444_800); // 2100-01-01
        storage.create(&expired, b"v").await.unwrap();
        storage.create(&fresh, b"v").await.unwrap();
        let resolved = storage
            .resolve_for_agent("t1", "ws-1", "repo-1", None)
            .await
            .unwrap();
        let names: Vec<&str> = resolved.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["NEW"]);
    }

    #[tokio::test]
    async fn tampered_ciphertext_fails_decryption() {
        let (_tmp, storage) = tmp_storage();
        let secret = sample_secret("A", SecretScope::Repo, "repo-1", "t1");
        storage.create(&secret, b"some-secret-value").await.unwrap();
        let mut conn = storage.pool.get().unwrap();
        diesel::update(secrets::table.filter(secrets::id.eq(secret.id.as_str())))
            .set(secrets::encrypted_value.eq(vec![0u8; 32]))
            .execute(&mut conn)
            .unwrap();
        drop(conn);
        let err = storage.get_value(&secret.id, "t1").await.unwrap_err();
        assert!(err.to_string().contains("decryption failed"));
    }

    #[tokio::test]
    async fn wrong_key_fails_decryption() {
        let (tmp, storage) = tmp_storage();
        let secret = sample_secret("A", SecretScope::Repo, "repo-1", "t1");
        storage.create(&secret, b"some-secret-value").await.unwrap();
        // Reopen with a different explicit key.
        drop(storage);
        std::env::set_var(
            "GYRE_SECRET_ENCRYPTION_KEY",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        );
        let storage2 = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        let err = storage2.get_value(&secret.id, "t1").await.unwrap_err();
        std::env::remove_var("GYRE_SECRET_ENCRYPTION_KEY");
        assert!(err.to_string().contains("decryption failed"));
    }

    #[tokio::test]
    async fn auto_generated_key_persists_across_reopen() {
        let (tmp, storage) = tmp_storage();
        let secret = sample_secret("A", SecretScope::Repo, "repo-1", "t1");
        storage.create(&secret, b"stable-value").await.unwrap();
        drop(storage);
        // No env var set: the persisted auto-generated key must decrypt it.
        let storage2 = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        let value = storage2
            .get_value(&secret.id, "t1")
            .await
            .unwrap()
            .expect("secret should decrypt with persisted key");
        assert_eq!(value, b"stable-value".to_vec());
    }
}
