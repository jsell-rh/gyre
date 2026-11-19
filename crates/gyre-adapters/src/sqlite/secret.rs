use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::{Id, Secret, SecretScope, SecretType};
use gyre_ports::SecretRepository;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};

use super::SqliteStorage;
use crate::schema::secrets;
use crate::sops::{self, AgeIdentity};

/// KV namespace + key where the auto-generated encryption key is persisted
/// (only used when GYRE_SECRET_ENCRYPTION_KEY is not set).
const KEY_NAMESPACE: &str = "secrets";
const KEY_KV_KEY: &str = "encryption_key";

/// KV key (same `secrets` namespace) holding the auto-generated SOPS age
/// identity used when GYRE_SOPS_AGE_KEY / GYRE_SOPS_AGE_KEY_FILE is not
/// configured (admin-panel.md / platform-model.md §7: SOPS is the default
/// secret backend; the identity is persisted server-side only).
const SOPS_AGE_KV_KEY: &str = "sops_age_identity";

/// The at-rest format for newly written secrets.
///
/// - `Sops` (default, `GYRE_SECRET_BACKEND` unset or `sops`): values are
///   stored as genuine SOPS documents (see [`crate::sops`]).
/// - `Legacy` (`GYRE_SECRET_BACKEND=legacy`): the pre-SOPS ring
///   AES-256-GCM format — an operator escape hatch; legacy rows are always
///   readable either way and are re-encrypted to SOPS on first access when
///   the backend is `Sops`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtRestFormat {
    Sops,
    Legacy,
}

fn at_rest_format() -> AtRestFormat {
    match std::env::var("GYRE_SECRET_BACKEND")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "legacy" | "aesgcm" | "aes-gcm" => AtRestFormat::Legacy,
        // Default: SOPS (admin-panel.md: "SOPS as default"). The `vault`
        // value never reaches this adapter — build_state wires the Vault
        // repository instead — but treating it like the default here keeps
        // a misconfigured deployment readable.
        _ => AtRestFormat::Sops,
    }
}

/// Load the SOPS age identity.
///
/// Priority:
/// 1. `GYRE_SOPS_AGE_KEY` (an `AGE-SECRET-KEY-1…` bech32 string).
/// 2. `GYRE_SOPS_AGE_KEY_FILE` (a file containing the identity string).
/// 3. An auto-generated identity, persisted in the kv_store table so it is
///    stable across server restarts (server-side only — never exposed).
fn load_sops_identity(conn: &mut SqliteConnection) -> Result<AgeIdentity> {
    if let Ok(key) = std::env::var("GYRE_SOPS_AGE_KEY") {
        if !key.trim().is_empty() {
            return sops::AgeIdentity::from_str(&key);
        }
    }
    if let Ok(path) = std::env::var("GYRE_SOPS_AGE_KEY_FILE") {
        if !path.trim().is_empty() {
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("reading GYRE_SOPS_AGE_KEY_FILE {path}"))?;
            return sops::AgeIdentity::from_str(&contents);
        }
    }

    match kv_get(conn, KEY_NAMESPACE, SOPS_AGE_KV_KEY)? {
        Some(stored) => sops::AgeIdentity::from_str(&stored)
            .with_context(|| "persisted SOPS age identity is unreadable"),
        None => {
            let identity = AgeIdentity::generate();
            kv_set(
                conn,
                KEY_NAMESPACE,
                SOPS_AGE_KV_KEY,
                &identity.to_secret_string(),
            )?;
            tracing::warn!(
                "SOPS age identity: GYRE_SOPS_AGE_KEY is not set; generated an identity and \
                 persisted it in the database. Decryption at rest depends on the database \
                 contents — set GYRE_SOPS_AGE_KEY (or GYRE_SOPS_AGE_KEY_FILE) to an identity \
                 managed outside the database, and re-encrypt stored secrets, to rotate."
            );
            Ok(identity)
        }
    }
}

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

/// Derive the 32-byte AES key from the `GYRE_SECRET_ENCRYPTION_KEY` value.
///
/// Accepts a 64-char hex string (32 bytes) or any other string, which is
/// hashed with SHA-256 to derive 32 key bytes. Returns `None` when the env
/// var is unset. Pure: no process state mutated, so it is unit-testable.
fn key_bytes_from_env_value(env_key: &str) -> Result<[u8; 32]> {
    use ring::digest::{digest, SHA256};
    if env_key.trim().len() == 64 {
        // Hex-encoded 32-byte key.
        let bytes = hex::decode(env_key.trim())
            .map_err(|e| anyhow::anyhow!("GYRE_SECRET_ENCRYPTION_KEY is not valid hex: {e}"))?;
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    } else {
        // Arbitrary passphrase: derive 32 bytes via SHA-256.
        let d = digest(&SHA256, env_key.as_bytes());
        let mut arr = [0u8; 32];
        arr.copy_from_slice(d.as_ref());
        Ok(arr)
    }
}

