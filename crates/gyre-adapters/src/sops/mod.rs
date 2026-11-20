//! SOPS-compatible secret encryption at rest (TASK-273; admin-panel.md /
//! platform-model.md §7 Storage Backend: "SOPS as default").
//!
//! Secret values are encrypted as genuine SOPS documents in the *binary
//! envelope* JSON format used by sops v3.9.x (getsops/sops):
//!
//! ```json
//! {
//!   "data": "ENC[AES256_GCM,data:…,iv:…,tag:…,type:str]",
//!   "sops": {
//!     "kms": null, "gcp_kms": null, "azure_kv": null, "hc_vault": null,
//!     "age": [ { "recipient": "age1…", "enc": "-----BEGIN AGE ENCRYPTED FILE-----…" } ],
//!     "lastmodified": "2026-11-19T12:00:00Z",
//!     "mac": "ENC[AES256_GCM,…,type:str]",
//!     "pgp": null, "unencrypted_suffix": "", "version": "3.9.4"
//!   }
//! }
//! ```
//!
//! Format fidelity (verified against the fetched getsops/sops v3.9.4
//! source, and by cross-implementation tests — see
//! `sops_decrypt_accepts_independent_implementation_vector`):
//!
//! - **Data key**: 32 random bytes. Wrapped with age (age-encryption.org/v1,
//!   X25519 recipient stanza) and stored ASCII-armored in
//!   `sops.age[0].enc` — exactly what sops's `age` key source stores — so
//!   the document is decryptable with the sops CLI given the identity
//!   (`sops -d`, or the `age` CLI on the unwrapped stanza).
//! - **Value**: AES-256-GCM under the data key with a random 32-byte IV
//!   (sops uses GCM with `nonceSize = 32`, not the GCM default 12),
//!   additional data `"data:"` (the tree path of the single `data` key,
//!   colon-joined with a trailing colon), ciphertext and 16-byte tag
//!   base64-encoded separately in the `ENC[...]` string, `type:str`
//!   (sops's binary envelope encrypts raw bytes as a string value).
//! - **MAC**: SHA-512 over the decrypted VALUE bytes — sops v3.9.4's
//!   `Tree.Encrypt`/`Tree.Decrypt` (sops.go) does
//!   `hash.Write(ToBytes(value))` per non-comment leaf and key paths
//!   NEVER enter the hash, so a single-value binary envelope hashes its
//!   one value — formatted as uppercase hex (`%X`), then itself encrypted
//!   as an `ENC[...]` string with the data key and the RFC 3339
//!   `lastmodified` string as additional data (cmd/sops/common/common.go
//!   `Encrypter`: `cipher.Encrypt(mac, dataKey,
//!   LastModified.Format(time.RFC3339))`). Decryption verifies the MAC
//!   (fail closed), exactly as `sops -d` does (Decrypter:
//!   `cipher.Decrypt(metadata.mac, dataKey, lastmodified)` compared
//!   against the recomputed tree hash).
//!
//! The legacy pre-SOPS storage format (ring AES-256-GCM with a 12-byte
//! nonce in the `nonce` column) is transparently decryptable and
//! re-encrypted on first access — see the secret adapters.

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use sha2::digest::Digest;
use sha2::Sha512;
use std::io::{Read, Write};

/// AES-256-GCM with the 32-byte nonce size sops uses for values.
type Aes256Gcm32 = aes_gcm::AesGcm<aes_gcm::aes::Aes256, typenum::U32>;
type Nonce32 = aes_gcm::aead::generic_array::GenericArray<u8, typenum::U32>;

/// Additional data for the single `data` value of the binary envelope
/// (sops: `strings.Join(path, ":") + ":"` with path `["data"]`).
const VALUE_AAD: &[u8] = b"data:";

/// `version` sops records for documents it emits.
const SOPS_VERSION: &str = "3.9.4";

// ── age identities �──────────────────────────────────────────────────────────

/// An age X25519 identity (`AGE-SECRET-KEY-1…`) with its public recipient
/// (`age1…`).
#[derive(Clone)]
pub struct AgeIdentity {
    inner: age::x25519::Identity,
}

