//! Authorization provenance types (authorization-provenance.md §1–§6).
//!
//! These types form the cryptographic proof chain that work was authorized,
//! constrained to what was authorized, and verifiable without trusting the platform.

use serde::{Deserialize, Serialize};

use crate::gate::{GateStatus, GateType};
use crate::key_binding::KeyBinding;

// ── §1.1 Trust Anchor ──────────────────────────────────────────────────

/// The type of identity issuer a trust anchor represents.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustAnchorType {
    /// Human user identity (Keycloak, Okta, Entra ID).
    User,
    /// Agent workload identity (Gyre OIDC issuer).
    Agent,
    /// External system identity (GitHub Actions OIDC, Sigstore).
    Addon,
}

/// A registered identity issuer the verification algorithm trusts (§1.1).
///
/// Trust anchors are tenant-scoped and external to the platform — Gyre is never
/// its own trust root for authorization provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrustAnchor {
    /// Stable identifier (e.g., "tenant-keycloak").
    pub id: String,
    /// OIDC issuer URL or SPIFFE trust domain.
    pub issuer: String,
    /// Public key endpoint.
    pub jwks_uri: String,
    /// What kind of identity this anchor authenticates.
    pub anchor_type: TrustAnchorType,
    /// Anchor-level output constraints (§3.2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<OutputConstraint>,
}

// ── §2.2 Signed Input ──────────────────────────────────────────────────

/// File-level boundaries of what an authorization permits (§2.2).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScopeConstraint {
    /// Files the agent may modify (e.g., `["src/payments/**"]`).
    /// Empty means "any file".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_paths: Vec<String>,
    /// Files the agent must not modify (e.g., `["src/auth/**"]`).
    /// Always enforced.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbidden_paths: Vec<String>,
}

/// A reference to a persona required for implementation (§2.2).
///
/// Structured type (not a bare string) so that downstream CEL evaluators
/// can access fields: `input.persona_constraints[0].name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonaRef {
    /// Persona name (e.g., "security").
    pub name: String,
}

/// The content that a human signs when approving a spec (§2.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputContent {
    /// Spec file path (e.g., "specs/system/payments.md").
    pub spec_path: String,
    /// Git blob SHA at approval time.
    pub spec_sha: String,
    /// Scoping boundary — workspace this authorization applies to.
    pub workspace_id: String,
    /// Target repository.
    pub repo_id: String,
    /// Required persona(s) for implementation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub persona_constraints: Vec<PersonaRef>,
    /// Hash of the bound meta-spec set at approval time.
    pub meta_spec_set_sha: String,
    /// What parts of the repo this authorization covers.
    pub scope: ScopeConstraint,
}

/// The authorization root — a cryptographic authorization binding spec approval
/// to output constraints (§2.1–§2.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignedInput {
    /// The content being authorized.
    pub content: InputContent,
    /// Explicit user constraints (§3.1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_constraints: Vec<OutputConstraint>,
    /// Hard expiry timestamp (Unix epoch seconds).
    pub valid_until: u64,
    /// Optional monotonic counter for replay prevention (§2.4).
    pub expected_generation: Option<u32>,
    /// Ed25519 signature over the content.
    pub signature: Vec<u8>,
    /// The full key binding that produced this signature (§2.3).
    ///
    /// The full `KeyBinding` is embedded (not a reference) so the attestation
    /// chain is self-contained for offline verification (§6.3).
    pub key_binding: KeyBinding,
}

// ── §3.1 Output Constraint ─────────────────────────────────────────────

/// A named CEL predicate that output must satisfy at verification time (§3.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputConstraint {
    /// Human-readable description.
    pub name: String,
    /// CEL expression that must evaluate to true.
    pub expression: String,
}

// ── §3.2 Gate Constraint ───────────────────────────────────────────────

/// An output constraint produced by a quality gate (§3.2).
///
/// Gate constraints are additive — a gate can tighten but never loosen
/// the constraint set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateConstraint {
    /// Quality gate identifier.
    pub gate_id: String,
    /// Human-readable gate name.
    pub gate_name: String,
    /// The constraint this gate imposes.
    pub constraint: OutputConstraint,
    /// Gate agent's signature over the constraint.
    pub signed_by: Vec<u8>,
}

// ── §4.1 Derived Input ─────────────────────────────────────────────────

