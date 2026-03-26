//! SQLite adapter for ConversationRepository (HSI §5).

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::{Id, TurnCommitLink};
use gyre_ports::ConversationRepository;
use sha2::{Digest, Sha256};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::{conversations, turn_commit_links};

/// Threshold in bytes for on-disk storage vs inline BLOB.
/// Conversations whose compressed bytes exceed this are stored on disk.
const DISK_THRESHOLD_BYTES: usize = 512 * 1024; // 512 KB compressed ≈ 1 MB uncompressed

#[derive(Queryable, Selectable)]
#[diesel(table_name = conversations)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct ConversationRow {
    #[allow(dead_code)]
    sha: String,
    #[allow(dead_code)]
    agent_id: String,
    #[allow(dead_code)]
    workspace_id: String,
    blob: Option<Vec<u8>>,
    file_path: Option<String>,
    #[allow(dead_code)]
    created_at: i64,
    #[allow(dead_code)]
    tenant_id: String,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = conversations)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct ConversationMetaRow {
    #[allow(dead_code)]
    sha: String,
    agent_id: String,
    workspace_id: String,
    #[allow(dead_code)]
    blob: Option<Vec<u8>>,
    #[allow(dead_code)]
    file_path: Option<String>,
    #[allow(dead_code)]
    created_at: i64,
    #[allow(dead_code)]
    tenant_id: String,
}

#[derive(Insertable)]
#[diesel(table_name = conversations)]
struct NewConversationRow<'a> {
    sha: &'a str,
    agent_id: &'a str,
    workspace_id: &'a str,
    blob: Option<&'a [u8]>,
    file_path: Option<&'a str>,
    created_at: i64,
    tenant_id: &'a str,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = turn_commit_links)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct TurnCommitLinkRow {
    id: String,
    agent_id: String,
    turn_number: i32,
    commit_sha: String,
    files_changed: String,
    conversation_sha: Option<String>,
    timestamp: i64,
    tenant_id: String,
}

#[derive(Insertable)]
#[diesel(table_name = turn_commit_links)]
struct NewTurnCommitLinkRow<'a> {
    id: &'a str,
    agent_id: &'a str,
    turn_number: i32,
    commit_sha: &'a str,
    files_changed: &'a str,
    conversation_sha: Option<&'a str>,
    timestamp: i64,
    tenant_id: &'a str,
}

impl TurnCommitLinkRow {
    fn into_link(self) -> Result<TurnCommitLink> {
        let files_changed: Vec<String> =
            serde_json::from_str(&self.files_changed).context("parse files_changed JSON")?;
        Ok(TurnCommitLink {
            id: Id::new(&self.id),
            agent_id: Id::new(&self.agent_id),
            turn_number: self.turn_number as u32,
            commit_sha: self.commit_sha,
            files_changed,
            conversation_sha: self.conversation_sha,
            timestamp: self.timestamp as u64,
            tenant_id: Id::new(&self.tenant_id),
        })
    }
}

// ── Encryption at rest ────────────────────────────────────────────────────────
// Blobs are optionally encrypted with AES-256-GCM.
// Set `GYRE_CONVERSATION_KEY` to a 64-hex-char (32-byte) key to enable.
// Format on disk / in DB: [1 byte marker][12-byte nonce][ciphertext+16-byte tag]
// Marker 0x00 = plaintext, 0x01 = AES-256-GCM encrypted.

const MARKER_PLAIN: u8 = 0x00;
const MARKER_AES_GCM: u8 = 0x01;
const NONCE_LEN: usize = 12;

fn read_conv_key() -> Option<Vec<u8>> {
    let hex_key = std::env::var("GYRE_CONVERSATION_KEY").ok()?;
    hex::decode(hex_key.trim()).ok().filter(|k| k.len() == 32)
}

