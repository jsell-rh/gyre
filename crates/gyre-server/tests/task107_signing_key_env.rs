//! Integration test: `GYRE_AGENT_SIGNING_KEY` provisioning and rotation
//! semantics for the forge Ed25519 signing key (task-107 F12).
//!
//! The env var provisions the active forge key explicitly (operator-owned
//! rotation). A record signed by a previous key must be reported honestly as
//! unverifiable-by-rotation (kid mismatch in the verification reason), not as
//! a tamper failure; a record signed by the active key must verify; invalid
//! provisioned material must abort startup rather than silently fall back to
//! a different key.
//!
//! Own test binary (own process): `build_state` reads `GYRE_DATABASE_URL` and
//! `GYRE_AGENT_SIGNING_KEY` from the process environment.

use gyre_server::{
    build_state,
    commit_signatures::{sign_commit_local, SigningAttribution},
    sigstore::verify_state_commit_signature,
    AGENT_SIGNING_KEY_KV_KEY, AGENT_SIGNING_KEY_KV_NAMESPACE,
};
use tempfile::TempDir;

fn attribution() -> SigningAttribution {
    SigningAttribution {
        agent_id: "agent-f12".to_string(),
        task_id: "task-f12".to_string(),
        spawned_by: "user-f12".to_string(),
    }
}

/// Boot a fresh state over a fresh SQLite DB and return the temp dir, the
/// DB URL, the active kid, and the base64 PKCS#8 of the key that boot
/// generated and persisted (read back through the public KV port — the
/// same encoding `GYRE_AGENT_SIGNING_KEY` accepts).
async fn generate_key_material() -> (TempDir, String, String, String) {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("gyre.db");
    let db_url = format!("sqlite://{}", db_path.display());
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &db_url) };
    let state = build_state("f12-token", "http://127.0.0.1:1", None);
    let kid = state.agent_signing_key.kid.clone();
    let encoded = state
        .kv_store
        .kv_get(AGENT_SIGNING_KEY_KV_NAMESPACE, AGENT_SIGNING_KEY_KV_KEY)
        .await
        .unwrap()
        .expect("fresh DB-backed boot must persist its generated key");
    (dir, db_url, kid, encoded)
}

/// A PKCS#8 **v1** Ed25519 document — the encoding every common operator
/// tool emits (`openssl genpkey -algorithm ed25519 -outform DER | base64`,
/// Node `privateKey.export({type:'pkcs8'})`, Python `cryptography`). v1 has
/// no embedded public key, so ring's strict `Ed25519KeyPair::from_pkcs8`
/// rejects it with `VersionNotSupported`; the production loader must accept
/// it (ring's documented entry point for v1 derives the public key from the
/// private seed). Generated with the exact command documented for
/// `GYRE_AGENT_SIGNING_KEY` in docs/server-config.md.
const OPENSSL_V1_PKCS8_B64: &str =
    "MC4CAQAwBQYDK2VwBCIEIL184aaMioYbIJKWxfXjdTEHvvwzB976iDkNhbySu6x8";
/// kid = SHA-256(public key)[..16] for the key above, precomputed from its
/// derived public key so the test is independent of `derive_kid` itself.
const OPENSSL_V1_KEY_KID: &str = "b618c0c4f5a201452c2b8a6e8be184b8";