impl AgeIdentity {
    /// Generate a fresh identity (CSPRNG).
    pub fn generate() -> Self {
        Self {
            inner: age::x25519::Identity::generate(),
        }
    }

    /// Parse an `AGE-SECRET-KEY-1…` bech32 string.
    pub fn parse(s: &str) -> Result<Self> {
        let inner = s
            .trim()
            .parse::<age::x25519::Identity>()
            .map_err(|e| anyhow::anyhow!("invalid age identity: {e}"))?;
        Ok(Self { inner })
    }

    /// Serialize as the `AGE-SECRET-KEY-1…` bech32 string.
    pub fn to_secret_string(&self) -> String {
        use age::secrecy::ExposeSecret;
        self.inner.to_string().expose_secret().to_string()
    }

    /// The public recipient key (`age1…`).
    pub fn recipient(&self) -> String {
        self.inner.to_public().to_string()
    }

    /// The wrapped age identity (for the `age` crate's decrypt path).
    pub(crate) fn inner(&self) -> &age::x25519::Identity {
        &self.inner
    }
}

// ── age data-key wrap ─────────────────────────────────────────────────────────

/// Wrap `data_key` for `recipient` as an ASCII-armored age file — the exact
/// artifact sops stores in `sops.age[].enc`.
fn age_wrap(data_key: &[u8; 32], recipient: &age::x25519::Recipient) -> Result<String> {
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(recipient as &dyn age::Recipient))
            .map_err(|e| anyhow::anyhow!("age encryptor construction failed: {e}"))?;

    let mut armored = Vec::new();
    {
        let mut armor =
            age::armor::ArmoredWriter::wrap_output(&mut armored, age::armor::Format::AsciiArmor)
                .map_err(|e| anyhow::anyhow!("age armor writer failed: {e}"))?;
        let mut writer = encryptor
            .wrap_output(&mut armor)
            .map_err(|e| anyhow::anyhow!("age wrap failed: {e}"))?;
        writer
            .write_all(data_key)
            .map_err(|e| anyhow::anyhow!("writing age payload failed: {e}"))?;
        writer
            .finish()
            .map_err(|e| anyhow::anyhow!("finishing age payload failed: {e}"))?;
        armor
            .finish()
            .map_err(|e| anyhow::anyhow!("finishing age armor failed: {e}"))?;
    }
    String::from_utf8(armored).context("age armor is not UTF-8 (impossible by construction)")
}

/// Unwrap an armored age file (from `sops.age[].enc`) with `identity`,
/// returning the 32-byte data key.
fn age_unwrap(armored: &str, identity: &age::x25519::Identity) -> Result<[u8; 32]> {
    let mut reader = age::armor::ArmoredReader::new(armored.as_bytes());
    let decryptor = age::Decryptor::new(&mut reader)
        .map_err(|e| anyhow::anyhow!("stored SOPS data key is not a valid age file: {e}"))?;
    let mut decrypted = decryptor
        .decrypt(std::iter::once(identity as &dyn age::Identity))
        .map_err(|e| anyhow::anyhow!("age data-key unwrap failed (wrong identity?): {e}"))?;
    let mut key = Vec::with_capacity(32);
    decrypted
        .read_to_end(&mut key)
        .map_err(|e| anyhow::anyhow!("reading age data key failed: {e}"))?;
    let key: [u8; 32] = key
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("SOPS data key has wrong length {}", key.len()))?;
    Ok(key)
}

// ── RFC 3339 timestamp (UTC, second precision) ────────────────────────────────