/// Encrypt `plaintext` for storage.  Returns plaintext with marker if no key is configured.
fn encrypt_conv(plaintext: &[u8]) -> Result<Vec<u8>> {
    let Some(key_bytes) = read_conv_key() else {
        let mut out = Vec::with_capacity(1 + plaintext.len());
        out.push(MARKER_PLAIN);
        out.extend_from_slice(plaintext);
        return Ok(out);
    };
    use ring::aead::{self, LessSafeKey, Nonce, UnboundKey};
    use ring::rand::SecureRandom;
    let rng = ring::rand::SystemRandom::new();
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| anyhow!("RNG failure generating nonce"))?;
    let unbound = UnboundKey::new(&aead::AES_256_GCM, &key_bytes)
        .map_err(|_| anyhow!("invalid AES-256-GCM key"))?;
    let key = LessSafeKey::new(unbound);
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut buf = plaintext.to_vec();
    key.seal_in_place_append_tag(nonce, aead::Aad::empty(), &mut buf)
        .map_err(|_| anyhow!("AES-GCM encryption failed"))?;
    let mut out = Vec::with_capacity(1 + NONCE_LEN + buf.len());
    out.push(MARKER_AES_GCM);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&buf);
    Ok(out)
}

/// Decrypt storage bytes produced by [`encrypt_conv`].
fn decrypt_conv(data: &[u8]) -> Result<Vec<u8>> {
    if data.is_empty() {
        return Err(anyhow!("empty encrypted blob"));
    }
    match data[0] {
        MARKER_PLAIN => Ok(data[1..].to_vec()),
        MARKER_AES_GCM => {
            if data.len() < 1 + NONCE_LEN {
                return Err(anyhow!("encrypted blob too short"));
            }
            let key_bytes = read_conv_key()
                .ok_or_else(|| anyhow!("GYRE_CONVERSATION_KEY not set but blob is encrypted"))?;
            use ring::aead::{self, LessSafeKey, Nonce, UnboundKey};
            let nonce_bytes: [u8; NONCE_LEN] = data[1..1 + NONCE_LEN].try_into()?;
            let mut buf = data[1 + NONCE_LEN..].to_vec();
            let unbound = UnboundKey::new(&aead::AES_256_GCM, &key_bytes)
                .map_err(|_| anyhow!("invalid AES-256-GCM key"))?;
            let key = LessSafeKey::new(unbound);
            let nonce = Nonce::assume_unique_for_key(nonce_bytes);
            let plaintext = key
                .open_in_place(nonce, aead::Aad::empty(), &mut buf)
                .map_err(|_| anyhow!("AES-GCM decryption failed (wrong key or corrupt blob)"))?;
            Ok(plaintext.to_vec())
        }
        m => Err(anyhow!("unknown encryption marker byte: {m:#x}")),
    }
}

/// Compute SHA-256 hex digest of the given bytes.
fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Directory for on-disk conversation blobs (GYRE_CONVERSATIONS_PATH or ./conversations).
fn conversations_dir() -> String {
    std::env::var("GYRE_CONVERSATIONS_PATH").unwrap_or_else(|_| "./conversations".to_string())
}