#[tokio::test(flavor = "multi_thread")]
async fn env_provisioned_key_wins_and_rotation_is_reported_as_rotation() {
    // Leg 0 — openssl-generated v1 PKCS#8 must provision the active forge
    // key (the encoding the documented generation command produces) and
    // sign records that verify. Runs first and clears the env var so the
    // rotation legs below start from a clean environment.
    {
        let dir = TempDir::new().unwrap();
        let db_url = format!("sqlite://{}", dir.path().join("gyre.db").display());
        unsafe { std::env::set_var("GYRE_DATABASE_URL", &db_url) };
        unsafe { std::env::set_var("GYRE_AGENT_SIGNING_KEY", OPENSSL_V1_PKCS8_B64) };

        let state = build_state("f12-token", "http://127.0.0.1:1", None);
        assert_eq!(
            state.agent_signing_key.kid, OPENSSL_V1_KEY_KID,
            "v1 PKCS#8 (openssl-generated) must provision the active forge key \
             with the kid derived from its public key"
        );
        // The env-provisioned key is never written to the DB.
        let persisted = state
            .kv_store
            .kv_get(AGENT_SIGNING_KEY_KV_NAMESPACE, AGENT_SIGNING_KEY_KV_KEY)
            .await
            .unwrap();
        assert_eq!(
            persisted, None,
            "operator-provisioned key must not be persisted"
        );

        // The provisioned key actually signs, and its records verify.
        let repo_id = "repo-f12-v1";
        let commit_sha = "0123456789abcdef0123456789abcdef01234567";
        let record = sign_commit_local(
            repo_id,
            commit_sha,
            &attribution(),
            &state.agent_signing_key,
            "http://127.0.0.1:1",
        );
        state.commit_signatures.save(&record).await.unwrap();
        let result = verify_state_commit_signature(&state, repo_id, commit_sha)
            .await
            .unwrap()
            .expect("record must exist");
        assert!(
            result.valid,
            "v1-provisioned key must sign verifiable records"
        );

        unsafe { std::env::remove_var("GYRE_AGENT_SIGNING_KEY") };
    }

    // Key A: the deployment's original forge key (DB 1).
    let (_dir_a, db_a, kid_a, key_a) = generate_key_material().await;
    // Key B: material generated against an unrelated DB — a distinct key.
    let (_dir_b, _db_b, kid_b, key_b) = generate_key_material().await;
    assert_ne!(kid_a, kid_b, "the two boots must hold distinct keys");

    // Point back at DB 1, which holds key A, and sign a record with it.
    let repo_id = "repo-f12-rot";
    let commit_sha = "fedcba9876543210fedcba9876543210fedcba98";
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &db_a) };
    {
        let state = build_state("f12-token", "http://127.0.0.1:1", None);
        assert_eq!(state.agent_signing_key.kid, kid_a);
        let record = sign_commit_local(
            repo_id,
            commit_sha,
            &attribution(),
            &state.agent_signing_key,
            "http://127.0.0.1:1",
        );
        state.commit_signatures.save(&record).await.unwrap();
    }

    // The env var is set to the SAME material as key A (an operator
    // re-provisioning the existing key): the active key is unchanged.
    unsafe { std::env::set_var("GYRE_AGENT_SIGNING_KEY", &key_a) };
    {
        let state = build_state("f12-token", "http://127.0.0.1:1", None);
        assert_eq!(
            state.agent_signing_key.kid, kid_a,
            "re-provisioning the same material must keep the same active key"
        );
    }

    // Operator rotates to key B: the active key changes, and the record
    // signed by key A is reported as unverifiable-by-rotation, not tampered.
    unsafe { std::env::set_var("GYRE_AGENT_SIGNING_KEY", &key_b) };
    {
        let state = build_state("f12-token", "http://127.0.0.1:1", None);
        assert_eq!(
            state.agent_signing_key.kid, kid_b,
            "GYRE_AGENT_SIGNING_KEY must provision the active forge key"
        );

        let result = verify_state_commit_signature(&state, repo_id, commit_sha)
            .await
            .unwrap()
            .expect("record must survive");
        assert!(!result.valid);
        let reason = result.reason.as_deref().unwrap_or("");
        assert!(
            reason.contains("key rotated"),
            "rotated-key record must name the rotation, got: {reason}"
        );
        assert!(
            reason.contains(&kid_a),
            "reason must name the record's sign-time kid {kid_a}, got: {reason}"
        );

        // A record signed by the NEW active key verifies normally.
        let new_sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let new_record = sign_commit_local(
            repo_id,
            new_sha,
            &attribution(),
            &state.agent_signing_key,
            "http://127.0.0.1:1",
        );
        state.commit_signatures.save(&new_record).await.unwrap();
        let ok = verify_state_commit_signature(&state, repo_id, new_sha)
            .await
            .unwrap()
            .expect("new record must exist");
        assert!(ok.valid, "record signed by the active key must verify");
    }

    // Invalid provisioned material is a hard startup error: silently falling
    // back to a different key would break every signature the operator
    // expects to verify.
    unsafe { std::env::set_var("GYRE_AGENT_SIGNING_KEY", "!!!not-base64///") };
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let panic_result = std::panic::catch_unwind(|| {
        build_state("f12-token", "http://127.0.0.1:1", None);
    });
    std::panic::set_hook(prev_hook);
    assert!(
        panic_result.is_err(),
        "invalid GYRE_AGENT_SIGNING_KEY must abort startup, not fall back"
    );

    unsafe { std::env::remove_var("GYRE_AGENT_SIGNING_KEY") };
    unsafe { std::env::remove_var("GYRE_DATABASE_URL") };
}
