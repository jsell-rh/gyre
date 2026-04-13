use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::{Attestation, AttestationInput, AttestationMetadata, AttestationOutput};
use gyre_ports::ChainAttestationRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::chain_attestations;

#[derive(Queryable, Selectable)]
#[diesel(table_name = chain_attestations)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct ChainAttestationRow {
    id: String,
    #[allow(dead_code)]
    input_type: String,
    input_json: String,
    output_json: String,
    #[allow(dead_code)]
    metadata_json: String,
    parent_ref: Option<String>,
    chain_depth: i32,
    workspace_id: String,
    repo_id: String,
    task_id: String,
    agent_id: String,
    created_at: i64,
    #[allow(dead_code)]
    tenant_id: String,
    #[allow(dead_code)]
    commit_sha: String,
}

impl ChainAttestationRow {
    fn into_attestation(self) -> Result<Attestation> {
        let input: AttestationInput =
            serde_json::from_str(&self.input_json).context("deserialize attestation input")?;
        let output: AttestationOutput =
            serde_json::from_str(&self.output_json).context("deserialize attestation output")?;
        let metadata = AttestationMetadata {
            created_at: self.created_at as u64,
            workspace_id: self.workspace_id,
            repo_id: self.repo_id,
            task_id: self.task_id,
            agent_id: self.agent_id,
            chain_depth: self.chain_depth as u32,
        };
        Ok(Attestation {
            id: self.id,
            input,
            output,
            metadata,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = chain_attestations)]
struct NewChainAttestationRow<'a> {
    id: &'a str,
    input_type: &'a str,
    input_json: &'a str,
    output_json: &'a str,
    metadata_json: &'a str,
    parent_ref: Option<&'a str>,
    chain_depth: i32,
    workspace_id: &'a str,
    repo_id: &'a str,
    task_id: &'a str,
    agent_id: &'a str,
    created_at: i64,
    tenant_id: &'a str,
    commit_sha: &'a str,
}

/// Extract the parent attestation ID from an AttestationInput.
///
/// The `DerivedInput.parent_ref` field stores the content hash of the parent
/// attestation as raw bytes. Since attestation IDs are content-addressable hash
/// strings, we convert the bytes to a hex string prefixed with "sha256:" to match
/// the parent's `id` column. If the bytes are valid UTF-8 that already looks like
/// an attestation ID (e.g., "sha256:..."), we use that directly.
fn extract_parent_ref(input: &AttestationInput) -> Option<String> {
    match input {
        AttestationInput::Derived(d) => {
            // If the parent_ref bytes are valid UTF-8 and look like an attestation ID,
            // use them directly. Otherwise, hex-encode them.
            match String::from_utf8(d.parent_ref.clone()) {
                Ok(s) if !s.is_empty() => Some(s),
                _ => Some(hex::encode(&d.parent_ref)),
            }
        }
        AttestationInput::Signed(_) => None,
    }
}

/// Return "signed" or "derived" for the attestation input discriminant.
fn input_type_str(input: &AttestationInput) -> &'static str {
    match input {
        AttestationInput::Signed(_) => "signed",
        AttestationInput::Derived(_) => "derived",
    }
}

