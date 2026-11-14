//! Authentication middleware for gyre-server.
//!
//! Auth chain (first match wins):
//! 1. Global `auth_token` -- for system/dev use, resolves as "system".
//! 2. Per-agent tokens -- issued at registration, resolves as the agent id.
//!    - UUID tokens: looked up in agent_tokens HashMap.
//!    - JWT tokens (start with "ey"): looked up in HashMap + cryptographically validated.
//! 3. API keys -- resolves as the owning user's name.
//! 4. JWT (OIDC/Keycloak) -- validates RS256 token; auto-creates user on first login.
//! 5. Federation JWT (G11) -- EdDSA JWT from a trusted remote Gyre instance.
//!    `iss` claim must match one of `GYRE_TRUSTED_ISSUERS`. JWKS fetched via OIDC
//!    discovery from the remote instance and cached for 5 minutes.
//!
//! If `jwt_config` is None the server runs without Keycloak (agent tokens only).
//! Gyre always mints its own agent JWTs via `AgentSigningKey` (EdDSA/Ed25519).

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use gyre_common::Id;
use gyre_domain::{AuditEvent, AuditEventType, AuditOutcome, User, UserRole};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::{collections::HashMap, sync::Arc};
use subtle::ConstantTimeEq;

use crate::AppState;

// -- Federation JWKS cache (G11) ----------------------------------------------

/// Cached JWKS entry for a trusted remote Gyre instance.
pub struct RemoteJwksEntry {
    pub keys: HashMap<String, jsonwebtoken::DecodingKey>,
    pub fetched_at: std::time::Instant,
}

/// TTL for cached remote JWKS (5 minutes).
const REMOTE_JWKS_TTL_SECS: u64 = 300;

// -- Gate-agent capability scopes (task-134, agent-gates.md) -------------------

/// OIDC capability granted to an AgentReview gate's reviewer agent: it may
/// read MR context and submit its verdict, nothing else (§Gate Agent
/// Lifecycle: "read-only access", "identity-scoped").
pub const REVIEW_SUBMIT_SCOPE: &str = "review:submit";

/// OIDC capability granted to an AgentValidation gate's validator agent: it
/// may read MR context to run its domain-specific check, nothing else. The
/// validation contract is exit-code pass/fail, so this capability carries no
/// review-submission rights — a validator must not be able to influence
/// review-based approval surfaces.
pub const VALIDATION_REPORT_SCOPE: &str = "validation:report";

// -- Agent JWT signing (Gyre as OIDC provider) --------------------------------

/// Claims embedded in agent JWTs minted by Gyre's built-in OIDC provider.
///
/// Hierarchy claims (platform-model.md §1 Token Scoping): every agent token
/// carries its tenant/workspace scope, an optional repo binding, and a
/// fine-grained `scope` list (e.g. `["repo:gyre-server:write"]`). The MCP
/// server validates these on every tool call.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentJwtClaims {
    /// Subject — the agent's UUID.
    pub sub: String,
    /// Issuer — the server's base URL.
    pub iss: String,
    /// Issued-at (Unix epoch seconds).
    pub iat: u64,
    /// Expiry (Unix epoch seconds).
    pub exp: u64,
    /// Fine-grained permission scopes, e.g. `["repo:gyre-server:write"]`.
    /// Workers carry repo write; orchestrators carry read+spawn for their tier.
    #[serde(default)]
    pub scope: Vec<String>,
    /// JWT ID — unique identifier for this token. Recorded in
    /// `AgentTokenRevoked` audit events when the token is revoked
    /// (observability.md §Audit Event Types). Empty on tokens minted
    /// before the claim existed.
    #[serde(default)]
    pub jti: String,
    /// Task assigned to this agent at spawn time.
    pub task_id: String,
    /// Identity that called POST /api/v1/agents/spawn.
    pub spawned_by: String,
    /// Tenant the agent's workspace belongs to (hierarchy claim, §1).
    #[serde(default)]
    pub tenant_id: String,

    // -- TASK-093: Orchestrator scope claims (platform-model.md §3) -----------
    /// Workspace the orchestrator governs. Present on orchestrator JWTs only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// Repo a repo orchestrator is bound to. Absent on workspace orchestrator
    /// JWTs (they see all repos in the workspace) and on worker JWTs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    /// Orchestrator tier: "workspace_orchestrator" or "repo_orchestrator".
    /// Absent on worker JWTs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestrator_type: Option<String>,

    /// Persona slug the agent was spawned under (e.g. "security")
    /// (platform-model.md §1 Token Scoping; identity-security.md
    /// §Task-Scoped JWT Claims). Absent when the agent was spawned
    /// without a persona assignment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    /// Workload attestation confidence level (platform-model.md §1;
    /// identity-security.md): 1=raw subprocess, 2=CLI-managed,
    /// 3=Gyre-managed container with attestation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attestation_level: Option<u32>,

    // -- G10: Workload attestation claims -------------------------------------
    /// OS PID of the agent process (workload identity, G10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wl_pid: Option<u32>,
    /// Hostname where the agent process is running (workload identity, G10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wl_hostname: Option<String>,
    /// Compute target identifier: "local", docker container ID, SSH host (G10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wl_compute_target: Option<String>,
    /// Stack fingerprint hash at spawn time (G10). The spec's `stack_hash`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wl_stack_hash: Option<String>,
    // -- M19.4: Container workload claims -------------------------------------
    /// Container ID when the agent was spawned via ContainerTarget (M19.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wl_container_id: Option<String>,
    /// Image digest (sha256) when spawned via ContainerTarget (M19.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wl_image_hash: Option<String>,
}

/// Ed25519 key pair used to mint and verify agent JWTs.
///
/// The key is persisted across server restarts (task-107 F12): local-mode
/// commit signature records survive restarts in the `CommitSignatureRepository`,
/// so the forge key that signed them must survive too, or every pre-restart
/// local-mode record would report `valid:false` (false integrity alarm). The
/// public key is exposed via `GET /.well-known/jwks.json` so external
/// verifiers can validate tokens.
pub struct AgentSigningKey {
    pub encoding_key: jsonwebtoken::EncodingKey,
    pub decoding_key: jsonwebtoken::DecodingKey,
    /// Pre-serialised JWKS JSON response body.
    pub jwks_json: String,
    /// Key ID embedded in JWT headers.
    pub kid: String,
    /// Raw PKCS#8 bytes retained for signing arbitrary data (attestation bundles).
    pkcs8_bytes: Vec<u8>,
    /// Raw 32-byte public key for external verification.
    pub public_key_bytes: Vec<u8>,
}

/// Derive a stable key id from the public key bytes. The kid must be a pure
/// function of the key material (not a random UUID) so that a key loaded
/// from persisted PKCS#8 keeps the kid it had when signature records
/// stored it in `signing_key_id` — a random kid would make every
/// load-from-persistence look like a key rotation.
fn derive_kid(public_key_bytes: &[u8]) -> String {
    use ring::digest::{digest, SHA256};
    let hash = digest(&SHA256, public_key_bytes);
    hash.as_ref()
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl AgentSigningKey {
    /// Generate a fresh Ed25519 key pair.
    pub fn generate() -> Self {
        use ring::rand::SystemRandom;
        use ring::signature::Ed25519KeyPair;

        let rng = SystemRandom::new();
        let pkcs8 =
            Ed25519KeyPair::generate_pkcs8(&rng).expect("Ed25519 key generation must succeed");
        Self::from_pkcs8(pkcs8.as_ref()).expect("freshly generated PKCS#8 must be valid")
    }

    /// Reconstruct the key pair from PKCS#8 DER bytes (e.g. loaded from the
    /// `GYRE_AGENT_SIGNING_KEY` env var or the persisted server-identity KV
    /// entry). The derived `kid` is identical to the one the same key
    /// material had when it was generated, so `signing_key_id` values in
    /// stored commit-signature records keep matching the active key.
    ///
    /// Accepts both PKCS#8 v1 and v2 documents. Every common operator tool
    /// (`openssl genpkey -algorithm ed25519`, Node's
    /// `privateKey.export({type:'pkcs8'})`, Python `cryptography`) emits v1,
    /// which has no embedded public key; ring's strict `from_pkcs8` rejects
    /// v1 with `VersionNotSupported` and ring's own documentation directs
    /// v1 callers to `from_pkcs8_maybe_unchecked`. For v1 the public key is
    /// derived from the private seed (there is no separately-supplied public
    /// key to distrust — the material is the operator's own key); v2 parsing
    /// is byte-for-byte identical to `from_pkcs8`, including the embedded
    /// public-key/seed consistency check, so ring-generated and DB-persisted
    /// keys are unaffected. `jsonwebtoken`'s EdDSA signing path uses the same
    /// `from_pkcs8_maybe_unchecked` entry point.
    pub fn from_pkcs8(pkcs8: &[u8]) -> Result<Self, String> {
        use ring::signature::Ed25519KeyPair;
        let key_pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(pkcs8)
            .map_err(|e| format!("invalid Ed25519 PKCS#8 key material: {e:?}"))?;
        use ring::signature::KeyPair as _;
        let pub_key_bytes = key_pair.public_key().as_ref();

        let kid = derive_kid(pub_key_bytes);
        let x = URL_SAFE_NO_PAD.encode(pub_key_bytes);

        let jwks_json = serde_json::json!({
            "keys": [{
                "kty": "OKP",
                "crv": "Ed25519",
                "use": "sig",
                "alg": "EdDSA",
                "kid": kid,
                "x": x
            }]
        })
        .to_string();

        let encoding_key = jsonwebtoken::EncodingKey::from_ed_der(pkcs8.as_ref());
        let decoding_key = jsonwebtoken::DecodingKey::from_ed_der(pub_key_bytes);

        Ok(Self {
            encoding_key,
            decoding_key,
            jwks_json,
            kid,
            pkcs8_bytes: pkcs8.to_vec(),
            public_key_bytes: pub_key_bytes.to_vec(),
        })
    }

    /// The PKCS#8 DER encoding of this key pair, for persistence across
    /// restarts. Callers persisting this value are responsible for storing
    /// it in server-side storage with the same protection as other forge
    /// secret material.
    pub fn pkcs8_der(&self) -> &[u8] {
        &self.pkcs8_bytes
    }

    /// Sign arbitrary bytes with the Ed25519 private key.
    ///
    /// Used by the attestation module to sign merge attestation bundles.
    pub fn sign_bytes(&self, data: &[u8]) -> Vec<u8> {
        use ring::signature::Ed25519KeyPair;
        // Same v1-or-v2 tolerance as `from_pkcs8`: `pkcs8_bytes` may hold an
        // operator-provisioned v1 document (see `GYRE_AGENT_SIGNING_KEY`).
        let key_pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&self.pkcs8_bytes)
            .expect("stored pkcs8 bytes must remain valid");
        key_pair.sign(data).as_ref().to_vec()
    }

    /// Mint a signed EdDSA JWT for an agent.
    ///
    /// Scope: fine-grained `repo:{name}:write` (platform-model.md §1 Token
    /// Scoping — a worker agent's token is scoped to its repo).
    #[allow(clippy::too_many_arguments)]
    pub fn mint(
        &self,
        agent_id: &str,
        task_id: &str,
        spawned_by: &str,
        issuer: &str,
        ttl_secs: u64,
        tenant_id: &str,
        workspace_id: &str,
        repo_id: &str,
        repo_name: &str,
    ) -> Result<String, String> {
        self.mint_with_workload(
            agent_id,
            task_id,
            spawned_by,
            issuer,
            ttl_secs,
            tenant_id,
            workspace_id,
            repo_id,
            repo_name,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    /// Validate a self-issued agent JWT, returning its claims on success.
    pub fn validate(&self, token: &str, expected_issuer: &str) -> Result<AgentJwtClaims, String> {
        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::EdDSA);
        validation.set_issuer(&[expected_issuer]);
        validation.validate_aud = false;
        jsonwebtoken::decode::<AgentJwtClaims>(token, &self.decoding_key, &validation)
            .map(|td| td.claims)
            .map_err(|e| format!("agent JWT validation: {e}"))
    }

    /// Mint a signed EdDSA JWT with embedded G10 workload attestation claims.
    /// Optionally includes M19.4 container identity claims.
    ///
    /// §1 Token Scoping: populates the hierarchy claims (tenant_id,
    /// workspace_id, repo_id) and the fine-grained scope list
    /// (`repo:{repo_name}:write` for workers) from spawn context.
    #[allow(clippy::too_many_arguments)]
    pub fn mint_with_workload(
        &self,
        agent_id: &str,
        task_id: &str,
        spawned_by: &str,
        issuer: &str,
        ttl_secs: u64,
        tenant_id: &str,
        workspace_id: &str,
        repo_id: &str,
        repo_name: &str,
        persona: Option<&str>,
        attestation_level: Option<u32>,
        wl_pid: Option<u32>,
        wl_hostname: Option<String>,
        wl_compute_target: Option<String>,
        wl_stack_hash: Option<String>,
        wl_container_id: Option<String>,
        wl_image_hash: Option<String>,
    ) -> Result<String, String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let claims = AgentJwtClaims {
            sub: agent_id.to_string(),
            iss: issuer.to_string(),
            iat: now,
            exp: now + ttl_secs,
            scope: vec![format!("repo:{repo_name}:write")],
            jti: uuid::Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            spawned_by: spawned_by.to_string(),
            tenant_id: tenant_id.to_string(),
            workspace_id: Some(workspace_id.to_string()),
            repo_id: Some(repo_id.to_string()),
            orchestrator_type: None,
            persona: persona.map(|p| p.to_string()),
            attestation_level,
            wl_pid,
            wl_hostname,
            wl_compute_target,
            wl_stack_hash,
            wl_container_id,
            wl_image_hash,
        };
        self.encode(claims)
    }

    /// Extract the `jti` (JWT ID) claim from an agent token without verifying
    /// the signature or expiry.
    ///
    /// crypto-verify:ok — audit-detail extraction only, never an authentication
    /// decision. Callers pass tokens read from the server's own `agent_tokens`
    /// KV bucket (minted by this key), never caller-supplied input; the value
    /// feeds the `jti` field of an `AgentTokenRevoked` audit event. All auth
    /// paths use `validate()` (full EdDSA signature verification) instead.
    pub fn extract_jti_unverified(&self, token: &str) -> Option<String> {
        // JWT format: header.payload.signature (base64url, no padding).
        let payload_b64 = token.split('.').nth(1)?;
        let bytes = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
        let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        let jti = claims.get("jti")?.as_str()?;
        if jti.is_empty() {
            None
        } else {
            Some(jti.to_string())
        }
    }

    /// Mint an orchestrator JWT with scope claims (platform-model.md §3).
    ///
    /// Workspace orchestrators carry `workspace_id` only (they see all repos
    /// in the workspace); repo orchestrators carry both `workspace_id` and
    /// `repo_id`. `task_id` carries the orchestrator's own agent id.
    /// Orchestrators are not bound to a task, but the claim is required on
    /// all agent JWTs for `gyre_agent_complete` compatibility.
    ///
    /// §1 Token Scoping: workspace orchestrators are scoped to their
    /// workspace (`workspace:{id}:read` + `workspace:{id}:spawn`); repo
    /// orchestrators to their repo (`repo:{name}:write` + `repo:{name}:spawn`).
    #[allow(clippy::too_many_arguments)]
    pub fn mint_orchestrator(
        &self,
        agent_id: &str,
        spawned_by: &str,
        issuer: &str,
        ttl_secs: u64,
        tenant_id: &str,
        workspace_id: &str,
        repo_id: Option<&str>,
        repo_name: Option<&str>,
        orchestrator_type: &str,
        persona: &str,
    ) -> Result<String, String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let scope = if orchestrator_type == "workspace_orchestrator" {
            vec![
                format!("workspace:{workspace_id}:read"),
                format!("workspace:{workspace_id}:spawn"),
            ]
        } else {
            // Repo orchestrator — repo_name is required to mint its scope.
            let name = repo_name.ok_or_else(|| {
                "repo orchestrator JWT requires repo_name for repo:... scope".to_string()
            })?;
            vec![format!("repo:{name}:write"), format!("repo:{name}:spawn")]
        };
        let claims = AgentJwtClaims {
            sub: agent_id.to_string(),
            iss: issuer.to_string(),
            iat: now,
            exp: now + ttl_secs,
            scope,
            jti: uuid::Uuid::new_v4().to_string(),
            task_id: agent_id.to_string(),
            spawned_by: spawned_by.to_string(),
            tenant_id: tenant_id.to_string(),
            workspace_id: Some(workspace_id.to_string()),
            repo_id: repo_id.map(|r| r.to_string()),
            orchestrator_type: Some(orchestrator_type.to_string()),
            persona: Some(persona.to_string()),
            attestation_level: None,
            wl_pid: None,
            wl_hostname: None,
            wl_compute_target: None,
            wl_stack_hash: None,
            wl_container_id: None,
            wl_image_hash: None,
        };
        self.encode(claims)
    }

    /// Encode claims into a signed EdDSA JWT.
    fn encode(&self, claims: AgentJwtClaims) -> Result<String, String> {
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::EdDSA);
        header.kid = Some(self.kid.clone());
        jsonwebtoken::encode(&header, &claims, &self.encoding_key)
            .map_err(|e| format!("JWT mint error: {e}"))
    }

    /// Mint a worker JWT scoped to the repo it was spawned against.
    ///
    /// Workers always carry `workspace_id` + `repo_id` claims so ABAC subject
    /// attributes (`subject.repo_scope`, §Attributes "Agent OIDC claim") are
    /// real for every request the agent makes. Unlike orchestrator JWTs there
    /// is no `orchestrator_type` claim. `wl_image_hash` carries the resolved
    /// image digest (agent-runtime.md §3, best-effort: `None` when the image
    /// runtime could not resolve it).
    ///
    /// §1 Token Scoping: the worker's fine-grained scope list
    /// (`["repo:{repo_name}:write"]`), `tenant_id`, and the hierarchy claims
    /// are minted from spawn context via [`AgentSigningKey::mint_with_workload`]
    /// — a worker agent's token is scoped to its repo.
    #[allow(clippy::too_many_arguments)]
    pub fn mint_worker_scoped(
        &self,
        agent_id: &str,
        task_id: &str,
        spawned_by: &str,
        issuer: &str,
        ttl_secs: u64,
        tenant_id: &str,
        workspace_id: &str,
        repo_id: &str,
        repo_name: &str,
        persona: Option<&str>,
        attestation_level: Option<u8>,
        wl_image_hash: Option<String>,
    ) -> Result<String, String> {
        self.mint_with_workload(
            agent_id,
            task_id,
            spawned_by,
            issuer,
            ttl_secs,
            tenant_id,
            workspace_id,
            repo_id,
            repo_name,
            persona,
            // identity-security.md attestation levels are 1-3; the §1 claim
            // is the widened `u32` form (platform-model.md §1 Token Scoping).
            attestation_level.map(|level| level as u32),
            None,
            None,
            None,
            None,
            None,
            wl_image_hash,
        )
    }

    /// Mint a scoped agent JWT (task-134 gate-agent protocol).
    ///
    /// Gate agents are ephemeral, single-purpose identities. An AgentReview
    /// gate's reviewer carries [`REVIEW_SUBMIT_SCOPE`] (submit verdict, read
    /// MR context); an AgentValidation gate's validator carries
    /// [`VALIDATION_REPORT_SCOPE`] (read MR context, no submission — its
    /// contract is the process exit code). The capability string is enforced
    /// exactly (no substring matching) by `git_http` (push denial), the ABAC
    /// middleware (route allow-list), and `submit_review` (reviewer identity
    /// binding). `task_id` carries the gate id so the token is traceable to
    /// the gate run that minted it.
    ///
    /// §1 Token Scoping: the capability string is stored as the token's
    /// fine-grained scope list (`scope: Vec<String>`), so every downstream
    /// scope check reads the array form. `tenant_id` is the tenant of the
    /// gated MR's workspace, resolved fail-closed by the caller — never a
    /// fabricated literal. `persona` is the gate persona's slug.
    #[allow(clippy::too_many_arguments)]
    pub fn mint_scoped(
        &self,
        agent_id: &str,
        task_id: &str,
        spawned_by: &str,
        issuer: &str,
        ttl_secs: u64,
        scope: &str,
        tenant_id: &str,
        persona: Option<&str>,
    ) -> Result<String, String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let claims = AgentJwtClaims {
            sub: agent_id.to_string(),
            iss: issuer.to_string(),
            iat: now,
            exp: now + ttl_secs,
            scope: vec![scope.to_string()],
            jti: uuid::Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            spawned_by: spawned_by.to_string(),
            tenant_id: tenant_id.to_string(),
            workspace_id: None,
            repo_id: None,
            orchestrator_type: None,
            persona: persona.map(|p| p.to_string()),
            attestation_level: None,
            wl_pid: None,
            wl_hostname: None,
            wl_compute_target: None,
            wl_stack_hash: None,
            wl_container_id: None,
            wl_image_hash: None,
        };
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::EdDSA);
        header.kid = Some(self.kid.clone());
        jsonwebtoken::encode(&header, &claims, &self.encoding_key)
            .map_err(|e| format!("JWT mint error: {e}"))
    }
    /// Mint an impersonation JWT (identity-security.md §Impersonation).
    ///
    /// `sub` is the **target** user (so permissions match theirs) while
    /// `impersonator_id` always names the acting admin. Short TTL enforced
    /// at validation time with zero leeway (both mint and validate use the
    /// same server clock, so no skew allowance is needed).
    pub fn mint_impersonation(
        &self,
        target_user_id: &str,
        impersonator_id: &str,
        session_id: &str,
        tenant_id: &str,
        issuer: &str,
        ttl_secs: u64,
    ) -> Result<String, String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let claims = ImpersonationJwtClaims {
            sub: target_user_id.to_string(),
            iss: issuer.to_string(),
            iat: now,
            exp: now + ttl_secs,
            scope: "impersonation".to_string(),
            impersonating: true,
            impersonator_id: impersonator_id.to_string(),
            target_user_id: target_user_id.to_string(),
            session_id: session_id.to_string(),
            tenant_id: tenant_id.to_string(),
        };
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::EdDSA);
        header.kid = Some(self.kid.clone());
        jsonwebtoken::encode(&header, &claims, &self.encoding_key)
            .map_err(|e| format!("JWT mint error: {e}"))
    }

    /// Validate an impersonation JWT, returning its claims on success.
    ///
    /// Zero leeway on `exp`: the TTL is the hard impersonation budget.
    /// Returns `Err` for any token that is not a valid impersonation JWT
    /// (including agent JWTs, which lack the `impersonating` claim —
    /// callers use the claim to decide whether the token applies to this
    /// path at all; see `validate_impersonation_opt`).
    pub fn validate_impersonation(
        &self,
        token: &str,
        expected_issuer: &str,
    ) -> Result<ImpersonationJwtClaims, String> {
        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::EdDSA);
        validation.set_issuer(&[expected_issuer]);
        validation.validate_aud = false;
        // Self-minted token, same clock: expiry is exact, no skew allowance.
        validation.leeway = 0;
        jsonwebtoken::decode::<ImpersonationJwtClaims>(token, &self.decoding_key, &validation)
            .map(|td| td.claims)
            .map_err(|e| format!("impersonation JWT validation: {e}"))
    }

    /// Validate a token as an impersonation JWT, returning `None` when the
    /// token is well-formed and signature-valid but is NOT an impersonation
    /// token (no `impersonating: true` claim) so the caller can fall through
    /// to the next link of the auth chain.
    pub fn validate_impersonation_opt(
        &self,
        token: &str,
        expected_issuer: &str,
    ) -> Option<ImpersonationJwtClaims> {
        let claims = self.validate_impersonation(token, expected_issuer).ok()?;
        if claims.impersonating {
            Some(claims)
        } else {
            None
        }
    }
}

