//! ABAC policy engine REST API (M22.6).
//!
//! Provides CRUD for policy entities, a dry-run evaluation endpoint, a
//! decision audit log query, and an effective-permissions explorer.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{
    builtin_policies, Condition, ConditionOp, ConditionValue, Policy, PolicyEffect, PolicyScope,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    api::error::ApiError,
    auth::AuthenticatedAgent,
    policy_engine::{self, AttributeContext},
    AppState,
};

// ---------------------------------------------------------------------------
// Policy CRUD
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListPoliciesQuery {
    pub scope: Option<String>,
    pub scope_id: Option<String>,
}

pub async fn list_policies(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListPoliciesQuery>,
    _auth: AuthenticatedAgent,
) -> Result<Json<Vec<Policy>>, ApiError> {
    let policies = if let Some(scope_str) = q.scope {
        let scope = parse_scope(&scope_str)?;
        state
            .policies
            .list_by_scope(&scope, q.scope_id.as_deref())
            .await?
    } else {
        state.policies.list().await?
    };
    Ok(Json(policies))
}

#[derive(Deserialize)]
pub struct CreatePolicyRequest {
    pub name: String,
    pub description: Option<String>,
    pub scope: String,
    pub scope_id: Option<String>,
    pub priority: u32,
    pub effect: String,
    pub conditions: Vec<ConditionRequest>,
    pub actions: Vec<String>,
    pub resource_types: Vec<String>,
    pub enabled: Option<bool>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ConditionRequest {
    pub attribute: String,
    pub operator: String,
    pub value: serde_json::Value,
}

pub async fn create_policy(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<CreatePolicyRequest>,
) -> Result<(StatusCode, Json<Policy>), ApiError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Reserved name prefixes: trust: and builtin: are managed by the server.
    // User-created policies cannot use these prefixes (HSI §2).
    let name = req.name;
    if name.starts_with("trust:") || name.starts_with("builtin:") {
        return Err(ApiError::InvalidInput(format!(
            "policy name '{name}' uses a reserved prefix ('trust:' or 'builtin:'); \
             these are managed by the server"
        )));
    }

    let policy = Policy {
        id: Id::new(uuid::Uuid::new_v4().to_string()),
        name,
        description: req.description.unwrap_or_default(),
        scope: parse_scope(&req.scope)?,
        scope_id: req.scope_id,
        priority: req.priority,
        effect: parse_effect(&req.effect)?,
        conditions: req
            .conditions
            .into_iter()
            .map(parse_condition)
            .collect::<Result<Vec<_>, _>>()?,
        actions: req.actions,
        resource_types: req.resource_types,
        enabled: req.enabled.unwrap_or(true),
        built_in: false,
        immutable: false,
        created_by: auth.agent_id,
        created_at: now,
        updated_at: now,
    };

    state.policies.create(&policy).await?;
    Ok((StatusCode::CREATED, Json(policy)))
}

pub async fn get_policy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    _auth: AuthenticatedAgent,
) -> Result<Json<Policy>, ApiError> {
    state
        .policies
        .find_by_id(&id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("policy {id} not found")))
        .map(Json)
}

#[derive(Deserialize)]
pub struct UpdatePolicyRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub priority: Option<u32>,
    pub effect: Option<String>,
    pub conditions: Option<Vec<ConditionRequest>>,
    pub actions: Option<Vec<String>>,
    pub resource_types: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

