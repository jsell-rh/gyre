//! Postgres implementation for the SecretRepository port (TASK-273:
//! replaces the former `bail!` stub).
//!
//! Mirrors the SQLite adapter: SOPS is the default at-rest format (see
//! [`crate::sops`]); `GYRE_SECRET_BACKEND=legacy` selects the ring
//! AES-256-GCM format; legacy rows remain readable under either backend
//! and are re-encrypted to SOPS on first access when the backend is SOPS.
//! The SOPS age identity comes from `GYRE_SOPS_AGE_KEY` /
//! `GYRE_SOPS_AGE_KEY_FILE`, or an auto-generated identity persisted in
//! kv_store.

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::{Id, Secret, SecretScope, SecretType};
use gyre_ports::SecretRepository;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::{kv_store, secrets};
use crate::sops::{self, AgeIdentity};

/// KV namespace + key where the auto-generated encryption key is persisted
/// (only used when GYRE_SECRET_ENCRYPTION_KEY is not set).
const KEY_NAMESPACE: &str = "secrets";
const KEY_KV_KEY: &str = "encryption_key";

/// KV key (same `secrets` namespace) holding the auto-generated SOPS age
/// identity used when GYRE_SOPS_AGE_KEY / GYRE_SOPS_AGE_KEY_FILE is not
/// configured.
const SOPS_AGE_KV_KEY: &str = "sops_age_identity";

/// The at-rest format for newly written secrets (see the SQLite adapter's
/// documentation for the semantics).
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
        _ => AtRestFormat::Sops,
    }
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = secrets)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Key / identity management ────────────────────────────────────────────────

fn kv_get(conn: &mut PgConnection, namespace: &str, key: &str) -> Result<Option<String>> {
    let row = kv_store::table
        .filter(kv_store::namespace.eq(namespace))
        .filter(kv_store::key.eq(key))
        .select(kv_store::value_json)
        .first::<String>(conn)
        .optional()?;
    Ok(row)
}

fn kv_set(conn: &mut PgConnection, namespace: &str, key: &str, value: &str) -> Result<()> {
    let now = now_secs() as i64;
    diesel::insert_into(kv_store::table)
        .values((
            kv_store::namespace.eq(namespace),
            kv_store::key.eq(key),
            kv_store::value_json.eq(value),
            kv_store::updated_at.eq(now),
        ))
        .on_conflict((kv_store::namespace, kv_store::key))
        .do_update()
        .set((
            kv_store::value_json.eq(value),
            kv_store::updated_at.eq(now),
        ))
        .execute(conn)?;
    Ok(())
}

/// Derive the 32-byte AES key from the `GYRE_SECRET_ENCRYPTION_KEY` value
/// (hex string or SHA-256-hashed passphrase), identical to the SQLite
/// adapter.
fn key_bytes_from_env_value(env_key: &str) -> Result<[u8; 32]> {
    use ring::digest::{digest, SHA256};
    if env_key.trim().len() == 64 {
        let bytes = hex::decode(env_key.trim())
            .map_err(|e| anyhow::anyhow!("GYRE_SECRET_ENCRYPTION_KEY is not valid hex: {e}"))?;
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    } else {
        let d = digest(&SHA256, env_key.as_bytes());
        let mut arr = [0u8; 32];
        arr.copy_from_slice(d.as_ref());
        Ok(arr)
    }
}

fn load_encryption_key(conn: &mut PgConnection) -> Result<LessSafeKey> {
    let key_bytes: [u8; 32] = if let Ok(env_key) = std::env::var("GYRE_SECRET_ENCRYPTION_KEY") {
        key_bytes_from_env_value(&env_key)?
    } else {
        let existing = kv_get(conn, KEY_NAMESPACE, KEY_KV_KEY)?;
        if let Some(hex_key) = existing {
            let bytes = hex::decode(&hex_key)
                .context("persisted secret encryption key is not valid hex")?;
            let mut arr = [0u8; 32];
            anyhow::ensure!(
                bytes.len() == 32,
                "persisted secret encryption key has wrong length {}",
                bytes.len()
            );
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
    Ok(LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, &key_bytes).expect("key length checked above"),
    ))
}

/// Load the SOPS age identity (env key / key file / auto-generated and
/// persisted in kv_store), identical to the SQLite adapter.
fn load_sops_identity(conn: &mut PgConnection) -> Result<AgeIdentity> {
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
                 persisted it in the database. Set GYRE_SOPS_AGE_KEY (or \
                 GYRE_SOPS_AGE_KEY_FILE) to manage the identity outside the database."
            );
            Ok(identity)
        }
    }
}

// ── Crypto helpers ───────────────────────────────────────────────────────────

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