/// Format a Unix-epoch-seconds timestamp as RFC 3339 UTC
/// (`2026-11-19T12:34:56Z`) — the format sops uses for `lastmodified` and
/// as the MAC's additional data.
pub fn rfc3339_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant's `civil_from_days`: days since the Unix epoch →
/// (year, month, day) in the proleptic Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Canonicalize a stored `lastmodified` value to second-precision RFC
/// 3339 — the exact additional data sops seals the MAC under
/// (`LastModified.Format(time.RFC3339)` in cmd/sops/common/common.go).
/// Go's JSON marshal of `time.Time` emits RFC 3339 with (up to)
/// nanosecond precision, so a document produced by the sops CLI carries
/// fractional digits in the STORED field while its MAC AAD is the
/// truncated second-precision string: canonicalizing before MAC AAD use
/// makes CLI-generated documents verify (review task-273 — using the raw
/// stored string as AAD would fail MAC verification on every
/// CLI-encrypted document). Fractional digits are truncated; the zone
/// (`Z` or `±HH:MM`) is preserved verbatim (Go formats in the time's own
/// location); anything malformed fails closed.
fn canonical_lastmodified(raw: &str) -> Result<String> {
    let t = raw
        .find('T')
        .context("lastmodified is not RFC 3339 (no date/time separator)")?;
    let (date, rest) = raw.split_at(t + 1);
    anyhow::ensure!(
        date.len() == 11 && date.ends_with('T'),
        "lastmodified date is not YYYY-MM-DD: {raw:?}"
    );
    // The time part runs to the first fraction ('.') or zone marker.
    let time_end = rest.find(['.', 'Z', '+', '-']).unwrap_or(rest.len());
    let (time, tail) = rest.split_at(time_end);
    anyhow::ensure!(
        time.len() == 8 && time.as_bytes()[2] == b':' && time.as_bytes()[5] == b':',
        "lastmodified time is not HH:MM:SS: {raw:?}"
    );
    let zone = match tail.strip_prefix('.') {
        // Fractional digits: skip to the zone marker that follows.
        Some(frac) => {
            let z = frac
                .find(['Z', '+', '-'])
                .context("lastmodified has fractional digits but no zone")?;
            &frac[z..]
        }
        None => tail,
    };
    anyhow::ensure!(
        zone == "Z"
            || (zone.len() == 6
                && (zone.starts_with('+') || zone.starts_with('-'))
                && zone.as_bytes()[3] == b':'),
        "lastmodified zone is malformed: {raw:?}"
    );
    Ok(format!("{date}{time}{zone}"))
}

// ── ENC[AES256_GCM,...] values ────────────────────────────────────────────────

/// Serialize one `ENC[AES256_GCM,data:…,iv:…,tag:…,type:str]` string from
/// the seal output (ciphertext-without-tag, 32-byte IV, tag).
fn enc_string(data: &[u8], iv: &[u8; 32], tag: &[u8]) -> String {
    format!(
        "ENC[AES256_GCM,data:{},iv:{},tag:{},type:str]",
        B64.encode(data),
        B64.encode(iv),
        B64.encode(tag),
    )
}

/// Parsed `ENC[...]` value.
struct EncValue {
    data: Vec<u8>,
    iv: Vec<u8>,
    tag: Vec<u8>,
}

/// Parse an `ENC[AES256_GCM,...]` string. Mirrors sops's regex
/// `^ENC\[AES256_GCM,data:(.+),iv:(.+),tag:(.+),type:(.+)\]`.
fn parse_enc(value: &str) -> Result<EncValue> {
    let inner = value
        .strip_prefix("ENC[AES256_GCM,")
        .and_then(|v| v.strip_suffix(']'))
        .context("value does not match sops' data format")?;
    let mut parts = inner.splitn(4, ',');
    let data = parts.next().context("missing data")?;
    let iv = parts.next().context("missing iv")?;
    let tag = parts.next().context("missing tag")?;
    let typ = parts.next().context("missing type")?;
    let data = data.strip_prefix("data:").context("missing data prefix")?;
    let iv = iv.strip_prefix("iv:").context("missing iv prefix")?;
    let tag = tag.strip_prefix("tag:").context("missing tag prefix")?;
    let typ = typ.strip_prefix("type:").context("missing type prefix")?;
    anyhow::ensure!(typ == "str", "unsupported sops datatype {typ:?}");
    let bad = |what: &str| anyhow::anyhow!("base64-decoding {what} failed");
    Ok(EncValue {
        data: B64.decode(data).with_context(|| bad("data"))?,
        iv: B64.decode(iv).with_context(|| bad("iv"))?,
        tag: B64.decode(tag).with_context(|| bad("tag"))?,
    })
}