pub async fn update_policy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    _auth: AuthenticatedAgent,
    Json(req): Json<UpdatePolicyRequest>,
) -> Result<Json<Policy>, ApiError> {
    let mut policy = state
        .policies
        .find_by_id(&id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("policy {id} not found")))?;

    if policy.built_in {
        return Err(ApiError::InvalidInput(format!(
            "policy '{id}' is a built-in policy and cannot be modified"
        )));
    }
    if policy.immutable {
        return Err(ApiError::InvalidInput(format!(
            "policy '{id}' is immutable and cannot be modified"
        )));
    }

    if let Some(name) = req.name {
        if name.starts_with("trust:") || name.starts_with("builtin:") {
            return Err(ApiError::InvalidInput(format!(
                "policy name '{name}' uses a reserved prefix ('trust:' or 'builtin:') and cannot be set by callers"
            )));
        }
        policy.name = name;
    }
    if let Some(desc) = req.description {
        policy.description = desc;
    }
    if let Some(priority) = req.priority {
        policy.priority = priority;
    }
    if let Some(effect) = req.effect {
        policy.effect = parse_effect(&effect)?;
    }
    if let Some(conditions) = req.conditions {
        policy.conditions = conditions
            .into_iter()
            .map(parse_condition)
            .collect::<Result<Vec<_>, _>>()?;
    }
    if let Some(actions) = req.actions {
        policy.actions = actions;
    }
    if let Some(resource_types) = req.resource_types {
        policy.resource_types = resource_types;
    }
    if let Some(enabled) = req.enabled {
        policy.enabled = enabled;
    }
    policy.updated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    state.policies.update(&policy).await?;
    Ok(Json(policy))
}

