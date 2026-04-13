//! Attribute-Based Access Control (ABAC) for gyre-server (G6).
//!
//! Policies are defined per-repo and evaluated against the caller's JWT claims.
//! A repo with no policies is unrestricted. A repo with policies requires the
//! caller's JWT to satisfy AT LEAST ONE policy (OR logic across policies;
//! AND logic across required_claims within a policy).
//!
//! Callers authenticated via the global admin token or API keys bypass ABAC
//! (they carry no JWT claims).

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{auth::AuthenticatedAgent, AppState};

use super::api::error::ApiError;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single ABAC policy. All `required_claims` must match (AND logic).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AbacPolicy {
    /// Resource type this policy applies to (e.g. "repo", "task").
    pub resource_type: String,
    /// Optional specific resource ID. When set, policy only applies to that ID.
    /// When absent, policy applies to all resources of the given `resource_type`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    /// Claim key → required value. All must be present in the caller's JWT.
    /// String claims: exact match. Array claims: the expected value must appear
    /// in the array.
    pub required_claims: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Policy evaluation
// ---------------------------------------------------------------------------

/// Returns `true` if all `required_claims` in `policy` are satisfied by
/// the provided JWT `claims` JSON value.
pub fn evaluate_policy(policy: &AbacPolicy, claims: &serde_json::Value) -> bool {
    for (key, expected) in &policy.required_claims {
        match claims.get(key) {
            Some(serde_json::Value::String(s)) => {
                if s != expected {
                    return false;
                }
            }
            Some(serde_json::Value::Array(arr)) => {
                if !arr.iter().any(|v| v.as_str() == Some(expected.as_str())) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

/// Check ABAC policies for a repo. Returns `Ok(())` if access is granted,
/// `Err(reason)` if denied.
///
/// - No policies configured → open access.
/// - `auth.jwt_claims` is `None` (global token or API key) → bypass ABAC.
/// - Otherwise: at least one policy must be satisfied.
pub async fn check_repo_abac(
    state: &AppState,
    repo_id: &str,
    auth: &AuthenticatedAgent,
) -> Result<(), String> {
    // Global token and API keys carry no JWT claims → admin bypass.
    // This check must run BEFORE policy parsing so that corrupt policy data
    // in the KV store does not block the global admin token.
    let claims = match &auth.jwt_claims {
        Some(c) => c,
        None => return Ok(()),
    };

    // Security invariant (M29.5B-A): if the store returns data that cannot
    // be parsed, DENY access (fail-closed) rather than silently treating it
    // as "no policies" which would bypass ABAC enforcement.
    let raw = state
        .kv_store
        .kv_get("abac_policies", repo_id)
        .await
        .ok()
        .flatten();

    let repo_policies: Vec<AbacPolicy> = match raw {
        None => vec![], // No policies stored = unrestricted.
        Some(s) => serde_json::from_str(&s).map_err(|e| {
            tracing::error!(
                security = "abac_policy_parse_fail",
                repo_id = repo_id,
                err = %e,
                "DENY: corrupt ABAC policy data in kv store"
            );
            format!("ABAC policy data corrupt for repo {repo_id}")
        })?,
    };

    if repo_policies.is_empty() {
        return Ok(()); // No policies = unrestricted.
    }

    // Evaluate each policy; pass if any matches.
    for policy in &repo_policies {
        if evaluate_policy(policy, claims) {
            return Ok(());
        }
    }

    Err(format!(
        "ABAC denied: no policy satisfied for repo {repo_id}"
    ))
}

/// Check workspace-level ABAC for read access (used by per-handler auth in conversation endpoint).
///
/// Returns `Ok(())` if access is allowed, `Err(reason)` if denied.
/// This mirrors `check_repo_abac` but scoped to a workspace resource.
pub async fn check_workspace_abac_for_read(
    state: &AppState,
    workspace_id: &str,
    claims: &serde_json::Value,
) -> Result<(), String> {
    let raw = state
        .kv_store
        .kv_get("abac_policies", workspace_id)
        .await
        .ok()
        .flatten();

    let ws_policies: Vec<AbacPolicy> = match raw {
        None => return Ok(()), // No policies = unrestricted.
        Some(s) => serde_json::from_str(&s).map_err(|e| {
            tracing::error!(
                security = "abac_policy_parse_fail",
                workspace_id = workspace_id,
                err = %e,
                "DENY: corrupt ABAC policy data"
            );
            format!("ABAC policy data corrupt for workspace {workspace_id}")
        })?,
    };

    if ws_policies.is_empty() {
        return Ok(());
    }

    for policy in &ws_policies {
        if evaluate_policy(policy, claims) {
            return Ok(());
        }
    }

    Err(format!(
        "ABAC denied: no policy satisfied for workspace {workspace_id}"
    ))
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

/// Response body for GET/PUT abac-policy.
#[derive(Serialize)]
pub struct AbacPolicyResponse {
    pub repo_id: String,
    pub policies: Vec<AbacPolicy>,
}

/// GET /api/v1/repos/:id/abac-policy — list ABAC policies for a repo.
pub async fn get_abac_policy(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<AbacPolicyResponse>, ApiError> {
    state
        .repos
        .find_by_id(&gyre_common::Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let policies: Vec<AbacPolicy> = state
        .kv_store
        .kv_get("abac_policies", &repo_id)
        .await
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    Ok(Json(AbacPolicyResponse { repo_id, policies }))
}

/// Request body for PUT abac-policy.
#[derive(Deserialize)]
pub struct SetAbacPoliciesRequest {
    pub policies: Vec<AbacPolicy>,
}

/// PUT /api/v1/repos/:id/abac-policy — set ABAC policies (admin-only via ABAC middleware).
pub async fn set_abac_policy(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Json(req): Json<SetAbacPoliciesRequest>,
) -> Result<(StatusCode, Json<AbacPolicyResponse>), ApiError> {
    state
        .repos
        .find_by_id(&gyre_common::Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let json = serde_json::to_string(&req.policies).map_err(|e| ApiError::Internal(e.into()))?;
    state
        .kv_store
        .kv_set("abac_policies", &repo_id, json)
        .await
        .map_err(ApiError::Internal)?;

    Ok((
        StatusCode::OK,
        Json(AbacPolicyResponse {
            repo_id,
            policies: req.policies,
        }),
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_domain::Repository;
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    // --- evaluate_policy tests -----------------------------------------------

    #[test]
    fn policy_no_claims_required_always_passes() {
        let policy = AbacPolicy {
            resource_type: "repo".to_string(),
            resource_id: None,
            required_claims: HashMap::new(),
        };
        let claims = serde_json::json!({});
        assert!(evaluate_policy(&policy, &claims));
    }

    #[test]
    fn policy_string_claim_exact_match() {
        let policy = AbacPolicy {
            resource_type: "repo".to_string(),
            resource_id: None,
            required_claims: [("scope".to_string(), "repo:A".to_string())]
                .into_iter()
                .collect(),
        };

        let matching = serde_json::json!({ "scope": "repo:A" });
        assert!(evaluate_policy(&policy, &matching));

        let wrong = serde_json::json!({ "scope": "repo:B" });
        assert!(!evaluate_policy(&policy, &wrong));

        let missing = serde_json::json!({});
        assert!(!evaluate_policy(&policy, &missing));
    }

    #[test]
    fn policy_array_claim_contains_check() {
        let policy = AbacPolicy {
            resource_type: "repo".to_string(),
            resource_id: None,
            required_claims: [("groups".to_string(), "infra".to_string())]
                .into_iter()
                .collect(),
        };

        let has_group = serde_json::json!({ "groups": ["infra", "dev"] });
        assert!(evaluate_policy(&policy, &has_group));

        let missing_group = serde_json::json!({ "groups": ["dev"] });
        assert!(!evaluate_policy(&policy, &missing_group));
    }

    #[test]
    fn policy_multiple_required_claims_all_must_match() {
        let policy = AbacPolicy {
            resource_type: "repo".to_string(),
            resource_id: None,
            required_claims: [
                ("scope".to_string(), "repo:A".to_string()),
                ("department".to_string(), "engineering".to_string()),
            ]
            .into_iter()
            .collect(),
        };

        let all_match = serde_json::json!({ "scope": "repo:A", "department": "engineering" });
        assert!(evaluate_policy(&policy, &all_match));

        let partial = serde_json::json!({ "scope": "repo:A" });
        assert!(!evaluate_policy(&policy, &partial));
    }

    // --- check_repo_abac tests -----------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn no_policies_grants_access() {
        let state = test_state();
        let auth = AuthenticatedAgent {
            agent_id: "agent-1".to_string(),
            user_id: None,
            roles: vec![],
            tenant_id: "default".to_string(),
            jwt_claims: Some(serde_json::json!({ "scope": "repo:X" })),
            deprecated_token_auth: false,
        };
        assert!(check_repo_abac(&state, "repo-1", &auth).await.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn no_jwt_claims_bypasses_abac() {
        let state = test_state();
        // Add a restrictive policy.
        let policy = vec![AbacPolicy {
            resource_type: "repo".to_string(),
            resource_id: None,
            required_claims: [("scope".to_string(), "repo:X".to_string())]
                .into_iter()
                .collect(),
        }];
        state
            .kv_store
            .kv_set(
                "abac_policies",
                "repo-1",
                serde_json::to_string(&policy).unwrap(),
            )
            .await
            .unwrap();
        // Global token / API key: jwt_claims is None → bypass.
        let auth = AuthenticatedAgent {
            agent_id: "system".to_string(),
            user_id: None,
            roles: vec![gyre_domain::UserRole::Admin],
            tenant_id: "default".to_string(),
            jwt_claims: None,
            deprecated_token_auth: false,
        };
        assert!(check_repo_abac(&state, "repo-1", &auth).await.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn matching_claim_grants_access() {
        let state = test_state();
        let policy_a = vec![AbacPolicy {
            resource_type: "repo".to_string(),
            resource_id: None,
            required_claims: [("scope".to_string(), "repo:A".to_string())]
                .into_iter()
                .collect(),
        }];
        state
            .kv_store
            .kv_set(
                "abac_policies",
                "repo-A",
                serde_json::to_string(&policy_a).unwrap(),
            )
            .await
            .unwrap();
        let auth = AuthenticatedAgent {
            agent_id: "agent-1".to_string(),
            user_id: None,
            roles: vec![],
            tenant_id: "default".to_string(),
            jwt_claims: Some(serde_json::json!({ "scope": "repo:A" })),
            deprecated_token_auth: false,
        };
        assert!(check_repo_abac(&state, "repo-A", &auth).await.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn mismatching_claim_denies_access() {
        let state = test_state();
        let policy_b = vec![AbacPolicy {
            resource_type: "repo".to_string(),
            resource_id: None,
            required_claims: [("scope".to_string(), "repo:B".to_string())]
                .into_iter()
                .collect(),
        }];
        state
            .kv_store
            .kv_set(
                "abac_policies",
                "repo-B",
                serde_json::to_string(&policy_b).unwrap(),
            )
            .await
            .unwrap();
        // Agent only has scope for repo:A, not repo:B
        let auth = AuthenticatedAgent {
            agent_id: "agent-1".to_string(),
            user_id: None,
            roles: vec![],
            tenant_id: "default".to_string(),
            jwt_claims: Some(serde_json::json!({ "scope": "repo:A" })),
            deprecated_token_auth: false,
        };
        assert!(check_repo_abac(&state, "repo-B", &auth).await.is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn agent_with_scope_a_can_access_repo_a_but_not_repo_b() {
        let state = test_state();
        // Set up policies for both repos.
        {
            let pa = vec![AbacPolicy {
                resource_type: "repo".to_string(),
                resource_id: None,
                required_claims: [("scope".to_string(), "repo:A".to_string())]
                    .into_iter()
                    .collect(),
            }];
            let pb = vec![AbacPolicy {
                resource_type: "repo".to_string(),
                resource_id: None,
                required_claims: [("scope".to_string(), "repo:B".to_string())]
                    .into_iter()
                    .collect(),
            }];
            state
                .kv_store
                .kv_set(
                    "abac_policies",
                    "repo-A",
                    serde_json::to_string(&pa).unwrap(),
                )
                .await
                .unwrap();
            state
                .kv_store
                .kv_set(
                    "abac_policies",
                    "repo-B",
                    serde_json::to_string(&pb).unwrap(),
                )
                .await
                .unwrap();
        }
        let auth = AuthenticatedAgent {
            agent_id: "agent-1".to_string(),
            user_id: None,
            roles: vec![],
            tenant_id: "default".to_string(),
            jwt_claims: Some(serde_json::json!({ "scope": "repo:A" })),
            deprecated_token_auth: false,
        };
        assert!(check_repo_abac(&state, "repo-A", &auth).await.is_ok());
        assert!(check_repo_abac(&state, "repo-B", &auth).await.is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn corrupt_abac_data_does_not_block_admin_bypass() {
        // The admin bypass (jwt_claims = None) must run BEFORE policy parsing.
        // If corrupt JSON is stored, a non-admin token gets 403 (fail-closed),
        // but the global token must still bypass.
        let state = test_state();
        state
            .kv_store
            .kv_set(
                "abac_policies",
                "repo-corrupt",
                "NOT VALID JSON {{{".to_string(),
            )
            .await
            .unwrap();

        // Admin token (jwt_claims = None) — must succeed despite corrupt data.
        let admin_auth = AuthenticatedAgent {
            agent_id: "system".to_string(),
            user_id: None,
            roles: vec![gyre_domain::UserRole::Admin],
            tenant_id: "default".to_string(),
            jwt_claims: None,
            deprecated_token_auth: false,
        };
        assert!(
            check_repo_abac(&state, "repo-corrupt", &admin_auth)
                .await
                .is_ok(),
            "admin bypass must not be blocked by corrupt ABAC policy data"
        );

        // Non-admin token (jwt_claims = Some(...)) — must fail-closed.
        let agent_auth = AuthenticatedAgent {
            agent_id: "agent-1".to_string(),
            user_id: None,
            roles: vec![],
            tenant_id: "default".to_string(),
            jwt_claims: Some(serde_json::json!({ "scope": "repo:corrupt" })),
            deprecated_token_auth: false,
        };
        assert!(
            check_repo_abac(&state, "repo-corrupt", &agent_auth)
                .await
                .is_err(),
            "non-admin token must be denied (fail-closed) on corrupt ABAC policy data"
        );
    }

    // --- HTTP handler tests --------------------------------------------------

    fn app_with_repo() -> (Router, std::sync::Arc<crate::AppState>) {
        let state = test_state();
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

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_abac_policy_returns_empty_by_default() {
        let (app, _state) = app_with_repo();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/abac-policy")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["repo_id"], "repo-1");
        assert_eq!(json["policies"].as_array().unwrap().len(), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_and_get_abac_policy_round_trips() {
        let (app, _state) = app_with_repo();
        let body = serde_json::json!({
            "policies": [{
                "resource_type": "repo",
                "required_claims": { "scope": "repo:1" }
            }]
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/repo-1/abac-policy")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["policies"].as_array().unwrap().len(), 1);
        assert_eq!(json["policies"][0]["required_claims"]["scope"], "repo:1");

        // GET returns persisted value.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/abac-policy")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["policies"][0]["required_claims"]["scope"], "repo:1");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_abac_policy_unknown_repo_returns_404() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state);
        let body = serde_json::json!({ "policies": [] });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/no-such/abac-policy")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