/// Load the AES-256-GCM key.
///
/// Priority:
/// 1. `GYRE_SECRET_ENCRYPTION_KEY` env var (see [`key_bytes_from_env_value`]).
/// 2. Auto-generated random key, persisted in the kv_store table so it is
///    stable across server restarts.
fn load_encryption_key(conn: &mut SqliteConnection) -> Result<LessSafeKey> {
    let key_bytes: [u8; 32] = if let Ok(env_key) = std::env::var("GYRE_SECRET_ENCRYPTION_KEY") {
        key_bytes_from_env_value(&env_key)?
    } else {
        // Auto-generate and persist so restarts can still decrypt stored secrets.
        let existing = kv_get(conn, KEY_NAMESPACE, KEY_KV_KEY)?;
        if let Some(hex_key) = existing {
            tracing::warn!(
                "secret encryption key: GYRE_SECRET_ENCRYPTION_KEY is not set; using the \
                 auto-generated key persisted in the database. Encryption at rest is degraded \
                 to obfuscation against an attacker who can read the database (the key sits \
                 alongside the ciphertext in kv_store). Set GYRE_SECRET_ENCRYPTION_KEY to a \
                 value managed outside the database."
            );
            let bytes = hex::decode(&hex_key)
                .map_err(|e| anyhow::anyhow!("persisted secret key is corrupt: {e}"))?;
            if bytes.len() != 32 {
                anyhow::bail!("persisted secret key has wrong length {}", bytes.len());
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
        } else {
            tracing::warn!(
                "secret encryption key: GYRE_SECRET_ENCRYPTION_KEY is not set and no key is \
                 persisted yet; generated a random key and persisted it in the database. \
                 Encryption at rest is degraded to obfuscation against an attacker who can \
                 read the database (the key sits alongside the ciphertext in kv_store). Set \
                 GYRE_SECRET_ENCRYPTION_KEY to a value managed outside the database."
            );
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

/// Encrypt `plaintext` in the configured at-rest format.
/// Returns `(encrypted_value, nonce)`: for SOPS the nonce column is empty
/// (the document carries its own IVs); for legacy it holds the 12-byte
/// AES-GCM nonce.
fn encrypt_at_rest(conn: &mut SqliteConnection, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    match at_rest_format() {
        AtRestFormat::Sops => {
            let identity = load_sops_identity(conn)?;
            let doc = sops::sops_encrypt(plaintext, &identity, now_secs())?;
            Ok((doc, Vec::new()))
        }
        AtRestFormat::Legacy => {
            let key = load_encryption_key(conn)?;
            encrypt_value(&key, plaintext)
        }
    }
}

/// Decrypt a stored value, dispatching on the stored format (a SOPS JSON
/// document vs. legacy raw AES-256-GCM ciphertext). When the backend is
/// SOPS and the row is legacy, it is transparently re-encrypted as a SOPS
/// document so pre-upgrade secrets remain resolvable AND migrate
/// (admin-panel.md: secrets stored before the change must remain
/// resolvable after the upgrade).
fn decrypt_at_rest(
    conn: &mut SqliteConnection,
    encrypted_value: &[u8],
    nonce: &[u8],
    row_id: &str,
) -> Result<Vec<u8>> {
    if sops::looks_like_sops(encrypted_value) {
        let identity = load_sops_identity(conn)?;
        return sops::sops_decrypt(encrypted_value, &identity);
    }

    // Legacy row: AES-256-GCM with the nonce column.
    let key = load_encryption_key(conn)?;
    let plaintext = decrypt_value(&key, encrypted_value, nonce)?;

    if at_rest_format() == AtRestFormat::Sops {
        // First-access migration: re-encrypt as a SOPS document.
        let identity = load_sops_identity(conn)?;
        match sops::sops_encrypt(&plaintext, &identity, now_secs()) {
            Ok(doc) => {
                let updated = diesel::update(secrets::table.filter(secrets::id.eq(row_id)))
                    .set((
                        secrets::encrypted_value.eq(doc),
                        secrets::nonce.eq(Vec::<u8>::new()),
                    ))
                    .execute(conn);
                if let Err(e) = updated {
                    // The value decrypted fine — a failed migration write
                    // must not fail the read; the row migrates on the next
                    // access.
                    tracing::warn!(
                        secret_id = row_id,
                        "re-encrypting legacy secret to SOPS failed (will retry): {e}"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    secret_id = row_id,
                    "re-encrypting legacy secret to SOPS failed (will retry): {e}"
                );
            }
        }
    }
    Ok(plaintext)
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
        let (encrypted_value, nonce) = encrypt_at_rest(&mut conn, value)?;
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
        let row = secrets::table
            .filter(secrets::id.eq(id.as_str()))
            .filter(secrets::tenant_id.eq(tenant_id))
            .select((secrets::encrypted_value, secrets::nonce))
            .first::<(Vec<u8>, Vec<u8>)>(&mut conn)
            .optional()?;
        match row {
            None => Ok(None),
            Some((encrypted_value, nonce)) => Ok(Some(decrypt_at_rest(
                &mut conn,
                &encrypted_value,
                &nonce,
                id.as_str(),
            )?)),
        }
    }

    async fn find_by_id(&self, id: &Id, tenant_id: &str) -> Result<Option<Secret>> {
        let mut conn = self.pool.get()?;
        let row = secrets::table
            .filter(secrets::id.eq(id.as_str()))
            .filter(secrets::tenant_id.eq(tenant_id))
            .select(SecretRow::as_select())
            .first(&mut conn)
            .optional()?;
        Ok(row.map(SecretRow::into_secret).transpose()?)
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
        let (encrypted_value, nonce) = encrypt_at_rest(&mut conn, new_value)?;
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
        let mut by_name: std::collections::HashMap<String, (String, Vec<u8>, Vec<u8>)> =
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
                .select((
                    secrets::name,
                    secrets::id,
                    secrets::encrypted_value,
                    secrets::nonce,
                ))
                .load::<(String, String, Vec<u8>, Vec<u8>)>(&mut conn)?;
            for (name, row_id, encrypted_value, nonce) in rows {
                by_name.insert(name, (row_id, encrypted_value, nonce));
            }
        }

        let mut resolved: Vec<(String, Vec<u8>)> = Vec::with_capacity(by_name.len());
        for (name, (row_id, encrypted_value, nonce)) in by_name {
            resolved.push((
                name,
                decrypt_at_rest(&mut conn, &encrypted_value, &nonce, &row_id)?,
            ));
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

    /// Env-var manipulation must be serialized across tests in this module
    /// (they share a process).
    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
    async fn stored_value_is_a_real_sops_document_by_default() {
        let _guard = ENV_LOCK.lock().await;
        let (_tmp, storage) = tmp_storage();
        let plaintext = b"postgres://localhost/gyre".to_vec();
        let secret = sample_secret("DATABASE_URL", SecretScope::Repo, "repo-1", "t1");
        storage.create(&secret, &plaintext).await.unwrap();
        let (encrypted_value, nonce) = decrypt_row(&storage, secret.id.as_str());

        // SOPS is the default backend: the stored bytes are a JSON SOPS
        // document (decryptable with the sops CLI given the age identity)
        // with an empty nonce column (the document carries its own IVs).
        assert_eq!(nonce, Vec::<u8>::new());
        let doc: serde_json::Value = serde_json::from_slice(&encrypted_value).unwrap();
        assert!(doc.get("data").is_some(), "SOPS document has a data key");
        let sops = doc.get("sops").expect("SOPS document has a sops key");
        assert!(
            sops.get("age")
                .is_some_and(|a| a.as_array().is_some_and(|arr| !arr.is_empty())),
            "data key is age-encrypted: {sops}"
        );
        let stored = doc["data"].as_str().unwrap();
        assert!(
            stored.starts_with("ENC[AES256_GCM,data:"),
            "value is SOPS-encrypted, got {stored}"
        );
        // Plaintext must not appear anywhere in the stored document.
        assert!(!encrypted_value
            .windows(plaintext.len().min(encrypted_value.len()))
            .any(|w| w == plaintext));
    }

    #[tokio::test]
    async fn sops_document_resolves_with_identity_from_env() {
        // A GYRE_SOPS_AGE_KEY from the environment must decrypt values
        // written under the same identity (managed-outside-DB deployment).
        let _guard = ENV_LOCK.lock().await;
        let identity = AgeIdentity::generate();
        std::env::set_var("GYRE_SOPS_AGE_KEY", identity.to_secret_string());
        let (_tmp, storage) = tmp_storage();
        let secret = sample_secret("K", SecretScope::Repo, "repo-1", "t1");
        storage.create(&secret, b"env-key-value").await.unwrap();
        let value = storage.get_value(&secret.id, "t1").await.unwrap().unwrap();
        assert_eq!(value, b"env-key-value".to_vec());
        // The stored document can be decrypted with the raw identity
        // through the SOPS module (the format the sops CLI consumes).
        let (encrypted, _) = decrypt_row(&storage, secret.id.as_str());
        let decrypted = sops::sops_decrypt(&encrypted, &identity).unwrap();
        assert_eq!(decrypted, b"env-key-value".to_vec());
        std::env::remove_var("GYRE_SOPS_AGE_KEY");
    }

    #[tokio::test]
    async fn sops_identity_from_key_file() {
        let _guard = ENV_LOCK.lock().await;
        let identity = AgeIdentity::generate();
        let keyfile = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(keyfile.path(), identity.to_secret_string()).unwrap();
        std::env::set_var("GYRE_SOPS_AGE_KEY_FILE", keyfile.path());
        let (_tmp, storage) = tmp_storage();
        let secret = sample_secret("K", SecretScope::Repo, "repo-1", "t1");
        storage.create(&secret, b"file-key-value").await.unwrap();
        let value = storage.get_value(&secret.id, "t1").await.unwrap().unwrap();
        assert_eq!(value, b"file-key-value".to_vec());
        std::env::remove_var("GYRE_SOPS_AGE_KEY_FILE");
    }

    #[tokio::test]
    async fn auto_generated_identity_is_persisted_and_stable() {
        let _guard = ENV_LOCK.lock().await;
        std::env::remove_var("GYRE_SOPS_AGE_KEY");
        std::env::remove_var("GYRE_SOPS_AGE_KEY_FILE");
        let (tmp, storage) = tmp_storage();
        let secret = sample_secret("K", SecretScope::Repo, "repo-1", "t1");
        storage.create(&secret, b"auto-value").await.unwrap();

        // The generated identity is persisted in kv_store (server-side
        // only) and the same storage reopens with the same identity.
        {
            let mut conn = storage.pool.get().unwrap();
            let stored = kv_get(&mut conn, KEY_NAMESPACE, SOPS_AGE_KV_KEY)
                .unwrap()
                .expect("identity persisted");
            assert!(stored.starts_with("AGE-SECRET-KEY-"));
        }
        drop(storage);
        let storage2 = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        let value = storage2.get_value(&secret.id, "t1").await.unwrap().unwrap();
        assert_eq!(value, b"auto-value".to_vec());
    }

    #[tokio::test]
    async fn legacy_rows_remain_resolvable_and_migrate_to_sops() {
        let _guard = ENV_LOCK.lock().await;
        // Write in the legacy AES-GCM format (pre-upgrade rows).
        std::env::set_var("GYRE_SECRET_BACKEND", "legacy");
        std::env::remove_var("GYRE_SOPS_AGE_KEY");
        std::env::remove_var("GYRE_SOPS_AGE_KEY_FILE");
        let (_tmp, storage) = tmp_storage();
        let secret = sample_secret("LEGACY", SecretScope::Workspace, "ws-1", "t1");
        storage.create(&secret, b"legacy-plaintext").await.unwrap();
        let (legacy_value, legacy_nonce) = decrypt_row(&storage, secret.id.as_str());
        assert_eq!(legacy_nonce.len(), ring::aead::NONCE_LEN);
        assert!(sops::looks_like_sops(&legacy_value) == false);

        // Upgrade: default (SOPS) backend. The legacy row must remain
        // resolvable and be re-encrypted on first access.
        std::env::remove_var("GYRE_SECRET_BACKEND");
        let value = storage
            .get_value(&secret.id, "t1")
            .await
            .unwrap()
            .expect("legacy secret remains resolvable");
        assert_eq!(value, b"legacy-plaintext".to_vec());

        let (migrated, nonce) = decrypt_row(&storage, secret.id.as_str());
        assert_eq!(
            nonce,
            Vec::<u8>::new(),
            "nonce column cleared after migration"
        );
        assert!(
            sops::looks_like_sops(&migrated),
            "row re-encrypted as a SOPS document"
        );

        // resolve_for_agent also serves the migrated value.
        let resolved = storage
            .resolve_for_agent("t1", "ws-1", "repo-1", None)
            .await
            .unwrap();
        assert!(resolved
            .iter()
            .any(|(n, v)| n == "LEGACY" && *v == b"legacy-plaintext".to_vec()));
    }

    #[tokio::test]
    async fn legacy_backend_option_still_writes_legacy_format() {
        let _guard = ENV_LOCK.lock().await;
        std::env::set_var("GYRE_SECRET_BACKEND", "legacy");
        let (_tmp, storage) = tmp_storage();
        let plaintext = b"escape-hatch".to_vec();
        let secret = sample_secret("X", SecretScope::Repo, "repo-1", "t1");
        storage.create(&secret, &plaintext).await.unwrap();
        let (encrypted_value, nonce) = decrypt_row(&storage, secret.id.as_str());
        assert_ne!(encrypted_value, plaintext);
        assert_eq!(encrypted_value.len(), plaintext.len() + 16);
        assert_eq!(nonce.len(), ring::aead::NONCE_LEN);
        assert!(!encrypted_value
            .windows(plaintext.len().min(encrypted_value.len()))
            .any(|w| w == plaintext));
        std::env::remove_var("GYRE_SECRET_BACKEND");
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
        let (encrypted_value, _) = decrypt_row(&storage, secret.id.as_str());
        // Tamper with the ciphertext inside the SOPS document: replace the
        // first base64 character of the encrypted value segment with a
        // different valid base64 character (the document stays valid JSON,
        // the ciphertext no longer matches its authentication tag).
        let mut text = String::from_utf8(encrypted_value.clone()).unwrap();
        let marker = "ENC[AES256_GCM,data:";
        let pos = text
            .find(marker)
            .expect("document contains an encrypted value")
            + marker.len();
        let original = text.as_bytes()[pos];
        let replacement = if original == b'A' { b'B' } else { b'A' };
        unsafe {
            text.as_bytes_mut()[pos] = replacement;
        }
        let err = sops::sops_decrypt(text.as_bytes(), &AgeIdentity::generate()).unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("fail")
                || err.to_string().to_lowercase().contains("mac")
                || err.to_string().to_lowercase().contains("malformed"),
            "tampered SOPS document must not decrypt, got: {err}"
        );
    }

    #[tokio::test]
    async fn wrong_key_fails_decryption() {
        let _guard = ENV_LOCK.lock().await;
        // Legacy-format row (AES-GCM): encrypt with the storage's key, then
        // decrypt with a different key constructed directly.
        std::env::set_var("GYRE_SECRET_BACKEND", "legacy");
        let (_tmp, storage) = tmp_storage();
        let secret = sample_secret("A", SecretScope::Repo, "repo-1", "t1");
        storage.create(&secret, b"some-secret-value").await.unwrap();
        std::env::remove_var("GYRE_SECRET_BACKEND");
        let (ciphertext, nonce) = decrypt_row(&storage, secret.id.as_str());
        let other_bytes = [7u8; 32];
        let other_key = LessSafeKey::new(
            UnboundKey::new(&AES_256_GCM, &other_bytes).expect("valid key length"),
        );
        let err = decrypt_value(&other_key, &ciphertext, &nonce).unwrap_err();
        assert!(err.to_string().contains("decryption failed"));
    }

    #[tokio::test]
    async fn sops_document_with_wrong_identity_fails() {
        // SOPS-format row: a document encrypted for another age identity
        // must not decrypt (fail closed).
        let (_tmp, storage) = tmp_storage();
        let secret = sample_secret("A", SecretScope::Repo, "repo-1", "t1");
        storage.create(&secret, b"some-secret-value").await.unwrap();
        let (ciphertext, _) = decrypt_row(&storage, secret.id.as_str());
        let err = sops::sops_decrypt(&ciphertext, &AgeIdentity::generate()).unwrap_err();
        assert!(
            ["age", "decrypt", "unwrap", "key"]
                .iter()
                .any(|k| err.to_string().to_lowercase().contains(k)),
            "wrong identity must fail closed, got: {err}"
        );
    }

    #[test]
    fn env_key_derivation_hex_and_passphrase() {
        // 64-hex-char value decodes to the raw 32 bytes.
        let hex_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(
            key_bytes_from_env_value(hex_key).unwrap(),
            [
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
                0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67,
                0x89, 0xab, 0xcd, 0xef
            ]
        );
        // Non-hex value is hashed with SHA-256.
        let d = ring::digest::digest(&ring::digest::SHA256, b"passphrase");
        let mut expected = [0u8; 32];
        expected.copy_from_slice(d.as_ref());
        assert_eq!(key_bytes_from_env_value("passphrase").unwrap(), expected);
        // Whitespace around a hex value is trimmed before decoding.
        assert_eq!(
            key_bytes_from_env_value(&format!("  {hex_key}  ")).unwrap(),
            key_bytes_from_env_value(hex_key).unwrap()
        );
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
