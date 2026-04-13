//! Merge attestation domain types and attestation chain helpers.

use gyre_common::{AgentCompletionSummary, Attestation, AttestationInput};

use crate::MetaSpecUsed;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Attestation chain helpers (authorization-provenance.md §7.2)
// ---------------------------------------------------------------------------

/// Extract the root signer from an attestation chain.
///
/// Walks the chain to find the root `SignedInput` (chain_depth == 0) and
/// returns the `user_identity` from its `key_binding`. Returns `None` for
/// an empty chain or a chain without a `SignedInput` root.
pub fn root_signer(chain: &[Attestation]) -> Option<String> {
    chain
        .iter()
        .find(|a| a.metadata.chain_depth == 0)
        .and_then(|a| match &a.input {
            AttestationInput::Signed(si) => Some(si.key_binding.user_identity.clone()),
            _ => None,
        })
}

/// Count accumulated constraints across an attestation chain.
///
/// Counts explicit output constraints from each node's input plus gate
/// constraints from each node's gate results. This covers:
/// - Explicit user constraints (`SignedInput.output_constraints`)
/// - Derived constraints (`DerivedInput.output_constraints`)
/// - Gate constraints (`GateAttestation.constraint`)
pub fn constraint_count(chain: &[Attestation]) -> usize {
    let mut count = 0;
    for att in chain {
        // Count input-level constraints (explicit + derived).
        count += match &att.input {
            AttestationInput::Signed(si) => si.output_constraints.len(),
            AttestationInput::Derived(di) => di.output_constraints.len(),
        };
        // Count gate constraints.
        count += att
            .output
            .gate_results
            .iter()
            .filter(|gr| gr.constraint.is_some())
            .count();
    }
    count
}

/// Snapshot of a single gate result captured at merge time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationGateResult {
    pub gate_id: String,
    pub gate_type: String,
    pub status: String,
    pub output: Option<String>,
}

/// The attestation payload for one merge event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeAttestation {
    /// Gyre attestation format version.
    pub attestation_version: u32,
    pub mr_id: String,
    pub merge_commit_sha: String,
    /// Unix epoch seconds when the merge was recorded.
    pub merged_at: u64,
    /// Gate results at the time of merge.
    pub gate_results: Vec<AttestationGateResult>,
    /// Spec reference bound to this MR, if any.
    pub spec_ref: Option<String>,
    /// Whether every referenced spec had an active approval at merge time.
    pub spec_fully_approved: bool,
    /// Agent ID of the MR author.
    pub author_agent_id: Option<String>,
    /// SHA-256 of the agent's conversation blob (HSI §5 provenance).
    /// Populated from the KV store at merge time; None if the agent did not upload a conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_sha: Option<String>,
    /// Agent completion summary (HSI §4) — populated when the agent calls `agent.complete`
    /// with a `summary` field. Contains decisions, uncertainties, and conversation_sha.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_summary: Option<AgentCompletionSummary>,
    /// Meta-specs that were consulted during the agent's work session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meta_specs_used: Vec<MetaSpecUsed>,
}

