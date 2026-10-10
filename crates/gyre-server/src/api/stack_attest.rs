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
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

use super::error::ApiError;

// AgentStack / HookEntry / McpServerEntry live in gyre-domain so the CLI can
// compute and verify the SAME fingerprint over the SAME schema (the server
// no longer owns a private copy). Re-exported here to keep existing import
// paths (git_http.rs, spawn.rs, pre_accept.rs) working.
pub use gyre_domain::stack::{AgentStack, HookEntry, McpServerEntry, StackLockfile};

// ---------------------------------------------------------------------------
// Repo stack policy type
// ---------------------------------------------------------------------------

/// Structured repo stack policy stored in KV (`repo_stack_policies`).
///
/// Contains the required fingerprint, the minimum attestation level
/// (supply-chain.md §Attestation Levels), and the enforcement mode applied
/// when a push arrives from an agent below the minimum:
/// - `Block` (default): reject the push,
/// - `Warn`: accept the push and emit a `constraint_violation` event.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RepoStackPolicy {
    pub fingerprint: String,
    pub required_level: i64,
    /// Minimum attestation level (1..=3). Kept in sync with `required_level`:
    /// the legacy field continues to drive merge-constraint evaluation
    /// (constraint_check.rs) and provenance, while `min_attestation_level`
    /// drives push-time enforcement (git_http.rs).
    #[serde(default = "default_min_attestation_level")]
    pub min_attestation_level: u8,
    /// How below-minimum pushes are handled (supply-chain.md §Policy per
    /// Level: "rejected or flagged based on policy").
    #[serde(default = "default_enforcement")]
    pub enforcement: StackEnforcement,
}

/// Enforcement mode for below-minimum attestation pushes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StackEnforcement {
    /// Reject the push (default — security by default).
    Block,
    /// Accept the push but flag it (ConstraintViolation event + log).
    Warn,
}

fn default_min_attestation_level() -> u8 {
    2
}

fn default_enforcement() -> StackEnforcement {
    StackEnforcement::Block
}

impl StackEnforcement {
    pub fn as_str(&self) -> &'static str {
        match self {
            StackEnforcement::Block => "block",
            StackEnforcement::Warn => "warn",
        }
    }
}

/// Parse a `repo_stack_policies` KV entry value.
///
/// Handles the structured JSON format
/// (`{"fingerprint": "...", "required_level": N, "min_attestation_level": M,
///    "enforcement": "block"|"warn"}`)
/// and the legacy plain-string format (just a fingerprint string, defaulting
/// to Level 2 / block). Legacy JSON entries without the new fields get them
/// derived: `min_attestation_level = required_level.clamp(1, 3)` — a legacy
/// level-3 policy carried exactly the "container-verified only" intent.
pub fn parse_stack_policy(value: &str) -> RepoStackPolicy {
    match serde_json::from_str::<RepoStackPolicy>(value) {
        Ok(mut policy) => {
            // Keep the two level fields consistent regardless of which one
            // the stored JSON carried.
            let min = policy.required_level.clamp(1, 3) as u8;
            if policy.min_attestation_level == default_min_attestation_level()
                && min != default_min_attestation_level()
            {
                policy.min_attestation_level = min;
            }
            policy.required_level = policy.min_attestation_level as i64;
            policy
        }
        Err(_) => {
            // Backward compat: legacy entries store just the fingerprint string.
            RepoStackPolicy {
                fingerprint: value.to_string(),
                required_level: 2,
                min_attestation_level: 2,
                enforcement: StackEnforcement::Block,
            }
        }
    }
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
    /// Minimum attestation level (1..=3) enforced at push time
    /// (supply-chain.md §Policy per Level). Defaults to `required_level`.
    pub min_attestation_level: Option<u8>,
    /// Enforcement mode for below-minimum pushes: "block" (default) or "warn".
    pub enforcement: Option<String>,
}

