use super::*;

#[test]
fn rfc3339_formatting() {
    assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
    // 2026-11-19T15:45:55Z
    assert_eq!(rfc3339_utc(1_795_047_955), "2026-11-19T00:25:55Z");
    // Leap-year day: 2024-02-29T12:00:00Z = 1709208000
    assert_eq!(rfc3339_utc(1_709_208_000), "2024-02-29T12:00:00Z");
    // Year boundary: 2025-01-01T00:00:00Z = 1735689600
    assert_eq!(rfc3339_utc(1_735_689_600), "2025-01-01T00:00:00Z");
}

/// Cross-implementation vector from sops v3.9.4's own test suite
/// (aes/cipher_test.go TestDecrypt): decrypting this ENC value with key
/// `b"ffffffff…"` (32 bytes) and additional data `bar:` must yield `foo`.
/// This proves the AES-256-GCM-with-32-byte-nonce implementation is
/// byte-compatible with sops's Go cipher — a plain 12-byte-nonce GCM cannot
/// pass it.
#[test]
fn sops_value_gcm_matches_sops_go_vector() {
    let key = [b'f'; 32];
    let message = "ENC[AES256_GCM,data:oYyi,iv:MyIDYbT718JRr11QtBkcj3Dwm4k1aCGZBVeZf0EyV8o=,tag:t5z2Z023Up0kxwCgw1gNxg==,type:str]";
    let parsed = parse_enc(message).unwrap();
    let plaintext = gcm_open(&key, &parsed.data, &parsed.iv, &parsed.tag, b"bar:").unwrap();
    assert_eq!(plaintext, b"foo");
}

/// The same vector must FAIL with the wrong additional data — the AAD is
/// authenticated (sops's TestDecryptInvalidAad).
#[test]
fn sops_value_gcm_rejects_wrong_aad() {
    let key = [b'f'; 32];
    let message = "ENC[AES256_GCM,data:oYyi,iv:MyIDYbT718JRr11QtBkcj3Dwm4k1aCGZBVeZf0EyV8o=,tag:t5z2Z023Up0kxwCgw1gNxg==,type:str]";
    let parsed = parse_enc(message).unwrap();
    let err = gcm_open(&key, &parsed.data, &parsed.iv, &parsed.tag, b"").unwrap_err();
    assert!(err.to_string().contains("decryption failed"));
}

