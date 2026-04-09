//! Merge attestation bundles (G5).
//!
//! After every successful merge the merge processor assembles a `MergeAttestation`
//! record containing the MR ID, merge commit SHA, gate results, spec approval
//! status, and author identity.  The record is canonicalised to JSON, signed with
//! the server's Ed25519 key, wrapped in an `AttestationBundle`, stored in the
//! in-memory `attestation_store`, and attached to the merge commit as a git note
//! under `refs/notes/attestations`.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use gyre_common::Attestation;

pub use gyre_domain::{AttestationBundle, AttestationGateResult, MergeAttestation};

// ── Signing ──────────────────────────────────────────────────────────────────

/// Sign `attestation` with `signing_key` and return an `AttestationBundle`.
///
/// The canonical form is deterministic JSON (struct field order as declared).
/// The signature covers the UTF-8 bytes of that JSON.
///
/// **Note:** `AttestationBundle` is deprecated (Phase 4). This function
/// continues to produce bundles for backward compatibility during the
/// dual-write period. New code should use the chain attestation API.
pub fn sign_attestation(
    attestation: MergeAttestation,
    signing_key: &crate::auth::AgentSigningKey,
) -> AttestationBundle {
    let canonical =
        serde_json::to_string(&attestation).expect("MergeAttestation serialisation must not fail");
    let raw_sig = signing_key.sign_bytes(canonical.as_bytes());
    let signature = BASE64.encode(&raw_sig);
    let signing_key_id = signing_key.kid.clone();
    AttestationBundle {
        attestation,
        signature,
        signing_key_id,
        deprecation_notice: Some(
            "This format is deprecated. Use GET /api/v1/repos/{id}/attestations/{commit_sha}/verification for chain attestation.".to_string()
        ),
    }
}

// ── Chain attestation git notes (§5.3, location 2) ─────────────────────────

/// The git notes ref used for chain attestations — same as the legacy
/// `AttestationBundle` ref per spec §5.3: "same as the existing merge
/// attestation bundle." The chain attestation overwrites the legacy note
/// on the same ref (using `-f`).
pub const CHAIN_ATTESTATION_NOTES_REF: &str = "refs/notes/attestations";

/// Attach the full attestation chain as a git note under `refs/notes/attestations`.
///
/// The chain (root to leaf) is serialized as a JSON array and written as a note
/// on the specified commit. Uses `-f` to allow overwriting (e.g., when a gate
/// result appends to the leaf attestation and re-saves). The full chain is stored
/// so that an offline verifier can walk and verify the complete authorization
/// chain from the note alone (§5.3 clone-time portability).
///
/// This is a non-blocking fire-and-forget operation — git note failures are
/// logged but do not prevent the attestation from being persisted in the database.
pub async fn attach_chain_attestation_note(
    repo_path: &str,
    commit_sha: &str,
    chain: &[Attestation],
) {
    let note_json = match serde_json::to_string(chain) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!(
                commit_sha = %commit_sha,
                error = %e,
                "failed to serialize chain attestation for git note"
            );
            return;
        }
    };

    let repo_path = repo_path.to_string();
    let commit_sha = commit_sha.to_string();
    tokio::spawn(async move {
        let out = tokio::process::Command::new("git")
            .args([
                "-C",
                &repo_path,
                "notes",
                &format!("--ref={CHAIN_ATTESTATION_NOTES_REF}"),
                "add",
                "-f",
                "-m",
                &note_json,
                &commit_sha,
            ])
            .output()
            .await;
        match out {
            Ok(o) if o.status.success() => {
                tracing::info!(
                    sha = %commit_sha,
                    "chain attestation note attached"
                );
            }
            Ok(o) => {
                tracing::warn!(
                    sha = %commit_sha,
                    stderr = %String::from_utf8_lossy(&o.stderr),
                    "chain attestation git note failed — attestation stored in database only"
                );
            }
            Err(e) => {
                tracing::warn!(
                    sha = %commit_sha,
                    error = %e,
                    "git not found — chain attestation stored in database only"
                );
            }
        }
    });
}