pub async fn delete_policy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    _auth: AuthenticatedAgent,
) -> Result<StatusCode, ApiError> {
    let policy = state
        .policies
        .find_by_id(&id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("policy {id} not found")))?;
    if policy.built_in {
        return Err(ApiError::InvalidInput(format!(
            "policy '{id}' is a built-in policy and cannot be deleted"
        )));
    }
    state
        .policies
        .delete(&id)
        .await
        .map_err(|e| ApiError::InvalidInput(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Dry-run evaluation
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct EvaluateRequest {
    /// Subject attributes. Merged into the evaluation context.
    pub subject: SubjectAttrs,
    pub action: String,
    pub resource: ResourceAttrs,
}

#[derive(Deserialize)]
pub struct SubjectAttrs {
    pub r#type: Option<String>,
    pub id: Option<String>,
    pub workspace_role: Option<String>,
    pub tenant_id: Option<String>,
    pub persona: Option<String>,
    pub repo_scope: Option<String>,
    pub attestation_level: Option<i64>,
    /// Attestation chain depth (0 = root SignedInput, increments per derivation).
    pub chain_depth: Option<i64>,
    /// Root signer identity from the chain's root SignedInput key_binding.
    pub root_signer: Option<String>,
    /// Total accumulated constraint count (explicit + gate).
    pub constraint_count: Option<i64>,
    /// Additional arbitrary JWT claims.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
pub struct ResourceAttrs {
    pub r#type: String,
    pub id: Option<String>,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
}

#[derive(Serialize)]
pub struct EvaluateResponse {
    pub decision: String,
    pub matched_policy: Option<String>,
    pub evaluated_policies: u32,
    pub evaluation_ms: f64,
}

pub async fn evaluate_policy(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Json(req): Json<EvaluateRequest>,
) -> Result<Json<EvaluateResponse>, ApiError> {
    let mut ctx = AttributeContext::default();

    // Subject attributes.
    if let Some(t) = &req.subject.r#type {
        ctx.set("subject.type", t);
    }
    if let Some(id) = &req.subject.id {
        ctx.set("subject.id", id);
    }
    if let Some(role) = &req.subject.workspace_role {
        ctx.set("subject.workspace_role", role);
    }
    if let Some(tid) = &req.subject.tenant_id {
        ctx.set("subject.tenant_id", tid);
    }
    if let Some(persona) = &req.subject.persona {
        ctx.set("subject.persona", persona);
    }
    if let Some(scope) = &req.subject.repo_scope {
        ctx.set("subject.repo_scope", scope);
    }
    if let Some(level) = req.subject.attestation_level {
        ctx.set_number("subject.attestation_level", level);
    }
    if let Some(depth) = req.subject.chain_depth {
        ctx.set_number("subject.chain_depth", depth);
    }
    if let Some(signer) = &req.subject.root_signer {
        ctx.set("subject.root_signer", signer);
    }
    if let Some(count) = req.subject.constraint_count {
        ctx.set_number("subject.constraint_count", count);
    }
    // Extra claims.
    let extra_claims = serde_json::Value::Object(req.subject.extra);
    ctx.merge_jwt_claims(&extra_claims);

    // Resource attributes.
    ctx.set("resource.type", &req.resource.r#type);
    if let Some(id) = &req.resource.id {
        ctx.set("resource.id", id);
    }
    if let Some(ws) = &req.resource.workspace_id {
        ctx.set("resource.workspace_id", ws);
    }
    if let Some(repo) = &req.resource.repo_id {
        ctx.set("resource.repo_id", repo);
    }

    let policies = state.policies.list().await?;
    let result = policy_engine::evaluate(policies, &ctx, &req.action, &req.resource.r#type);

    Ok(Json(EvaluateResponse {
        decision: match result.effect {
            PolicyEffect::Allow => "allow".to_string(),
            PolicyEffect::Deny => "deny".to_string(),
        },
        matched_policy: result.matched_policy,
        evaluated_policies: result.evaluated_count,
        evaluation_ms: result.evaluation_ms,
    }))
}

// ---------------------------------------------------------------------------
// Decision audit log
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListDecisionsQuery {
    pub subject_id: Option<String>,
    pub resource_type: Option<String>,
    pub limit: Option<usize>,
}

pub async fn list_decisions(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Query(q): Query<ListDecisionsQuery>,
) -> Result<Json<Vec<gyre_domain::PolicyDecision>>, ApiError> {
    let decisions = state
        .policies
        .list_decisions(
            q.subject_id.as_deref(),
            q.resource_type.as_deref(),
            q.limit.unwrap_or(100),
        )
        .await?;
    Ok(Json(decisions))
}

// ---------------------------------------------------------------------------
// Effective permissions
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct EffectiveQuery {
    pub subject_id: Option<String>,
    pub resource_type: Option<String>,
}

#[derive(Serialize)]
pub struct EffectivePermission {
    pub action: String,
    pub resource_type: String,
    pub decision: String,
    pub matched_policy: Option<String>,
}

pub async fn effective_permissions(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Query(q): Query<EffectiveQuery>,
) -> Result<Json<Vec<EffectivePermission>>, ApiError> {
    let policies = state.policies.list().await?;

    // Build a basic context from query params.
    let mut ctx = AttributeContext::default();
    if let Some(sid) = &q.subject_id {
        ctx.set("subject.id", sid);
    }

    // Sample action/resource_type pairs to test.
    let resource_type = q.resource_type.as_deref().unwrap_or("*");
    let actions = [
        "read", "write", "delete", "approve", "spawn", "push", "merge", "generate",
    ];

    let mut results = Vec::new();
    for action in &actions {
        let result = policy_engine::evaluate(policies.clone(), &ctx, action, resource_type);
        results.push(EffectivePermission {
            action: action.to_string(),
            resource_type: resource_type.to_string(),
            decision: match result.effect {
                PolicyEffect::Allow => "allow".to_string(),
                PolicyEffect::Deny => "deny".to_string(),
            },
            matched_policy: result.matched_policy,
        });
    }
    Ok(Json(results))
}

// ---------------------------------------------------------------------------
// Bootstrap built-in policies
// ---------------------------------------------------------------------------

/// Seed the built-in system policies into the policy store.
/// Idempotent — skips policies that already exist.
pub async fn seed_builtin_policies(state: &AppState) {
    let builtins = builtin_policies("system");
    for policy in builtins {
        if state
            .policies
            .find_by_id(policy.id.as_str())
            .await
            .ok()
            .flatten()
            .is_none()
        {
            let _ = state.policies.create(&policy).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_scope(s: &str) -> Result<PolicyScope, ApiError> {
    match s {
        "tenant" => Ok(PolicyScope::Tenant),
        "workspace" => Ok(PolicyScope::Workspace),
        "repo" => Ok(PolicyScope::Repo),
        _ => Err(ApiError::InvalidInput(format!(
            "unknown policy scope '{s}'; expected tenant, workspace, or repo"
        ))),
    }
}

fn parse_effect(s: &str) -> Result<PolicyEffect, ApiError> {
    match s {
        "allow" => Ok(PolicyEffect::Allow),
        "deny" => Ok(PolicyEffect::Deny),
        _ => Err(ApiError::InvalidInput(format!(
            "unknown policy effect '{s}'; expected allow or deny"
        ))),
    }
}

fn parse_operator(s: &str) -> Result<ConditionOp, ApiError> {
    match s {
        "equals" => Ok(ConditionOp::Equals),
        "not_equals" => Ok(ConditionOp::NotEquals),
        "in" => Ok(ConditionOp::In),
        "not_in" => Ok(ConditionOp::NotIn),
        "greater_than" => Ok(ConditionOp::GreaterThan),
        "less_than" => Ok(ConditionOp::LessThan),
        "contains" => Ok(ConditionOp::Contains),
        "exists" => Ok(ConditionOp::Exists),
        _ => Err(ApiError::InvalidInput(format!(
            "unknown condition operator '{s}'"
        ))),
    }
}

fn parse_condition(req: ConditionRequest) -> Result<Condition, ApiError> {
    let operator = parse_operator(&req.operator)?;
    let value = match &req.value {
        serde_json::Value::String(s) => ConditionValue::String(s.clone()),
        serde_json::Value::Array(arr) => ConditionValue::StringList(
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
        ),
        serde_json::Value::Number(n) => ConditionValue::Number(n.as_i64().unwrap_or(0)),
        serde_json::Value::Bool(b) => ConditionValue::Bool(*b),
        serde_json::Value::Null => ConditionValue::Null,
        _ => ConditionValue::Null,
    };
    Ok(Condition {
        attribute: req.attribute,
        operator,
        value,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_policies_empty_initially() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/policies")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn create_and_get_policy() {
        let app = app();
        let body = serde_json::json!({
            "name": "test-allow",
            "scope": "tenant",
            "priority": 50,
            "effect": "allow",
            "conditions": [],
            "actions": ["read"],
            "resource_types": ["repo"]
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let created = body_json(resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        // GET by id
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/policies/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["name"], "test-allow");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn evaluate_allow_when_no_conditions() {
        let app = app();
        // Create a blanket allow policy.
        let body = serde_json::json!({
            "name": "allow-all",
            "scope": "tenant",
            "priority": 10,
            "effect": "allow",
            "conditions": [],
            "actions": ["*"],
            "resource_types": ["*"]
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Dry-run evaluate.
        let eval_body = serde_json::json!({
            "subject": { "type": "user", "id": "user-1" },
            "action": "read",
            "resource": { "type": "spec" }
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies/evaluate")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&eval_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["decision"], "allow");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn evaluate_deny_when_no_policies() {
        let eval_body = serde_json::json!({
            "subject": { "type": "agent" },
            "action": "push",
            "resource": { "type": "repo" }
        });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies/evaluate")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&eval_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["decision"], "deny");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_non_builtin_policy() {
        let app = app();
        let body = serde_json::json!({
            "name": "to-delete",
            "scope": "repo",
            "priority": 5,
            "effect": "deny",
            "conditions": [],
            "actions": ["delete"],
            "resource_types": ["task"]
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let created = body_json(resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/policies/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_decisions_empty_initially() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/policies/decisions")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn effective_permissions_returns_actions() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/policies/effective?resource_type=repo")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert!(!arr.is_empty());
        // With no policies, everything should be deny.
        assert!(arr.iter().all(|p| p["decision"] == "deny"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn create_policy_rejects_trust_prefix() {
        let app = app();
        let body = serde_json::json!({
            "name": "trust:my-policy",
            "scope": "tenant",
            "priority": 50,
            "effect": "deny",
            "conditions": [],
            "actions": ["merge"],
            "resource_types": ["mr"]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn create_policy_rejects_builtin_prefix() {
        let app = app();
        let body = serde_json::json!({
            "name": "builtin:my-policy",
            "scope": "tenant",
            "priority": 999,
            "effect": "deny",
            "conditions": [],
            "actions": ["approve"],
            "resource_types": ["spec"]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // --- TASK-061: Attestation chain ABAC subject attribute tests ---

    #[tokio::test(flavor = "multi_thread")]
    async fn dry_run_accepts_chain_depth_root_signer_constraint_count() {
        let app = app();
        // Create a blanket allow policy for attestation resources.
        let policy_body = serde_json::json!({
            "name": "allow-attested-push",
            "scope": "tenant",
            "priority": 50,
            "effect": "allow",
            "conditions": [],
            "actions": ["push"],
            "resource_types": ["attestation"]
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&policy_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Evaluate with attestation chain attributes.
        let eval_body = serde_json::json!({
            "subject": {
                "type": "agent",
                "id": "worker-42",
                "chain_depth": 2,
                "root_signer": "user:jsell",
                "constraint_count": 5
            },
            "action": "push",
            "resource": { "type": "attestation", "repo_id": "repo-1" }
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies/evaluate")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&eval_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["decision"], "allow");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn custom_policy_denies_deep_chains_via_chain_depth() {
        let app = app();
        // Create a deny policy for chain_depth > 5.
        let deny_body = serde_json::json!({
            "name": "deny-deep-chains",
            "scope": "tenant",
            "priority": 100,
            "effect": "deny",
            "conditions": [{
                "attribute": "subject.chain_depth",
                "operator": "greater_than",
                "value": 5
            }],
            "actions": ["push", "merge"],
            "resource_types": ["attestation"]
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&deny_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Also create a blanket allow so non-deep chains pass.
        let allow_body = serde_json::json!({
            "name": "allow-attestation",
            "scope": "tenant",
            "priority": 10,
            "effect": "allow",
            "conditions": [],
            "actions": ["push", "merge"],
            "resource_types": ["attestation"]
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&allow_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Evaluate with chain_depth = 6 (> 5) → should be denied.
        let eval_deep = serde_json::json!({
            "subject": {
                "type": "agent",
                "chain_depth": 6,
                "root_signer": "user:alice",
                "constraint_count": 10
            },
            "action": "push",
            "resource": { "type": "attestation" }
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies/evaluate")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&eval_deep).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["decision"], "deny");

        // Evaluate with chain_depth = 3 (≤ 5) → should be allowed.
        let eval_shallow = serde_json::json!({
            "subject": {
                "type": "agent",
                "chain_depth": 3,
                "root_signer": "user:bob",
                "constraint_count": 4
            },
            "action": "push",
            "resource": { "type": "attestation" }
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies/evaluate")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&eval_shallow).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["decision"], "allow");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn custom_policy_matches_root_signer() {
        let app = app();
        // Create policy: only allow pushes where root_signer is "user:trusted".
        let allow_body = serde_json::json!({
            "name": "allow-trusted-signer",
            "scope": "tenant",
            "priority": 50,
            "effect": "allow",
            "conditions": [{
                "attribute": "subject.root_signer",
                "operator": "equals",
                "value": "user:trusted"
            }],
            "actions": ["push"],
            "resource_types": ["attestation"]
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&allow_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Trusted signer → allow.
        let eval_body = serde_json::json!({
            "subject": {
                "type": "agent",
                "chain_depth": 1,
                "root_signer": "user:trusted",
                "constraint_count": 3
            },
            "action": "push",
            "resource": { "type": "attestation" }
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies/evaluate")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&eval_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        assert_eq!(json["decision"], "allow");

        // Untrusted signer → default deny (no matching policy).
        let eval_body2 = serde_json::json!({
            "subject": {
                "type": "agent",
                "chain_depth": 1,
                "root_signer": "user:untrusted",
                "constraint_count": 3
            },
            "action": "push",
            "resource": { "type": "attestation" }
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/policies/evaluate")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&eval_body2).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        assert_eq!(json["decision"], "deny");
    }
}