/// Claims embedded in impersonation JWTs minted by Gyre
/// (identity-security.md §Impersonation).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImpersonationJwtClaims {
    /// Subject — the **target** (impersonated) user's id. Permissions are
    /// resolved against the target; attribution always uses
    /// `impersonator_id`.
    pub sub: String,
    /// Issuer — the server's base URL.
    pub iss: String,
    /// Issued-at (Unix epoch seconds).
    pub iat: u64,
    /// Expiry (Unix epoch seconds). Short-lived by design.
    pub exp: u64,
    /// Always "impersonation" for impersonation JWTs.
    pub scope: String,
    /// Always true on impersonation JWTs; distinguishes them from agent JWTs.
    pub impersonating: bool,
    /// User id of the admin acting as the target.
    pub impersonator_id: String,
    /// User id of the impersonated user (mirrors `sub`).
    pub target_user_id: String,
    /// Impersonation session id — the revocation key. Ending the session
    /// invalidates the token before `exp`.
    pub session_id: String,
    /// Tenant scope resolved at approval time.
    pub tenant_id: String,
}

// -- Security helpers ---------------------------------------------------------

/// Compare two tokens in constant time to prevent timing attacks.
pub(crate) fn tokens_equal(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.as_bytes().ct_eq(b.as_bytes()).into()
}

/// Hash an API key with SHA-256 before storage/lookup.
/// Prevents leaking raw keys if the database is compromised.
pub(crate) fn hash_api_key(key: &str) -> String {
    let result = sha2::Sha256::digest(key.as_bytes());
    result.iter().map(|b| format!("{b:02x}")).collect()
}

/// Impersonation context attached to an `AuthenticatedAgent` resolved from
/// an impersonation JWT (identity-security.md §Impersonation).
///
/// The principal (`agent_id` / `user_id` / `roles`) is the **target** user so
/// permissions match theirs, while `impersonator_id` names the admin who is
/// actually acting. Every audit record written for the request must be
/// attributed to the impersonator.
#[derive(Debug, Clone)]
pub struct ImpersonationContext {
    /// User id of the admin who requested the impersonation.
    pub impersonator_id: Id,
    /// Display name of the impersonator (for audit attribution strings).
    pub impersonator_name: String,
    /// User id of the impersonated (target) user — equals `user_id`.
    pub target_user_id: Id,
    /// Impersonation session id (kv `impersonation_sessions` key).
    pub session_id: String,
}

/// Tenant scope for server-originated principals: the global token, spawned
/// agent tokens, and federated agents.
///
/// This is the DOCUMENTED contract, not a fabricated fallback:
/// - hierarchy-enforcement.md §1 "Bootstrap Behavior": on first startup the
///   server provisions a default tenant, and legacy `tenant_id DEFAULT
///   'default'` columns reference it.
/// - message-bus.md "Origin and tenant resolution from auth context": the
///   global `GYRE_AUTH_TOKEN` resolves as the default (bootstrap) tenant.
///
/// Callers that can resolve a real scope from stored state (e.g. API-key
/// users via `user.tenant_id`, workspaces via `ws.tenant_id`) MUST do so
/// and must fail closed when it is absent.
pub(crate) fn system_principal_tenant() -> String {
    "default".to_string()
}

/// Resolved principal injected by the auth extractor.
#[derive(Clone)]
pub struct AuthenticatedAgent {
    pub agent_id: String,
    /// Present when auth was performed via JWT or API key.
    pub user_id: Option<Id>,
    pub roles: Vec<UserRole>,
    /// Tenant scope for this request.
    /// - JWT auth: extracted from the validated `tenant_id` claim.
    /// - Agent/federated JWT with §1 hierarchy claims: mirrored from the
    ///   signature-verified claims by `with_hierarchy_from_claims`.
    /// - API-key auth: the user's stored tenant binding (fail-closed when
    ///   the user has none — task-099 F1).
    /// - Global token, agent tokens, federated JWTs: the server-originated
    ///   default tenant (`system_principal_tenant`).
    pub tenant_id: String,
    /// Workspace scope derived from the validated `workspace_id` claim
    /// (platform-model.md §1 Token Scoping). `None` for callers whose token
    /// carries no hierarchy claims (global token, API key, legacy UUID agent
    /// token) — available to all downstream handlers.
    pub workspace_id: Option<String>,
    /// Repo scope derived from the validated `repo_id` claim. `None` for
    /// workspace orchestrators (scoped to the whole workspace) and for
    /// callers without hierarchy claims.
    pub repo_id: Option<String>,
    /// Fine-grained scopes from the validated `scope` claim, e.g.
    /// `["repo:gyre-server:write"]`. Empty for callers without hierarchy
    /// claims (they are gated by ABAC/RBAC instead of token scope).
    pub scope: Vec<String>,
    /// Raw JWT claims for ABAC evaluation (G6).
    /// - Global token or API key: `None` — ABAC checks are bypassed for these.
    /// - JWT auth (Keycloak or agent JWT): populated with the full claims object.
    pub jwt_claims: Option<serde_json::Value>,
    /// The raw bearer token presented by the caller, when auth came from the
    /// Authorization header. Needed at signing boundaries (task-107): the
    /// Fulcio flow presents the caller's own validated JWT to the CA.
    pub bearer_token: Option<String>,
    /// True when auth was performed via the deprecated `?token=` query parameter.
    /// Used by WebSocket handlers to send a deprecation warning to the client.
    pub deprecated_token_auth: bool,
    /// Present when the caller authenticated with an impersonation JWT:
    /// the principal is the target user, this field names the acting admin.
    /// Identity-security.md §Impersonation requires every action taken with
    /// this token to be attributed to the impersonator in the audit trail.
    pub impersonation: Option<ImpersonationContext>,
}

// -- Auth audit events (task-104: observability.md §Audit Event Types) --------

/// Request-extension marker deduplicating AuthSuccess emission.
///
/// A single request may validate the bearer token multiple times (ABAC
/// middleware extractor + handler extractor); the token was *accepted* once.
/// The first validation records the event and inserts this marker into the
/// request extensions so subsequent extractions in the same request skip
/// emission.
#[derive(Clone, Copy)]
struct AuthAuditMark;

/// Record an `AuthSuccess` audit event (observability.md §Audit Event Types:
/// valid token accepted). `token_kind` is one of global / agent_jwt /
/// uuid_token / api_key / user_jwt / federated_jwt.
async fn record_auth_success(
    state: &AppState,
    token_kind: &str,
    agent_id: Option<&str>,
    user_id: Option<&Id>,
    source_ip: Option<String>,
    user_agent: Option<String>,
) {
    let agent = agent_id
        .filter(|s| !s.is_empty())
        .map(|s| Id::new(s.to_string()));
    let event = AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        AuditEventType::AuthSuccess,
        agent,
        user_id.cloned(),
        None,
        None,
        None,
        "auth".to_string(),
        None,
        AuditOutcome::Success,
        serde_json::json!({
            "token_kind": token_kind,
            "agent_id": agent_id,
        }),
        source_ip,
        user_agent,
        crate::api::now_secs(),
    );
    crate::api::audit::emit_server_audit_event(state, event).await;
}

/// Record an `AuthFailure` audit event (observability.md §Audit Event Types:
/// invalid or revoked token). `token_kind` names the rejected credential
/// kind when it can be determined ("unknown" otherwise).
async fn record_auth_failure(
    state: &AppState,
    token_kind: &str,
    rejection_reason: &str,
    source_ip: Option<String>,
    user_agent: Option<String>,
) {
    let event = AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        AuditEventType::AuthFailure,
        None,
        None,
        None,
        None,
        None,
        "auth".to_string(),
        None,
        AuditOutcome::Failure,
        serde_json::json!({
            "token_kind": token_kind,
            "rejection_reason": rejection_reason,
        }),
        source_ip,
        user_agent,
        crate::api::now_secs(),
    );
    crate::api::audit::emit_server_audit_event(state, event).await;
}

/// Best-effort forensic attributes for auth events: the socket peer address
/// (CWE-348 — never a caller-controlled forwarded header) and User-Agent.
fn request_forensic_parts(parts: &Parts) -> (Option<String>, Option<String>) {
    let source_ip = parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|c| c.0.ip().to_string());
    let user_agent = parts
        .headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    (source_ip, user_agent)
}

/// Record an `AuthFailure` audit event on behalf of the global auth
/// middleware (`require_auth_middleware` in lib.rs), which rejects tokens
/// before any extractor — and thus before the extractor's own emission
/// points — runs.
pub(crate) async fn record_auth_failure_for_middleware(
    state: &AppState,
    token_kind: &str,
    rejection_reason: &str,
    source_ip: Option<String>,
    user_agent: Option<String>,
) {
    record_auth_failure(state, token_kind, rejection_reason, source_ip, user_agent).await;
}

