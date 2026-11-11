//! Integration test: the forge Ed25519 signing key persists across server
//! restarts (task-107 F12, identity-security.md §Layer 3 local mode).
//!
//! `CommitSignature` records survive restarts in the `CommitSignatureRepository`
//! (F9), so the forge key that signed them must survive too. Before the fix,
//! `build_state` regenerated the key on every boot, which made every
//! pre-restart local-mode record report `valid:false` after restart — a false
//! integrity alarm on legitimate commits, propagated as a full provenance-chain
//! failure by Phase 6.
//!
//! This test lives in its own test binary (own process) because `build_state`
//! reads `GYRE_DATABASE_URL` from the process environment; other integration
//! binaries must not see the variable.
//!
//! Against the old behavior (`AgentSigningKey::generate()` unconditionally in
//! `build_state`) the second boot has a different key and the restart
//! verification below fails with `valid:false`.

use gyre_server::{
    build_state,
    commit_signatures::{sign_commit_local, SigningAttribution},
    sigstore::verify_state_commit_signature,
};
use tempfile::TempDir;

fn attribution() -> SigningAttribution {
    SigningAttribution {
        agent_id: "agent-f12".to_string(),
        task_id: "task-f12".to_string(),
        spawned_by: "user-f12".to_string(),
    }
}

/// Two genuinely fresh `build_state` boots over the same SQLite file: sign a
/// commit locally in boot 1, drop the state, then require the record to still
/// verify in boot 2. Also asserts the active key (kid, JWKS) is identical
/// across boots.
#[tokio::test(flavor = "multi_thread")]
async fn local_commit_signatures_verify_across_restart_on_sqlite() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("gyre.db");
    let db_url = format!("sqlite://{}", db_path.display());
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &db_url) };

    let repo_id = "repo-f12";
    let commit_sha = "0123456789abcdef0123456789abcdef01234567";

    // Boot 1: sign and persist a local-mode record.
    let (kid1, jwks1) = {
        let state = build_state("f12-token", "http://127.0.0.1:1", None);
        let record = sign_commit_local(
            repo_id,
            commit_sha,
            &attribution(),
            &state.agent_signing_key,
            "http://127.0.0.1:1",
        );
        state.commit_signatures.save(&record).await.unwrap();

        // Same-boot verification must pass.
        let result = verify_state_commit_signature(&state, repo_id, commit_sha)
            .await
            .unwrap()
            .expect("record must exist");
        assert!(result.valid, "same-boot verification must pass");
        assert!(result.signature_valid);
        (
            state.agent_signing_key.kid.clone(),
            state.agent_signing_key.jwks_json.clone(),
        )
    };

    // Boot 2: a genuinely fresh process state over the same DB file. The
    // record survived (F9); the signing key must have survived with it.
    {
        let state = build_state("f12-token", "http://127.0.0.1:1", None);
        assert_eq!(
            state.agent_signing_key.kid, kid1,
            "the persisted forge key (not a fresh one) must be active after restart"
        );
        assert_eq!(state.agent_signing_key.jwks_json, jwks1);

        let result = verify_state_commit_signature(&state, repo_id, commit_sha)
            .await
            .unwrap()
            .expect("record must survive the restart");
        assert!(
            result.valid,
            "pre-restart local-mode record must still verify after restart (F12); got: {:?}",
            result.reason
        );
        assert!(result.signature_valid);
    }

    unsafe { std::env::remove_var("GYRE_DATABASE_URL") };
}