#[derive(Serialize)]
pub struct StackPolicyResponse {
    pub repo_id: String,
    pub required_fingerprint: Option<String>,
    /// Minimum attestation level required by this policy. `None` when no policy is set.
    pub required_level: Option<i64>,
    /// Minimum attestation level (1..=3) enforced at push time.
    pub min_attestation_level: Option<u8>,
    /// Enforcement mode for below-minimum pushes.
    pub enforcement: Option<String>,
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
                min_attestation_level: Some(policy.min_attestation_level),
                enforcement: Some(policy.enforcement.as_str().to_string()),
            }))
        }
        None => Ok(Json(StackPolicyResponse {
            repo_id,
            required_fingerprint: None,
            required_level: None,
            min_attestation_level: None,
            enforcement: None,
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
            if !(1..=3).contains(&level) {
                return Err(ApiError::InvalidInput(format!(
                    "required_level must be 1..=3, got {level}"
                )));
            }
            let min_level = req.min_attestation_level.unwrap_or(level.clamp(1, 3) as u8);
            if !(1..=3).contains(&min_level) {
                return Err(ApiError::InvalidInput(format!(
                    "min_attestation_level must be 1..=3, got {min_level}"
                )));
            }
            let enforcement = match req.enforcement.as_deref() {
                None => StackEnforcement::Block,
                Some("block") => StackEnforcement::Block,
                Some("warn") => StackEnforcement::Warn,
                Some(other) => {
                    return Err(ApiError::InvalidInput(format!(
                        "enforcement must be \"block\" or \"warn\", got \"{other}\""
                    )));
                }
            };
            let policy = RepoStackPolicy {
                fingerprint: fp.clone(),
                required_level: min_level as i64,
                min_attestation_level: min_level,
                enforcement,
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
                required_level: Some(min_level as i64),
                min_attestation_level: Some(min_level),
                enforcement: Some(policy.enforcement.as_str().to_string()),
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
                min_attestation_level: None,
                enforcement: None,
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

    #[test]
    fn parse_stack_policy_legacy_level3_gets_min_level_3() {
        // Legacy JSON entries without the new fields derive them:
        // required_level 3 carried the "container-verified only" intent.
        let json = r#"{"fingerprint":"sha256:abc","required_level":3}"#;
        let policy = parse_stack_policy(json);
        assert_eq!(policy.min_attestation_level, 3);
        assert_eq!(policy.required_level, 3);
        assert_eq!(policy.enforcement, StackEnforcement::Block);
    }

    #[test]
    fn parse_stack_policy_new_fields_roundtrip() {
        let policy = RepoStackPolicy {
            fingerprint: "sha256:abc".to_string(),
            required_level: 2,
            min_attestation_level: 2,
            enforcement: StackEnforcement::Warn,
        };
        let json = serde_json::to_string(&policy).unwrap();
        let parsed = parse_stack_policy(&json);
        assert_eq!(parsed, policy);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_stack_policy_min_level_and_warn_mode() {
        let (app, _state) = app_with_agent_and_repo();

        let body = serde_json::json!({
            "required_fingerprint": "fp-warn",
            "required_level": 2,
            "min_attestation_level": 3,
            "enforcement": "warn"
        });
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
        assert_eq!(json["min_attestation_level"].as_u64().unwrap(), 3);
        assert_eq!(json["enforcement"].as_str().unwrap(), "warn");

        // GET returns the same policy.
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
        assert_eq!(json["min_attestation_level"].as_u64().unwrap(), 3);
        assert_eq!(json["enforcement"].as_str().unwrap(), "warn");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_stack_policy_rejects_bad_level_and_mode() {
        let (app, _state) = app_with_agent_and_repo();

        // Out-of-range min level → 400.
        let body = serde_json::json!({
            "required_fingerprint": "fp-x",
            "min_attestation_level": 4
        });
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
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Unknown enforcement mode → 400.
        let body = serde_json::json!({
            "required_fingerprint": "fp-x",
            "enforcement": "ignore"
        });
        let resp = app
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
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