/// Read the full attestation chain from git notes under `refs/notes/attestations`.
///
/// Returns `None` if no note exists for the given commit, the note is not valid
/// JSON, or git is not available. The chain is stored as a JSON array (root to
/// leaf) for clone-time portability (§5.3).
pub async fn read_chain_attestation_note(
    repo_path: &str,
    commit_sha: &str,
) -> Option<Vec<Attestation>> {
    let repo_path = repo_path.to_string();
    let commit_sha = commit_sha.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let output = std::process::Command::new("git")
            .args([
                "-C",
                &repo_path,
                "notes",
                &format!("--ref={CHAIN_ATTESTATION_NOTES_REF}"),
                "show",
                &commit_sha,
            ])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let note_text = String::from_utf8_lossy(&o.stdout);
                match serde_json::from_str::<Vec<Attestation>>(note_text.trim()) {
                    Ok(chain) => Some(chain),
                    Err(e) => {
                        tracing::warn!(
                            commit_sha = %commit_sha,
                            error = %e,
                            "chain attestation note exists but failed to parse"
                        );
                        None
                    }
                }
            }
            _ => None,
        }
    })
    .await;
    result.unwrap_or(None)
}

/// Write the full attestation chain as a git note if the attestation has a
/// non-empty `commit_sha`. Called after persisting the attestation to the
/// database.
///
/// Loads the full chain from the database via `load_chain` before writing so
/// that the git note contains the complete authorization chain for offline
/// verification (§5.3 clone-time portability). Resolves the repo path from
/// `state.repos`. If the repo lookup fails or commit_sha is empty, the
/// operation is a no-op (attestation is still in the database).
pub async fn write_chain_note_if_committed(state: &crate::AppState, attestation: &Attestation) {
    let commit_sha = &attestation.output.commit_sha;
    if commit_sha.is_empty() {
        return;
    }
    let repo_id = &attestation.metadata.repo_id;
    if repo_id.is_empty() {
        return;
    }

    // Load the full chain (root to leaf) from the database.
    let chain = match state.chain_attestations.load_chain(&attestation.id).await {
        Ok(c) if !c.is_empty() => c,
        Ok(_) => {
            // Fallback: if load_chain returns empty (e.g., root node with no
            // parent), write just this attestation as a single-element chain.
            vec![attestation.clone()]
        }
        Err(e) => {
            tracing::warn!(
                attestation_id = %attestation.id,
                error = %e,
                "failed to load chain — writing single attestation as git note"
            );
            vec![attestation.clone()]
        }
    };

    match state.repos.find_by_id(&gyre_common::Id::new(repo_id)).await {
        Ok(Some(repo)) => {
            attach_chain_attestation_note(&repo.path, commit_sha, &chain).await;
        }
        Ok(None) => {
            tracing::debug!(
                repo_id = %repo_id,
                "repo not found — skipping chain attestation git note"
            );
        }
        Err(e) => {
            tracing::warn!(
                repo_id = %repo_id,
                error = %e,
                "failed to look up repo for chain attestation git note"
            );
        }
    }
}

/// Attach the full attestation chain as a git note (synchronous inner helper).
///
/// Unlike `attach_chain_attestation_note`, this blocks until the git command
/// completes and returns a result. Used internally for testing.
#[cfg(test)]
async fn attach_chain_attestation_note_sync(
    repo_path: &str,
    commit_sha: &str,
    chain: &[Attestation],
) -> anyhow::Result<()> {
    let note_json = serde_json::to_string(chain)?;
    let repo_path = repo_path.to_string();
    let commit_sha = commit_sha.to_string();
    tokio::task::spawn_blocking(move || {
        let output = std::process::Command::new("git")
            .args([
                "-C",
                &repo_path,
                "notes",
                &format!("--ref={CHAIN_ATTESTATION_NOTES_REF}"),
                "add",
                "-f",
                "-m",
                &note_json,
                &commit_sha,
            ])
            .output()?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git notes add failed: {stderr}");
        }
    })
    .await?
}