fn encrypt_at_rest(conn: &mut PgConnection, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
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

fn decrypt_at_rest(
    conn: &mut PgConnection,
    encrypted_value: &[u8],
    nonce: &[u8],
    row_id: &str,
) -> Result<Vec<u8>> {
    if sops::looks_like_sops(encrypted_value) {
        let identity = load_sops_identity(conn)?;
        return sops::sops_decrypt(encrypted_value, &identity);
    }

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

#[async_trait]
impl SecretRepository for PgStorage {
    async fn create(&self, secret: &Secret, value: &[u8]) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let secret = secret.clone();
        let value = value.to_vec();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let (encrypted_value, nonce) = encrypt_at_rest(&mut conn, &value)?;
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
        })
        .await?
    }

    async fn get_value(&self, id: &Id, tenant_id: &str) -> Result<Option<Vec<u8>>> {
        let pool = Arc::clone(&self.pool);
        let id = id.to_string();
        let tenant_id = tenant_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Vec<u8>>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = secrets::table
                .filter(secrets::id.eq(&id))
                .filter(secrets::tenant_id.eq(&tenant_id))
                .select((secrets::encrypted_value, secrets::nonce))
                .first::<(Vec<u8>, Vec<u8>)>(&mut conn)
                .optional()?;
            match row {
                None => Ok(None),
                Some((encrypted_value, nonce)) => {
                    Ok(Some(decrypt_at_rest(&mut conn, &encrypted_value, &nonce, &id)?))
                }
            }
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id, tenant_id: &str) -> Result<Option<Secret>> {
        let pool = Arc::clone(&self.pool);
        let id = id.to_string();
        let tenant_id = tenant_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Secret>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = secrets::table
                .filter(secrets::id.eq(&id))
                .filter(secrets::tenant_id.eq(&tenant_id))
                .select(SecretRow::as_select())
                .first(&mut conn)
                .optional()?;
            Ok(row.map(SecretRow::into_secret).transpose()?)
        })
        .await?
    }

    async fn list_by_scope(
        &self,
        scope: SecretScope,
        scope_id: &str,
        tenant_id: &str,
    ) -> Result<Vec<Secret>> {
        let pool = Arc::clone(&self.pool);
        let scope = scope.as_str().to_string();
        let scope_id = scope_id.to_string();
        let tenant_id = tenant_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<Secret>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = secrets::table
                .filter(secrets::scope.eq(&scope))
                .filter(secrets::scope_id.eq(&scope_id))
                .filter(secrets::tenant_id.eq(&tenant_id))
                .order(secrets::name)
                .select(SecretRow::as_select())
                .load(&mut conn)?;
            rows.into_iter().map(SecretRow::into_secret).collect()
        })
        .await?
    }

    async fn delete(&self, id: &Id, tenant_id: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.to_string();
        let tenant_id = tenant_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(secrets::table)
                .filter(secrets::id.eq(&id))
                .filter(secrets::tenant_id.eq(&tenant_id))
                .execute(&mut conn)?;
            Ok(())
        })
        .await?
    }

    async fn rotate(&self, id: &Id, new_value: &[u8], tenant_id: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.to_string();
        let new_value = new_value.to_vec();
        let tenant_id = tenant_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let (encrypted_value, nonce) = encrypt_at_rest(&mut conn, &new_value)?;
            let rows = diesel::update(
                secrets::table
                    .filter(secrets::id.eq(&id))
                    .filter(secrets::tenant_id.eq(&tenant_id)),
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
        })
        .await?
    }

    async fn resolve_for_agent(
        &self,
        tenant_id: &str,
        workspace_id: &str,
        repo_id: &str,
        task_id: Option<&str>,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = tenant_id.to_string();
        let workspace_id = workspace_id.to_string();
        let repo_id = repo_id.to_string();
        let task_id = task_id.map(str::to_string);
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, Vec<u8>)>> {
            let mut conn = pool.get().context("get db connection")?;

            // Collect candidate rows from coarse to fine scope; finer
            // scopes overwrite same-name entries (nearest scope wins).
            let mut scope_targets: Vec<(String, String)> = vec![
                (SecretScope::Tenant.as_str().to_string(), tenant_id.clone()),
                (
                    SecretScope::Workspace.as_str().to_string(),
                    workspace_id.clone(),
                ),
                (SecretScope::Repo.as_str().to_string(), repo_id.clone()),
            ];
            if let Some(tid) = &task_id {
                scope_targets.push((SecretScope::Task.as_str().to_string(), tid.clone()));
            }

            let now = now_secs() as i64;
            let mut by_name: std::collections::HashMap<String, (String, Vec<u8>, Vec<u8>)> =
                std::collections::HashMap::new();
            for (scope, sid) in &scope_targets {
                let rows = secrets::table
                    .filter(secrets::scope.eq(scope))
                    .filter(secrets::scope_id.eq(sid))
                    .filter(secrets::tenant_id.eq(&tenant_id))
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
            resolved.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(resolved)
        })
        .await?
    }
}