#[test]
fn sops_roundtrip() {
    let identity = AgeIdentity::generate();
    let plaintext = b"postgres://localhost/gyre";
    let now = 1_795_047_955u64;
    let doc = sops_encrypt(plaintext, &identity, now).unwrap();
    assert!(looks_like_sops(&doc));

    // Structural fidelity: the document carries the sops metadata shape
    // (age key entry with recipient + armored enc, lastmodified, mac,
    // version) and the data value in ENC[...] form.
    let v: serde_json::Value = serde_json::from_slice(&doc).unwrap();
    assert!(v["data"]
        .as_str()
        .unwrap()
        .starts_with("ENC[AES256_GCM,data:"));
    let sops = &v["sops"];
    assert_eq!(sops["lastmodified"], "2026-11-19T00:25:55Z");
    assert_eq!(sops["version"], "3.9.4");
    let age_arr = sops["age"].as_array().unwrap();
    assert_eq!(age_arr.len(), 1);
    assert_eq!(age_arr[0]["recipient"], identity.recipient());
    let enc = age_arr[0]["enc"].as_str().unwrap();
    assert!(enc.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"));
    assert!(enc.contains("-----END AGE ENCRYPTED FILE-----"));
    assert!(sops["mac"].as_str().unwrap().starts_with("ENC[AES256_GCM,"));

    // Round-trip.
    let decrypted = sops_decrypt(&doc, &identity).unwrap();
    assert_eq!(decrypted, plaintext);
}

#[test]
fn sops_roundtrip_binary_and_empty_values() {
    let identity = AgeIdentity::generate();
    let now = 1_795_047_955u64;
    for plaintext in [
        b"v".to_vec(),
        "unicode: \u{1F680} and nul \0 bytes".as_bytes().to_vec(),
        Vec::new(),
        vec![0u8; 4096],
    ] {
        let doc = sops_encrypt(&plaintext, &identity, now).unwrap();
        let decrypted = sops_decrypt(&doc, &identity).unwrap();
        assert_eq!(decrypted, plaintext);
    }
}

#[test]
fn sops_tampering_fails_mac_verification() {
    let identity = AgeIdentity::generate();
    let plaintext = b"some-secret-value";
    let doc = sops_encrypt(plaintext, &identity, 1_795_047_955).unwrap();

    // Flip a bit inside the encrypted data value: decryption must fail
    // (either at the value or, if only the tag path is disturbed, at MAC
    // verification) — never return a different plaintext.
    let mut tampered = doc.clone();
    let idx = tampered
        .windows(4)
        .position(|w| w == b"data")
        .expect("data key present");
    tampered[idx + 10] ^= 0x01;
    let result = sops_decrypt(&tampered, &identity);
    assert!(result.is_err(), "tampered document must not decrypt");

    // Swap the MAC's lastmodified-derived AAD: recompute with a different
    // timestamp so the stored MAC no longer opens.
    let mut v: serde_json::Value = serde_json::from_slice(&doc).unwrap();
    v["sops"]["lastmodified"] = serde_json::json!("2000-01-01T00:00:00Z");
    let tampered = serde_json::to_vec(&v).unwrap();
    let result = sops_decrypt(&tampered, &identity);
    assert!(
        result.is_err(),
        "MAC sealed under a different lastmodified must not verify"
    );
}

#[test]
fn sops_wrong_identity_fails() {
    let encryptor = AgeIdentity::generate();
    let other = AgeIdentity::generate();
    let doc = sops_encrypt(b"secret", &encryptor, 1_795_047_955).unwrap();
    let err = sops_decrypt(&doc, &other).unwrap_err();
    assert!(
        err.to_string().contains("unwrap failed"),
        "expected age unwrap failure, got: {err:#}"
    );
}

#[test]
fn age_identity_bech32_roundtrip() {
    let identity = AgeIdentity::generate();
    let secret = identity.to_secret_string();
    assert!(secret.starts_with("AGE-SECRET-KEY-1"));
    let parsed = AgeIdentity::from_str(&secret).unwrap();
    assert_eq!(parsed.recipient(), identity.recipient());
    assert_eq!(parsed.to_secret_string(), secret);

    let recipient = identity.recipient();
    assert!(recipient.starts_with("age1"));
    assert_eq!(recipient.len(), 62);

    // Invalid inputs are rejected.
    assert!(AgeIdentity::from_str("not-a-key").is_err());
    assert!(AgeIdentity::from_str(&recipient).is_err());
}

#[test]
fn age_wrap_is_spec_conformant_age_armor() {
    // The enc field must be a well-formed armored age file that the age
    // crate (the reference implementation) can decrypt directly — this is
    // the exact artifact sops stores, so the sops CLI can unwrap it given
    // the identity.
    let identity = AgeIdentity::generate();
    let doc = sops_encrypt(b"payload", &identity, 1_795_047_955).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&doc).unwrap();
    let enc = v["sops"]["age"][0]["enc"].as_str().unwrap();

    let mut reader = age::armor::ArmoredReader::new(enc.as_bytes());
    let decryptor = age::Decryptor::new(&mut reader).unwrap();
    let mut data_key = Vec::new();
    decryptor
        .decrypt(std::iter::once(identity.inner() as &dyn age::Identity))
        .unwrap()
        .read_to_end(&mut data_key)
        .unwrap();
    assert_eq!(data_key.len(), 32, "SOPS data key must be 32 bytes");
}

#[test]
fn looks_like_sops_distinguishes_formats() {
    let identity = AgeIdentity::generate();
    let doc = sops_encrypt(b"x", &identity, 0).unwrap();
    assert!(looks_like_sops(&doc));
    assert!(!looks_like_sops(&[0u8; 48])); // legacy AES-GCM ciphertext
    assert!(!looks_like_sops(b"{}"));
    assert!(!looks_like_sops(b"not json at all"));
}