impl AuthenticatedAgent {
    /// Derive the §1 Token Scoping hierarchy fields from validated JWT
    /// claims. Called on every path that populates `jwt_claims` so
    /// `tenant_id`/`workspace_id`/`repo_id`/`scope` always mirror the claims
    /// the signature actually verified.
    ///
    /// `tenant_id` is mirrored too (task-130 review 29e132008c6748508f2c52ec982d78d6):
    /// tenant-containment handlers (spawn_repo_orchestrator_core,
    /// spawn_workspace_orchestrator_core, explorer/git_http scope checks)
    /// compare the caller's tenant against the target entity's real tenant,
    /// and a fabricated extractor "default" identity denied correctly
    /// minted scoped tokens their own happy path. The claim is only honored
    /// when present — tokens without a tenant claim keep the
    /// extractor-resolved identity.
    fn with_hierarchy_from_claims(mut self, claims: &serde_json::Value) -> Self {
        if let Some(tenant) = claims.get("tenant_id").and_then(|v| v.as_str()) {
            self.tenant_id = tenant.to_string();
        }
        self.workspace_id = claims
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        self.repo_id = claims
            .get("repo_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        self.scope = claims
            .get("scope")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        self
    }

    /// True only for the genuine system principal: the global dev/system
    /// token. NOT a plain `agent_id == "system"` string match — API-key and
    /// Keycloak-JWT paths set `agent_id` from `user.display_name` /
    /// `preferred_username`, so a user *named* "system" would spoof every
    /// call site that gates on that string (abac_middleware's ABAC bypass,
    /// the task-099 tenant-containment gates). The system principal has no
    /// backing user row: `user_id.is_none()` is the discriminator an
    /// attacker cannot forge (their key/JWT resolves to their own user id).
    pub fn is_system_principal(&self) -> bool {
        self.agent_id == "system" && self.user_id.is_none()
    }
}

// -- JWT claim types ----------------------------------------------------------

#[derive(Debug, Deserialize)]
struct JwtClaims {
    sub: String,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    realm_access: Option<RealmAccess>,
    /// Tenant scope claim. An IdP that does not issue tenant claims is a
    /// single-tenant deployment whose callers belong to the bootstrapped
    /// "default" tenant (hierarchy-enforcement.md §1) — expressed as the
    /// claim's schema default (applied at deserialization), not as a
    /// use-site fallback.
    #[serde(default = "default_tenant_claim")]
    tenant_id: String,
    /// Email verification flag (OIDC standard). Required to claim an
    /// invitation-created account by email — see find_or_create_user_tenant.
    #[serde(default)]
    email_verified: Option<bool>,
}

/// Serde default for [`JwtClaims::tenant_id`]: an absent tenant claim maps
/// to the bootstrapped "default" tenant — the documented single-tenant IdP
/// contract ("Absent = \"default\""). This is a deterministic function of
/// the cryptographically verified token, not a fabricated scope identity on
/// a lookup failure: there is no fallible lookup between the verified claims
/// and this value, and "default" is a real Tenant row materialized at
/// startup (bootstrap_default_tenant in lib.rs).
fn default_tenant_claim() -> String {
    "default".to_string()
}

#[derive(Debug, Deserialize)]
struct RealmAccess {
    #[serde(default)]
    roles: Vec<String>,
}

/// Validate a tenant_id value extracted from a JWT claim.
///
/// Rules:
/// - Must be 1-64 ASCII alphanumeric characters, hyphens, or underscores.
/// - The value "system" is reserved for Admin-role callers only.
fn validate_tenant_id(tenant_id: &str, roles: &[UserRole]) -> Result<(), String> {
    if tenant_id.is_empty() || tenant_id.len() > 64 {
        return Err(format!(
            "tenant_id length must be 1-64 chars, got {}",
            tenant_id.len()
        ));
    }
    if !tenant_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "tenant_id '{tenant_id}' contains invalid characters (allowed: a-z A-Z 0-9 - _)"
        ));
    }
    // "system" is a reserved tenant -- only Admin callers may use it.
    if tenant_id == "system" && !roles.contains(&UserRole::Admin) {
        return Err("tenant 'system' requires Admin role".to_string());
    }
    Ok(())
}

fn roles_from_claims(claims: &JwtClaims) -> Vec<UserRole> {
    let raw = claims
        .realm_access
        .as_ref()
        .map(|ra| ra.roles.as_slice())
        .unwrap_or(&[]);
    raw.iter().filter_map(|s| UserRole::from_str(s)).collect()
}

// -- JWKS refresh -------------------------------------------------------------

#[derive(Deserialize)]
struct OidcDiscovery {
    jwks_uri: String,
}

#[derive(Deserialize)]
struct JwkSet {
    keys: Vec<serde_json::Value>,
}

/// Fetch JWKS from the OIDC discovery endpoint and populate the key cache.
async fn refresh_jwks(
    issuer: &str,
    http_client: &reqwest::Client,
    cache: &std::sync::RwLock<HashMap<String, jsonwebtoken::DecodingKey>>,
) {
    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let discovery: OidcDiscovery = match http_client.get(&discovery_url).send().await {
        Ok(r) => match r.json().await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("OIDC discovery parse error: {e}");
                return;
            }
        },
        Err(e) => {
            tracing::warn!("OIDC discovery fetch error: {e}");
            return;
        }
    };

    let jwk_set: JwkSet = match http_client.get(&discovery.jwks_uri).send().await {
        Ok(r) => match r.json().await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("JWKS parse error: {e}");
                return;
            }
        },
        Err(e) => {
            tracing::warn!("JWKS fetch error: {e}");
            return;
        }
    };

    let mut new_keys: HashMap<String, jsonwebtoken::DecodingKey> = HashMap::new();
    for jwk in &jwk_set.keys {
        let kid = match jwk.get("kid").and_then(|v| v.as_str()) {
            Some(k) => k.to_string(),
            None => continue,
        };
        let jwk_typed: jsonwebtoken::jwk::Jwk = match serde_json::from_value(jwk.clone()) {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!("Failed to parse JWK {kid}: {e}");
                continue;
            }
        };
        match jsonwebtoken::DecodingKey::from_jwk(&jwk_typed) {
            Ok(dk) => {
                new_keys.insert(kid, dk);
            }
            Err(e) => tracing::warn!("Failed to build DecodingKey for {kid}: {e}"),
        }
    }

    if !new_keys.is_empty() {
        if let Ok(mut cache_guard) = cache.write() {
            cache_guard.extend(new_keys);
        }
    }
}

// -- Auth extractor -----------------------------------------------------------

/// Identity-security.md §Impersonation: attribute every authenticated call
/// made under an impersonation token at the auth-resolution layer. This is
/// the shared choke point for surfaces that authenticate via the
/// `AuthenticatedAgent` extractor WITHOUT the ABAC middleware (git smart
/// HTTP, MCP, conversations, explorer, ws-ticket) — the middleware-covered
/// `/api/v1` routes are recorded by `abac_middleware` instead (with the
/// policy outcome for evaluated routes), and mark the request first so
/// this never double-counts the same call.
fn record_impersonated_extraction(
    parts: &mut Parts,
    state: &Arc<AppState>,
    auth: &AuthenticatedAgent,
    surface: &'static str,
) {
    use crate::impersonation::ImpersonationActionRecorded;
    if parts
        .extensions
        .get::<ImpersonationActionRecorded>()
        .is_some()
    {
        return; // already attributed for this request
    }
    let Some(imp) = auth.impersonation.clone() else {
        return;
    };
    parts.extensions.insert(ImpersonationActionRecorded);
    crate::impersonation::spawn_record_impersonated_action(
        state,
        &imp,
        parts.method.as_str(),
        parts.uri.path(),
        true,
        surface,
    );
}

#[axum::async_trait]
impl FromRequestParts<Arc<AppState>> for AuthenticatedAgent {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // 0. Check for single-use WS ticket first (?ticket= query param).
        //    Tickets are short-lived (30s), single-use, and don't expose the
        //    real auth token in URLs/logs/browser history.
        if let Some(ticket) = parts.uri.query().and_then(|q| {
            q.split('&')
                .find(|p| p.starts_with("ticket="))
                .map(|p| &p[7..])
        }) {
            if let Some(auth) = state.ws_tickets.consume(ticket) {
                record_impersonated_extraction(parts, state, &auth, "ws-ticket");
                return Ok(auth);
            }
            return Err((StatusCode::UNAUTHORIZED, "Invalid or expired ticket").into_response());
        }

        // Check Authorization header first, then fall back to ?token= query param.
        // DEPRECATED: ?token= leaks credentials to logs/history. Use ?ticket= instead.
        let header_token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        let query_token = parts.uri.query().and_then(|q| {
            q.split('&')
                .find(|p| p.starts_with("token="))
                .map(|p| &p[6..])
        });
        let deprecated_token_auth = header_token.is_none() && query_token.is_some();
        if deprecated_token_auth {
            tracing::warn!(
                uri = %parts.uri,
                "Deprecated ?token= query parameter auth used. \
                 Migrate to ticket-based auth (POST /api/v1/ws-ticket) \
                 to avoid token leakage in logs and browser history."
            );
        }
        let bearer_token = header_token.map(|t| t.to_string());
        let token = header_token
            .or(query_token)
            .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Missing Bearer token").into_response())?;

        // task-104: audit AuthSuccess exactly once per request even when the
        // token is validated by both the ABAC middleware and the handler
        // extractor. The marker travels in the request extensions.
        let auth_already_audited = parts.extensions.get::<AuthAuditMark>().is_some();
        if !auth_already_audited {
            parts.extensions.insert(AuthAuditMark);
        }
        let (forensic_ip, forensic_ua) = request_forensic_parts(parts);

        // 1. Global auth token (dev / system usage). Use constant-time compare.
        if tokens_equal(token, &state.auth_token) {
            if !auth_already_audited {
                record_auth_success(
                    state,
                    "global",
                    Some("system"),
                    None,
                    forensic_ip,
                    forensic_ua,
                )
                .await;
            }
            return Ok(AuthenticatedAgent {
                agent_id: "system".to_string(),
                user_id: None,
                roles: vec![UserRole::Admin],
                tenant_id: system_principal_tenant(),
                workspace_id: None,
                repo_id: None,
                scope: Vec::new(),
                jwt_claims: None, // Admin bypass — no ABAC evaluation.
                bearer_token,
                deprecated_token_auth,
                impersonation: None,
            });
        }

        // 1.5. Impersonation JWT (identity-security.md §Impersonation).
        //     Cryptographically validated EdDSA token minted at approval
        //     time; the session record must still be active, so ending a
        //     session revokes its token before `exp`. Non-impersonation
        //     tokens fall through to the next link of the chain.
        if token.starts_with("ey") {
            if let Some(claims) = state
                .agent_signing_key
                .validate_impersonation_opt(token, &state.base_url)
            {
                return match crate::impersonation::resolve_impersonation_auth(&claims, state).await
                {
                    Some(mut auth) => {
                        auth.deprecated_token_auth = deprecated_token_auth;
                        record_impersonated_extraction(parts, state, &auth, "per-handler");
                        Ok(auth)
                    }
                    None => Err((
                        StatusCode::UNAUTHORIZED,
                        "Impersonation session is not active",
                    )
                        .into_response()),
                };
            }
        }

        // 2. Per-agent tokens issued at spawn (UUID legacy or JWT).
        {
            let token_pairs = state
                .kv_store
                .kv_list("agent_tokens")
                .await
                .unwrap_or_default();
            if let Some(agent_id) = token_pairs
                .iter()
                .find(|(_, t)| t.as_str() == token)
                .map(|(id, _)| id.clone())
            {
                // JWT tokens: additionally validate signature and expiry.
                let jwt_claims = if token.starts_with("ey") {
                    match state.agent_signing_key.validate(token, &state.base_url) {
                        Ok(agent_claims) => serde_json::to_value(&agent_claims).ok(),
                        Err(e) => {
                            tracing::debug!("Agent JWT validation failed: {e}");
                            record_auth_failure(
                                state,
                                "agent_jwt",
                                "invalid or expired agent token",
                                forensic_ip,
                                forensic_ua,
                            )
                            .await;
                            return Err((
                                StatusCode::UNAUTHORIZED,
                                "Invalid or expired agent token",
                            )
                                .into_response());
                        }
                    }
                } else {
                    None // Legacy UUID token — no JWT claims for ABAC.
                };
                let token_kind = if token.starts_with("ey") {
                    "agent_jwt"
                } else {
                    "uuid_token"
                };
                if !auth_already_audited {
                    record_auth_success(
                        state,
                        token_kind,
                        Some(agent_id.as_str()),
                        None,
                        forensic_ip,
                        forensic_ua,
                    )
                    .await;
                }
                let auth = AuthenticatedAgent {
                    agent_id,
                    user_id: None,
                    roles: vec![UserRole::Agent],
                    tenant_id: system_principal_tenant(),
                    workspace_id: None,
                    repo_id: None,
                    scope: Vec::new(),
                    jwt_claims: jwt_claims.clone(),
                    bearer_token,
                    deprecated_token_auth,
                    impersonation: None,
                };
                // §1 Token Scoping: mirror the hierarchy claims the signature
                // verified onto the resolved principal.
                let auth = match jwt_claims.as_ref() {
                    Some(claims) => auth.with_hierarchy_from_claims(claims),
                    None => auth,
                };
                return Ok(auth);
            }
        }

        // 2.5. JWT token not found in agent_tokens — treat as revoked or unknown.
        //      Do not fall through to API-key or Keycloak for "ey" tokens,
        //      as that would allow a compromised agent JWT to escalate.
        if token.starts_with("ey") {
            // Attempt cryptographic validation to give a better error message.
            match state.agent_signing_key.validate(token, &state.base_url) {
                Ok(_) => {
                    // Valid signature but not in HashMap — token was revoked.
                    record_auth_failure(
                        state,
                        "agent_jwt",
                        "agent token has been revoked",
                        forensic_ip,
                        forensic_ua,
                    )
                    .await;
                    return Err(
                        (StatusCode::UNAUTHORIZED, "Agent token has been revoked").into_response()
                    );
                }
                Err(_) => {
                    // Invalid signature — fall through to Keycloak JWT path.
                    // (Could be a legitimate Keycloak JWT that starts with "ey".)
                }
            }
        }

        // 3. API keys -- look up by SHA-256 hash of the raw token.
        if let Ok(Some(user_id)) = state.api_keys.find_user_id(&hash_api_key(token)).await {
            if let Ok(Some(user)) = state.users.find_by_id(&user_id).await {
                // F1: the tenant scope comes from the user's stored binding.
                // Fail closed when it is absent: fabricating a tenant here
                // made API-key admins global superusers across tenants.
                let tenant_id = match user.tenant_id.clone() {
                    Some(t) => t.to_string(),
                    None => {
                        tracing::warn!(
                            user_id = %user.id,
                            username = %user.username,
                            "API-key auth for user without tenant binding; rejecting. \
                             Re-provision the user via POST /api/v1/users with tenant_id."
                        );
                        // task-104: AuthFailure audit event — the token is
                        // rejected because its user has no tenant binding.
                        record_auth_failure(
                            state,
                            "api_key",
                            "user has no tenant binding; re-provision via POST /api/v1/users",
                            forensic_ip,
                            forensic_ua,
                        )
                        .await;
                        return Err((
                            StatusCode::FORBIDDEN,
                            "user has no tenant binding; re-provision via POST /api/v1/users",
                        )
                            .into_response());
                    }
                };
                if !auth_already_audited {
                    record_auth_success(
                        state,
                        "api_key",
                        Some(user.display_name.as_str()),
                        Some(&user.id),
                        forensic_ip,
                        forensic_ua,
                    )
                    .await;
                }
                return Ok(AuthenticatedAgent {
                    agent_id: user.display_name.clone(),
                    user_id: Some(user.id),
                    roles: user.roles,
                    tenant_id,
                    workspace_id: None,
                    repo_id: None,
                    scope: Vec::new(),
                    jwt_claims: None, // API key — no ABAC evaluation.
                    bearer_token,
                    deprecated_token_auth,
                    impersonation: None,
                });
            }
        }

        // 4. JWT validation.
        if let Some(jwt_cfg) = &state.jwt_config {
            if let Ok(mut auth) = validate_jwt(token, jwt_cfg, state).await {
                auth.deprecated_token_auth = deprecated_token_auth;
                auth.bearer_token = bearer_token.clone();
                if !auth_already_audited {
                    record_auth_success(
                        state,
                        "user_jwt",
                        Some(auth.agent_id.as_str()),
                        auth.user_id.as_ref(),
                        forensic_ip,
                        forensic_ua,
                    )
                    .await;
                }
                return Ok(auth);
            }
        }

        // 5. Federation JWT from a trusted remote Gyre instance (G11).
        if token.starts_with("ey") {
            if let Some(mut auth) = validate_federated_jwt(token, state).await {
                auth.deprecated_token_auth = deprecated_token_auth;
                auth.bearer_token = bearer_token.clone();
                if !auth_already_audited {
                    record_auth_success(
                        state,
                        "federated_jwt",
                        Some(auth.agent_id.as_str()),
                        auth.user_id.as_ref(),
                        forensic_ip,
                        forensic_ua,
                    )
                    .await;
                }
                return Ok(auth);
            }
        }

        // No auth method accepted the token. Classify best-effort: an "ey"
        // prefix means a JWT-shaped credential was rejected; anything else
        // is an unknown credential kind.
        let token_kind = if token.starts_with("ey") {
            "agent_jwt"
        } else {
            "unknown"
        };
        record_auth_failure(state, token_kind, "invalid token", forensic_ip, forensic_ua).await;
        Err((StatusCode::UNAUTHORIZED, "Invalid token").into_response())
    }
}