#[async_trait]
impl ChainAttestationRepository for SqliteStorage {
    async fn save(&self, attestation: &Attestation) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = self.tenant_id.clone();
        let attestation = attestation.clone();
        let input_json =
            serde_json::to_string(&attestation.input).context("serialize attestation input")?;
        let output_json =
            serde_json::to_string(&attestation.output).context("serialize attestation output")?;
        let metadata_json = serde_json::to_string(&attestation.metadata)
            .context("serialize attestation metadata")?;
        let parent_ref = extract_parent_ref(&attestation.input);
        let input_type = input_type_str(&attestation.input).to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewChainAttestationRow {
                id: &attestation.id,
                input_type: &input_type,
                input_json: &input_json,
                output_json: &output_json,
                metadata_json: &metadata_json,
                parent_ref: parent_ref.as_deref(),
                chain_depth: attestation.metadata.chain_depth as i32,
                workspace_id: &attestation.metadata.workspace_id,
                repo_id: &attestation.metadata.repo_id,
                task_id: &attestation.metadata.task_id,
                agent_id: &attestation.metadata.agent_id,
                created_at: attestation.metadata.created_at as i64,
                tenant_id: &tenant_id,
                commit_sha: &attestation.output.commit_sha,
            };
            diesel::insert_into(chain_attestations::table)
                .values(&row)
                .on_conflict(chain_attestations::id)
                .do_update()
                .set((
                    chain_attestations::input_type.eq(&input_type),
                    chain_attestations::input_json.eq(&input_json),
                    chain_attestations::output_json.eq(&output_json),
                    chain_attestations::metadata_json.eq(&metadata_json),
                    chain_attestations::parent_ref.eq(parent_ref.as_deref()),
                    chain_attestations::chain_depth.eq(attestation.metadata.chain_depth as i32),
                    chain_attestations::commit_sha.eq(&attestation.output.commit_sha),
                ))
                .execute(&mut *conn)
                .context("upsert chain attestation")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &str) -> Result<Option<Attestation>> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = self.tenant_id.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Attestation>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = chain_attestations::table
                .find(&id)
                .filter(chain_attestations::tenant_id.eq(&tenant_id))
                .first::<ChainAttestationRow>(&mut *conn)
                .optional()
                .context("find chain attestation by id")?;
            row.map(ChainAttestationRow::into_attestation).transpose()
        })
        .await?
    }

    async fn load_chain(&self, leaf_id: &str) -> Result<Vec<Attestation>> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = self.tenant_id.clone();
        let leaf_id = leaf_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<Attestation>> {
            let mut conn = pool.get().context("get db connection")?;
            let mut chain = Vec::new();
            let mut current_id = Some(leaf_id);

            // Walk the chain from leaf to root via parent_ref.
            // Guard against infinite loops with a max depth of 100.
            while let Some(id) = current_id.take() {
                if chain.len() >= 100 {
                    anyhow::bail!("attestation chain exceeded max depth of 100");
                }
                let row = chain_attestations::table
                    .find(&id)
                    .filter(chain_attestations::tenant_id.eq(&tenant_id))
                    .first::<ChainAttestationRow>(&mut *conn)
                    .optional()
                    .context("load chain attestation")?;
                match row {
                    Some(r) => {
                        let parent = r.parent_ref.clone();
                        let attestation = r.into_attestation()?;
                        chain.push(attestation);
                        current_id = parent;
                    }
                    None => break,
                }
            }

            // Reverse so root is first, leaf is last.
            chain.reverse();
            Ok(chain)
        })
        .await?
    }

    async fn find_by_task(&self, task_id: &str) -> Result<Vec<Attestation>> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = self.tenant_id.clone();
        let task_id = task_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<Attestation>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = chain_attestations::table
                .filter(chain_attestations::tenant_id.eq(&tenant_id))
                .filter(chain_attestations::task_id.eq(&task_id))
                .order(chain_attestations::created_at.asc())
                .load::<ChainAttestationRow>(&mut *conn)
                .context("find chain attestations by task")?;
            rows.into_iter()
                .map(ChainAttestationRow::into_attestation)
                .collect()
        })
        .await?
    }

    async fn find_by_commit(&self, commit_sha: &str) -> Result<Option<Attestation>> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = self.tenant_id.clone();
        let commit_sha = commit_sha.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Attestation>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = chain_attestations::table
                .filter(chain_attestations::tenant_id.eq(&tenant_id))
                .filter(chain_attestations::commit_sha.eq(&commit_sha))
                .first::<ChainAttestationRow>(&mut *conn)
                .optional()
                .context("find chain attestation by commit")?;
            row.map(ChainAttestationRow::into_attestation).transpose()
        })
        .await?
    }

    async fn find_by_repo(
        &self,
        repo_id: &str,
        since: u64,
        until: u64,
    ) -> Result<Vec<Attestation>> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = self.tenant_id.clone();
        let repo_id = repo_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<Attestation>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = chain_attestations::table
                .filter(chain_attestations::tenant_id.eq(&tenant_id))
                .filter(chain_attestations::repo_id.eq(&repo_id))
                .filter(chain_attestations::created_at.ge(since as i64))
                .filter(chain_attestations::created_at.le(until as i64))
                .order(chain_attestations::created_at.asc())
                .load::<ChainAttestationRow>(&mut *conn)
                .context("find chain attestations by repo")?;
            rows.into_iter()
                .map(ChainAttestationRow::into_attestation)
                .collect()
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use gyre_common::attestation::*;
    use gyre_common::gate::{GateStatus, GateType};
    use gyre_common::KeyBinding;
    use tempfile::NamedTempFile;

    fn tmp_storage() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, storage)
    }

    fn sample_key_binding() -> KeyBinding {
        KeyBinding {
            public_key: vec![1, 2, 3, 4],
            user_identity: "user:jsell".to_string(),
            issuer: "https://keycloak.example.com".to_string(),
            trust_anchor_id: "tenant-keycloak".to_string(),
            issued_at: 1_700_000_000,
            expires_at: 1_700_003_600,
            user_signature: vec![10, 20, 30, 40],
            platform_countersign: vec![50, 60, 70, 80],
        }
    }

    fn sample_signed_attestation(id: &str, task_id: &str, commit_sha: &str) -> Attestation {
        Attestation {
            id: id.to_string(),
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
                        forbidden_paths: vec![],
                    },
                },
                output_constraints: vec![],
                valid_until: 1_700_100_000,
                expected_generation: None,
                signature: vec![10, 20, 30],
                key_binding: sample_key_binding(),
            }),
            output: AttestationOutput {
                content_hash: vec![1, 2, 3],
                commit_sha: commit_sha.to_string(),
                agent_signature: None,
                gate_results: vec![GateAttestation {
                    gate_id: "gate-1".to_string(),
                    gate_name: "unit-tests".to_string(),
                    gate_type: GateType::TestCommand,
                    status: GateStatus::Passed,
                    output_hash: vec![80, 90],
                    constraint: None,
                    signature: vec![11, 22, 33],
                    key_binding: sample_key_binding(),
                }],
            },
            metadata: AttestationMetadata {
                created_at: 1_700_000_000,
                workspace_id: "ws-1".to_string(),
                repo_id: "repo-1".to_string(),
                task_id: task_id.to_string(),
                agent_id: "agent:worker-42".to_string(),
                chain_depth: 0,
            },
        }
    }

    #[tokio::test]
    async fn save_and_find_by_id() {
        let (_tmp, storage) = tmp_storage();
        let att = sample_signed_attestation("sha256:root", "TASK-001", "abc123");
        storage.save(&att).await.unwrap();
        let found = storage.find_by_id("sha256:root").await.unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.id, "sha256:root");
        assert_eq!(found.metadata.task_id, "TASK-001");
        assert_eq!(found.metadata.chain_depth, 0);
    }

    #[tokio::test]
    async fn find_missing_returns_none() {
        let (_tmp, storage) = tmp_storage();
        let found = storage.find_by_id("nonexistent").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn find_by_task() {
        let (_tmp, storage) = tmp_storage();
        let a1 = sample_signed_attestation("sha256:a1", "TASK-001", "commit1");
        let a2 = sample_signed_attestation("sha256:a2", "TASK-001", "commit2");
        let a3 = sample_signed_attestation("sha256:a3", "TASK-002", "commit3");
        storage.save(&a1).await.unwrap();
        storage.save(&a2).await.unwrap();
        storage.save(&a3).await.unwrap();
        let results = storage.find_by_task("TASK-001").await.unwrap();
        assert_eq!(results.len(), 2);
        let task2 = storage.find_by_task("TASK-002").await.unwrap();
        assert_eq!(task2.len(), 1);
    }

    #[tokio::test]
    async fn find_by_commit() {
        let (_tmp, storage) = tmp_storage();
        let att = sample_signed_attestation("sha256:root", "TASK-001", "abc123");
        storage.save(&att).await.unwrap();
        let found = storage.find_by_commit("abc123").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "sha256:root");
        let missing = storage.find_by_commit("nonexistent").await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn find_by_repo() {
        let (_tmp, storage) = tmp_storage();
        let a1 = sample_signed_attestation("sha256:a1", "TASK-001", "commit1");
        let mut a2 = sample_signed_attestation("sha256:a2", "TASK-002", "commit2");
        a2.metadata.created_at = 1_700_050_000;
        storage.save(&a1).await.unwrap();
        storage.save(&a2).await.unwrap();
        // Query full range
        let all = storage
            .find_by_repo("repo-1", 0, i64::MAX as u64)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        // Query narrow range
        let narrow = storage
            .find_by_repo("repo-1", 1_700_000_000, 1_700_000_001)
            .await
            .unwrap();
        assert_eq!(narrow.len(), 1);
        assert_eq!(narrow[0].id, "sha256:a1");
    }

    #[tokio::test]
    async fn load_chain_single_root() {
        let (_tmp, storage) = tmp_storage();
        let root = sample_signed_attestation("sha256:root", "TASK-001", "commit1");
        storage.save(&root).await.unwrap();
        let chain = storage.load_chain("sha256:root").await.unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].id, "sha256:root");
    }

    #[tokio::test]
    async fn load_chain_walks_parent_refs() {
        let (_tmp, storage) = tmp_storage();
        // Build a chain: root -> child -> grandchild.
        // DerivedInput.parent_ref stores the parent attestation ID as UTF-8 bytes.
        let root = sample_signed_attestation("root-id", "TASK-001", "commit1");
        storage.save(&root).await.unwrap();

        let child = Attestation {
            id: "child-id".to_string(),
            input: AttestationInput::Derived(DerivedInput {
                parent_ref: "root-id".as_bytes().to_vec(),
                preconditions: vec![],
                update: "narrow_scope".to_string(),
                output_constraints: vec![],
                signature: vec![44, 55],
                key_binding: sample_key_binding(),
            }),
            output: AttestationOutput {
                content_hash: vec![4, 5, 6],
                commit_sha: "commit2".to_string(),
                agent_signature: None,
                gate_results: vec![],
            },
            metadata: AttestationMetadata {
                created_at: 1_700_000_001,
                workspace_id: "ws-1".to_string(),
                repo_id: "repo-1".to_string(),
                task_id: "TASK-001".to_string(),
                agent_id: "agent:worker-43".to_string(),
                chain_depth: 1,
            },
        };
        storage.save(&child).await.unwrap();

        let grandchild = Attestation {
            id: "grandchild-id".to_string(),
            input: AttestationInput::Derived(DerivedInput {
                parent_ref: "child-id".as_bytes().to_vec(),
                preconditions: vec![],
                update: "final".to_string(),
                output_constraints: vec![],
                signature: vec![66, 77],
                key_binding: sample_key_binding(),
            }),
            output: AttestationOutput {
                content_hash: vec![7, 8, 9],
                commit_sha: "commit3".to_string(),
                agent_signature: None,
                gate_results: vec![],
            },
            metadata: AttestationMetadata {
                created_at: 1_700_000_002,
                workspace_id: "ws-1".to_string(),
                repo_id: "repo-1".to_string(),
                task_id: "TASK-001".to_string(),
                agent_id: "agent:worker-44".to_string(),
                chain_depth: 2,
            },
        };
        storage.save(&grandchild).await.unwrap();

        let chain = storage.load_chain("grandchild-id").await.unwrap();
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].id, "root-id"); // root first
        assert_eq!(chain[1].id, "child-id");
        assert_eq!(chain[2].id, "grandchild-id"); // leaf last
    }

    #[tokio::test]
    async fn load_chain_missing_leaf_returns_empty() {
        let (_tmp, storage) = tmp_storage();
        let chain = storage.load_chain("nonexistent").await.unwrap();
        assert!(chain.is_empty());
    }

    #[tokio::test]
    async fn attestation_input_roundtrip_signed() {
        let (_tmp, storage) = tmp_storage();
        let att = sample_signed_attestation("sha256:signed", "TASK-001", "commit1");
        storage.save(&att).await.unwrap();
        let found = storage.find_by_id("sha256:signed").await.unwrap().unwrap();
        match &found.input {
            AttestationInput::Signed(si) => {
                assert_eq!(si.content.spec_path, "specs/system/payments.md");
                assert_eq!(si.content.persona_constraints.len(), 1);
                assert_eq!(si.content.persona_constraints[0].name, "security");
            }
            AttestationInput::Derived(_) => panic!("expected Signed input"),
        }
    }

    #[tokio::test]
    async fn attestation_gate_results_roundtrip() {
        let (_tmp, storage) = tmp_storage();
        let att = sample_signed_attestation("sha256:gated", "TASK-001", "commit1");
        storage.save(&att).await.unwrap();
        let found = storage.find_by_id("sha256:gated").await.unwrap().unwrap();
        assert_eq!(found.output.gate_results.len(), 1);
        assert_eq!(found.output.gate_results[0].gate_id, "gate-1");
        assert_eq!(
            found.output.gate_results[0].gate_type,
            GateType::TestCommand
        );
        assert_eq!(found.output.gate_results[0].status, GateStatus::Passed);
    }

    #[tokio::test]
    async fn upsert_overwrites() {
        let (_tmp, storage) = tmp_storage();
        let mut att = sample_signed_attestation("sha256:root", "TASK-001", "commit1");
        storage.save(&att).await.unwrap();
        att.output.commit_sha = "commit2".to_string();
        storage.save(&att).await.unwrap();
        let found = storage.find_by_id("sha256:root").await.unwrap().unwrap();
        assert_eq!(found.output.commit_sha, "commit2");
    }

    #[tokio::test]
    async fn tenant_isolation_find_by_id() {
        let (_tmp, storage) = tmp_storage();
        let att = sample_signed_attestation("sha256:t1", "TASK-001", "commit1");
        storage.save(&att).await.unwrap();
        // Same DB, different tenant — should not see the attestation
        let other = storage.with_tenant("other-tenant");
        let found = other.find_by_id("sha256:t1").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn tenant_isolation_find_by_task() {
        let (_tmp, storage) = tmp_storage();
        let att = sample_signed_attestation("sha256:t2", "TASK-001", "commit1");
        storage.save(&att).await.unwrap();
        let other = storage.with_tenant("other-tenant");
        let results = other.find_by_task("TASK-001").await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn tenant_isolation_find_by_commit() {
        let (_tmp, storage) = tmp_storage();
        let att = sample_signed_attestation("sha256:t3", "TASK-001", "commit-iso");
        storage.save(&att).await.unwrap();
        let other = storage.with_tenant("other-tenant");
        let found = other.find_by_commit("commit-iso").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn tenant_isolation_find_by_repo() {
        let (_tmp, storage) = tmp_storage();
        let att = sample_signed_attestation("sha256:t4", "TASK-001", "commit1");
        storage.save(&att).await.unwrap();
        let other = storage.with_tenant("other-tenant");
        let results = other.find_by_repo("repo-1", 0, u64::MAX).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn tenant_isolation_load_chain() {
        let (_tmp, storage) = tmp_storage();
        let att = sample_signed_attestation("sha256:t5", "TASK-001", "commit1");
        storage.save(&att).await.unwrap();
        let other = storage.with_tenant("other-tenant");
        let chain = other.load_chain("sha256:t5").await.unwrap();
        assert!(chain.is_empty());
    }
}
