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
use gyre_domain::{Condition, ConditionOp, ConditionValue, Policy, PolicyEffect, PolicyScope};
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
        scope_id: req.scope_id.map(Id::new),
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
        created_by: Id::new(auth.agent_id),
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
    // Review round 6 (task-129): the reserved-prefix invariant is enforced
    // on the STORED name, not just a newly supplied one. Trust-preset
    // policies (`trust:...`) are built_in: false and immutable: false, so
    // without this check a caller could disable or rewrite one BY ID —
    // e.g. PUT {"enabled": false} on `trust:require-human-mr-review` —
    // silently desyncing workspace.trust_level from the enforced policy
    // set and bypassing the atomic, transition-validated
    // apply_trust_transition path that owns these policies.
    if policy.name.starts_with("trust:") || policy.name.starts_with("builtin:") {
        return Err(ApiError::InvalidInput(format!(
            "policy '{}' has a reserved prefix ('trust:' or 'builtin:') and is \
             server-managed; it cannot be modified via the API — use trust-level \
             transitions instead",
            policy.name
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
    // Review round 6 (task-129): same stored-name guard as update_policy —
    // trust-preset policies are not built_in, so the flag alone let a
    // caller delete `trust:require-human-mr-review` by id and silently
    // desync workspace.trust_level from the enforced policy set.
    if policy.name.starts_with("trust:") || policy.name.starts_with("builtin:") {
        return Err(ApiError::InvalidInput(format!(
            "policy '{}' has a reserved prefix ('trust:' or 'builtin:') and is \
             server-managed; it cannot be deleted via the API — use trust-level \
             transitions instead",
            policy.name
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
    /// The resource's tenant — the third leg of the scope chain the
    /// engine's canonical scope filter (`policy_engine::scope_applies`)
    /// evaluates tenant-scoped policies against (review round 4).
    pub tenant_id: Option<String>,
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
    if let Some(tid) = &req.resource.tenant_id {
        ctx.set("resource.tenant_id", tid);
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
    /// Subject id, named `subject` per the §API table
    /// (`GET /api/v1/policies/effective?subject={id}&resource_type={type}`)
    /// and docs/api-reference.md. `subject_id` is accepted as an alias so
    /// callers using the decisions-endpoint spelling keep working — a
    /// caller following the documented `?subject=` form must NOT fall
    /// through to the bare-context (deny-everything) path.
    #[serde(alias = "subject_id")]
    pub subject: Option<String>,
    pub resource_type: Option<String>,
    /// Optional resource scope the caller wants the answer scoped to
    /// (review round 4): the engine's canonical scope filter
    /// (`policy_engine::scope_applies`) evaluates Workspace/Repo/Tenant-
    /// scoped policies against the resource scope chain, so without these
    /// the endpoint can only answer for an unscoped resource — scoped
    /// policies conservatively do not apply.
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
    pub tenant_id: Option<String>,
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

    // Build the subject's attribute context from REAL stores (§API:
    // "What can this user/agent do on this resource type?"). A bare
    // subject.id cannot answer that — the builtin allows key on
    // global_role / workspace_role / tenant, so before this the endpoint
    // answered "deny everything" for every real user. Resolution order:
    // user by id → user by external id (the JWT `sub`) → agent by id →
    // fallback to just subject.id (unknown subject, conditions on other
    // attributes fail closed).
    let mut ctx = AttributeContext::default();
    if let Some(sid) = &q.subject {
        ctx.set("subject.id", sid);
        let user = match state.users.find_by_id(&Id::new(sid.clone())).await {
            Ok(u) => u,
            Err(e) => {
                return Err(ApiError::Internal(anyhow::anyhow!(
                    "user lookup for effective permissions failed: {e}"
                )))
            }
        };
        let user = match user {
            Some(u) => Some(u),
            None => match state.users.find_by_external_id(sid).await {
                Ok(u) => u,
                Err(e) => {
                    return Err(ApiError::Internal(anyhow::anyhow!(
                        "user lookup for effective permissions failed: {e}"
                    )))
                }
            },
        };
        if let Some(user) = user {
            ctx.set("subject.type", "user");
            // Role SET semantics — same as the middleware's extraction
            // (review round 6, task-129): empty roles leave the attribute
            // unset (fail closed), multi-role callers get a list the
            // engine evaluates with membership semantics. An arbitrary
            // `first()` pick would answer this query differently than the
            // middleware answers the real request.
            match user.roles.as_slice() {
                [] => {}
                [only] => ctx.set("subject.global_role", only.as_str()),
                many => ctx.set_list(
                    "subject.global_role",
                    many.iter().map(|r| r.as_str().to_string()).collect(),
                ),
            }
            if let Some(tid) = &user.tenant_id {
                ctx.set("subject.tenant_id", tid.as_str());
            }
            // Memberships + teams, same sources as the middleware's
            // extraction (§Attributes: "Memberships", "Team memberships").
            match state.workspace_memberships.list_by_user(&user.id).await {
                Ok(memberships) => {
                    let workspace_ids: Vec<String> = memberships
                        .iter()
                        .map(|m| m.workspace_id.to_string())
                        .collect();
                    if !workspace_ids.is_empty() {
                        ctx.set_list("subject.workspace_ids", workspace_ids);
                    }
                    let mut team_ids: Vec<String> = Vec::new();
                    for m in &memberships {
                        if let Ok(teams) =
                            state.teams.list_for_user(&m.workspace_id, &user.id).await
                        {
                            for team in teams {
                                team_ids.push(team.id.to_string());
                            }
                        }
                    }
                    if !team_ids.is_empty() {
                        ctx.set_list("subject.team_ids", team_ids);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        subject_id = %sid,
                        err = %e,
                        "membership lookup failed for effective permissions"
                    );
                }
            }
        } else if let Ok(Some(_agent)) = state.agents.find_by_id(&Id::new(sid.clone())).await {
            ctx.set("subject.type", "agent");
        }
    }

    // Resource scope chain for the engine's canonical scope filter
    // (review round 4): scoped policies apply only when the queried scope
    // matches; without a scope the answer is for an unscoped resource and
    // scoped policies conservatively do not apply.
    if let Some(ws) = &q.workspace_id {
        ctx.set("resource.workspace_id", ws);
    }
    if let Some(repo) = &q.repo_id {
        ctx.set("resource.repo_id", repo);
    }
    if let Some(tid) = &q.tenant_id {
        ctx.set("resource.tenant_id", tid);
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
// Helpers
// ---------------------------------------------------------------------------

fn parse_scope(s: &str) -> Result<PolicyScope, ApiError> {
    // Case-insensitive: the §UI URL pattern documents the filter as
    // `?scope=Workspace&scope_id=<id>` (capitalized) while policy JSON
    // serializes scopes snake_case ("workspace") — both spellings must
    // resolve to the same scope, or the documented query form 400s.
    match s.to_lowercase().as_str() {
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
    async fn list_policies_initially_only_builtins() {
        // Production parity: `test_state` pre-seeds the built-in ABAC set
        // (the same set `seed_builtin_policies` persists at startup), so a
        // fresh deployment NEVER lists zero policies — the spec's
        // `system-full-access` and its siblings are always present. What
        // "empty" means here is "no operator-defined policies": every
        // listed policy is built-in, and the spec-table builtins
        // (system-full-access, tenant-isolation, default-deny) are all
        // accounted for.
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
        let policies = json.as_array().unwrap();
        assert!(
            policies
                .iter()
                .all(|p| p["built_in"].as_bool().unwrap_or(false)),
            "a fresh state must list only built-in policies, got {:?}",
            policies
                .iter()
                .filter(|p| !p["built_in"].as_bool().unwrap_or(false))
                .map(|p| p["name"].clone())
                .collect::<Vec<_>>()
        );
        for expected in [
            "system-full-access",
            "tenant-isolation",
            "workspace-membership-required",
            "default-deny",
        ] {
            assert!(
                policies
                    .iter()
                    .any(|p| p["name"].as_str() == Some(expected)),
                "built-in policy {expected} must be seeded in a fresh state"
            );
        }
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

    /// Review round 4 (scope applicability): the dry-run endpoint must honor
    /// the engine's canonical scope filter — a workspace-scoped policy does
    /// not apply to a dry-run resource in a different workspace, and a
    /// tenant-scoped policy does not apply across tenants (via the
    /// resource.tenant_id scope-chain leg this round added to the request).
    #[tokio::test(flavor = "multi_thread")]
    async fn evaluate_respects_policy_scope() {
        let app = app();

        async fn create(app: &Router, body: serde_json::Value) -> String {
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
            created["id"].as_str().unwrap().to_string()
        }

        async fn evaluate(app: &Router, resource: serde_json::Value) -> serde_json::Value {
            let eval_body = serde_json::json!({
                "subject": { "type": "user", "id": "user-1" },
                "action": "write",
                "resource": resource
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
            assert_eq!(resp.status(), StatusCode::OK);
            body_json(resp).await
        }

        // Workspace-scoped Allow for ws-A only (the reviewer's repro shape).
        let ws_id = create(
            &app,
            serde_json::json!({
                "name": "ws-a-allow",
                "scope": "workspace",
                "scope_id": "ws-A",
                "priority": 900,
                "effect": "allow",
                "conditions": [],
                "actions": ["*"],
                "resource_types": ["*"]
            }),
        )
        .await;

        // Out of scope (ws-B): default deny. The dry-run state carries the
        // full startup builtin set (production parity), so the deny is
        // ATTRIBUTED to the tenant-level `default-deny` catchall (priority
        // 1, no conditions) — never to the out-of-scope ws-A Allow, which
        // the engine's scope filter must exclude.
        let json = evaluate(
            &app,
            serde_json::json!({ "type": "task", "workspace_id": "ws-B" }),
        )
        .await;
        assert_eq!(
            json["decision"], "deny",
            "ws-A policy must not apply to ws-B"
        );
        assert_eq!(
            json["matched_policy"], "builtin-default-deny",
            "out-of-scope policy must not be the matched policy — only the \
             default-deny catchall may decide an out-of-scope dry-run"
        );

        // In scope (ws-A): allowed by the scoped policy.
        let json = evaluate(
            &app,
            serde_json::json!({ "type": "task", "workspace_id": "ws-A" }),
        )
        .await;
        assert_eq!(json["decision"], "allow");
        assert_eq!(json["matched_policy"], ws_id);

        // Tenant-scoped Allow for t-1: applies via resource.tenant_id.
        let t_id = create(
            &app,
            serde_json::json!({
                "name": "t1-allow",
                "scope": "tenant",
                "scope_id": "t-1",
                "priority": 890,
                "effect": "allow",
                "conditions": [],
                "actions": ["*"],
                "resource_types": ["*"]
            }),
        )
        .await;

        let json = evaluate(
            &app,
            serde_json::json!({ "type": "task", "tenant_id": "t-2" }),
        )
        .await;
        assert_eq!(json["decision"], "deny", "t-1 policy must not apply to t-2");

        let json = evaluate(
            &app,
            serde_json::json!({ "type": "task", "tenant_id": "t-1" }),
        )
        .await;
        assert_eq!(json["decision"], "allow");
        assert_eq!(json["matched_policy"], t_id);
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

    /// Review round 6 (task-129): trust-preset policies are
    /// `built_in: false` and `immutable: false`, so those flags alone let a
    /// caller disable, rewrite, or delete one BY ID — silently desyncing
    /// `workspace.trust_level` from the enforced policy set and bypassing
    /// the atomic, transition-validated `apply_trust_transition` path.
    /// The STORED name's reserved prefix must reject update and delete.
    /// Killing direction: without the stored-name guard, the PUT returns
    /// 200 and the policy comes back disabled.
    #[tokio::test(flavor = "multi_thread")]
    async fn trust_preset_policy_cannot_be_updated_or_deleted_by_id() {
        let state = test_state();
        // Seed exactly what apply_trust_transition persists for a
        // Supervised workspace (gyre-domain trust_policies_for_level):
        // a trust:-prefixed Deny that is NOT built_in and NOT immutable —
        // the flags must not be what protects it.
        let trust = gyre_domain::policy::trust_policies_for_level(
            &gyre_domain::TrustLevel::Supervised,
            "ws-trust",
            "test",
        );
        assert_eq!(trust.len(), 1);
        let trust = trust.into_iter().next().unwrap();
        assert!(
            !trust.built_in && !trust.immutable,
            "fixture shape: neither flag may protect the policy"
        );
        let id = trust.id.as_str().to_string();
        state.policies.create(&trust).await.unwrap();
        let app = crate::api::api_router().with_state(state.clone());

        // PUT {"enabled": false} — the desync vector from the review
        // finding: disabling the Supervised merge-hold Deny by id.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/policies/{id}"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({"enabled": false})).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "updating a trust: policy by id must be rejected"
        );

        // DELETE likewise.
        let resp = app
            .clone()
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
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "deleting a trust: policy by id must be rejected"
        );

        // The policy is still there, still enabled — the enforced set
        // still matches workspace.trust_level.
        let still = state
            .policies
            .find_by_id(&id)
            .await
            .unwrap()
            .expect("trust policy must survive the rejected mutations");
        assert!(still.enabled, "the trust Deny must still be enforced");
        assert_eq!(still.name, "trust:require-human-mr-review");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_policies_scope_query_is_case_insensitive() {
        // The §UI URL pattern documents the workspace filter as
        // `?scope=Workspace&scope_id=<id>` (capitalized) while the policy
        // JSON serializes scopes snake_case. Both spellings must resolve
        // to the same scope: the task-132 UI (and the pre-existing
        // `workspaceAbacPolicies` client call) send the capitalized form —
        // a case-sensitive parse_scope 400s the documented query.
        let app = app();
        let body = serde_json::json!({
            "name": "ws-case-allow",
            "scope": "workspace",
            "scope_id": "ws-CASE",
            "priority": 10,
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

        // Capitalized spelling (spec §UI): must list the ws-CASE policy.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/policies?scope=Workspace&scope_id=ws-CASE")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let names: Vec<&str> = json
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p["name"].as_str())
            .collect();
        assert!(
            names.contains(&"ws-case-allow"),
            "capitalized ?scope=Workspace must filter like lowercase: got {names:?}"
        );

        // Lowercase spelling must keep working (policy JSON wire form).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/policies?scope=workspace&scope_id=ws-CASE")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(
            json.as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"].as_str() == Some("ws-case-allow"))
        );

        // A different workspace's scope_id must not match.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/policies?scope=Workspace&scope_id=ws-OTHER")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(
            !json.as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"].as_str() == Some("ws-case-allow"))
        );
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
        // A bare (no-?subject) context carries no subject attributes, so no
        // seeded builtin — every one keys on a subject attribute — can
        // match: everything must be deny.
        assert!(arr.iter().all(|p| p["decision"] == "deny"));
    }

    /// The `?subject=` query spelling is the one the §API table and
    /// docs/api-reference.md document
    /// (`GET /api/v1/policies/effective?subject={id}&resource_type={type}`).
    /// A caller following the docs must get the subject's REAL attributes
    /// (resolved identity from the store) — not the no-subject fallback
    /// (bare context → deny-everything). Killing case: a handler binding
    /// only `subject_id` ignores `?subject=` entirely and this test fails
    /// with read=deny. The `subject_id` alias must keep working too.
    #[tokio::test(flavor = "multi_thread")]
    async fn effective_permissions_binds_documented_subject_param() {
        use gyre_common::Id;
        use gyre_domain::{
            Condition, ConditionOp, ConditionValue, Policy, PolicyEffect, PolicyScope, User,
        };

        let state = test_state();

        // A real user whose identity the handler must load from the store.
        // The custom Allow keys on `subject.id` at priority 2000 — above
        // every seeded builtin (system-full-access is 1000) — because the
        // seeded builtin role Allows (admin 900 / developer 800 / readonly
        // 600) would otherwise shadow a role-keyed custom policy and the
        // matched_policy assertion below could not pin THIS policy. The
        // user carries no global role, so no builtin Allow matches: the
        // only path to `read=allow` is the handler binding `?subject=`.
        let mut user = User::new(
            Id::new("eff-subject-user"),
            "eff-sub-external",
            "eff-user",
            0,
        );
        user.tenant_id = Some(Id::new("default"));
        state.users.create(&user).await.unwrap();

        // An Allow keyed on the attribute only the real-subject path sets:
        // with the bare-context fallback, subject.id is absent and the
        // condition fails closed → deny.
        state
            .policies
            .create(&Policy {
                id: Id::new("eff-subject-read"),
                name: "eff-subject-read".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 2000,
                effect: PolicyEffect::Allow,
                conditions: vec![Condition {
                    attribute: "subject.id".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("eff-subject-user".to_string()),
                }],
                actions: vec!["read".to_string()],
                resource_types: vec!["repo".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        let app = crate::api::api_router().with_state(state.clone());

        // The documented spelling: ?subject={id}.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/policies/effective?subject=eff-subject-user&resource_type=repo")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let read = json
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["action"] == "read")
            .expect("read must be sampled");
        assert_eq!(
            read["decision"], "allow",
            "?subject= must bind the subject and load its real attributes \
             (bare-context fallback would deny)"
        );
        assert_eq!(read["matched_policy"], "eff-subject-read");

        // The alias spelling (?subject_id=) must keep working.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(
                        "/api/v1/policies/effective?subject_id=eff-subject-user&resource_type=repo",
                    )
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let read = json
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["action"] == "read")
            .expect("read must be sampled");
        assert_eq!(read["decision"], "allow");
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