/// Validate a raw token string and return an `AuthenticatedAgent`.
///
/// Used by the WebSocket handler which receives the token as a message payload
/// rather than an HTTP Authorization header.
pub async fn authenticate_token(
    token: &str,
    state: &Arc<AppState>,
) -> Result<AuthenticatedAgent, &'static str> {
    // 1. Global auth token.
    if tokens_equal(token, &state.auth_token) {
        record_auth_success(state, "global", Some("system"), None, None, None).await;
        return Ok(AuthenticatedAgent {
            agent_id: "system".to_string(),
            user_id: None,
            roles: vec![UserRole::Admin],
            tenant_id: system_principal_tenant(),
            workspace_id: None,
            repo_id: None,
            scope: Vec::new(),
            jwt_claims: None,
            bearer_token: Some(token.to_string()),
            deprecated_token_auth: false,
            impersonation: None,
        });
    }

    // 1.5. Impersonation JWT (identity-security.md §Impersonation).
    if token.starts_with("ey") {
        if let Some(claims) = state
            .agent_signing_key
            .validate_impersonation_opt(token, &state.base_url)
        {
            return match crate::impersonation::resolve_impersonation_auth(&claims, state).await {
                Some(auth) => Ok(auth),
                None => Err("Impersonation session is not active"),
            };
        }
    }

    // 2. Per-agent tokens.
    {
        let token_pairs = state
            .kv_store
            .kv_list("agent_tokens")
            .await
            .unwrap_or_default();
        if let Some(agent_id) = token_pairs
            .iter()
            .find(|(_, t)| t.as_str() == token)
            .map(|(id, _)| id.clone())
        {
            let jwt_claims = if token.starts_with("ey") {
                match state.agent_signing_key.validate(token, &state.base_url) {
                    Ok(agent_claims) => serde_json::to_value(&agent_claims).ok(),
                    Err(_) => {
                        record_auth_failure(
                            state,
                            "agent_jwt",
                            "invalid or expired agent token",
                            None,
                            None,
                        )
                        .await;
                        return Err("Invalid or expired agent token");
                    }
                }
            } else {
                None
            };
            let token_kind = if token.starts_with("ey") {
                "agent_jwt"
            } else {
                "uuid_token"
            };
            record_auth_success(state, token_kind, Some(agent_id.as_str()), None, None, None).await;
            let auth = AuthenticatedAgent {
                agent_id,
                user_id: None,
                roles: vec![UserRole::Agent],
                tenant_id: system_principal_tenant(),
                workspace_id: None,
                repo_id: None,
                scope: Vec::new(),
                jwt_claims: jwt_claims.clone(),
                bearer_token: Some(token.to_string()),
                deprecated_token_auth: false,
                impersonation: None,
            };
            let auth = match jwt_claims.as_ref() {
                Some(claims) => auth.with_hierarchy_from_claims(claims),
                None => auth,
            };
            return Ok(auth);
        }
    }

    // 2.5. JWT token not in agent_tokens — check if revoked.
    if token.starts_with("ey")
        && state
            .agent_signing_key
            .validate(token, &state.base_url)
            .is_ok()
    {
        record_auth_failure(
            state,
            "agent_jwt",
            "agent token has been revoked",
            None,
            None,
        )
        .await;
        return Err("Agent token has been revoked");
    }

    // 3. API keys.
    if let Ok(Some(user_id)) = state.api_keys.find_user_id(&hash_api_key(token)).await {
        if let Ok(Some(user)) = state.users.find_by_id(&user_id).await {
            // F1: fail closed when the user has no tenant binding (see the
            // extractor's API-key path for the rationale).
            let tenant_id = match user.tenant_id.clone() {
                Some(t) => t.to_string(),
                None => {
                    tracing::warn!(
                        user_id = %user.id,
                        username = %user.username,
                        "API-key auth for user without tenant binding; rejecting"
                    );
                    // task-104: AuthFailure audit event — the token is
                    // rejected because its user has no tenant binding.
                    record_auth_failure(
                        state,
                        "api_key",
                        "user has no tenant binding; re-provision via POST /api/v1/users",
                        None,
                        None,
                    )
                    .await;
                    return Err("user has no tenant binding; re-provision via POST /api/v1/users");
                }
            };
            record_auth_success(
                state,
                "api_key",
                Some(user.display_name.as_str()),
                Some(&user.id),
                None,
                None,
            )
            .await;
            return Ok(AuthenticatedAgent {
                agent_id: user.display_name.clone(),
                user_id: Some(user.id),
                roles: user.roles,
                tenant_id,
                workspace_id: None,
                repo_id: None,
                scope: Vec::new(),
                jwt_claims: None,
                bearer_token: Some(token.to_string()),
                deprecated_token_auth: false,
                impersonation: None,
            });
        }
    }

    // 4. Keycloak JWT.
    if let Some(jwt_cfg) = &state.jwt_config {
        if let Ok(auth) = validate_jwt(token, jwt_cfg, state).await {
            record_auth_success(
                state,
                "user_jwt",
                Some(auth.agent_id.as_str()),
                auth.user_id.as_ref(),
                None,
                None,
            )
            .await;
            return Ok(auth);
        }
    }

    // 5. Federated JWT.
    if token.starts_with("ey") {
        if let Some(auth) = validate_federated_jwt(token, state).await {
            record_auth_success(
                state,
                "federated_jwt",
                Some(auth.agent_id.as_str()),
                auth.user_id.as_ref(),
                None,
                None,
            )
            .await;
            return Ok(auth);
        }
    }

    let token_kind = if token.starts_with("ey") {
        "agent_jwt"
    } else {
        "unknown"
    };
    record_auth_failure(state, token_kind, "invalid token", None, None).await;
    Err("Invalid token")
}

/// Public wrapper for middleware JWT validation (checks token validity only).
pub async fn validate_jwt_middleware(
    token: &str,
    jwt_cfg: &crate::JwtConfig,
    state: &Arc<crate::AppState>,
) -> Result<(), String> {
    validate_jwt(token, jwt_cfg, state).await.map(|_| ())
}

/// Public wrapper for federation JWT validation used by the global auth middleware.
pub async fn validate_federated_jwt_middleware(
    token: &str,
    state: &Arc<crate::AppState>,
) -> Result<(), String> {
    validate_federated_jwt(token, state)
        .await
        .map(|_| ())
        .ok_or_else(|| "federated JWT validation failed".to_string())
}

async fn validate_jwt(
    token: &str,
    jwt_cfg: &crate::JwtConfig,
    state: &Arc<AppState>,
) -> Result<AuthenticatedAgent, String> {
    // Decode header to get kid.
    let header = jsonwebtoken::decode_header(token).map_err(|e| format!("decode header: {e}"))?;
    let kid = header.kid.unwrap_or_else(|| "default".to_string());

    // Try to find key; refresh JWKS once if not found.
    let decoding_key = {
        let cache = jwt_cfg.keys.read().unwrap();
        cache.get(&kid).cloned()
    };

    let decoding_key = if let Some(dk) = decoding_key {
        dk
    } else {
        refresh_jwks(&jwt_cfg.issuer, &state.http_client, &jwt_cfg.keys).await;
        let cache = jwt_cfg.keys.read().unwrap();
        cache
            .get(&kid)
            .cloned()
            .ok_or_else(|| format!("no JWK found for kid={kid}"))?
    };

    // Build validation.
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    validation.set_issuer(&[&jwt_cfg.issuer]);
    if let Some(aud) = &jwt_cfg.audience {
        validation.set_audience(&[aud]);
    } else {
        validation.validate_aud = false;
    }

    let token_data = jsonwebtoken::decode::<JwtClaims>(token, &decoding_key, &validation)
        .map_err(|e| format!("decode: {e}"))?;

    // Also capture the raw claims as JSON for ABAC evaluation (G6).
    let raw_claims = jsonwebtoken::decode::<serde_json::Value>(token, &decoding_key, &validation)
        .map(|td| td.claims)
        .ok();

    let claims = token_data.claims;
    let roles = roles_from_claims(&claims);
    let preferred_username = claims
        .preferred_username
        .clone()
        .unwrap_or_else(|| claims.sub.clone());

    // Extract and validate tenant_id from JWT claim. The claim's schema
    // default ("default" — the bootstrapped single-tenant scope, the same
    // tenant system_principal_tenant resolves server-originated principals
    // to) was already applied at deserialization, so the value here is fully
    // determined by the verified token.
    let tenant_id = claims.tenant_id;
    validate_tenant_id(&tenant_id, &roles).map_err(|e| format!("invalid tenant_id in JWT: {e}"))?;

    // Find or auto-create user. The tenant scope enables the invitation
    // email-link (an invitation accepted in local mode is claimed by the
    // SSO subject on first login when the emails match in the same
    // tenant and the IdP has verified the email).
    let user = find_or_create_user_tenant(
        state,
        &claims.sub,
        Some(&tenant_id),
        &preferred_username,
        claims.email.as_deref(),
        claims.email_verified.unwrap_or(false),
        &roles,
    )
    .await
    .map_err(|e| format!("user resolution: {e}"))?;

    let auth = AuthenticatedAgent {
        agent_id: user.display_name.clone(),
        user_id: Some(user.id),
        roles: user.roles,
        tenant_id,
        workspace_id: None,
        repo_id: None,
        scope: Vec::new(),
        jwt_claims: raw_claims.clone(),
        bearer_token: Some(token.to_string()),
        deprecated_token_auth: false,
        impersonation: None,
    };
    // Keycloak tokens carry no §1 hierarchy claims (they are user tokens);
    // if a deployment ever embeds them, mirror them onto the principal.
    Ok(match raw_claims.as_ref() {
        Some(claims) => auth.with_hierarchy_from_claims(claims),
        None => auth,
    })
}

/// Find or auto-provision the user for a verified JWT, linking a
/// previously accepted invitation (user-management.md §Tenant-Level
/// User Onboarding, task-110 F2).
///
/// Resolution order:
/// 1. `external_id` — the Keycloak subject. `last_login_at` is stamped
///    on every authentication (Spec §User Entity); the username is
///    immutable after creation, so later logins never rewrite it even
///    if the IdP later changes `preferred_username`.
/// 2. Email link within the tenant: an invitation accepted in local
///    mode has external_id `tenant:{tenant}:email:{email}`. When the
///    SSO subject's email matches that account in the same tenant, the
///    account is CLAIMED: its external_id is re-pointed to the SSO
///    subject via the port's sanctioned claim transition
///    (`claim_external_id`; plain `update()` rejects external_id
///    changes per §Username vs Display Name) so the pre-assigned
///    memberships and invited GlobalRole attach to the SSO identity
///    instead of a duplicate account. Requires `email_verified=true` on
///    the token — an attacker who controls an unverified email in the
///    IdP must not gain a tenant account by choosing the invitee's
///    address.
/// 3. Otherwise auto-provision a new user (tenant-scoped when the JWT
///    carries a tenant claim): the username is derived from the SSO
///    `preferred_username` claim (sanitized to the URL-safe handle
///    contract, with a generated unique handle when sanitization
///    yields nothing), display name starts as the raw
///    preferred_username (user-management.md §Username vs Display
///    Name).
async fn find_or_create_user_tenant(
    state: &Arc<AppState>,
    external_id: &str,
    tenant_id: Option<&str>,
    preferred_username: &str,
    email: Option<&str>,
    email_verified: bool,
    roles: &[UserRole],
) -> anyhow::Result<User> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if let Some(mut existing) = state.users.find_by_external_id(external_id).await? {
        // Spec §User Entity: last_login_at is set on each authentication.
        // username/external_id stay untouched (immutable after creation).
        existing.record_login(now);
        state.users.update(&existing).await?;
        return Ok(existing);
    }

    // Email link (only meaningful with a tenant scope, a verified email,
    // and a matching invitation-created account).
    if let (Some(tenant), Some(email), true) = (tenant_id, email, email_verified) {
        let local_ns = format!("tenant:{tenant}:email:{}", email.to_lowercase());
        if let Some(mut invited) = state.users.find_by_external_id(&local_ns).await? {
            // Claim: same email in the same tenant. Re-point the
            // external_id at the SSO subject via the port's sanctioned
            // claim transition — update() rejects external_id changes
            // (user-management.md §Username vs Display Name) — so the
            // pre-assigned memberships and invited GlobalRole resolve to
            // this one account instead of a duplicate.
            state
                .users
                .claim_external_id(&invited.id, external_id)
                .await?;
            invited.external_id = external_id.to_string();
            if !roles.is_empty() {
                invited.roles = roles.to_vec();
            }
            // The claim is the invited account's first SSO authentication:
            // stamp it like any other login.
            invited.record_login(now);
            state.users.update(&invited).await?;
            tracing::info!(
                user_id = %invited.id,
                tenant_id = %tenant,
                "linked invitation-created account to SSO subject"
            );
            return Ok(invited);
        }
    }

    // Derive the URL-safe username from the SSO preferred_username. When
    // sanitization leaves nothing usable, fall back to the subject — but
    // sanitized like any other handle: the subject is an opaque IdP
    // identifier (RFC 7519 StringOrURI; Auth0 subjects contain '|'), not a
    // URL-safe string, and the handle contract (validate_username) must hold
    // for every persisted username. When neither sanitizes (e.g. a purely
    // numeric-keypad subject), use migration 000058's escape hatch for
    // unsanitizable names: "u-<row id>", URL-safe and unique by the freshly
    // generated primary key.
    // sanitize_username is lossy — distinct IdP handles ("Jordan Sell" /
    // "Jordan_Sell") can derive the same URL-safe username — and usernames
    // are globally unique (idx_users_username + the create() port
    // contract), so the first login of a second user whose derived handle
    // collides resolves it with the migration's deterministic suffix
    // scheme (base, base-2, base-3, ...) instead of failing the login
    // (a 401 that would repeat forever: find_by_external_id keeps
    // missing, and the username is immutable).
    let id = Id::new(uuid::Uuid::new_v4().to_string());
    let base_username = User::sanitize_username(preferred_username)
        .or_else(|| User::sanitize_username(external_id))
        .unwrap_or_else(|| format!("u-{}", id.as_str()));
    let username = match resolve_unique_username(state, &base_username).await {
        Ok(handle) => handle,
        Err(e) => {
            // Every Err means the base handle is already taken (the walk
            // probes the base first), so falling back to the subject
            // verbatim would fail create() and reproduce the lockout.
            // Instead mirror migration 000058's escape hatch for
            // unsanitizable names: "u-<row id>", URL-safe and unique by
            // the freshly generated primary key.
            tracing::warn!(
                external_id,
                base = %base_username,
                error = %e,
                "no unique handle derivable from the SSO username; assigning a generated handle"
            );
            format!("u-{}", id.as_str())
        }
    };

    let mut user = User::new_sso(
        id,
        external_id,
        username,
        preferred_username, // display_name: human-readable, editable
        now,
    );
    user.email = email.map(|e| e.to_string());
    // F1: bind the user to the validated tenant claim so later API-key or
    // JWT auth for this user resolves a real scope. The JWT path always
    // supplies the claim (the serde schema default maps an absent claim
    // to the bootstrapped tenant), so the Option is Some on every call.
    if let Some(t) = tenant_id {
        user.tenant_id = Some(Id::new(t.to_string()));
    }
    if !roles.is_empty() {
        user.roles = roles.to_vec();
    }
    user.last_login_at = Some(now);

    // create() can still fail on the unique index if another user was
    // provisioned with the same handle between the uniqueness probe and the
    // insert (TOCTOU): the next login re-runs this resolution, so the
    // transient conflict self-heals rather than locking the user out.
    state.users.create(&user).await?;
    Ok(user)
}

/// Derive a globally-unique handle for a freshly provisioned user.
///
/// Mirrors migration 000058's dedup pass 1: the first user keeps the base
/// handle, later colliding users get deterministic numeric suffixes
/// (base-2, base-3, ...). An exhausted suffix space bails and the caller
/// assigns a generated `u-<id>` handle (unique by the fresh primary key).
pub(crate) async fn resolve_unique_username(
    state: &Arc<AppState>,
    base: &str,
) -> anyhow::Result<String> {
    let mut candidate = base.to_string();
    // Bounded at 2..=10_000 so a pathological store cannot pin this loop
    // open; an exhausted space bails to the generated-handle fallback.
    for n in 2u32..=10_000 {
        if state.users.find_by_username(&candidate).await?.is_none() {
            return Ok(candidate);
        }
        candidate = suffixed_username(base, n)?;
    }
    anyhow::bail!("username suffix space exhausted for base {base:?}");
}