/// Delegation provenance — a new authorization cryptographically linked
/// to its parent (§4.1).
///
/// Constraints only grow: derived constraints are additive (parent + new).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedInput {
    /// Content hash of the parent attestation.
    pub parent_ref: Vec<u8>,
    /// CEL predicates that must hold on the parent's state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preconditions: Vec<String>,
    /// CEL expression defining what changed in the derivation.
    pub update: String,
    /// Additional constraints (additive only — never removing parent constraints).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_constraints: Vec<OutputConstraint>,
    /// Orchestrator's signature over the derivation.
    pub signature: Vec<u8>,
    /// Orchestrator's full key binding for offline verification (§2.3, §6.3).
    pub key_binding: KeyBinding,
}

// ── §5.1 Attestation ───────────────────────────────────────────────────

/// The authorization input — either a root signed input or a derived delegation (§5.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum AttestationInput {
    /// Root authorization from a human spec approval.
    Signed(SignedInput),
    /// Delegated authorization from an orchestrator.
    Derived(DerivedInput),
}

/// The output portion of an attestation record (§5.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttestationOutput {
    /// Hash of the actual output (diff, commit).
    pub content_hash: Vec<u8>,
    /// Git commit SHA.
    pub commit_sha: String,
    /// Agent's signature over the output (if the agent is capable of signing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_signature: Option<Vec<u8>>,
    /// Per-gate signed results.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gate_results: Vec<GateAttestation>,
}

/// Metadata for an attestation record (§5.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttestationMetadata {
    /// When this attestation was created (Unix epoch seconds).
    pub created_at: u64,
    /// Workspace scope.
    pub workspace_id: String,
    /// Target repository.
    pub repo_id: String,
    /// Task that produced this attestation.
    pub task_id: String,
    /// Agent that produced this attestation.
    pub agent_id: String,
    /// 0 for root SignedInput, increments per derivation.
    pub chain_depth: u32,
}

/// A per-gate signed result in the attestation chain (§5.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateAttestation {
    /// Quality gate identifier.
    pub gate_id: String,
    /// Human-readable gate name.
    pub gate_name: String,
    /// Quality gate type — matches the gate definition's discriminant (§5.1).
    pub gate_type: GateType,
    /// Gate execution status (§5.1).
    pub status: GateStatus,
    /// Hash of gate output.
    pub output_hash: Vec<u8>,
    /// Optional gate constraint produced by this gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraint: Option<GateConstraint>,
    /// Gate agent or forge signature.
    pub signature: Vec<u8>,
    /// Gate agent's full key binding for offline verification (§2.3, §6.3).
    pub key_binding: KeyBinding,
}

impl GateAttestation {
    /// Returns the canonical bytes that are signed/verified for this gate attestation.
    ///
    /// Both the signing site (`gate_executor`) and verification site
    /// (`verify_output_signatures`) MUST use this helper to ensure sign/verify
    /// message parity (see checklist item 44). The signable message is a JSON
    /// object containing 5 fields serialized via `serde_json::to_vec`.
    pub fn signable_bytes(&self) -> Vec<u8> {
        let sign_content = serde_json::json!({
            "gate_id": self.gate_id,
            "gate_name": self.gate_name,
            "gate_type": self.gate_type,
            "status": self.status,
            "output_hash": hex::encode(&self.output_hash),
        });
        serde_json::to_vec(&sign_content).unwrap_or_default()
    }
}

/// The complete attestation record — packages input, output, and metadata (§5.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attestation {
    /// Content-addressable hash of this attestation.
    pub id: String,
    /// The authorization input (signed root or derived delegation).
    pub input: AttestationInput,
    /// The actual output and gate results.
    pub output: AttestationOutput,
    /// Metadata (who, when, where).
    pub metadata: AttestationMetadata,
}

// ── §6.4 Verification Result ───────────────────────────────────────────