#[async_trait]
impl ConversationRepository for SqliteStorage {
    async fn store(
        &self,
        agent_id: &Id,
        workspace_id: &Id,
        tenant_id: &Id,
        conversation: &[u8],
    ) -> Result<String> {
        let sha = sha256_hex(conversation);
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.clone();
        let workspace_id = workspace_id.clone();
        let tenant_id_str = tenant_id.as_str().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Encrypt before storing (AES-256-GCM if GYRE_CONVERSATION_KEY is set).
        let encrypted = encrypt_conv(conversation)?;

        // Decide: inline BLOB or on-disk file.
        let (blob_data, file_path_str): (Option<Vec<u8>>, Option<String>) =
            if conversation.len() > DISK_THRESHOLD_BYTES {
                // Store on disk; filename = SHA.
                let dir = conversations_dir();
                std::fs::create_dir_all(&dir)
                    .with_context(|| format!("create conversations dir: {dir}"))?;
                let path = format!("{dir}/{sha}");
                std::fs::write(&path, &encrypted)
                    .with_context(|| format!("write conversation to disk: {path}"))?;
                (None, Some(path))
            } else {
                (Some(encrypted), None)
            };

        let sha_clone = sha.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewConversationRow {
                sha: &sha_clone,
                agent_id: agent_id.as_str(),
                workspace_id: workspace_id.as_str(),
                blob: blob_data.as_deref(),
                file_path: file_path_str.as_deref(),
                created_at: now,
                tenant_id: &tenant_id_str,
            };
            diesel::insert_into(conversations::table)
                .values(&row)
                .on_conflict(conversations::sha)
                .do_nothing()
                .execute(&mut *conn)
                .context("insert conversation")?;
            Ok(())
        })
        .await??;

        Ok(sha)
    }

    async fn get(&self, conversation_sha: &str, tenant_id: &Id) -> Result<Option<Vec<u8>>> {
        let pool = Arc::clone(&self.pool);
        let sha = conversation_sha.to_string();
        let tid = tenant_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Vec<u8>>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = conversations::table
                .find(&sha)
                .filter(conversations::tenant_id.eq(&tid))
                .first::<ConversationRow>(&mut *conn)
                .optional()
                .context("query conversation")?;
            let Some(row) = row else {
                return Ok(None);
            };
            // Load (possibly encrypted) bytes — from DB blob or from disk.
            let stored = if let Some(blob) = row.blob {
                blob
            } else if let Some(fp) = row.file_path {
                std::fs::read(&fp).with_context(|| format!("read conversation from disk: {fp}"))?
            } else {
                return Err(anyhow!("conversation {sha} has neither blob nor file_path"));
            };
            // Decrypt (no-op if stored plaintext) then decompress zstd.
            let compressed = decrypt_conv(&stored).context("decrypt conversation blob")?;
            let decompressed =
                zstd::decode_all(compressed.as_slice()).context("decompress zstd conversation")?;
            Ok(Some(decompressed))
        })
        .await?
    }

    async fn record_turn_link(&self, link: &TurnCommitLink) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = link.id.as_str().to_string();
        let agent_id = link.agent_id.as_str().to_string();
        let turn_number = link.turn_number as i32;
        let commit_sha = link.commit_sha.clone();
        let files_changed =
            serde_json::to_string(&link.files_changed).context("serialize files_changed")?;
        let conversation_sha = link.conversation_sha.clone();
        let timestamp = link.timestamp as i64;
        let tenant_id = link.tenant_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewTurnCommitLinkRow {
                id: &id,
                agent_id: &agent_id,
                turn_number,
                commit_sha: &commit_sha,
                files_changed: &files_changed,
                conversation_sha: conversation_sha.as_deref(),
                timestamp,
                tenant_id: &tenant_id,
            };
            diesel::insert_into(turn_commit_links::table)
                .values(&row)
                .on_conflict(turn_commit_links::id)
                .do_nothing()
                .execute(&mut *conn)
                .context("insert turn_commit_link")?;
            Ok(())
        })
        .await?
    }

    async fn get_turn_links(
        &self,
        conversation_sha: &str,
        tenant_id: &Id,
    ) -> Result<Vec<TurnCommitLink>> {
        let pool = Arc::clone(&self.pool);
        let sha = conversation_sha.to_string();
        let tid = tenant_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<TurnCommitLink>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = turn_commit_links::table
                .filter(turn_commit_links::conversation_sha.eq(&sha))
                .filter(turn_commit_links::tenant_id.eq(&tid))
                .order(turn_commit_links::turn_number.asc())
                .load::<TurnCommitLinkRow>(&mut *conn)
                .context("load turn_commit_links")?;
            rows.into_iter().map(|r| r.into_link()).collect()
        })
        .await?
    }

    async fn get_metadata(
        &self,
        conversation_sha: &str,
        tenant_id: &Id,
    ) -> Result<Option<(Id, Id)>> {
        let pool = Arc::clone(&self.pool);
        let sha = conversation_sha.to_string();
        let tid = tenant_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<(Id, Id)>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = conversations::table
                .find(&sha)
                .filter(conversations::tenant_id.eq(&tid))
                .first::<ConversationMetaRow>(&mut *conn)
                .optional()
                .context("query conversation metadata")?;
            Ok(row.map(|r| (Id::new(&r.agent_id), Id::new(&r.workspace_id))))
        })
        .await?
    }

    async fn list_by_agent(&self, agent_id: &Id, tenant_id: &Id) -> Result<Vec<String>> {
        let pool = Arc::clone(&self.pool);
        let aid = agent_id.as_str().to_string();
        let tid = tenant_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<String>> {
            let mut conn = pool.get().context("get db connection")?;
            let shas = conversations::table
                .filter(conversations::agent_id.eq(&aid))
                .filter(conversations::tenant_id.eq(&tid))
                .order(conversations::created_at.desc())
                .select(conversations::sha)
                .load::<String>(&mut *conn)
                .context("list conversations by agent")?;
            Ok(shas)
        })
        .await?
    }

    async fn backfill_turn_links(
        &self,
        agent_id: &Id,
        conversation_sha: &str,
        tenant_id: &Id,
    ) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let aid = agent_id.as_str().to_string();
        let sha = conversation_sha.to_string();
        let tid = tenant_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let count = diesel::update(
                turn_commit_links::table
                    .filter(turn_commit_links::agent_id.eq(&aid))
                    .filter(turn_commit_links::tenant_id.eq(&tid))
                    .filter(turn_commit_links::conversation_sha.is_null()),
            )
            .set(turn_commit_links::conversation_sha.eq(&sha))
            .execute(&mut *conn)
            .context("backfill turn_commit_links")?;
            Ok(count as u64)
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    /// Create a minimal valid zstd-compressed payload.
    fn make_compressed(data: &[u8]) -> Vec<u8> {
        zstd::encode_all(data, 0).unwrap()
    }

    #[tokio::test]
    async fn store_and_get_conversation() {
        let (_tmp, s) = setup();
        let agent = Id::new("agent-1");
        let ws = Id::new("ws-1");
        let tenant = Id::new("t-1");
        let raw = b"hello conversation";
        let compressed = make_compressed(raw);

        let sha = ConversationRepository::store(&s, &agent, &ws, &tenant, &compressed)
            .await
            .unwrap();
        assert!(!sha.is_empty());

        let got = ConversationRepository::get(&s, &sha, &tenant)
            .await
            .unwrap();
        assert_eq!(got.as_deref(), Some(raw.as_slice()));
    }

    #[tokio::test]
    async fn get_metadata() {
        let (_tmp, s) = setup();
        let agent = Id::new("agent-meta");
        let ws = Id::new("ws-meta");
        let tenant = Id::new("t-meta");
        let compressed = make_compressed(b"some data");

        let sha = ConversationRepository::store(&s, &agent, &ws, &tenant, &compressed)
            .await
            .unwrap();
        let meta = ConversationRepository::get_metadata(&s, &sha, &tenant)
            .await
            .unwrap();
        assert!(meta.is_some());
        let (got_agent, got_ws) = meta.unwrap();
        assert_eq!(got_agent.as_str(), "agent-meta");
        assert_eq!(got_ws.as_str(), "ws-meta");
    }

    #[tokio::test]
    async fn wrong_tenant_returns_none() {
        let (_tmp, s) = setup();
        let agent = Id::new("agent-x");
        let ws = Id::new("ws-x");
        let tenant = Id::new("t-x");
        let other_tenant = Id::new("t-other");
        let compressed = make_compressed(b"secret");

        let sha = ConversationRepository::store(&s, &agent, &ws, &tenant, &compressed)
            .await
            .unwrap();
        let got = ConversationRepository::get(&s, &sha, &other_tenant)
            .await
            .unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn record_and_get_turn_links() {
        let (_tmp, s) = setup();
        let agent = Id::new("agent-t");
        let tenant = Id::new("t-t");
        let link = TurnCommitLink {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            agent_id: agent.clone(),
            turn_number: 3,
            commit_sha: "abc123".to_string(),
            files_changed: vec!["src/lib.rs".to_string()],
            conversation_sha: None,
            timestamp: 1000,
            tenant_id: tenant.clone(),
        };
        ConversationRepository::record_turn_link(&s, &link)
            .await
            .unwrap();

        // Back-fill with SHA.
        let n = ConversationRepository::backfill_turn_links(&s, &agent, "sha-xyz", &tenant)
            .await
            .unwrap();
        assert_eq!(n, 1);

        let links = ConversationRepository::get_turn_links(&s, "sha-xyz", &tenant)
            .await
            .unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].turn_number, 3);
        assert_eq!(links[0].conversation_sha.as_deref(), Some("sha-xyz"));
    }

    #[tokio::test]
    async fn list_by_agent() {
        let (_tmp, s) = setup();
        let agent = Id::new("agent-list");
        let ws = Id::new("ws-list");
        let tenant = Id::new("t-list");

        let c1 = make_compressed(b"conv 1");
        let c2 = make_compressed(b"conv 2");
        let sha1 = ConversationRepository::store(&s, &agent, &ws, &tenant, &c1)
            .await
            .unwrap();
        let sha2 = ConversationRepository::store(&s, &agent, &ws, &tenant, &c2)
            .await
            .unwrap();

        let shas = ConversationRepository::list_by_agent(&s, &agent, &tenant)
            .await
            .unwrap();
        assert!(shas.contains(&sha1));
        assert!(shas.contains(&sha2));
    }
}