/// Cross-implementation ground truth (review round for the task-273 SOPS
/// backend): this document was produced by an INDEPENDENT implementation
/// of the sops v3.9.4 algorithm — Node's crypto for AES-256-GCM (32-byte
/// IV, verified against sops's own Go test vector) and the npm
/// `age-encryption` package for the data-key wrap — following the fetched
/// getsops/sops v3.9.4 source:
///
/// - `sops.go Tree.Encrypt`: the MAC hash writes `ToBytes(value)` per
///   non-comment leaf — KEY PATHS NEVER ENTER THE HASH (a single-value
///   binary envelope hashes its one value), formatted `%X` uppercase hex;
/// - `cmd/sops/common/common.go:121-128`: the MAC string is sealed with
///   the data key using `LastModified.Format(time.RFC3339)` as the AAD;
/// - `aes/cipher.go`: 32-byte IVs, `ENC[...]` field format.
///
/// If the Rust implementation hashed key paths into the MAC (the
/// reviewer's concern), sealed the MAC under a different AAD, or wrapped
/// the data key non-conformantly, this decryption would fail. The fixed
/// identity is the vector's private key; the plaintext is deterministic.
#[test]
fn sops_decrypt_accepts_independent_implementation_vector() {
    const DOC: &str = r#"{"data":"ENC[AES256_GCM,data:x6/ONQmhHZbiveotUqIRDmazl8GqOAQ4cAioBhqTZQ/yb6w=,iv:DxIVGBseISQnKi0wMzY5PD9CRUhLTlFUV1pdYGNmaWw=,tag:vJV2EVLE4icTA8hKSRLwQg==,type:str]","sops":{"kms":null,"gcp_kms":null,"azure_kv":null,"hc_vault":null,"age":[{"recipient":"age145tc8r9pk3akl2r3l3gugrmzpuu32flruvewvz0e0er36kptv93s8tmw8w","enc":"-----BEGIN AGE ENCRYPTED FILE-----\nYWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSBrWEU4RHRQd0UyakJUZUpN\nN2o3Y25yZ3E5TzNlR3d6R2JPWGJEWVN0cUVzCkRLdTJlZzhaRWFYWUpNd2dqaVlT\nRzZSazdqaThpR1ExdWFkeitnUWFxVDAKLS0tIGJQM3hhOCthRFNFMkJsY1FGRjcw\naERKbUlWcG9XYml0Y0VjTVBBY0RSMFEK349+lVcAho+0XNFjZD3ZozvxhcsVSnwM\nEi0kRiW5txPe0bSSsJ214oeM9Qley3Aj/HMPZS04y/WC6rE/iu+Rrw==\n-----END AGE ENCRYPTED FILE-----\n"}],"lastmodified":"2026-11-19T00:25:55Z","mac":"ENC[AES256_GCM,data:5kROQBD3I+vmmQm6QzrvdW2oyla4odjhBMXzSyqi2chl2g3jG/GHxm2JXPdjpF50EMUGQmHxGjGsDatIEGqVsm5rCP7bDKCpxicFJFXo3+xtNz+e341lNPWgORQVi3809K9R4Rwxi5cCxtMgmont46BW8lVNKttSNThbVhl0lGI=,iv:P0ZNVFtiaXB3foWMk5qhqK+2vcTL0tng5+71/AMKERg=,tag:Mu8p0uiWw7Ir9AyHS2AreA==,type:str]","pgp":null,"unencrypted_suffix":"","version":"3.9.4"}}"#;
    const IDENTITY: &str = "AGE-SECRET-KEY-1LRT6VMP96WQ9ZA466EGQ46WY5ZMQK0NTH7E9FTEXL4FFJF7K2WUSMFGVNM";
    let identity = AgeIdentity::from_str(IDENTITY).unwrap();
    let decrypted = sops_decrypt(DOC.as_bytes(), &identity).unwrap();
    assert_eq!(
        decrypted,
        b"sops-cross-check-ground-truth-value",
        "an independently generated sops v3.9.4 document must decrypt"
    );
}


