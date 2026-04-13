//! Agent stack fingerprinting (M14.1) and push attestation policy (M14.2) API.
//!
//! Stack fingerprinting lets agents self-report their runtime configuration
//! (AGENTS.md version, hooks, MCP servers, model, CLI version, etc.).  The
//! server computes a SHA-256 fingerprint over canonical JSON and stores it.
//!
//! Attestation policies let admins pin a required stack fingerprint per repo.
//! The `stack-attestation` pre-accept gate rejects pushes whose agent fingerprint
//! does not match the repo policy.
//!
//! Routes:
//!   POST /api/v1/agents/:id/stack         — agent reports its stack
//!   GET  /api/v1/agents/:id/stack         — query agent's registered stack
//!   GET  /api/v1/repos/:id/stack-policy   — get repo attestation policy
//!   PUT  /api/v1/repos/:id/stack-policy   — set repo policy (Admin only)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

use super::error::ApiError;

// ---------------------------------------------------------------------------
// AgentStack domain type
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HookEntry {
    pub id: String,
    pub hash: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpServerEntry {
    pub name: String,
    pub version: String,
    pub config_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentStack {
    /// SHA-256 hash of the AGENTS.md / CLAUDE.md file at agent startup.
    pub agents_md_hash: String,
    /// Pre-commit / pre-push hooks with their content hashes.
    pub hooks: Vec<HookEntry>,
    /// MCP servers the agent has configured.
    pub mcp_servers: Vec<McpServerEntry>,
    /// Model identifier (e.g. "claude-sonnet-4-6").
    pub model: String,
    /// CLI version string (e.g. "1.2.3").
    pub cli_version: String,
    /// SHA-256 hash of settings.json / settings.local.json.
    pub settings_hash: String,
    /// Optional SHA-256 hash of the persona / system-prompt file.
    pub persona_hash: Option<String>,
}

impl AgentStack {
    /// Compute a SHA-256 fingerprint of the stack by hashing canonical JSON
    /// with keys sorted alphabetically.  The resulting hex string uniquely
    /// identifies a particular agent configuration.
    pub fn fingerprint(&self) -> String {
        // Build a canonical JSON representation with sorted keys.
        let canonical = serde_json::json!({
            "agents_md_hash": self.agents_md_hash,
            "cli_version": self.cli_version,
            "hooks": self.hooks.iter().map(|h| serde_json::json!({
                "enabled": h.enabled,
                "hash": h.hash,
                "id": h.id,
            })).collect::<Vec<_>>(),
            "mcp_servers": self.mcp_servers.iter().map(|m| serde_json::json!({
                "config_hash": m.config_hash,
                "name": m.name,
                "version": m.version,
            })).collect::<Vec<_>>(),
            "model": self.model,
            "persona_hash": self.persona_hash,
            "settings_hash": self.settings_hash,
        });

        let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let result = hasher.finalize();
        result.iter().map(|b| format!("{b:02x}")).collect()
    }
}

// ---------------------------------------------------------------------------
// Repo stack policy type
// ---------------------------------------------------------------------------

/// Structured repo stack policy stored in KV (`repo_stack_policies`).
///
/// Contains both the required fingerprint and the minimum attestation level.
/// Level 2 = stack-attested (fingerprint match), Level 3 = container-verified
/// (supply-chain.md §2, Policy per Level).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepoStackPolicy {
    pub fingerprint: String,
    pub required_level: i64,
}

/// Parse a `repo_stack_policies` KV entry value.
///
/// Handles both the structured JSON format (`{"fingerprint": "...", "required_level": N}`)
/// and the legacy plain-string format (just a fingerprint string, defaulting to Level 2).
pub fn parse_stack_policy(value: &str) -> RepoStackPolicy {
    serde_json::from_str::<RepoStackPolicy>(value).unwrap_or_else(|_| {
        // Backward compat: legacy entries store just the fingerprint string.
        RepoStackPolicy {
            fingerprint: value.to_string(),
            required_level: 2,
        }
    })
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// POST body — agent self-reports its stack.
pub type RegisterStackRequest = AgentStack;

#[derive(Serialize)]
pub struct StackResponse {
    pub agent_id: String,
    pub stack: AgentStack,
    pub fingerprint: String,
}

#[derive(Deserialize)]
pub struct SetStackPolicyRequest {
    /// Required fingerprint (SHA-256 hex).  Pass `null` to clear the policy.
    pub required_fingerprint: Option<String>,
    /// Minimum attestation level required (2 = stack-attested, 3 = container-verified).
    /// Defaults to 2 if omitted (supply-chain.md §2).
    pub required_level: Option<i64>,
}

#[derive(Serialize)]
pub struct StackPolicyResponse {
    pub repo_id: String,
    pub required_fingerprint: Option<String>,
    /// Minimum attestation level required by this policy. `None` when no policy is set.
    pub required_level: Option<i64>,
}

// ---------------------------------------------------------------------------
// Handlers — Agent stack
// ---------------------------------------------------------------------------

/// POST /api/v1/agents/:id/stack — agent self-reports its runtime stack.
pub async fn register_stack(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<String>,
    Json(stack): Json<RegisterStackRequest>,
) -> Result<(StatusCode, Json<StackResponse>), ApiError> {
    // Verify agent exists.
    state
        .agents
        .find_by_id(&Id::new(&agent_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {agent_id} not found")))?;

    let fingerprint = stack.fingerprint();

    let json = serde_json::to_string(&stack).map_err(|e| ApiError::Internal(e.into()))?;
    state
        .kv_store
        .kv_set("agent_stacks", &agent_id, json)
        .await
        .map_err(ApiError::Internal)?;

    Ok((
        StatusCode::CREATED,
        Json(StackResponse {
            agent_id,
            stack,
            fingerprint,
        }),
    ))
}

/// GET /api/v1/agents/:id/stack — query an agent's registered stack.
pub async fn get_stack(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<String>,
) -> Result<Json<StackResponse>, ApiError> {
    // Verify agent exists.
    state
        .agents
        .find_by_id(&Id::new(&agent_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {agent_id} not found")))?;

    let stack = state
        .kv_store
        .kv_get("agent_stacks", &agent_id)
        .await
        .map_err(ApiError::Internal)?
        .and_then(|s| serde_json::from_str::<AgentStack>(&s).ok())
        .ok_or_else(|| ApiError::NotFound(format!("no stack registered for agent {agent_id}")))?;

    let fingerprint = stack.fingerprint();

    Ok(Json(StackResponse {
        agent_id,
        stack,
        fingerprint,
    }))
}

// ---------------------------------------------------------------------------
// Handlers — Repo stack policy
// ---------------------------------------------------------------------------

/// GET /api/v1/repos/:id/stack-policy — get the required stack fingerprint.
pub async fn get_stack_policy(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<StackPolicyResponse>, ApiError> {
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let raw = state
        .kv_store
        .kv_get("repo_stack_policies", &repo_id)
        .await
        .ok()
        .flatten();

    match raw {
        Some(value) => {
            let policy = parse_stack_policy(&value);
            Ok(Json(StackPolicyResponse {
                repo_id,
                required_fingerprint: Some(policy.fingerprint),
                required_level: Some(policy.required_level),
            }))
        }
        None => Ok(Json(StackPolicyResponse {
            repo_id,
            required_fingerprint: None,
            required_level: None,
        })),
    }
}

/// PUT /api/v1/repos/:id/stack-policy — set (or clear) the required stack fingerprint.
///
/// Admin only: stack attestation pins the required build environment for a repo.
/// Allowing agents to modify or clear this requirement would let agents bypass
/// stack integrity enforcement (NEW-39).
pub async fn set_stack_policy(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(repo_id): Path<String>,
    Json(req): Json<SetStackPolicyRequest>,
) -> Result<Json<StackPolicyResponse>, ApiError> {
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "only Admin role may update stack attestation policy".to_string(),
        ));
    }

    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    match &req.required_fingerprint {
        Some(fp) => {
            let level = req.required_level.unwrap_or(2);
            let policy = RepoStackPolicy {
                fingerprint: fp.clone(),
                required_level: level,
            };
            let json = serde_json::to_string(&policy).map_err(|e| ApiError::Internal(e.into()))?;
            state
                .kv_store
                .kv_set("repo_stack_policies", &repo_id, json)
                .await
                .map_err(ApiError::Internal)?;
            Ok(Json(StackPolicyResponse {
                repo_id,
                required_fingerprint: Some(fp.clone()),
                required_level: Some(level),
            }))
        }
        None => {
            let _ = state
                .kv_store
                .kv_remove("repo_stack_policies", &repo_id)
                .await;
            Ok(Json(StackPolicyResponse {
                repo_id,
                required_fingerprint: None,
                required_level: None,
            }))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_domain::Agent;
    use gyre_domain::Repository;
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app_with_agent_and_repo() -> (Router, std::sync::Arc<AppState>) {
        let state = test_state();
        let agent = Agent::new(gyre_common::Id::new("agent-1"), "test-agent", 0);
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(state.agents.create(&agent))
                .unwrap();
        });
        let repo = Repository::new(
            gyre_common::Id::new("repo-1"),
            gyre_common::Id::new("proj-1"),
            "test-repo",
            "/tmp/test-repo",
            0,
        );
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(state.repos.create(&repo))
                .unwrap();
        });
        let app = crate::api::api_router().with_state(state.clone());
        (app, state)
    }

    fn sample_stack_body() -> serde_json::Value {
        serde_json::json!({
            "agents_md_hash": "deadbeef",
            "hooks": [{"id": "pre-commit", "hash": "cafebabe", "enabled": true}],
            "mcp_servers": [{"name": "odis", "version": "1.0", "config_hash": "abc123"}],
            "model": "claude-sonnet-4-6",
            "cli_version": "1.2.3",
            "settings_hash": "aaaa",
            "persona_hash": null
        })
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn register_and_get_stack() {
        let (app, _state) = app_with_agent_and_repo();

        // POST stack
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/agent-1/stack")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&sample_stack_body()).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let fp = json["fingerprint"].as_str().unwrap().to_string();
        assert_eq!(fp.len(), 64); // SHA-256 hex

        // GET stack
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/agent-1/stack")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["fingerprint"].as_str().unwrap().len(), 64);
        assert_eq!(json["agent_id"].as_str().unwrap(), "agent-1");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_stack_not_found() {
        let (app, _state) = app_with_agent_and_repo();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/agent-1/stack")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_and_get_stack_policy() {
        let (app, _state) = app_with_agent_and_repo();

        let body = serde_json::json!({ "required_fingerprint": "abc123def456" });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/repo-1/stack-policy")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(
            json["required_fingerprint"].as_str().unwrap(),
            "abc123def456"
        );
        // Defaults to level 2 when not specified.
        assert_eq!(json["required_level"].as_i64().unwrap(), 2);

        // GET returns same policy
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/stack-policy")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(
            json["required_fingerprint"].as_str().unwrap(),
            "abc123def456"
        );
        assert_eq!(json["required_level"].as_i64().unwrap(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_and_get_stack_policy_level3() {
        let (app, _state) = app_with_agent_and_repo();

        let body =
            serde_json::json!({ "required_fingerprint": "abc123def456", "required_level": 3 });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/repo-1/stack-policy")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(
            json["required_fingerprint"].as_str().unwrap(),
            "abc123def456"
        );
        assert_eq!(json["required_level"].as_i64().unwrap(), 3);

        // GET returns Level 3 policy
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/stack-policy")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(
            json["required_fingerprint"].as_str().unwrap(),
            "abc123def456"
        );
        assert_eq!(json["required_level"].as_i64().unwrap(), 3);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn clear_stack_policy() {
        let (app, _state) = app_with_agent_and_repo();

        // Set policy
        let body = serde_json::json!({ "required_fingerprint": "fp1" });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/repo-1/stack-policy")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Clear policy
        let body = serde_json::json!({ "required_fingerprint": null });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/repo-1/stack-policy")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["required_fingerprint"].is_null());
        assert!(json["required_level"].is_null());

        // GET returns null
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/stack-policy")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        assert!(json["required_fingerprint"].is_null());
        assert!(json["required_level"].is_null());
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let stack = AgentStack {
            agents_md_hash: "hash1".to_string(),
            hooks: vec![HookEntry {
                id: "pre-commit".to_string(),
                hash: "cafebabe".to_string(),
                enabled: true,
            }],
            mcp_servers: vec![],
            model: "claude-sonnet-4-6".to_string(),
            cli_version: "1.0.0".to_string(),
            settings_hash: "settings1".to_string(),
            persona_hash: None,
        };
        let fp1 = stack.fingerprint();
        let fp2 = stack.fingerprint();
        assert_eq!(fp1, fp2);
        assert_eq!(fp1.len(), 64);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn agent_role_cannot_set_stack_policy() {
        // NEW-39 regression: Agent role must be rejected with 403.
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(seed_builtin_policies(&state))
        });

        let repo = gyre_domain::Repository::new(
            gyre_common::Id::new("repo-jwt"),
            gyre_common::Id::new("proj-1"),
            "jwt-repo",
            "/tmp/jwt-repo",
            0,
        );
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(state.repos.create(&repo))
                .unwrap();
        });

        let agent_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "rogue-agent",
                "preferred_username": "rogue-agent",
                "realm_access": { "roles": ["agent"] }
            }),
            3600,
        );

        let body = serde_json::json!({ "required_fingerprint": null });

        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/repo-jwt/stack-policy")
                    .header("authorization", format!("Bearer {agent_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "agent role must not modify stack attestation policy (NEW-39)"
        );
    }

    #[test]
    fn parse_stack_policy_structured_json() {
        let json = r#"{"fingerprint":"sha256:abc","required_level":3}"#;
        let policy = parse_stack_policy(json);
        assert_eq!(policy.fingerprint, "sha256:abc");
        assert_eq!(policy.required_level, 3);
    }

    #[test]
    fn parse_stack_policy_legacy_plain_string() {
        // Legacy entries store just the fingerprint string, not JSON.
        let policy = parse_stack_policy("sha256:legacy-fp");
        assert_eq!(policy.fingerprint, "sha256:legacy-fp");
        assert_eq!(policy.required_level, 2); // backward compat default
    }

    #[test]
    fn different_stacks_have_different_fingerprints() {
        let stack1 = AgentStack {
            agents_md_hash: "hash1".to_string(),
            hooks: vec![],
            mcp_servers: vec![],
            model: "claude-sonnet-4-6".to_string(),
            cli_version: "1.0.0".to_string(),
            settings_hash: "settings1".to_string(),
            persona_hash: None,
        };
        let stack2 = AgentStack {
            agents_md_hash: "hash2".to_string(),
            ..stack1.clone()
        };
        assert_ne!(stack1.fingerprint(), stack2.fingerprint());
    }
}