// ---------------------------------------------------------------------------
// Tests for attestation chain helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::{
        AttestationMetadata, AttestationOutput, DerivedInput, GateAttestation, GateConstraint,
        InputContent, KeyBinding, OutputConstraint, PersonaRef, ScopeConstraint, SignedInput,
    };

    fn sample_key_binding(identity: &str) -> KeyBinding {
        KeyBinding {
            public_key: vec![1, 2, 3, 4],
            user_identity: identity.to_string(),
            issuer: "https://keycloak.example.com".to_string(),
            trust_anchor_id: "tenant-keycloak".to_string(),
            issued_at: 1_700_000_000,
            expires_at: 1_700_003_600,
            user_signature: vec![10, 20, 30, 40],
            platform_countersign: vec![50, 60, 70, 80],
        }
    }

    fn sample_output_constraint(name: &str) -> OutputConstraint {
        OutputConstraint {
            name: name.to_string(),
            expression: format!("output.check(\"{name}\")"),
        }
    }

    fn sample_signed_input(identity: &str) -> SignedInput {
        SignedInput {
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
                    allowed_paths: vec!["src/**".to_string()],
                    forbidden_paths: vec![],
                },
            },
            output_constraints: vec![sample_output_constraint("scope-check")],
            valid_until: 1_700_000_000,
            expected_generation: Some(1),
            signature: vec![10, 20, 30],
            key_binding: sample_key_binding(identity),
        }
    }

    fn sample_gate_attestation(with_constraint: bool) -> GateAttestation {
        GateAttestation {
            gate_id: "gate-1".to_string(),
            gate_name: "unit-tests".to_string(),
            gate_type: gyre_common::gate::GateType::TestCommand,
            status: gyre_common::gate::GateStatus::Passed,
            output_hash: vec![80, 90],
            constraint: if with_constraint {
                Some(GateConstraint {
                    gate_id: "gate-1".to_string(),
                    gate_name: "unit-tests".to_string(),
                    constraint: sample_output_constraint("gate-constraint"),
                    signed_by: vec![50, 60, 70],
                })
            } else {
                None
            },
            signature: vec![11, 22, 33],
            key_binding: sample_key_binding("gate-agent"),
        }
    }

    fn root_attestation(identity: &str) -> Attestation {
        Attestation {
            id: "sha256:root".to_string(),
            input: AttestationInput::Signed(sample_signed_input(identity)),
            output: AttestationOutput {
                content_hash: vec![1, 2, 3],
                commit_sha: "aaa111".to_string(),
                agent_signature: None,
                gate_results: vec![sample_gate_attestation(true)],
            },
            metadata: AttestationMetadata {
                created_at: 1_700_000_000,
                workspace_id: "ws-1".to_string(),
                repo_id: "repo-1".to_string(),
                task_id: "TASK-001".to_string(),
                agent_id: "agent:worker-1".to_string(),
                chain_depth: 0,
            },
        }
    }

    fn derived_attestation(depth: u32) -> Attestation {
        Attestation {
            id: format!("sha256:derived-{depth}"),
            input: AttestationInput::Derived(DerivedInput {
                parent_ref: vec![99, 88],
                preconditions: vec![],
                update: "narrow_scope".to_string(),
                output_constraints: vec![
                    sample_output_constraint("derived-c1"),
                    sample_output_constraint("derived-c2"),
                ],
                signature: vec![11],
                key_binding: sample_key_binding("orchestrator"),
            }),
            output: AttestationOutput {
                content_hash: vec![4, 5, 6],
                commit_sha: format!("bbb{depth}"),
                agent_signature: None,
                gate_results: vec![sample_gate_attestation(false)],
            },
            metadata: AttestationMetadata {
                created_at: 1_700_000_100,
                workspace_id: "ws-1".to_string(),
                repo_id: "repo-1".to_string(),
                task_id: "TASK-002".to_string(),
                agent_id: "agent:worker-2".to_string(),
                chain_depth: depth,
            },
        }
    }

    // --- root_signer tests ---

    #[test]
    fn root_signer_extracts_identity_from_signed_root() {
        let chain = vec![root_attestation("user:jsell"), derived_attestation(1)];
        assert_eq!(root_signer(&chain), Some("user:jsell".to_string()));
    }

    #[test]
    fn root_signer_returns_none_for_empty_chain() {
        assert_eq!(root_signer(&[]), None);
    }

    #[test]
    fn root_signer_returns_none_when_root_is_derived() {
        // Chain where depth=0 has a DerivedInput (no SignedInput root).
        let mut att = derived_attestation(0);
        att.metadata.chain_depth = 0;
        let chain = vec![att];
        assert_eq!(root_signer(&chain), None);
    }

    #[test]
    fn root_signer_finds_root_regardless_of_order() {
        // Chain with leaf first, root second.
        let chain = vec![derived_attestation(1), root_attestation("user:alice")];
        assert_eq!(root_signer(&chain), Some("user:alice".to_string()));
    }

    // --- constraint_count tests ---

    #[test]
    fn constraint_count_counts_explicit_and_gate_constraints() {
        // Root has 1 output_constraint + 1 gate_constraint = 2
        let chain = vec![root_attestation("user:jsell")];
        assert_eq!(constraint_count(&chain), 2);
    }

    #[test]
    fn constraint_count_accumulates_across_chain() {
        // Root: 1 output + 1 gate = 2
        // Derived: 2 output + 0 gate (constraint=None) = 2
        // Total: 4
        let chain = vec![root_attestation("user:jsell"), derived_attestation(1)];
        assert_eq!(constraint_count(&chain), 4);
    }

    #[test]
    fn constraint_count_zero_for_empty_chain() {
        assert_eq!(constraint_count(&[]), 0);
    }
}

/// Signed attestation bundle returned by `GET /api/v1/merge-requests/{id}/attestation`.
///
/// **Deprecated (Phase 4):** This legacy format is subsumed by the chain attestation
/// system (`authorization-provenance.md` §5.2). New attestations are produced in both
/// formats during the dual-write period. Consumers should migrate to the chain attestation
/// API: `GET /api/v1/repos/{id}/attestations/{commit_sha}/verification`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationBundle {
    pub attestation: MergeAttestation,
    /// Base64-encoded Ed25519 signature over the canonical JSON of `attestation`.
    pub signature: String,
    /// `kid` of the Ed25519 key that produced `signature`.
    pub signing_key_id: String,
    /// Deprecation notice included in API responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecation_notice: Option<String>,
}