/// Build the `n`-th collision candidate for a base handle: the base
/// truncated so `base-{n}` fits the 64-char cap, with any truncation
/// artifact repaired so the candidate passes the URL-safe handle contract.
///
/// Pure (no storage probe) so it is directly unit-testable. Fails only when
/// no prefix of `base` can carry a suffix.
fn suffixed_username(base: &str, n: u32) -> anyhow::Result<String> {
    let suffix = format!("-{n}");
    // Handles are ASCII-only (validate_username), so byte length is char
    // length and byte slicing cannot split a char.
    let budget = 64usize.saturating_sub(suffix.len());
    let mut stem = base
        .as_bytes()
        .get(..budget)
        .unwrap_or(base.as_bytes())
        .to_vec();
    // Truncation can leave a trailing separator or a dangling "--": pop
    // until the stem itself passes the handle contract.
    while !User::validate_username(
        std::str::from_utf8(&stem)
            .map_err(|_| anyhow::anyhow!("username base {base:?} is not ASCII"))?,
    )
    .is_ok()
    {
        if stem.is_empty() {
            anyhow::bail!("no URL-safe prefix of username {base:?} fits the 64-char cap");
        }
        stem.pop();
    }
    Ok(format!(
        "{}{}",
        std::str::from_utf8(&stem).expect("stem is ASCII (validated above)"),
        suffix
    ))
}

// -- Federation JWT validation (G11) ------------------------------------------

/// Extract the `iss` claim from a JWT payload without verifying the signature.
/// Used to route federation tokens to the correct remote JWKS endpoint.
fn extract_iss_from_jwt_payload(token: &str) -> Option<String> {
    let payload_b64 = token.split('.').nth(1)?;
    let payload_bytes = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&payload_bytes).ok()?;
    value.get("iss")?.as_str().map(|s| s.to_string())
}

/// Return true iff `jwks_uri` shares the same scheme and host as `issuer`.
///
/// Prevents SSRF: a compromised remote Gyre's discovery document could set
/// `jwks_uri` to an internal metadata endpoint (e.g. `http://169.254.169.254/`)
/// unless we verify it stays on the same origin as the trusted issuer.
fn is_same_origin(issuer: &str, jwks_uri: &str) -> bool {
    let iss = url::Url::parse(issuer).ok();
    let jwks = url::Url::parse(jwks_uri).ok();
    match (iss, jwks) {
        (Some(a), Some(b)) => {
            a.scheme() == b.scheme()
                && a.host() == b.host()
                && a.port_or_known_default() == b.port_or_known_default()
        }
        _ => false,
    }
}

/// Fetch JWKS keys for a remote Gyre issuer via OIDC discovery.
async fn fetch_remote_jwks_for_issuer(
    issuer: &str,
    http_client: &reqwest::Client,
) -> Option<HashMap<String, jsonwebtoken::DecodingKey>> {
    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let discovery: OidcDiscovery = http_client
        .get(&discovery_url)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    // G11-A: Reject jwks_uri that redirects to a different origin (SSRF guard).
    if !is_same_origin(issuer, &discovery.jwks_uri) {
        tracing::warn!(
            issuer = %issuer,
            jwks_uri = %discovery.jwks_uri,
            "Federation SSRF guard: jwks_uri is not same-origin as issuer — rejecting"
        );
        return None;
    }

    let jwk_set: JwkSet = http_client
        .get(&discovery.jwks_uri)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    let mut keys = HashMap::new();
    for jwk in &jwk_set.keys {
        let kid = match jwk.get("kid").and_then(|v| v.as_str()) {
            Some(k) => k.to_string(),
            None => continue,
        };
        let jwk_typed: jsonwebtoken::jwk::Jwk = match serde_json::from_value(jwk.clone()) {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!("Federation: failed to parse JWK {kid} from {issuer}: {e}");
                continue;
            }
        };
        match jsonwebtoken::DecodingKey::from_jwk(&jwk_typed) {
            Ok(dk) => {
                keys.insert(kid, dk);
            }
            Err(e) => tracing::warn!(
                "Federation: failed to build DecodingKey for {kid} from {issuer}: {e}"
            ),
        }
    }

    if keys.is_empty() {
        tracing::warn!("Federation: no valid keys found in JWKS for {issuer}");
        None
    } else {
        Some(keys)
    }
}

/// Validate a JWT from a trusted remote Gyre instance (EdDSA/Ed25519).
///
/// Returns `Some(AuthenticatedAgent)` if the token is valid and its `iss`
/// matches a configured trusted issuer. Returns `None` if not applicable
/// (unknown issuer, invalid signature, or expired).
async fn validate_federated_jwt(token: &str, state: &Arc<AppState>) -> Option<AuthenticatedAgent> {
    if state.trusted_issuers.is_empty() {
        return None;
    }

    // Extract issuer without verifying signature.
    let iss = extract_iss_from_jwt_payload(token)?;

    // Check if the issuer is trusted.
    let normalized_iss = iss.trim_end_matches('/').to_string();
    if !state
        .trusted_issuers
        .iter()
        .any(|t| t.trim_end_matches('/') == normalized_iss)
    {
        return None;
    }

    let header = jsonwebtoken::decode_header(token).ok()?;
    let kid = header.kid.unwrap_or_else(|| "default".to_string());

    // Check cache for a fresh entry.
    let decoding_key = {
        let cache = state.remote_jwks_cache.read().await;
        cache.get(&normalized_iss).and_then(|entry| {
            if entry.fetched_at.elapsed().as_secs() < REMOTE_JWKS_TTL_SECS {
                entry.keys.get(&kid).cloned()
            } else {
                None
            }
        })
    };

    let decoding_key = if let Some(dk) = decoding_key {
        dk
    } else {
        // Fetch fresh JWKS.
        let keys = fetch_remote_jwks_for_issuer(&normalized_iss, &state.http_client).await?;
        let dk = keys.get(&kid).cloned()?;
        let mut cache = state.remote_jwks_cache.write().await;
        cache.insert(
            normalized_iss.clone(),
            RemoteJwksEntry {
                keys,
                fetched_at: std::time::Instant::now(),
            },
        );
        dk
    };

    // Validate signature and standard claims (exp, iss).
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::EdDSA);
    validation.set_issuer(&[&normalized_iss]);
    validation.validate_aud = false;

    let token_data =
        jsonwebtoken::decode::<AgentJwtClaims>(token, &decoding_key, &validation).ok()?;
    let claims = token_data.claims;

    tracing::debug!(
        issuer = %normalized_iss,
        sub = %claims.sub,
        "Federated JWT accepted"
    );

    // Federated agents receive Agent role; agent_id is prefixed with the
    // remote host to avoid collisions with local agent IDs.
    let remote_host = normalized_iss
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let fed_claims_json = serde_json::to_value(&claims).ok();
    let auth = AuthenticatedAgent {
        agent_id: format!("{remote_host}/{}", claims.sub),
        user_id: None,
        roles: vec![UserRole::Agent],
        tenant_id: system_principal_tenant(),
        workspace_id: None,
        repo_id: None,
        scope: Vec::new(),
        jwt_claims: fed_claims_json.clone(),
        bearer_token: Some(token.to_string()),
        deprecated_token_auth: false,
        impersonation: None,
    };
    // A federated instance's agent JWTs carry §1 hierarchy claims; mirror
    // them onto the resolved principal like local agent JWTs.
    Some(match fed_claims_json.as_ref() {
        Some(c) => auth.with_hierarchy_from_claims(c),
        None => auth,
    })
}

// AdminOnly extractor removed (M34 Slice 4). ABAC middleware is the sole
// authorization layer. Handlers that previously used AdminOnly now use
// AuthenticatedAgent directly; ABAC middleware enforces admin-only access
// via the admin-all-operations built-in policy.

// -- WebSocket ticket-based auth -----------------------------------------------
//
// Browser WebSocket API does not support custom headers. Instead of passing
// the real auth token in a query parameter (which leaks to logs, browser
// history, and proxy caches), we issue short-lived, single-use tickets.
//
// Flow:
//   1. Client calls POST /api/v1/ws-ticket with Bearer token in header
//   2. Server returns { "ticket": "<random>" } (valid 30 s, single use)
//   3. Client connects to WS with ?ticket=<random>
//   4. Server validates and consumes the ticket atomically

/// TTL for WebSocket tickets (30 seconds).
const WS_TICKET_TTL_SECS: u64 = 30;
/// Maximum tickets in the store (prevents DoS via ticket flooding).
const WS_TICKET_MAX: usize = 10_000;

/// A single-use WebSocket auth ticket.
#[derive(Clone)]
pub struct WsTicketEntry {
    /// The resolved identity for this ticket.
    pub auth: AuthenticatedAgent,
    /// When the ticket was created (monotonic).
    pub created: std::time::Instant,
}

/// Thread-safe ticket store with automatic GC.
#[derive(Clone, Default)]
pub struct WsTicketStore {
    tickets: Arc<std::sync::Mutex<HashMap<String, WsTicketEntry>>>,
}

impl WsTicketStore {
    pub fn new() -> Self {
        Self {
            tickets: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Issue a new ticket for the given authenticated identity.
    /// Returns the opaque ticket string.
    pub fn issue(&self, auth: AuthenticatedAgent) -> String {
        let ticket = uuid::Uuid::new_v4().to_string();
        let mut store = self.tickets.lock().unwrap_or_else(|e| e.into_inner());
        // GC expired tickets and enforce cap
        let now = std::time::Instant::now();
        store.retain(|_, v| now.duration_since(v.created).as_secs() < WS_TICKET_TTL_SECS);
        if store.len() >= WS_TICKET_MAX {
            // Drop oldest entries
            let mut entries: Vec<_> = store.drain().collect();
            entries.sort_by_key(|(_, v)| v.created);
            entries.truncate(WS_TICKET_MAX / 2);
            store.extend(entries);
        }
        store.insert(ticket.clone(), WsTicketEntry { auth, created: now });
        ticket
    }

    /// Consume a ticket: returns the auth identity if valid, removes it atomically.
    /// Returns None if expired, already used, or not found.
    pub fn consume(&self, ticket: &str) -> Option<AuthenticatedAgent> {
        let mut store = self.tickets.lock().unwrap_or_else(|e| e.into_inner());
        let entry = store.remove(ticket)?;
        let age = std::time::Instant::now().duration_since(entry.created);
        if age.as_secs() >= WS_TICKET_TTL_SECS {
            return None; // Expired
        }
        Some(entry.auth)
    }
}

// -- Tests --------------------------------------------------------------------

#[cfg(test)]
pub mod test_helpers {
    use super::*;
    use crate::JwtConfig;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use jsonwebtoken::DecodingKey;
    use std::time::{SystemTime, UNIX_EPOCH};

    pub const TEST_KID: &str = "test-key-1";

    /// Pre-generated RSA-2048 private key (PKCS8 DER, base64-encoded).
    /// Used by ring to sign test JWTs.
    pub const TEST_RSA_PRIVATE_DER_B64: &str =
        "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC+pxi5pumJYejs\
O48NpzBcwW7tu/Ukwo8CeOTkfcHwf4nnpTUHFds/OynA3OevJFt25U/tnS2nplU+\
3zpZ/w0eSwDzBbA7uvukR8hjOPp8726Jim8slmKVLmnlAFxMNgkaPOWuHQjNCvAe\
GbLm6hMigAAmpo1HyrtjsAViq7hezKAdegsbmSbcOShG9Yy1eNBRcoNr+BTut2bF\
RF5dGf24oHUXhVn1A89eYUDcYFLqstrZ/uyvyrYuSKKaJQmQ3O7J2PyfPoX9skqx\
ACOlyN1qJSkpSI1olqaXMhD/chqoyM5rvYerDts5iVp7kY9tBEyd5GnazD2qtsm0\
58vGdAdZAgMBAAECggEAClgm5mjU5kY8lPe4z+70t+2rYwUGHK3dRE8yrSaZI59J\
6FqNhWJFVHmVbwBVZkp413tC8lOrRtvGp4wFXp1w9OYxKZ5zBNSlA3F9IQwWBDoa\
A2ayWEcbF3jOmFUkDOMILKcl96Pd5sErjNiMXUzRR7eN+oriIsVwVCz4Z9WFIgiM\
XNB2WGrtAdpYRuhLcXXxKF4MyCNe5xwiTtdTxOPNYZCPmkBuGOH/cXW2nRZOX17t\
HzCYQApplay6Deav7tRFLFy9i0/oTKmOd+9MjaRG7AGkpojFTERQU1QC7wNKGM/G\
jWyTcLqCNz7Eeiw8xccu8K/4YWTIb1guxSXVR8tRkQKBgQD3kvgZkBxrwQ/LxPk9\
KwnqREjyU6TklAkMFi3hH8ztG6/CARf5Z/PAtG+LRFjjdA6ZUk0NQB3vIuVR+l8k\
X7/yGxR+AZFP+fC0WiOt3nK6dUQZjM1EVeXN8SF89AaLmVqYeVfOuFZASVNdOywV\
64Fs4vngyx1yRe6L4gmuJRTpsQKBgQDFJDC9x/pv3uwxiVUwQAuOmhxarm3zFIMA\
hdgElKarF//VSf2ClNlkBjOdOG1qygP/uDiKgEji74o/OC8W+7KJEc0GCC9+Re26\
VjGoAeW9tpdPcFG4pGy3O8TgUev4Kd7M5yXrP+t7Tk7BzFYbolyH2nStVnoGzmEV\
ovLRrVC6KQKBgQCHKNqAAjgx77Wy9ndnmI6SMfNtMJMOI2+JBLbJd/YF4J6kmLpA\
9CqUIy3eZ1Lr4llyBQeB22Y8Pt3ZUooMrqV5BKW8YRi3A+oQXEGsU4xpFUo3dLI/\
w3FxbFpvAD5uiit2oJB6kBKuiBmWBY787JS9IHFw8xLBZ0JjZeUJCL+REQKBgGyH\
aunfoMhbke4Ha8V/TJ15y+hEJLVwclokVPLYnxSLxsS10gFETpVZMKr5+IqUojTy\
RvZAf1+nfZUmKg5eDECXcyZB9VtvxdsSPo3+VWrmjVAnRmX+5vbMyaqwjtZ0vnpR\
OjfTXaDpUEQyaKkX7hbANgGDHPSIQzw1E7ZI0qtZAoGBAOYcsRdNUwGPYqLKzfua\
W89V2Jogg217+3HIxjgfmGn9nb8RqIwhGSoSNGBa2gKWJWUzPNzF4H3TIQw1MGyp\
l4UzPHH0YLfuMsgbYIGpoe9l0zTcucflm8gMLUHysu5WSxRDtHa7Kv8BVafmD7zL\
HKWsbrW0tHUPuMuz8Xgvs0yV";

    /// JWK (JSON) for the corresponding RSA-2048 public key.
    /// n = base64url(modulus), e = base64url(65537).
    const TEST_JWK_JSON: &str = r#"{"kty":"RSA","alg":"RS256","use":"sig","kid":"test-key-1","n":"vqcYuabpiWHo7DuPDacwXMFu7bv1JMKPAnjk5H3B8H-J56U1BxXbPzspwNznryRbduVP7Z0tp6ZVPt86Wf8NHksA8wWwO7r7pEfIYzj6fO9uiYpvLJZilS5p5QBcTDYJGjzlrh0IzQrwHhmy5uoTIoAAJqaNR8q7Y7AFYqu4XsygHXoLG5km3DkoRvWMtXjQUXKDa_gU7rdmxUReXRn9uKB1F4VZ9QPPXmFA3GBS6rLa2f7sr8q2LkiimiUJkNzuydj8nz6F_bJKsQAjpcjdaiUpKUiNaJamlzIQ_3IaqMjOa72Hqw7bOYlae5GPbQRMneRp2sw9qrbJtOfLxnQHWQ","e":"AQAB"}"#;

    pub fn make_jwt_config() -> Arc<JwtConfig> {
        let cfg = JwtConfig::new("http://localhost:8080/realms/gyre", None);

        let jwk: jsonwebtoken::jwk::Jwk =
            serde_json::from_str(TEST_JWK_JSON).expect("test JWK must be valid JSON");
        let dk = DecodingKey::from_jwk(&jwk).expect("test JWK must produce a valid DecodingKey");
        cfg.insert_key(TEST_KID, dk);
        Arc::new(cfg)
    }

    pub fn make_test_state_with_jwt() -> Arc<crate::AppState> {
        let base = crate::mem::test_state();
        let mut state = (*base).clone();
        state.jwt_config = Some(make_jwt_config());
        Arc::new(state)
    }

    /// Sign a test JWT using ring's RSA-PKCS1-SHA256 (RS256).
    /// Does NOT use jsonwebtoken's PEM feature -- no simple_asn1/time MSRV issue.
    pub fn sign_test_jwt(claims: &serde_json::Value, exp_delta_secs: i64) -> String {
        use base64::engine::general_purpose::STANDARD;
        use ring::rand::SystemRandom;
        use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};

        // Decode private key DER (standard base64, not URL-safe).
        let private_der = STANDARD
            .decode(TEST_RSA_PRIVATE_DER_B64)
            .expect("private key base64 must be valid");

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let mut payload = claims.clone();
        let obj = payload.as_object_mut().unwrap();
        obj.insert(
            "iss".to_string(),
            serde_json::json!("http://localhost:8080/realms/gyre"),
        );
        obj.insert("iat".to_string(), serde_json::json!(now));
        obj.insert("exp".to_string(), serde_json::json!(now + exp_delta_secs));

        // Build header JSON.
        let header = serde_json::json!({"alg":"RS256","typ":"JWT","kid": TEST_KID});
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&header).unwrap());
        let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&payload).unwrap());
        let signing_input = format!("{}.{}", header_b64, payload_b64);

        let key_pair =
            RsaKeyPair::from_pkcs8(&private_der).expect("private key DER must be valid PKCS8");
        let rng = SystemRandom::new();
        let mut sig = vec![0u8; key_pair.public().modulus_len()];
        key_pair
            .sign(&RSA_PKCS1_SHA256, &rng, signing_input.as_bytes(), &mut sig)
            .expect("RSA signing must succeed");

        let sig_b64 = URL_SAFE_NO_PAD.encode(&sig);
        format!("{}.{}", signing_input, sig_b64)
    }
}