/// AES-256-GCM seal under a 32-byte IV (sops nonce size), returning
/// (ciphertext-without-tag, iv, tag).
fn gcm_seal(
    data_key: &[u8; 32],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<(Vec<u8>, [u8; 32], Vec<u8>)> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    let cipher = Aes256Gcm32::new_from_slice(data_key)
        .map_err(|_| anyhow::anyhow!("invalid data key length"))?;
    let mut iv = [0u8; 32];
    fill_random(&mut iv)?;
    // The `aes-gcm` crate appends the tag to the ciphertext; sops splits
    // them at the 16-byte block size, so re-split here.
    let sealed = cipher
        .encrypt(
            Nonce32::from_slice(&iv),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("AES-256-GCM encryption failed"))?;
    let (data, tag) = sealed.split_at(sealed.len() - 16);
    Ok((data.to_vec(), iv, tag.to_vec()))
}

/// AES-256-GCM open under a 32-byte IV.
fn gcm_open(
    data_key: &[u8; 32],
    data: &[u8],
    iv: &[u8],
    tag: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    anyhow::ensure!(
        iv.len() == 32,
        "sops value IV has wrong length {} (expected 32)",
        iv.len()
    );
    let cipher = Aes256Gcm32::new_from_slice(data_key)
        .map_err(|_| anyhow::anyhow!("invalid data key length"))?;
    let mut iv_arr = [0u8; 32];
    iv_arr.copy_from_slice(iv);
    let mut sealed = data.to_vec();
    sealed.extend_from_slice(tag);
    cipher
        .decrypt(Nonce32::from_slice(&iv_arr), Payload { msg: &sealed, aad })
        .map_err(|_| {
            anyhow::anyhow!("AES-256-GCM decryption failed (wrong key or tampered ciphertext)")
        })
}

/// Fill `buf` from the system CSPRNG.
fn fill_random(buf: &mut [u8]) -> Result<()> {
    use ring::rand::SecureRandom;
    ring::rand::SystemRandom::new()
        .fill(buf)
        .map_err(|_| anyhow::anyhow!("system RNG failure"))
}

// ── SOPS document encryption/decryption ──────────────────────────────────────

/// Serialize the plaintext as a sops v3.9.x binary-envelope JSON document
/// (see the module docs for the exact format). The data key is freshly
/// generated and age-wrapped for `recipient`; the value is AES-256-GCM
/// sealed under it with the `"data:"` additional data; the MAC is the
/// SHA-512 of the value bytes (uppercase hex), itself sealed with the
/// `lastmodified` timestamp as additional data.
pub fn sops_encrypt(plaintext: &[u8], recipient: &AgeIdentity, now_secs: u64) -> Result<Vec<u8>> {
    // 1. Data key + age wrap.
    let mut data_key = [0u8; 32];
    fill_random(&mut data_key)?;
    let recipient_key: age::x25519::Recipient = recipient
        .recipient()
        .parse()
        .map_err(|e| anyhow::anyhow!("generated recipient is invalid (impossible): {e}"))?;
    let enc_key = age_wrap(&data_key, &recipient_key)?;

    // 2. Value.
    let (value_data, value_iv, value_tag) = gcm_seal(&data_key, plaintext, VALUE_AAD)?;
    let value_enc = enc_string(&value_data, &value_iv, &value_tag);

    // 3. MAC: SHA-512 over the value bytes, uppercase hex (sops `%X`),
    //    sealed with the CANONICAL second-precision lastmodified as
    //    additional data (sops: `LastModified.Format(time.RFC3339)`).
    let lastmodified = rfc3339_utc(now_secs);
    let mac_aad = canonical_lastmodified(&lastmodified)?;
    let mac_hex = format!("{:X}", Sha512::digest(plaintext));
    let (mac_data, mac_iv, mac_tag) = gcm_seal(&data_key, mac_hex.as_bytes(), mac_aad.as_bytes())?;
    let mac_enc = enc_string(&mac_data, &mac_iv, &mac_tag);

    let document = serde_json::json!({
        "data": value_enc,
        "sops": {
            "kms": null,
            "gcp_kms": null,
            "azure_kv": null,
            "hc_vault": null,
            "age": [
                {
                    "recipient": recipient.recipient(),
                    "enc": enc_key,
                }
            ],
            "lastmodified": lastmodified,
            "mac": mac_enc,
            "pgp": null,
            "unencrypted_suffix": "",
            "version": SOPS_VERSION,
        },
    });
    serde_json::to_vec(&document).context("serializing SOPS document")
}