/// Verify a bundle's signature using the provided raw 32-byte Ed25519 public key.
///
/// Returns `true` if the signature is valid.
pub fn verify_bundle(bundle: &AttestationBundle, public_key_bytes: &[u8]) -> bool {
    use ring::signature::{self, UnparsedPublicKey};
    let canonical = match serde_json::to_string(&bundle.attestation) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let sig_bytes = match BASE64.decode(&bundle.signature) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let pk = UnparsedPublicKey::new(&signature::ED25519, public_key_bytes);
    pk.verify(canonical.as_bytes(), &sig_bytes).is_ok()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::{
        AttestationInput, AttestationMetadata, AttestationOutput, InputContent, KeyBinding,
        OutputConstraint, PersonaRef, ScopeConstraint, SignedInput,
    };
    use tempfile::TempDir;

    fn sample_key_binding() -> KeyBinding {
        KeyBinding {
            public_key: vec![1, 2, 3, 4],
            user_identity: "user:tester".to_string(),
            issuer: "https://keycloak.example.com".to_string(),
            trust_anchor_id: "tenant-keycloak".to_string(),
            issued_at: 1_700_000_000,
            expires_at: 1_700_003_600,
            user_signature: vec![10, 20],
            platform_countersign: vec![30, 40],
        }
    }

    fn sample_attestation(commit_sha: &str) -> Attestation {
        Attestation {
            id: "sha256:test-note-1".to_string(),
            input: AttestationInput::Signed(SignedInput {
                content: InputContent {
                    spec_path: "specs/system/payments.md".to_string(),
                    spec_sha: "abc123".to_string(),
                    workspace_id: "ws-1".to_string(),
                    repo_id: "repo-1".to_string(),
                    persona_constraints: vec![PersonaRef {
                        name: "security".to_string(),
                    }],
                    meta_spec_set_sha: "def456".to_string(),
                    scope: ScopeConstraint {
                        allowed_paths: vec!["src/payments/**".to_string()],
                        forbidden_paths: vec!["src/auth/**".to_string()],
                    },
                },
                output_constraints: vec![OutputConstraint {
                    name: "scope check".to_string(),
                    expression: "output.changed_files.all(f, f.startsWith(\"src/payments/\"))"
                        .to_string(),
                }],
                valid_until: 1_700_100_000,
                expected_generation: Some(1),
                signature: vec![5, 6, 7],
                key_binding: sample_key_binding(),
            }),
            output: AttestationOutput {
                content_hash: vec![1, 2, 3],
                commit_sha: commit_sha.to_string(),
                agent_signature: None,
                gate_results: vec![],
            },
            metadata: AttestationMetadata {
                created_at: 1_700_000_000,
                workspace_id: "ws-1".to_string(),
                repo_id: "repo-1".to_string(),
                task_id: "TASK-TEST".to_string(),
                agent_id: "agent:test-1".to_string(),
                chain_depth: 0,
            },
        }
    }

    /// Create a temp git repo with an initial commit and return (dir, commit_sha).
    fn init_test_repo() -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_str().unwrap();

        std::process::Command::new("git")
            .args(["init", path])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["-C", path, "config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["-C", path, "config", "user.name", "Test"])
            .output()
            .unwrap();

        std::fs::write(dir.path().join("README.md"), "# Test").unwrap();
        std::process::Command::new("git")
            .args(["-C", path, "add", "."])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["-C", path, "commit", "-m", "initial"])
            .output()
            .unwrap();

        let output = std::process::Command::new("git")
            .args(["-C", path, "rev-parse", "HEAD"])
            .output()
            .unwrap();
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();

        (dir, sha)
    }

    /// Create a second commit and return its SHA.
    fn add_commit(dir: &TempDir, filename: &str, content: &str, msg: &str) -> String {
        let path = dir.path().to_str().unwrap();
        std::fs::write(dir.path().join(filename), content).unwrap();
        std::process::Command::new("git")
            .args(["-C", path, "add", "."])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["-C", path, "commit", "-m", msg])
            .output()
            .unwrap();
        let output = std::process::Command::new("git")
            .args(["-C", path, "rev-parse", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Build a two-node chain: root (depth 0) → leaf (depth 1).
    fn sample_chain(commit_sha: &str) -> Vec<Attestation> {
        let root = sample_attestation(commit_sha);
        let mut leaf = sample_attestation(commit_sha);
        leaf.id = "sha256:test-note-leaf".to_string();
        leaf.metadata.chain_depth = 1;
        leaf.metadata.task_id = "TASK-TEST-LEAF".to_string();
        // Root to leaf ordering per spec.
        vec![root, leaf]
    }

    #[tokio::test]
    async fn chain_note_roundtrip() {
        let (dir, sha) = init_test_repo();
        let repo_path = dir.path().to_str().unwrap();
        let chain = sample_chain(&sha);

        // Write the full chain as a git note.
        attach_chain_attestation_note_sync(repo_path, &sha, &chain)
            .await
            .unwrap();

        // Read it back.
        let result = read_chain_attestation_note(repo_path, &sha).await;
        assert!(result.is_some(), "expected chain note to be readable");
        let read_chain = result.unwrap();
        assert_eq!(read_chain.len(), 2, "full chain should have 2 nodes");
        assert_eq!(read_chain[0].id, chain[0].id);
        assert_eq!(read_chain[0].metadata.chain_depth, 0);
        assert_eq!(read_chain[1].id, chain[1].id);
        assert_eq!(read_chain[1].metadata.chain_depth, 1);
        assert_eq!(read_chain[1].metadata.task_id, "TASK-TEST-LEAF");
    }

    #[tokio::test]
    async fn chain_notes_different_commits_isolated() {
        let (dir, sha1) = init_test_repo();
        let sha2 = add_commit(&dir, "file2.txt", "content", "second");
        let repo_path = dir.path().to_str().unwrap();

        let chain1 = vec![sample_attestation(&sha1)];
        let mut att2 = sample_attestation(&sha2);
        att2.id = "sha256:test-note-2".to_string();
        att2.metadata.task_id = "TASK-TEST-2".to_string();
        att2.output.commit_sha = sha2.clone();
        let chain2 = vec![att2];

        // Write both notes.
        attach_chain_attestation_note_sync(repo_path, &sha1, &chain1)
            .await
            .unwrap();
        attach_chain_attestation_note_sync(repo_path, &sha2, &chain2)
            .await
            .unwrap();

        // Read back — each commit returns its own chain.
        let r1 = read_chain_attestation_note(repo_path, &sha1).await.unwrap();
        let r2 = read_chain_attestation_note(repo_path, &sha2).await.unwrap();
        assert_eq!(r1.len(), 1);
        assert_eq!(r1[0].id, "sha256:test-note-1");
        assert_eq!(r1[0].metadata.task_id, "TASK-TEST");
        assert_eq!(r2.len(), 1);
        assert_eq!(r2[0].id, "sha256:test-note-2");
        assert_eq!(r2[0].metadata.task_id, "TASK-TEST-2");
    }

    #[tokio::test]
    async fn chain_note_returns_none_for_uncommitted() {
        let (dir, _sha) = init_test_repo();
        let repo_path = dir.path().to_str().unwrap();

        // No note written — reading should return None.
        let result =
            read_chain_attestation_note(repo_path, "0000000000000000000000000000000000000000")
                .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn chain_note_overwrite_with_gate_results() {
        let (dir, sha) = init_test_repo();
        let repo_path = dir.path().to_str().unwrap();

        let chain = vec![sample_attestation(&sha)];

        // Write initial note.
        attach_chain_attestation_note_sync(repo_path, &sha, &chain)
            .await
            .unwrap();

        // Simulate gate result appended — overwrite with -f.
        let mut updated_leaf = chain[0].clone();
        updated_leaf
            .output
            .gate_results
            .push(gyre_common::GateAttestation {
                gate_id: "gate-1".to_string(),
                gate_name: "unit-tests".to_string(),
                gate_type: gyre_common::GateType::TestCommand,
                status: gyre_common::GateStatus::Passed,
                output_hash: vec![80, 90],
                constraint: None,
                signature: vec![11, 22],
                key_binding: sample_key_binding(),
            });
        let updated_chain = vec![updated_leaf];

        attach_chain_attestation_note_sync(repo_path, &sha, &updated_chain)
            .await
            .unwrap();

        // Read back — should have the gate result.
        let result = read_chain_attestation_note(repo_path, &sha).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].output.gate_results.len(), 1);
        assert_eq!(result[0].output.gate_results[0].gate_id, "gate-1");
    }

    #[tokio::test]
    async fn dual_write_chain_overwrites_legacy_on_same_ref() {
        let (dir, sha) = init_test_repo();
        let repo_path = dir.path().to_str().unwrap();

        // Write legacy note first (refs/notes/attestations).
        let legacy_json = r#"{"legacy":"bundle"}"#;
        let output = std::process::Command::new("git")
            .args([
                "-C",
                repo_path,
                "notes",
                "--ref=refs/notes/attestations",
                "add",
                "-f",
                "-m",
                legacy_json,
                &sha,
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "legacy note write failed");

        // Write chain attestation note (same ref, overwrites legacy with -f).
        let chain = sample_chain(&sha);
        attach_chain_attestation_note_sync(repo_path, &sha, &chain)
            .await
            .unwrap();

        // The note on refs/notes/attestations should now be the chain (JSON array),
        // having overwritten the legacy format. This is the spec's "same as" behavior.
        let read_chain = read_chain_attestation_note(repo_path, &sha).await;
        assert!(read_chain.is_some(), "chain note should be readable");
        let read_chain = read_chain.unwrap();
        assert_eq!(read_chain.len(), 2, "should contain full 2-node chain");
        assert_eq!(read_chain[0].id, chain[0].id);
        assert_eq!(read_chain[1].id, chain[1].id);
    }

    #[tokio::test]
    async fn chain_note_full_attestation_fields_preserved() {
        let (dir, sha) = init_test_repo();
        let repo_path = dir.path().to_str().unwrap();
        let chain = sample_chain(&sha);

        attach_chain_attestation_note_sync(repo_path, &sha, &chain)
            .await
            .unwrap();

        let result = read_chain_attestation_note(repo_path, &sha).await.unwrap();

        // Verify all fields round-trip correctly for the full chain.
        assert_eq!(result, chain);
    }
}