/// Recursive verification tree stored for audit (§6.4).
///
/// Every verification produces a tree of results — each node describes
/// what was verified, whether it passed, and sub-verifications.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationResult {
    /// What was verified (human-readable label).
    pub label: String,
    /// Whether this verification step passed.
    pub valid: bool,
    /// Human-readable explanation of the result.
    pub message: String,
    /// Sub-verifications (recursive tree).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<VerificationResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn sample_scope_constraint() -> ScopeConstraint {
        ScopeConstraint {
            allowed_paths: vec!["src/payments/**".to_string()],
            forbidden_paths: vec!["src/auth/**".to_string()],
        }
    }

    fn sample_input_content() -> InputContent {
        InputContent {
            spec_path: "specs/system/payments.md".to_string(),
            spec_sha: "abc123".to_string(),
            workspace_id: "ws-1".to_string(),
            repo_id: "repo-1".to_string(),
            persona_constraints: vec![PersonaRef {
                name: "security".to_string(),
            }],
            meta_spec_set_sha: "def456".to_string(),
            scope: sample_scope_constraint(),
        }
    }

    fn sample_output_constraint() -> OutputConstraint {
        OutputConstraint {
            name: "scope to payments".to_string(),
            expression: "output.changed_files.all(f, f.startsWith(\"src/payments/\"))".to_string(),
        }
    }

    fn sample_signed_input() -> SignedInput {
        SignedInput {
            content: sample_input_content(),
            output_constraints: vec![sample_output_constraint()],
            valid_until: 1_700_000_000,
            expected_generation: Some(1),
            signature: vec![10, 20, 30],
            key_binding: sample_key_binding(),
        }
    }

    fn sample_gate_constraint() -> GateConstraint {
        GateConstraint {
            gate_id: "gate-1".to_string(),
            gate_name: "code-review".to_string(),
            constraint: sample_output_constraint(),
            signed_by: vec![50, 60, 70],
        }
    }

    fn sample_gate_attestation() -> GateAttestation {
        GateAttestation {
            gate_id: "gate-1".to_string(),
            gate_name: "unit-tests".to_string(),
            gate_type: GateType::TestCommand,
            status: GateStatus::Passed,
            output_hash: vec![80, 90],
            constraint: Some(sample_gate_constraint()),
            signature: vec![11, 22, 33],
            key_binding: sample_key_binding(),
        }
    }

    fn sample_attestation() -> Attestation {
        Attestation {
            id: "sha256:abc".to_string(),
            input: AttestationInput::Signed(sample_signed_input()),
            output: AttestationOutput {
                content_hash: vec![1, 2, 3],
                commit_sha: "789abc".to_string(),
                agent_signature: Some(vec![44, 55]),
                gate_results: vec![sample_gate_attestation()],
            },
            metadata: AttestationMetadata {
                created_at: 1_700_000_000,
                workspace_id: "ws-1".to_string(),
                repo_id: "repo-1".to_string(),
                task_id: "TASK-007".to_string(),
                agent_id: "agent:worker-42".to_string(),
                chain_depth: 0,
            },
        }
    }

    // ── TrustAnchor ────────────────────────────────────────────────────

    #[test]
    fn trust_anchor_type_roundtrip() {
        for anchor_type in [
            TrustAnchorType::User,
            TrustAnchorType::Agent,
            TrustAnchorType::Addon,
        ] {
            let json = serde_json::to_string(&anchor_type).unwrap();
            let back: TrustAnchorType = serde_json::from_str(&json).unwrap();
            assert_eq!(back, anchor_type, "roundtrip failed for {:?}", anchor_type);
        }
    }

    #[test]
    fn trust_anchor_type_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&TrustAnchorType::User).unwrap(),
            "\"user\""
        );
        assert_eq!(
            serde_json::to_string(&TrustAnchorType::Agent).unwrap(),
            "\"agent\""
        );
        assert_eq!(
            serde_json::to_string(&TrustAnchorType::Addon).unwrap(),
            "\"addon\""
        );
    }

    #[test]
    fn trust_anchor_roundtrip() {
        let anchor = TrustAnchor {
            id: "tenant-keycloak".to_string(),
            issuer: "https://keycloak.example.com".to_string(),
            jwks_uri: "https://keycloak.example.com/.well-known/jwks.json".to_string(),
            anchor_type: TrustAnchorType::User,
            constraints: vec![sample_output_constraint()],
        };
        let json = serde_json::to_string(&anchor).unwrap();
        let back: TrustAnchor = serde_json::from_str(&json).unwrap();
        assert_eq!(back, anchor);
    }

    #[test]
    fn trust_anchor_empty_constraints_omitted() {
        let anchor = TrustAnchor {
            id: "gyre-oidc".to_string(),
            issuer: "https://gyre.example.com".to_string(),
            jwks_uri: "https://gyre.example.com/.well-known/jwks.json".to_string(),
            anchor_type: TrustAnchorType::Agent,
            constraints: vec![],
        };
        let json = serde_json::to_string(&anchor).unwrap();
        assert!(!json.contains("constraints"));
    }

    // ── ScopeConstraint ────────────────────────────────────────────────

    #[test]
    fn scope_constraint_roundtrip() {
        let scope = sample_scope_constraint();
        let json = serde_json::to_string(&scope).unwrap();
        let back: ScopeConstraint = serde_json::from_str(&json).unwrap();
        assert_eq!(back, scope);
    }

    #[test]
    fn scope_constraint_empty_paths_omitted() {
        let scope = ScopeConstraint {
            allowed_paths: vec![],
            forbidden_paths: vec![],
        };
        let json = serde_json::to_string(&scope).unwrap();
        assert!(!json.contains("allowed_paths"));
        assert!(!json.contains("forbidden_paths"));
    }

    // ── InputContent ───────────────────────────────────────────────────

    #[test]
    fn input_content_roundtrip() {
        let content = sample_input_content();
        let json = serde_json::to_string(&content).unwrap();
        let back: InputContent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, content);
    }

    // ── OutputConstraint ───────────────────────────────────────────────

    #[test]
    fn output_constraint_roundtrip() {
        let oc = sample_output_constraint();
        let json = serde_json::to_string(&oc).unwrap();
        let back: OutputConstraint = serde_json::from_str(&json).unwrap();
        assert_eq!(back, oc);
    }

    // ── SignedInput ────────────────────────────────────────────────────

    #[test]
    fn signed_input_roundtrip() {
        let si = sample_signed_input();
        let json = serde_json::to_string(&si).unwrap();
        let back: SignedInput = serde_json::from_str(&json).unwrap();
        assert_eq!(back, si);
    }

    #[test]
    fn signed_input_no_generation_roundtrip() {
        let mut si = sample_signed_input();
        si.expected_generation = None;
        let json = serde_json::to_string(&si).unwrap();
        let back: SignedInput = serde_json::from_str(&json).unwrap();
        assert_eq!(back.expected_generation, None);
    }

    // ── GateConstraint ─────────────────────────────────────────────────

    #[test]
    fn gate_constraint_roundtrip() {
        let gc = sample_gate_constraint();
        let json = serde_json::to_string(&gc).unwrap();
        let back: GateConstraint = serde_json::from_str(&json).unwrap();
        assert_eq!(back, gc);
    }

    // ── DerivedInput ───────────────────────────────────────────────────

    #[test]
    fn derived_input_roundtrip() {
        let di = DerivedInput {
            parent_ref: vec![99, 88, 77],
            preconditions: vec!["parent.status == \"passed\"".to_string()],
            update: "scope.narrow(\"src/payments/refund.rs\")".to_string(),
            output_constraints: vec![sample_output_constraint()],
            signature: vec![10, 20, 30],
            key_binding: sample_key_binding(),
        };
        let json = serde_json::to_string(&di).unwrap();
        let back: DerivedInput = serde_json::from_str(&json).unwrap();
        assert_eq!(back, di);
    }

    #[test]
    fn derived_input_empty_optional_fields_omitted() {
        let di = DerivedInput {
            parent_ref: vec![1],
            preconditions: vec![],
            update: "identity".to_string(),
            output_constraints: vec![],
            signature: vec![2],
            key_binding: sample_key_binding(),
        };
        let json = serde_json::to_string(&di).unwrap();
        assert!(!json.contains("preconditions"));
        assert!(!json.contains("output_constraints"));
    }

    // ── AttestationInput ───────────────────────────────────────────────

    #[test]
    fn attestation_input_signed_roundtrip() {
        let input = AttestationInput::Signed(sample_signed_input());
        let json = serde_json::to_string(&input).unwrap();
        let back: AttestationInput = serde_json::from_str(&json).unwrap();
        assert_eq!(back, input);
    }

    #[test]
    fn attestation_input_derived_roundtrip() {
        let input = AttestationInput::Derived(DerivedInput {
            parent_ref: vec![1, 2, 3],
            preconditions: vec![],
            update: "identity".to_string(),
            output_constraints: vec![],
            signature: vec![4, 5],
            key_binding: sample_key_binding(),
        });
        let json = serde_json::to_string(&input).unwrap();
        let back: AttestationInput = serde_json::from_str(&json).unwrap();
        assert_eq!(back, input);
    }

    #[test]
    fn attestation_input_tagged_discriminator() {
        let signed = AttestationInput::Signed(sample_signed_input());
        let json = serde_json::to_string(&signed).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "signed");

        let derived = AttestationInput::Derived(DerivedInput {
            parent_ref: vec![1],
            preconditions: vec![],
            update: "x".to_string(),
            output_constraints: vec![],
            signature: vec![2],
            key_binding: sample_key_binding(),
        });
        let json = serde_json::to_string(&derived).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "derived");
    }

    // ── AttestationOutput ──────────────────────────────────────────────

    #[test]
    fn attestation_output_roundtrip() {
        let output = AttestationOutput {
            content_hash: vec![1, 2],
            commit_sha: "abc123".to_string(),
            agent_signature: Some(vec![3, 4]),
            gate_results: vec![sample_gate_attestation()],
        };
        let json = serde_json::to_string(&output).unwrap();
        let back: AttestationOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(back, output);
    }

    #[test]
    fn attestation_output_no_signature_roundtrip() {
        let output = AttestationOutput {
            content_hash: vec![1],
            commit_sha: "def456".to_string(),
            agent_signature: None,
            gate_results: vec![],
        };
        let json = serde_json::to_string(&output).unwrap();
        let back: AttestationOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(back.agent_signature, None);
        assert!(back.gate_results.is_empty());
    }

    // ── AttestationMetadata ────────────────────────────────────────────

    #[test]
    fn attestation_metadata_roundtrip() {
        let meta = AttestationMetadata {
            created_at: 1_700_000_000,
            workspace_id: "ws-1".to_string(),
            repo_id: "repo-1".to_string(),
            task_id: "TASK-007".to_string(),
            agent_id: "agent:worker-42".to_string(),
            chain_depth: 2,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: AttestationMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(back, meta);
    }

    // ── GateAttestation ────────────────────────────────────────────────

    #[test]
    fn gate_attestation_roundtrip() {
        let ga = sample_gate_attestation();
        let json = serde_json::to_string(&ga).unwrap();
        let back: GateAttestation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ga);
    }

    #[test]
    fn gate_attestation_no_constraint_roundtrip() {
        let mut ga = sample_gate_attestation();
        ga.constraint = None;
        let json = serde_json::to_string(&ga).unwrap();
        let back: GateAttestation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.constraint, None);
    }

    #[test]
    fn gate_attestation_enum_fields_serialize_snake_case() {
        let ga = sample_gate_attestation();
        let json = serde_json::to_string(&ga).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["gate_type"], "test_command");
        assert_eq!(v["status"], "passed");
    }

    // ── Attestation (complete record) ──────────────────────────────────

    #[test]
    fn attestation_roundtrip() {
        let att = sample_attestation();
        let json = serde_json::to_string(&att).unwrap();
        let back: Attestation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, att);
    }

    #[test]
    fn attestation_with_derived_input_roundtrip() {
        let mut att = sample_attestation();
        att.input = AttestationInput::Derived(DerivedInput {
            parent_ref: vec![99, 88],
            preconditions: vec!["parent.valid".to_string()],
            update: "narrow_scope".to_string(),
            output_constraints: vec![],
            signature: vec![11],
            key_binding: sample_key_binding(),
        });
        att.metadata.chain_depth = 1;
        let json = serde_json::to_string(&att).unwrap();
        let back: Attestation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, att);
    }

    // ── VerificationResult ─────────────────────────────────────────────

    #[test]
    fn verification_result_roundtrip() {
        let vr = VerificationResult {
            label: "chain verification".to_string(),
            valid: true,
            message: "all 3 attestation nodes verified".to_string(),
            children: vec![
                VerificationResult {
                    label: "signature check".to_string(),
                    valid: true,
                    message: "Ed25519 signature valid".to_string(),
                    children: vec![],
                },
                VerificationResult {
                    label: "constraint evaluation".to_string(),
                    valid: true,
                    message: "5 constraints passed".to_string(),
                    children: vec![],
                },
            ],
        };
        let json = serde_json::to_string(&vr).unwrap();
        let back: VerificationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, vr);
    }

    #[test]
    fn verification_result_leaf_omits_children() {
        let vr = VerificationResult {
            label: "check".to_string(),
            valid: false,
            message: "failed".to_string(),
            children: vec![],
        };
        let json = serde_json::to_string(&vr).unwrap();
        assert!(!json.contains("children"));
    }

    // ── PersonaRef ─────────────────────────────────────────────────────

    #[test]
    fn persona_ref_roundtrip() {
        let pr = PersonaRef {
            name: "security".to_string(),
        };
        let json = serde_json::to_string(&pr).unwrap();
        let back: PersonaRef = serde_json::from_str(&json).unwrap();
        assert_eq!(back, pr);
    }

    #[test]
    fn persona_ref_serializes_as_object() {
        let pr = PersonaRef {
            name: "security".to_string(),
        };
        let json = serde_json::to_string(&pr).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["name"], "security");
    }

    #[test]
    fn persona_constraints_serialize_as_object_array() {
        let content = sample_input_content();
        let json = serde_json::to_string(&content).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let constraints = v["persona_constraints"].as_array().unwrap();
        assert_eq!(constraints[0]["name"], "security");
    }
}