/// Heuristic: do these stored bytes look like a SOPS JSON document (as
/// opposed to the legacy raw-AES-GCM ciphertext)? Used by the secret
/// adapters to pick the decrypt path and re-encrypt legacy rows.
pub fn looks_like_sops(stored: &[u8]) -> bool {
    // Cheap structural check before JSON parsing: a SOPS document is a
    // JSON object whose top-level "sops" key holds the metadata.
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(stored) else {
        return false;
    };
    v.get("sops").is_some() && v.get("data").is_some()
}

/// Decrypt a SOPS binary-envelope JSON document with `identity`, verifying
/// the MAC (fail closed). Returns the original plaintext bytes.
pub fn sops_decrypt(document: &[u8], identity: &AgeIdentity) -> Result<Vec<u8>> {
    let v: serde_json::Value =
        serde_json::from_slice(document).context("stored SOPS document is not valid JSON")?;
    let sops = v
        .get("sops")
        .ok_or_else(|| anyhow::anyhow!("stored SOPS document has no sops metadata"))?;
    let data_enc = v
        .get("data")
        .and_then(|d| d.as_str())
        .ok_or_else(|| anyhow::anyhow!("stored SOPS document has no data value"))?;
    let lastmodified = sops
        .get("lastmodified")
        .and_then(|m| m.as_str())
        .ok_or_else(|| anyhow::anyhow!("stored SOPS document has no lastmodified"))?;
    let mac_enc = sops
        .get("mac")
        .and_then(|m| m.as_str())
        .ok_or_else(|| anyhow::anyhow!("stored SOPS document has no mac"))?
        .to_string();
    let age_keys = sops
        .get("age")
        .and_then(|a| a.as_array())
        .ok_or_else(|| anyhow::anyhow!("stored SOPS document has no age keys"))?;
    anyhow::ensure!(
        !age_keys.is_empty(),
        "stored SOPS document has no age key entries"
    );
    let mut last_err: Option<anyhow::Error> = None;
    let mut data_key: Option<[u8; 32]> = None;
    for key in age_keys {
        let Some(enc) = key.get("enc").and_then(|e| e.as_str()) else {
            continue;
        };
        match age_unwrap(enc, identity.inner()) {
            Ok(k) => {
                data_key = Some(k);
                break;
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
    }
    let data_key = data_key
        .ok_or_else(|| last_err.unwrap_or_else(|| anyhow::anyhow!("no age enc entries")))?;

    // Value.
    let value = parse_enc(data_enc)?;
    let plaintext = gcm_open(&data_key, &value.data, &value.iv, &value.tag, VALUE_AAD)?;

    // MAC verification (fail closed): recompute SHA-512 over the decrypted
    // value bytes and compare with the stored (decrypted) MAC. The MAC's
    // additional data is the CANONICAL second-precision lastmodified —
    // the sops CLI marshals the stored field with nanosecond precision
    // while sealing the MAC under `Format(time.RFC3339)`, so the raw
    // stored string must be truncated before use (review task-273).
    let mac_aad = canonical_lastmodified(lastmodified)?;
    let mac = parse_enc(&mac_enc)?;
    let stored_mac = gcm_open(&data_key, &mac.data, &mac.iv, &mac.tag, mac_aad.as_bytes())?;
    let computed_mac = format!("{:X}", Sha512::digest(&plaintext));
    let stored_mac_str =
        String::from_utf8(stored_mac).context("stored SOPS MAC is not UTF-8 (corrupt document)")?;
    anyhow::ensure!(
        constant_time_eq(computed_mac.as_bytes(), stored_mac_str.as_bytes()),
        "SOPS MAC mismatch: stored document has been tampered with"
    );

    Ok(plaintext)
}

/// Constant-time byte-slice comparison.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests;