#[cfg(test)]
mod tests {
    use super::AgentSigningKey;
    use crate::mem::test_state;
    use axum::{body::Body, routing::get, Router};
    use http::{Request, StatusCode};
    use std::sync::Arc;
    use tower::ServiceExt;

    use super::{system_principal_tenant, test_helpers::*, AuthenticatedAgent, WsTicketStore};
    use gyre_domain::UserRole;

    #[test]
    fn impersonation_jwt_round_trip_and_expiry() {
        let key = super::AgentSigningKey::generate();
        let issuer = "http://localhost:3000";

        let token = key
            .mint_impersonation("target-1", "admin-1", "sess-1", "default", issuer, 900)
            .unwrap();
        let claims = key
            .validate_impersonation_opt(&token, issuer)
            .expect("fresh impersonation JWT must validate");
        assert!(claims.impersonating);
        assert_eq!(claims.sub, "target-1");
        assert_eq!(claims.impersonator_id, "admin-1");
        assert_eq!(claims.target_user_id, "target-1");
        assert_eq!(claims.session_id, "sess-1");
        assert_eq!(claims.scope, "impersonation");

        // Wrong issuer rejected.
        assert!(key
            .validate_impersonation_opt(&token, "http://evil.example")
            .is_none());

        // A token whose `exp` is in the past is rejected with zero leeway
        // (auto-expiry is exact — mint and validate share the server clock).
        let backdated = {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let claims = super::ImpersonationJwtClaims {
                sub: "target-1".to_string(),
                iss: issuer.to_string(),
                iat: now - 3600,
                exp: now - 1,
                scope: "impersonation".to_string(),
                impersonating: true,
                impersonator_id: "admin-1".to_string(),
                target_user_id: "target-1".to_string(),
                session_id: "sess-1".to_string(),
                tenant_id: "default".to_string(),
            };
            let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::EdDSA);
            header.kid = Some(key.kid.clone());
            jsonwebtoken::encode(&header, &claims, &key.encoding_key).unwrap()
        };
        assert!(
            key.validate_impersonation_opt(&backdated, issuer).is_none(),
            "expired impersonation JWT must be rejected (leeway 0)"
        );

        // Agent JWTs are NOT impersonation tokens (no `impersonating`
        // claim) — validate_impersonation_opt must return None so the auth
        // chain falls through to the agent-token path.
        let agent_jwt = key
            .mint(
                "agent-1", "task-1", "system", issuer, 3600, "t1", "ws-1", "repo-1", "repo-1",
            )
            .unwrap();
        assert!(key.validate_impersonation_opt(&agent_jwt, issuer).is_none());
    }

    async fn authenticated_handler(
        AuthenticatedAgent { agent_id, .. }: AuthenticatedAgent,
    ) -> String {
        agent_id
    }

    fn app_no_jwt() -> Router {
        let state = test_state();
        Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state)
    }

    #[allow(dead_code)]
    fn app_with_jwt() -> Router {
        let state = make_test_state_with_jwt();
        Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state)
    }

    fn app_with_jwt_state(state: Arc<crate::AppState>) -> Router {
        Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state)
    }

    #[tokio::test]
    async fn missing_auth_header_returns_401() {
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn invalid_token_returns_401() {
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer bad-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn global_auth_token_accepted() {
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // -- task-104: AuthSuccess / AuthFailure audit events ---------------------

    async fn audit_events_of(
        state: &crate::AppState,
        event_type: &str,
    ) -> Vec<gyre_domain::AuditEvent> {
        gyre_ports::AuditRepository::query(
            &*state.audit,
            &gyre_ports::AuditQueryFilter {
                event_type: Some(event_type.to_string()),
                limit: 100,
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn auth_success_audit_event_recorded_for_global_token() {
        let state = test_state();
        let app = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let events = audit_events_of(&state, "auth_success").await;
        assert_eq!(events.len(), 1, "exactly one auth_success event");
        let e = &events[0];
        assert_eq!(e.detail["token_kind"], "global");
        assert_eq!(e.detail["agent_id"], "system");
        assert_eq!(e.agent_id.as_ref().unwrap().as_str(), "system");
        assert_eq!(e.outcome, gyre_domain::AuditOutcome::Success);
        assert_eq!(e.resource_type, "auth");
    }

    #[tokio::test]
    async fn auth_success_audit_event_recorded_once_per_request() {
        // The full stack validates the token in the ABAC middleware AND in
        // the handler extractor; the AuthAuditMark extension must dedupe the
        // AuthSuccess emission to exactly one event per request.
        let state = test_state();
        let app = crate::build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/audit/events")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let events = audit_events_of(&state, "auth_success").await;
        assert_eq!(
            events.len(),
            1,
            "AuthSuccess must be recorded exactly once per request, got {}",
            events.len()
        );
        assert_eq!(events[0].detail["token_kind"], "global");
    }

    #[tokio::test]
    async fn auth_failure_audit_event_recorded_for_invalid_token() {
        // Rejection at the global auth middleware (extractors never run).
        let state = test_state();
        let app = crate::build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/audit/events")
                    .header("Authorization", "Bearer bad-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let events = audit_events_of(&state, "auth_failure").await;
        assert_eq!(events.len(), 1, "exactly one auth_failure event");
        let e = &events[0];
        assert_eq!(e.detail["token_kind"], "unknown");
        assert_eq!(e.detail["rejection_reason"], "invalid token");
        assert_eq!(e.outcome, gyre_domain::AuditOutcome::Failure);
        assert!(e.agent_id.is_none(), "rejected caller has no identity");
    }

    #[tokio::test]
    async fn auth_failure_audit_event_recorded_for_revoked_agent_jwt() {
        // A cryptographically valid agent JWT that is no longer in
        // agent_tokens is revoked: signature validates but lookup fails.
        let state = test_state();
        let token = state
            .agent_signing_key
            .mint(
                "agent-gone",
                "task-1",
                "system",
                &state.base_url,
                3600,
                "t1",
                "ws-1",
                "repo-1",
                "repo-1",
            )
            .expect("mint must succeed");
        let app = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let events = audit_events_of(&state, "auth_failure").await;
        assert_eq!(events.len(), 1, "exactly one auth_failure event");
        let e = &events[0];
        assert_eq!(e.detail["token_kind"], "agent_jwt");
        assert_eq!(e.detail["rejection_reason"], "agent token has been revoked");
        assert_eq!(e.outcome, gyre_domain::AuditOutcome::Failure);
    }

    #[tokio::test]
    async fn agent_token_accepted() {
        let state = test_state();
        state
            .kv_store
            .kv_set("agent_tokens", "agent-42", "agent-secret".to_string())
            .await
            .unwrap();

        let app = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer agent-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), b"agent-42");
    }

    #[tokio::test]
    async fn agent_jwt_tenant_mirrored_from_verified_claims() {
        // task-130 review 29e132008c6748508f2c52ec982d78d6: the extractor must
        // derive the caller's tenant from the signature-verified claims, not
        // fabricate "default" — tenant-containment handlers
        // (spawn_repo_orchestrator_core et al.) compare this value against
        // the target entity's real tenant, and a fabricated identity denied
        // correctly minted scoped tokens their own happy path.
        async fn scope_handler(
            crate::auth::AuthenticatedAgent {
                tenant_id,
                workspace_id,
                repo_id,
                scope,
                ..
            }: crate::auth::AuthenticatedAgent,
        ) -> String {
            format!(
                "{tenant_id}|{}|{}|{}",
                workspace_id.unwrap_or_default(),
                repo_id.unwrap_or_default(),
                scope.join(",")
            )
        }

        let state = test_state();
        // Workspace orchestrator scoped to tenant t1 / ws-1 (no repo).
        let ws_token = state
            .agent_signing_key
            .mint_orchestrator(
                "orch-tenant-mirror",
                "system",
                &state.base_url,
                state.agent_jwt_ttl_secs,
                "t1",
                "ws-1",
                None,
                None,
                "workspace_orchestrator",
                "workspace-orchestrator",
            )
            .unwrap();
        state
            .kv_store
            .kv_set("agent_tokens", "orch-tenant-mirror", ws_token.clone())
            .await
            .unwrap();

        let app = Router::new()
            .route("/protected", get(scope_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {ws_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = std::str::from_utf8(&bytes).unwrap();
        assert_eq!(
            body, "t1|ws-1||workspace:ws-1:read,workspace:ws-1:spawn",
            "tenant/workspace/scope must mirror the signature-verified claims, got: {body}"
        );
    }

    #[tokio::test]
    async fn valid_jwt_accepted() {
        let claims = serde_json::json!({
            "tenant_id": "default",
            "sub": "user-sub-001",
            "preferred_username": "alice",
            "email": "alice@example.com",
            "realm_access": { "roles": ["developer"] }
        });
        let token = sign_test_jwt(&claims, 3600);

        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), b"alice");
    }

    #[tokio::test]
    async fn expired_jwt_rejected() {
        let claims = serde_json::json!({
            "sub": "user-sub-002",
            "preferred_username": "bob"
        });
        // Negative delta = already expired.
        let token = sign_test_jwt(&claims, -3600);

        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn invalid_signature_rejected() {
        // Tamper with the token by altering a byte in the signature.
        let claims = serde_json::json!({ "tenant_id": "default", "sub": "user-sub-003", "preferred_username": "carol" });
        let mut token = sign_test_jwt(&claims, 3600);
        // Replace last char to corrupt signature.
        let last = token.pop().unwrap();
        token.push(if last == 'A' { 'B' } else { 'A' });

        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn jwt_auto_creates_user_on_first_login() {
        let state = make_test_state_with_jwt();

        let claims = serde_json::json!({
            "tenant_id": "default",
            "sub": "new-user-sub-999",
            "preferred_username": "dave",
            "email": "dave@example.com",
            "realm_access": { "roles": ["developer"] }
        });
        let token = sign_test_jwt(&claims, 3600);

        // No user exists yet.
        let before = state
            .users
            .find_by_external_id("new-user-sub-999")
            .await
            .unwrap();
        assert!(before.is_none());

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // User should now exist.
        let after = state
            .users
            .find_by_external_id("new-user-sub-999")
            .await
            .unwrap();
        assert!(after.is_some());
        let user = after.unwrap();
        assert_eq!(user.display_name, "dave");
        assert_eq!(user.email.as_deref(), Some("dave@example.com"));
    }

    #[tokio::test]
    async fn first_login_derives_url_safe_username_from_preferred() {
        // user-management.md §Username vs Display Name: username is derived
        // from SSO preferred_username (sanitized to URL-safe), display_name
        // stays human-readable, last_login_at is stamped on the first auth.
        let state = make_test_state_with_jwt();
        let claims = serde_json::json!({
            "tenant_id": "default",
            "sub": "derive-sub-1",
            "preferred_username": "Jordan.Sell@example.com",
            "email": "jordan@example.com"
        });
        let token = sign_test_jwt(&claims, 3600);
        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let user = state
            .users
            .find_by_external_id("derive-sub-1")
            .await
            .unwrap()
            .expect("user provisioned on first login");
        assert_eq!(
            user.username, "jordan-sell-example-com",
            "username must be the sanitized handle"
        );
        assert_eq!(
            user.display_name, "Jordan.Sell@example.com",
            "display name starts as the raw preferred_username"
        );
        assert!(
            user.last_login_at.is_some(),
            "first login stamps last_login_at"
        );
        assert_eq!(user.last_login_at, Some(user.updated_at));
    }

    #[tokio::test]
    async fn second_login_stamps_last_login_and_keeps_username() {
        // Spec: last_login_at is set on EACH authentication, and the
        // username is immutable after creation even if the IdP later
        // changes preferred_username.
        let state = make_test_state_with_jwt();
        let claims1 = serde_json::json!({
            "tenant_id": "default",
            "sub": "relogin-sub",
            "preferred_username": "alice"
        });
        let token1 = sign_test_jwt(&claims1, 3600);
        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token1}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let first = state
            .users
            .find_by_external_id("relogin-sub")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.username, "alice");
        let first_login = first.last_login_at.unwrap();

        // IdP now reports a different preferred_username for the same sub.
        let claims2 = serde_json::json!({
            "tenant_id": "default",
            "sub": "relogin-sub",
            "preferred_username": "alice-renamed"
        });
        let token2 = sign_test_jwt(&claims2, 3600);
        let app2: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state.clone());
        let resp = app2
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token2}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let second = state
            .users
            .find_by_external_id("relogin-sub")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            second.username, "alice",
            "username is immutable after creation"
        );
        assert_eq!(
            second.display_name, "alice",
            "display name also stable across logins"
        );
        let second_login = second.last_login_at.expect("re-login must re-stamp");
        assert!(
            second_login >= first_login,
            "last_login_at must advance (first={first_login}, second={second_login})"
        );
        assert_eq!(second_login, second.updated_at);
    }

    #[tokio::test]
    async fn preferred_username_unsanitizable_falls_back_to_subject() {
        // When sanitization yields nothing URL-safe ("!!!"), the username
        // falls back to the SSO subject instead of failing the login.
        let state = make_test_state_with_jwt();
        let claims = serde_json::json!({
            "tenant_id": "default",
            "sub": "opaque-sub-42",
            "preferred_username": "!!!"
        });
        let token = sign_test_jwt(&claims, 3600);
        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let user = state
            .users
            .find_by_external_id("opaque-sub-42")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(user.username, "opaque-sub-42");
    }

    #[tokio::test]
    async fn unsanitizable_preferred_username_falls_back_to_url_safe_handle() {
        use gyre_domain::User;

        // The subject is an opaque IdP identifier (RFC 7519 StringOrURI):
        // "auth0|12345" is a real Auth0 sub shape and is NOT URL-safe. The
        // fallback handle must still satisfy the URL-safe username contract
        // (validate_username) — the migration's sanitize pass renames such
        // rows to "u-<row id>", and the runtime provisioning path must not
        // persist what the migration would have repaired.
        let state = make_test_state_with_jwt();
        let claims = serde_json::json!({
            "tenant_id": "default",
            "sub": "auth0|12345",
            "preferred_username": "!!!"
        });
        let token = sign_test_jwt(&claims, 3600);
        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let user = state
            .users
            .find_by_external_id("auth0|12345")
            .await
            .unwrap()
            .expect("user provisioned");
        assert!(
            User::validate_username(&user.username).is_ok(),
            "fallback username {:?} must satisfy the URL-safe handle contract",
            user.username
        );
        assert!(
            !user.username.contains('|'),
            "the raw subject must not leak into the username verbatim"
        );
    }

    #[tokio::test]
    async fn colliding_derived_username_resolves_with_suffix_and_logs_in() {
        // sanitize_username is lossy: distinct IdP handles ("Jordan Sell"
        // and "Jordan_Sell") both derive "jordan-sell", and usernames are
        // globally unique. The second SSO user's first login must resolve
        // the collision with the migration's suffix scheme (base-2) instead
        // of failing with a 401 that would repeat forever (find_by_external_id
        // keeps missing, username immutable, no pre-auth recovery path).
        let state = make_test_state_with_jwt();
        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state.clone());

        let first_claims = serde_json::json!({
            "tenant_id": "default",
            "sub": "collision-sub-1",
            "preferred_username": "Jordan Sell"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header(
                        "Authorization",
                        format!("Bearer {}", sign_test_jwt(&first_claims, 3600)),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let second_claims = serde_json::json!({
            "tenant_id": "default",
            "sub": "collision-sub-2",
            "preferred_username": "Jordan_Sell"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header(
                        "Authorization",
                        format!("Bearer {}", sign_test_jwt(&second_claims, 3600)),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "a distinct SSO user whose derived handle collides must not be locked out"
        );

        let first = state
            .users
            .find_by_external_id("collision-sub-1")
            .await
            .unwrap()
            .expect("first user provisioned");
        assert_eq!(first.username, "jordan-sell");
        let second = state
            .users
            .find_by_external_id("collision-sub-2")
            .await
            .unwrap()
            .expect("second user provisioned");
        // Migration dedup pass 1 scheme: base keeps the handle, the next
        // colliding user gets "-2".
        assert_eq!(second.username, "jordan-sell-2");
        assert_eq!(second.display_name, "Jordan_Sell");
        assert!(second.last_login_at.is_some());
        assert_ne!(first.id, second.id);
    }

    #[tokio::test]
    async fn colliding_derived_username_uses_next_suffix_for_third_user() {
        use gyre_common::Id;
        use gyre_domain::User;

        // A pre-existing handle that already occupies the "-2" slot forces
        // the third user onto "-3" (the same walk the migration performs
        // before creating idx_users_username).
        let state = make_test_state_with_jwt();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut squatter = User::new_sso(
            Id::new("u-squatter".to_string()),
            "squatter-sub",
            "jordan-sell-2",
            "Squatter",
            now,
        );
        squatter.roles = vec![UserRole::ReadOnly];
        state.users.create(&squatter).await.unwrap();

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state.clone());
        for (sub, preferred) in [
            ("collision-sub-1", "Jordan Sell"),
            ("collision-sub-2", "Jordan_Sell"),
        ] {
            let claims = serde_json::json!({
                "tenant_id": "default",
                "sub": sub,
                "preferred_username": preferred
            });
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/protected")
                        .header(
                            "Authorization",
                            format!("Bearer {}", sign_test_jwt(&claims, 3600)),
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "login must succeed for {sub} even with the -2 slot squatted"
            );
        }

        let first = state
            .users
            .find_by_external_id("collision-sub-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.username, "jordan-sell");
        let second = state
            .users
            .find_by_external_id("collision-sub-2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            second.username, "jordan-sell-3",
            "the -2 slot is occupied, so the walk must continue to -3"
        );
    }

    #[test]
    fn suffixed_username_truncates_and_repairs_the_stem() {
        use super::suffixed_username;
        use gyre_domain::User;

        // Pure helper: the 64-char cap truncates the base, and truncation
        // artifacts (trailing separator, dangling "--") are repaired so the
        // candidate still passes the handle contract.
        let long = "a".repeat(70);
        let got = suffixed_username(&long, 2).unwrap();
        assert_eq!(got, format!("{}-2", "a".repeat(62)));
        assert!(User::validate_username(&got).is_ok());

        // Base whose 62-char prefix ends in a separator: the stem must be
        // trimmed back to the last alphanumeric before the suffix.
        let mut sp = "a".repeat(61);
        sp.push('-');
        let got = suffixed_username(&sp, 2).unwrap();
        assert_eq!(got, format!("{}-2", "a".repeat(61)));
        assert!(User::validate_username(&got).is_ok());

        // A non-ASCII base cannot produce an ASCII handle: hard error, not
        // a silently mangled fallback.
        assert!(suffixed_username("jörg", 2).is_err());
    }

    #[tokio::test]
    async fn role_extraction_from_jwt_claims() {
        use super::AuthenticatedAgent;

        let state = make_test_state_with_jwt();

        async fn role_handler(auth: AuthenticatedAgent) -> String {
            auth.roles
                .iter()
                .map(|r| r.as_str())
                .collect::<Vec<_>>()
                .join(",")
        }

        let claims = serde_json::json!({
            "tenant_id": "default",
            "sub": "role-test-sub",
            "preferred_username": "eve",
            "realm_access": { "roles": ["admin", "developer"] }
        });
        let token = sign_test_jwt(&claims, 3600);

        let app: Router = Router::new()
            .route("/roles", get(role_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/roles")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let roles_str = std::str::from_utf8(&body).unwrap();
        assert!(roles_str.contains("Admin"), "expected Admin in {roles_str}");
        assert!(
            roles_str.contains("Developer"),
            "expected Developer in {roles_str}"
        );
    }

    #[tokio::test]
    async fn api_key_auth_works() {
        use gyre_common::Id;
        use gyre_domain::User;

        let state = test_state();
        let now = 1000u64;
        // The user must carry a tenant binding (task-099 F1): API-key auth
        // resolves the tenant scope from user.tenant_id and fail-closes
        // when it is absent, so an unbound user would 403 here.
        let mut user = User::new(Id::new("u1"), "ext-1", "frank", now);
        user.tenant_id = Some(Id::new("t1"));
        state.users.create(&user).await.unwrap();
        // Store the SHA-256 hash of the API key (matching auth extractor behaviour).
        let raw_key = "gyre_test_api_key";
        state
            .api_keys
            .create(&super::hash_api_key(raw_key), &user.id, "test-key")
            .await
            .unwrap();

        let app = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer gyre_test_api_key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), b"frank");
    }

    #[tokio::test]
    async fn api_key_auth_rejects_user_without_tenant_binding() {
        // task-099 F1: an API-key user with no tenant binding must fail
        // closed (403), never authenticate as a fabricated tenant scope.
        use gyre_common::Id;
        use gyre_domain::User;

        let state = test_state();
        let user = User::new(Id::new("u2"), "ext-2", "orphan", 1000);
        state.users.create(&user).await.unwrap();
        let raw_key = "gyre_orphan_key";
        state
            .api_keys
            .create(&super::hash_api_key(raw_key), &user.id, "orphan-key")
            .await
            .unwrap();

        let app = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer gyre_orphan_key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn auth_failure_audit_event_recorded_for_api_key_user_without_tenant_binding() {
        // task-099 F1 + task-104: the fail-closed API-key rejection (user
        // has no tenant binding) must be visible in the audit trail as an
        // AuthFailure event naming the credential kind and the reason.
        use gyre_common::Id;
        use gyre_domain::User;

        let state = test_state();
        let user = User::new(Id::new("u3"), "ext-3", "orphan-audited", 1000);
        state.users.create(&user).await.unwrap();
        let raw_key = "gyre_orphan_audited_key";
        state
            .api_keys
            .create(
                &super::hash_api_key(raw_key),
                &user.id,
                "orphan-audited-key",
            )
            .await
            .unwrap();

        let app = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {raw_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let events = audit_events_of(&state, "auth_failure").await;
        assert_eq!(events.len(), 1, "exactly one auth_failure event");
        let e = &events[0];
        assert_eq!(e.detail["token_kind"], "api_key");
        assert_eq!(
            e.detail["rejection_reason"],
            "user has no tenant binding; re-provision via POST /api/v1/users"
        );
        assert_eq!(e.outcome, gyre_domain::AuditOutcome::Failure);
        assert!(e.agent_id.is_none(), "rejected caller has no identity");
    }

    #[tokio::test]
    async fn backward_compat_global_token_still_works_with_jwt_enabled() {
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), b"system");
    }

    #[tokio::test]
    async fn backward_compat_agent_token_with_jwt_enabled() {
        let state = make_test_state_with_jwt();
        state
            .kv_store
            .kv_set("agent_tokens", "agt-1", "agt-secret-xyz".to_string())
            .await
            .unwrap();

        let app = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer agt-secret-xyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), b"agt-1");
    }

    // -- Tenant resolution tests ----------------------------------------------

    #[tokio::test]
    async fn mint_with_workload_roundtrips_wl_image_hash_claim() {
        // agent-runtime.md §3 (Nix-Based Image Build): the image hash is
        // recorded in workload attestation via the `wl_image_hash` JWT
        // claim. The spawn flow resolves the image digest before minting
        // (the token must pre-exist the process) and passes it here — this
        // test proves the claim survives real EdDSA sign → verify with the
        // digest intact, so a verifier can reconstruct workload identity
        // from the token alone.
        let key = crate::auth::AgentSigningKey::generate();
        let issuer = "https://gyre.example.com";

        let token = key
            .mint_with_workload(
                "agt-img-1",
                "task-9",
                "orchestrator-1",
                issuer,
                3600,
                "tenant-acme",
                "ws-1",
                "repo-1",
                "repo-name",
                None,
                None,
                Some(4242),
                Some("worker-host".to_string()),
                Some("container".to_string()),
                Some("stackhash123".to_string()),
                Some("abc123def".to_string()),
                Some("sha256:9f2a7c1e".to_string()),
            )
            .expect("mint_with_workload must succeed");

        let claims = key
            .validate(&token, issuer)
            .expect("validate must verify the EdDSA signature");
        assert_eq!(claims.wl_image_hash.as_deref(), Some("sha256:9f2a7c1e"));
        assert_eq!(claims.wl_pid, Some(4242));
        assert_eq!(claims.wl_hostname.as_deref(), Some("worker-host"));
        assert_eq!(claims.wl_container_id.as_deref(), Some("abc123def"));

        // The no-workload mint must leave the claim absent, not defaulted.
        let bare = key
            .mint(
                "agt-img-2",
                "task-9",
                "orchestrator-1",
                issuer,
                3600,
                "tenant-acme",
                "ws-1",
                "repo-1",
                "repo-name",
            )
            .expect("mint must succeed");
        let bare_claims = key
            .validate(&bare, issuer)
            .expect("validate must verify the EdDSA signature");
        assert_eq!(bare_claims.wl_image_hash, None);
    }

    #[tokio::test]
    async fn jwt_tenant_id_extracted_from_claim() {
        use super::AuthenticatedAgent;

        let state = make_test_state_with_jwt();

        async fn tenant_handler(auth: AuthenticatedAgent) -> String {
            auth.tenant_id
        }

        let claims = serde_json::json!({
            "sub": "tenant-test-sub",
            "preferred_username": "grace",
            "tenant_id": "acme-corp"
        });
        let token = sign_test_jwt(&claims, 3600);

        let app: Router = Router::new()
            .route("/tenant", get(tenant_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/tenant")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), b"acme-corp");
    }

    #[tokio::test]
    async fn jwt_missing_tenant_id_defaults_to_default() {
        use super::AuthenticatedAgent;

        let state = make_test_state_with_jwt();

        async fn tenant_handler(auth: AuthenticatedAgent) -> String {
            auth.tenant_id
        }

        // An IdP that does not issue tenant claims is a single-tenant
        // deployment: the absent claim maps to the bootstrapped "default"
        // tenant via the claim's serde schema default (a deterministic
        // function of the verified token — hierarchy-enforcement.md §1,
        // task-104 verification repair), not a use-site fallback.
        let claims = serde_json::json!({
            "sub": "no-tenant-sub",
            "preferred_username": "henry"
        });
        let token = sign_test_jwt(&claims, 3600);

        let app: Router = Router::new()
            .route("/tenant", get(tenant_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/tenant")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), b"default");
    }

    #[tokio::test]
    async fn jwt_system_tenant_rejected_for_non_admin() {
        let claims = serde_json::json!({
            "sub": "attacker-sub",
            "preferred_username": "attacker",
            "realm_access": { "roles": ["developer"] },
            "tenant_id": "system"
        });
        let token = sign_test_jwt(&claims, 3600);

        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn jwt_system_tenant_allowed_for_admin() {
        use super::AuthenticatedAgent;

        let state = make_test_state_with_jwt();

        async fn tenant_handler(auth: AuthenticatedAgent) -> String {
            auth.tenant_id
        }

        let claims = serde_json::json!({
            "sub": "admin-sub",
            "preferred_username": "alice-admin",
            "realm_access": { "roles": ["admin"] },
            "tenant_id": "system"
        });
        let token = sign_test_jwt(&claims, 3600);

        let app: Router = Router::new()
            .route("/tenant", get(tenant_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/tenant")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), b"system");
    }

    // -- Federation JWT tests (G11) -------------------------------------------

    /// Build a minimal AppState that trusts a given issuer URL.
    fn make_state_with_trusted_issuer(issuer_url: &str) -> Arc<crate::AppState> {
        let base = crate::mem::test_state();
        let mut state = (*base).clone();
        state.trusted_issuers = vec![issuer_url.to_string()];
        Arc::new(state)
    }

    /// Mint an EdDSA JWT using a fresh AgentSigningKey, simulating a remote Gyre.
    fn mint_remote_jwt(
        signing_key: &crate::auth::AgentSigningKey,
        issuer: &str,
        sub: &str,
        ttl_secs: u64,
    ) -> String {
        signing_key
            .mint(
                sub,
                "task-1",
                "system",
                issuer,
                ttl_secs,
                "t1",
                "ws-remote",
                "repo-remote",
                "remote-repo",
            )
            .expect("mint must succeed")
    }

    /// Inject a remote JWKS entry into the state's cache so we don't need an
    /// actual HTTP server for most tests.
    async fn seed_remote_jwks(
        state: &Arc<crate::AppState>,
        issuer: &str,
        remote_key: &crate::auth::AgentSigningKey,
    ) {
        use jsonwebtoken::DecodingKey;
        let jwks: serde_json::Value =
            serde_json::from_str(&remote_key.jwks_json).expect("valid JWKS");
        let keys_arr = jwks["keys"].as_array().expect("keys array");
        let mut map = std::collections::HashMap::new();
        for jwk in keys_arr {
            let jwk_typed: jsonwebtoken::jwk::Jwk =
                serde_json::from_value(jwk.clone()).expect("valid JWK");
            let dk = DecodingKey::from_jwk(&jwk_typed).expect("valid DecodingKey");
            let kid = jwk["kid"].as_str().unwrap_or("default").to_string();
            map.insert(kid, dk);
        }
        state.remote_jwks_cache.write().await.insert(
            issuer.trim_end_matches('/').to_string(),
            crate::auth::RemoteJwksEntry {
                keys: map,
                fetched_at: std::time::Instant::now(),
            },
        );
    }

    #[tokio::test]
    async fn federated_jwt_from_trusted_issuer_accepted() {
        let remote_url = "http://remote-gyre.example.com";
        let state = make_state_with_trusted_issuer(remote_url);
        let remote_key = crate::auth::AgentSigningKey::generate();
        seed_remote_jwks(&state, remote_url, &remote_key).await;

        let token = mint_remote_jwt(&remote_key, remote_url, "agent-abc", 3600);

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = std::str::from_utf8(&bytes).unwrap();
        // agent_id should be prefixed with the remote host
        assert!(
            body.contains("agent-abc"),
            "expected agent-abc in agent_id, got: {body}"
        );
    }

    #[tokio::test]
    async fn federated_jwt_from_untrusted_issuer_rejected() {
        // Trust "http://trusted.example.com" but present a token from "http://evil.example.com"
        let state = make_state_with_trusted_issuer("http://trusted.example.com");
        let evil_key = crate::auth::AgentSigningKey::generate();
        // Do NOT seed the trusted cache — JWKS fetch would fail for the evil issuer anyway,
        // but the issuer check should reject before attempting JWKS fetch.

        let token = mint_remote_jwt(&evil_key, "http://evil.example.com", "evil-agent", 3600);

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn federated_jwt_expired_rejected() {
        let remote_url = "http://remote-gyre.example.com";
        let state = make_state_with_trusted_issuer(remote_url);
        let remote_key = crate::auth::AgentSigningKey::generate();
        seed_remote_jwks(&state, remote_url, &remote_key).await;

        // Negative TTL — token is already expired.
        // We use jsonwebtoken directly to mint an expired token.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let claims = crate::auth::AgentJwtClaims {
            sub: "agent-xyz".to_string(),
            iss: remote_url.to_string(),
            iat: now - 7200,
            exp: now - 3600,
            scope: vec!["repo:remote-repo:write".to_string()],
            jti: "test-jti".to_string(),
            task_id: "task-1".to_string(),
            spawned_by: "system".to_string(),
            tenant_id: "t1".to_string(),
            workspace_id: None,
            repo_id: None,
            orchestrator_type: None,
            persona: None,
            attestation_level: None,
            wl_pid: None,
            wl_hostname: None,
            wl_compute_target: None,
            wl_stack_hash: None,
            wl_container_id: None,
            wl_image_hash: None,
        };
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::EdDSA);
        header.kid = Some(remote_key.kid.clone());
        let token = jsonwebtoken::encode(&header, &claims, &remote_key.encoding_key).expect("mint");

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn federated_jwt_wrong_signature_rejected() {
        let remote_url = "http://remote-gyre.example.com";
        let state = make_state_with_trusted_issuer(remote_url);
        let trusted_key = crate::auth::AgentSigningKey::generate();
        seed_remote_jwks(&state, remote_url, &trusted_key).await;

        // Sign with a DIFFERENT key — same issuer but wrong signature.
        let attacker_key = crate::auth::AgentSigningKey::generate();
        let token = mint_remote_jwt(&attacker_key, remote_url, "agent-evil", 3600);

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn federated_jwt_no_trusted_issuers_rejected() {
        // State with no trusted issuers — federation step skipped entirely.
        let state = test_state(); // default test_state has trusted_issuers: vec![]
        let remote_key = crate::auth::AgentSigningKey::generate();
        let token = mint_remote_jwt(&remote_key, "http://some-gyre.example.com", "agent-1", 3600);

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn jwt_invalid_tenant_id_chars_rejected() {
        let claims = serde_json::json!({
            "sub": "bad-tenant-sub",
            "preferred_username": "badguy",
            "tenant_id": "acme; DROP TABLE tenants"
        });
        let token = sign_test_jwt(&claims, 3600);

        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // -- task-110 F2: invitation → SSO email link --------------------------------

    /// A JWT login with a verified email matching an invitation-created
    /// account (tenant-namespaced external_id) claims that account: the
    /// external_id is re-pointed at the Keycloak subject, so the
    /// pre-assigned memberships and invited GlobalRole ride the SSO
    /// identity instead of a duplicate account.
    #[tokio::test]
    async fn jwt_email_link_claims_invitation_account() {
        let state = make_test_state_with_jwt();

        // Invitation-created local account (as accept_tenant_invitation
        // would create it).
        let mut invited = gyre_domain::User::new(
            gyre_common::Id::new("invited-user-1"),
            "tenant:acme:email:alice@example.com",
            "alice",
            1000,
        );
        invited.email = Some("alice@example.com".to_string());
        invited.tenant_id = Some(gyre_common::Id::new("acme"));
        state.users.create(&invited).await.unwrap();

        // First SSO login: sub differs, email matches, verified.
        let claims = serde_json::json!({
            "sub": "keycloak-sub-alice",
            "preferred_username": "alice",
            "email": "alice@example.com",
            "email_verified": true,
            "tenant_id": "acme"
        });
        let token = sign_test_jwt(&claims, 3600);
        let resp = app_with_jwt_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // The account was claimed: resolved by the SSO subject, no
        // duplicate user created, and the tenant scope is preserved.
        let claimed = state
            .users
            .find_by_external_id("keycloak-sub-alice")
            .await
            .unwrap()
            .expect("account re-pointed to SSO subject");
        assert_eq!(claimed.id, invited.id, "same account, not a duplicate");
        assert_eq!(claimed.tenant_id, Some(gyre_common::Id::new("acme")));

        // The old namespaced id no longer resolves (single account).
        assert!(state
            .users
            .find_by_external_id("tenant:acme:email:alice@example.com")
            .await
            .unwrap()
            .is_none());
    }

    /// An UNVERIFIED email must not claim the invitation account — an
    /// attacker who can set an arbitrary unverified email in the IdP
    /// must not inherit the invitee's tenant memberships.
    #[tokio::test]
    async fn jwt_unverified_email_does_not_claim_invitation_account() {
        let state = make_test_state_with_jwt();
        let mut invited = gyre_domain::User::new(
            gyre_common::Id::new("invited-user-2"),
            "tenant:acme:email:bob@example.com",
            "bob",
            1000,
        );
        invited.email = Some("bob@example.com".to_string());
        invited.tenant_id = Some(gyre_common::Id::new("acme"));
        state.users.create(&invited).await.unwrap();

        let claims = serde_json::json!({
            "sub": "attacker-sub",
            "preferred_username": "mallory",
            "email": "bob@example.com",
            "email_verified": false,
            "tenant_id": "acme"
        });
        let token = sign_test_jwt(&claims, 3600);
        let resp = app_with_jwt_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // A new account was provisioned; the invitation account is
        // untouched (still resolvable by its namespaced id).
        let fresh = state
            .users
            .find_by_external_id("attacker-sub")
            .await
            .unwrap()
            .expect("auto-provisioned new account");
        assert_ne!(fresh.id, invited.id);
        let still_there = state
            .users
            .find_by_external_id("tenant:acme:email:bob@example.com")
            .await
            .unwrap()
            .expect("invitation account was NOT claimed");
        assert_eq!(still_there.id, invited.id);
    }

    /// Cross-tenant emails do not link: same email in a different
    /// tenant must not claim the account (tenants are isolation
    /// boundaries).
    #[tokio::test]
    async fn jwt_email_link_does_not_cross_tenants() {
        let state = make_test_state_with_jwt();
        let mut invited = gyre_domain::User::new(
            gyre_common::Id::new("invited-user-3"),
            "tenant:acme:email:carol@example.com",
            "carol",
            1000,
        );
        invited.email = Some("carol@example.com".to_string());
        invited.tenant_id = Some(gyre_common::Id::new("acme"));
        state.users.create(&invited).await.unwrap();

        // Same email, but the JWT tenant is "other".
        let claims = serde_json::json!({
            "sub": "other-tenant-sub",
            "preferred_username": "carol",
            "email": "carol@example.com",
            "email_verified": true,
            "tenant_id": "other"
        });
        let token = sign_test_jwt(&claims, 3600);
        let resp = app_with_jwt_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // New account in the other tenant; acme account untouched.
        let fresh = state
            .users
            .find_by_external_id("other-tenant-sub")
            .await
            .unwrap()
            .expect("auto-provisioned new account");
        assert_ne!(fresh.id, invited.id);
        assert!(
            state
                .users
                .find_by_external_id("tenant:acme:email:carol@example.com")
                .await
                .unwrap()
                .is_some(),
            "acme account was NOT claimed by another tenant's login"
        );
    }

    // -- G11-A SSRF guard tests -----------------------------------------------

    #[test]
    fn minted_jwt_carries_unique_jti() {
        // AgentTokenRevoked audit events attribute revocations by jti, so
        // every minted token must carry a unique, extractable JWT ID that
        // matches the validated claims.
        let key = super::AgentSigningKey::generate();
        let issuer = "http://localhost:3000";
        let t1 = key
            .mint(
                "agent-1",
                "task-1",
                "system",
                issuer,
                3600,
                "t1",
                "ws-1",
                "repo-1",
                "repo-1-name",
            )
            .expect("mint");
        let t2 = key
            .mint(
                "agent-1",
                "task-1",
                "system",
                issuer,
                3600,
                "t1",
                "ws-1",
                "repo-1",
                "repo-1-name",
            )
            .expect("mint");

        let j1 = key
            .extract_jti_unverified(&t1)
            .expect("minted token must carry a jti");
        let j2 = key
            .extract_jti_unverified(&t2)
            .expect("minted token must carry a jti");
        assert_ne!(j1, j2, "each minted token gets a fresh jti");

        // The extracted jti matches the signature-validated claims.
        let claims = key.validate(&t1, issuer).expect("validate");
        assert_eq!(claims.jti, j1);

        // Non-JWT input yields no jti — never a fabricated identifier.
        assert!(key.extract_jti_unverified("not-a-jwt").is_none());
        assert!(key.extract_jti_unverified("").is_none());
    }

    #[test]
    fn is_same_origin_same_host_accepted() {
        assert!(super::is_same_origin(
            "https://remote-gyre.example.com",
            "https://remote-gyre.example.com/jwks"
        ));
    }

    #[test]
    fn is_same_origin_different_host_rejected() {
        assert!(!super::is_same_origin(
            "https://remote-gyre.example.com",
            "http://169.254.169.254/latest/meta-data/iam/security-credentials/"
        ));
    }

    #[test]
    fn is_same_origin_different_scheme_rejected() {
        assert!(!super::is_same_origin(
            "https://remote-gyre.example.com",
            "http://remote-gyre.example.com/jwks"
        ));
    }

    #[test]
    fn is_same_origin_malformed_uri_rejected() {
        assert!(!super::is_same_origin(
            "https://remote-gyre.example.com",
            "not a url"
        ));
    }

    #[test]
    fn is_same_origin_different_port_rejected() {
        assert!(!super::is_same_origin(
            "https://remote-gyre.example.com",
            "https://remote-gyre.example.com:9999/jwks"
        ));
    }

    #[tokio::test]
    async fn cross_origin_jwks_uri_rejected_by_ssrf_guard() {
        use axum::routing::get;
        use axum::Router;
        use http::{Request, StatusCode};
        use tower::ServiceExt;

        // Build a state that trusts our "remote" issuer.
        let remote_url = "http://remote-gyre.example.com";
        let state = make_state_with_trusted_issuer(remote_url);
        let remote_key = crate::auth::AgentSigningKey::generate();

        // Instead of injecting via seed_remote_jwks (which bypasses the HTTP
        // fetch), we inject a *stale* cache entry so the code re-fetches.
        // But since we can't start an HTTP server here, we rely on the unit
        // test for is_same_origin above to cover the guard logic directly.
        //
        // This integration-level test verifies that when the cache is empty
        // and the issuer is trusted, but no real JWKS can be fetched (network
        // unavailable in test), the token is ultimately rejected.
        let token = mint_remote_jwt(&remote_key, remote_url, "agent-ssrf", 3600);

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

        // No JWKS seeded → fetch_remote_jwks_for_issuer will fail (no real
        // server) → token rejected.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn non_jwt_auth_uses_default_tenant() {
        use super::AuthenticatedAgent;

        let state = test_state();

        async fn tenant_handler(auth: AuthenticatedAgent) -> String {
            auth.tenant_id
        }

        let app = Router::new()
            .route("/tenant", get(tenant_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/tenant")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), b"default");
    }

    #[tokio::test]
    async fn jwt_wrong_kid_rejected() {
        // Sign a JWT with a kid that doesn't exist in the cache.
        // We build it manually using ring.
        use base64::{
            engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
            Engine as _,
        };
        use ring::rand::SystemRandom;
        use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};

        use crate::auth::test_helpers;
        let private_der = STANDARD
            .decode(test_helpers::TEST_RSA_PRIVATE_DER_B64)
            .unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let header = serde_json::json!({"alg":"RS256","typ":"JWT","kid":"unknown-kid"});
        let payload = serde_json::json!({
            "sub": "x",
            "iss": "http://localhost:8080/realms/gyre",
            "iat": now,
            "exp": now + 3600,
        });

        let h = URL_SAFE_NO_PAD.encode(serde_json::to_string(&header).unwrap());
        let p = URL_SAFE_NO_PAD.encode(serde_json::to_string(&payload).unwrap());
        let signing_input = format!("{h}.{p}");

        let key_pair = RsaKeyPair::from_pkcs8(&private_der).unwrap();
        let rng = SystemRandom::new();
        let mut sig = vec![0u8; key_pair.public().modulus_len()];
        key_pair
            .sign(&RSA_PKCS1_SHA256, &rng, signing_input.as_bytes(), &mut sig)
            .unwrap();

        let token = format!("{}.{}", signing_input, URL_SAFE_NO_PAD.encode(&sig));

        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_ws_ticket_issue_and_consume() {
        let store = WsTicketStore::new();
        let auth = AuthenticatedAgent {
            agent_id: "test-user".to_string(),
            user_id: None,
            roles: vec![UserRole::Admin],
            tenant_id: "default".to_string(),
            workspace_id: None,
            repo_id: None,
            scope: Vec::new(),
            jwt_claims: None,
            bearer_token: None,
            deprecated_token_auth: false,
            impersonation: None,
        };
        let ticket = store.issue(auth);
        assert!(!ticket.is_empty());

        // First consume succeeds
        let result = store.consume(&ticket);
        assert!(result.is_some());
        assert_eq!(result.unwrap().agent_id, "test-user");

        // Second consume fails (single-use)
        assert!(store.consume(&ticket).is_none());
    }

    #[test]
    fn test_ws_ticket_invalid_ticket() {
        let store = WsTicketStore::new();
        assert!(store.consume("nonexistent-ticket").is_none());
    }

    #[test]
    fn test_ws_ticket_does_not_expose_real_token() {
        let store = WsTicketStore::new();
        let auth = AuthenticatedAgent {
            agent_id: "test-user".to_string(),
            user_id: None,
            roles: vec![UserRole::Admin],
            tenant_id: "default".to_string(),
            workspace_id: None,
            repo_id: None,
            scope: Vec::new(),
            jwt_claims: None,
            bearer_token: None,
            deprecated_token_auth: false,
            impersonation: None,
        };
        let ticket = store.issue(auth);
        // Ticket is a UUID, not a Bearer token or API key
        assert!(ticket.len() == 36, "Ticket should be a UUID format");
        assert!(!ticket.starts_with("ey"), "Ticket must not be a JWT");
    }

    // ── task-107 F12: key persistence across restarts ─────────────────────

    /// A key reconstructed from persisted PKCS#8 bytes is the same key:
    /// same kid (so stored `signing_key_id` values keep matching), same
    /// public key, and signatures made by the original verify under the
    /// loaded one (and vice versa).
    #[test]
    fn from_pkcs8_roundtrips_key_material_and_kid() {
        let original = AgentSigningKey::generate();
        let reloaded = AgentSigningKey::from_pkcs8(original.pkcs8_der()).unwrap();

        assert_eq!(
            original.kid, reloaded.kid,
            "kid must be derived from key material so reloads keep it stable"
        );
        assert_eq!(original.public_key_bytes, reloaded.public_key_bytes);
        assert_eq!(original.jwks_json, reloaded.jwks_json);

        let message = b"commit-sha-under-test";
        let sig = original.sign_bytes(message);
        assert_eq!(sig, reloaded.sign_bytes(message));

        use ring::signature::{self, UnparsedPublicKey};
        let pk = UnparsedPublicKey::new(&signature::ED25519, &reloaded.public_key_bytes);
        assert!(
            pk.verify(message, &sig).is_ok(),
            "signature by the original key must verify under the reloaded key"
        );
    }

    /// Distinct key material must yield distinct kids — otherwise a key
    /// rotation would be indistinguishable from the same key and the
    /// `local_mode` kid check would be vacuous.
    #[test]
    fn kid_is_distinct_per_key() {
        let a = AgentSigningKey::generate();
        let b = AgentSigningKey::generate();
        assert_ne!(a.kid, b.kid);
        assert_ne!(a.public_key_bytes, b.public_key_bytes);
    }

    /// Garbage or wrong-algorithm PKCS#8 must be rejected with an error,
    /// not accepted as a usable key.
    #[test]
    fn from_pkcs8_rejects_invalid_material() {
        assert!(AgentSigningKey::from_pkcs8(b"").is_err());
        assert!(AgentSigningKey::from_pkcs8(b"not pkcs8 at all").is_err());
        // 32 bytes of zeros is a valid length for a raw Ed25519 seed but is
        // not PKCS#8 DER.
        assert!(AgentSigningKey::from_pkcs8(&[0u8; 32]).is_err());
    }

    #[test]
    fn is_system_principal_discriminates_spoof_shapes() {
        // Task-099 revision: API-key and Keycloak-JWT paths set agent_id
        // from user.display_name / preferred_username, so a user NAMED
        // "system" would spoof every `agent_id == "system"` gate (ABAC
        // bypass, tenant-containment bypasses) if the check were a plain
        // string match. `user_id.is_none()` is the discriminator a
        // spoofing user cannot forge: their key/JWT resolves to their own
        // user id.
        let global_token = AuthenticatedAgent {
            agent_id: "system".to_string(),
            user_id: None,
            roles: vec![UserRole::Admin],
            tenant_id: system_principal_tenant(),
            workspace_id: None,
            repo_id: None,
            scope: Vec::new(),
            jwt_claims: None,
            bearer_token: None,
            deprecated_token_auth: false,
            impersonation: None,
        };
        assert!(global_token.is_system_principal());

        // API-key user named "system": agent_id matches, user_id doesn't.
        let spoofed = AuthenticatedAgent {
            agent_id: "system".to_string(),
            user_id: Some(gyre_common::Id::new("u-sys")),
            roles: vec![UserRole::Admin],
            tenant_id: "t-2".to_string(),
            workspace_id: None,
            repo_id: None,
            scope: Vec::new(),
            jwt_claims: None,
            bearer_token: None,
            deprecated_token_auth: false,
            impersonation: None,
        };
        assert!(!spoofed.is_system_principal());

        // Ordinary user: neither field matches.
        let ordinary = AuthenticatedAgent {
            agent_id: "jsell".to_string(),
            user_id: Some(gyre_common::Id::new("u-1")),
            roles: vec![UserRole::Admin],
            tenant_id: "t-1".to_string(),
            workspace_id: None,
            repo_id: None,
            scope: Vec::new(),
            jwt_claims: None,
            bearer_token: None,
            deprecated_token_auth: false,
            impersonation: None,
        };
        assert!(!ordinary.is_system_principal());
    }
}
