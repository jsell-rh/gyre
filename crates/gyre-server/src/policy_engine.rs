//! ABAC policy evaluation engine (M22.6).
//!
//! Extracts subject/resource/action/environment attributes from the request
//! context and evaluates them against the applicable policy set. First matching
//! policy wins; higher priority is evaluated first; repo > workspace > tenant
//! for equal-priority policies.

use std::collections::HashMap;

use gyre_domain::policy::{
    Condition, ConditionOp, ConditionValue, Policy, PolicyDecision, PolicyEffect, PolicyScope,
};

// ---------------------------------------------------------------------------
// Attribute context
// ---------------------------------------------------------------------------

/// Flat attribute bag used during policy evaluation. All attributes are stored
/// as strings or lists of strings, matching JWT claim conventions.
#[derive(Default, Clone, Debug)]
pub struct AttributeContext {
    attrs: HashMap<String, AttrValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AttrValue {
    Single(String),
    List(Vec<String>),
    Number(i64),
    Bool(bool),
}

impl AttributeContext {
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.attrs
            .insert(key.into(), AttrValue::Single(value.into()));
    }

    pub fn set_list(&mut self, key: impl Into<String>, values: Vec<String>) {
        self.attrs.insert(key.into(), AttrValue::List(values));
    }

    pub fn set_number(&mut self, key: impl Into<String>, n: i64) {
        self.attrs.insert(key.into(), AttrValue::Number(n));
    }

    pub fn set_bool(&mut self, key: impl Into<String>, b: bool) {
        self.attrs.insert(key.into(), AttrValue::Bool(b));
    }

    /// Returns `true` if the key is present in the context.
    pub fn has(&self, key: &str) -> bool {
        self.attrs.contains_key(key)
    }

    /// Get the raw value for a key.
    pub fn get(&self, key: &str) -> Option<&AttrValue> {
        self.attrs.get(key)
    }

    /// Merge arbitrary claims into the context under the `subject.`
    /// namespace (insert-only: a key already present keeps its value).
    ///
    /// For DRY-RUN / what-if evaluation only (api/policies.rs
    /// `evaluate_policy`): the caller explicitly supplies hypothetical
    /// attribute values to test policies against. NEVER use this on a live
    /// access-decision path — a verified JWT's claim content is
    /// caller-chosen, so live extraction must use the allowlisted
    /// [`merge_agent_jwt_claims`] instead.
    pub fn merge_jwt_claims(&mut self, claims: &serde_json::Value) {
        if let Some(obj) = claims.as_object() {
            for (key, val) in obj {
                let full_key = format!("subject.{key}");
                if self.attrs.contains_key(&full_key) {
                    continue;
                }
                match val {
                    serde_json::Value::String(s) => {
                        self.set(full_key, s.clone());
                    }
                    serde_json::Value::Array(arr) => {
                        let strings: Vec<String> = arr
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect();
                        self.set_list(full_key, strings);
                    }
                    serde_json::Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            self.set_number(full_key, i);
                        }
                    }
                    serde_json::Value::Bool(b) => {
                        self.set_bool(full_key, *b);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Merge the agent-JWT claims that the spec sources from the "Agent OIDC
    /// claim" into the context under the `subject.` namespace.
    ///
    /// Allowlisted to exactly those four attribute names (persona,
    /// stack_hash, attestation_level, repo_scope) and called only for
    /// agent-type subjects. Every other `subject.*` attribute has a
    /// canonical NON-claim source — `type`/`id`/`global_role`/`tenant_id`
    /// from the auth extractor (§Attributes: "Auth context / OIDC"),
    /// `workspace_role`/`workspace_ids`/`team_ids` from the membership and
    /// team stores — so a raw claim carrying those names must never define
    /// the attribute. A verified JWT's claim CONTENT is caller-chosen
    /// (whoever controls token minting picks the JSON), and the membership
    /// block only overwrites `subject.workspace_role` when the request
    /// addresses a workspace the caller belongs to: before the allowlist,
    /// a human JWT carrying `{"workspace_role": "Admin"}` defined the
    /// attribute on repo-parented or unscoped routes and satisfied Allow
    /// policies keyed on it (spoofing channel, review round 2).
    ///
    /// Insert-only: a key already present in the context keeps its existing
    /// value, so a claim cannot override an extractor-resolved identity
    /// fact even for the allowlisted names.
    pub fn merge_agent_jwt_claims(&mut self, claims: &serde_json::Value) {
        /// The only `subject.*` attributes a raw JWT claim may define
        /// (§Attributes, source column "Agent OIDC claim").
        const AGENT_CLAIM_ATTRIBUTES: [&str; 4] =
            ["persona", "stack_hash", "attestation_level", "repo_scope"];
        let Some(obj) = claims.as_object() else {
            return;
        };
        for key in AGENT_CLAIM_ATTRIBUTES {
            let Some(val) = obj.get(key) else {
                continue;
            };
            let full_key = format!("subject.{key}");
            if self.attrs.contains_key(&full_key) {
                continue;
            }
            match val {
                serde_json::Value::String(s) => {
                    self.set(full_key, s.clone());
                }
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        self.set_number(full_key, i);
                    }
                }
                serde_json::Value::Bool(b) => {
                    self.set_bool(full_key, *b);
                }
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Dynamic references
// ---------------------------------------------------------------------------

/// Resolve dynamic references in a condition value (§Conditions).
///
/// A value starting with `$` (e.g. `"$resource.repo_id"`) is replaced at
/// evaluation time by the current value of the referenced attribute in the
/// context; list values resolve element-wise. Returns `None` when a reference
/// does not resolve: the condition cannot be established and must evaluate to
/// false. No value is fabricated for a missing attribute — an Allow with an
/// unresolvable reference must not grant (fail closed), mirroring the
/// `resolve_ref` discipline on git refs.
fn resolve_condition_value(
    value: &ConditionValue,
    ctx: &AttributeContext,
) -> Option<ConditionValue> {
    match value {
        ConditionValue::String(s) => {
            if s.starts_with('$') {
                resolve_dynamic_value(s, ctx)
            } else {
                Some(value.clone())
            }
        }
        ConditionValue::StringList(list) => {
            let mut resolved = Vec::with_capacity(list.len());
            for s in list {
                if s.starts_with('$') {
                    resolved.push(resolve_dynamic_scalar(s, ctx)?);
                } else {
                    resolved.push(s.clone());
                }
            }
            Some(ConditionValue::StringList(resolved))
        }
        other => Some(other.clone()),
    }
}

/// Resolve a `$path` reference to a full condition value, preserving the
/// referenced attribute's type (string, list, number, bool).
fn resolve_dynamic_value(s: &str, ctx: &AttributeContext) -> Option<ConditionValue> {
    let key = s.strip_prefix('$')?;
    match ctx.get(key)? {
        AttrValue::Single(v) => Some(ConditionValue::String(v.clone())),
        AttrValue::List(v) => Some(ConditionValue::StringList(v.clone())),
        AttrValue::Number(n) => Some(ConditionValue::Number(*n)),
        AttrValue::Bool(b) => Some(ConditionValue::Bool(*b)),
    }
}

/// Resolve a `$path` reference in scalar (string) position.
fn resolve_dynamic_scalar(s: &str, ctx: &AttributeContext) -> Option<String> {
    let key = s.strip_prefix('$')?;
    match ctx.get(key)? {
        AttrValue::Single(v) => Some(v.clone()),
        AttrValue::Number(n) => Some(n.to_string()),
        AttrValue::Bool(b) => Some(b.to_string()),
        // List-valued attribute referenced in scalar position.
        AttrValue::List(_) => None,
    }
}

// ---------------------------------------------------------------------------
// Condition evaluation
// ---------------------------------------------------------------------------

/// Evaluate a single condition against the attribute context.
fn eval_condition(cond: &Condition, ctx: &AttributeContext) -> bool {
    match &cond.operator {
        ConditionOp::Exists => ctx.has(&cond.attribute),
        _ => {
            let Some(attr_val) = ctx.get(&cond.attribute) else {
                return false;
            };
            // Dynamic references resolve against the same context; an
            // unresolvable reference fails the condition.
            let Some(value) = resolve_condition_value(&cond.value, ctx) else {
                return false;
            };
            match (&cond.operator, attr_val, &value) {
                (ConditionOp::Equals, AttrValue::Single(s), ConditionValue::String(expected)) => {
                    s == expected
                }
                (ConditionOp::Equals, AttrValue::Number(n), ConditionValue::Number(expected)) => {
                    n == expected
                }
                (ConditionOp::Equals, AttrValue::Bool(b), ConditionValue::Bool(expected)) => {
                    b == expected
                }
                (
                    ConditionOp::NotEquals,
                    AttrValue::Single(s),
                    ConditionValue::String(expected),
                ) => s != expected,
                (
                    ConditionOp::NotEquals,
                    AttrValue::Number(n),
                    ConditionValue::Number(expected),
                ) => n != expected,
                (ConditionOp::NotEquals, AttrValue::Bool(b), ConditionValue::Bool(expected)) => {
                    b != expected
                }
                // In: value must be in list
                (ConditionOp::In, AttrValue::Single(s), ConditionValue::StringList(list)) => {
                    list.contains(s)
                }
                // NotIn
                (ConditionOp::NotIn, AttrValue::Single(s), ConditionValue::StringList(list)) => {
                    !list.contains(s)
                }
                // GreaterThan / LessThan (numeric)
                (ConditionOp::GreaterThan, AttrValue::Number(n), ConditionValue::Number(t)) => {
                    n > t
                }
                (ConditionOp::LessThan, AttrValue::Number(n), ConditionValue::Number(t)) => n < t,
                // Contains: list attribute contains the expected string value
                (
                    ConditionOp::Contains,
                    AttrValue::List(list),
                    ConditionValue::String(expected),
                ) => list.contains(expected),
                // Contains on a single string: substring check
                (ConditionOp::Contains, AttrValue::Single(s), ConditionValue::String(expected)) => {
                    s.contains(expected.as_str())
                }
                _ => false,
            }
        }
    }
}

/// Returns `true` if all conditions in the policy match the context.
fn eval_policy_conditions(policy: &Policy, ctx: &AttributeContext) -> bool {
    policy.conditions.iter().all(|c| eval_condition(c, ctx))
}

// ---------------------------------------------------------------------------
// Evaluation result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EvalResult {
    pub effect: PolicyEffect,
    pub matched_policy: Option<String>,
    pub evaluated_count: u32,
    pub evaluation_ms: f64,
}

// ---------------------------------------------------------------------------
// Evaluation engine
// ---------------------------------------------------------------------------

/// Sort key for a policy: higher priority first, then scope specificity
/// (repo > workspace > tenant), then Deny before Allow — "deny takes
/// precedence over allow within the same priority" (§Policy Language) and
/// "Deny overrides Allow at the same priority and scope" (§Policy Composition
/// rule 3).
fn policy_sort_key(p: &Policy) -> (u32, u8, u8) {
    let scope_rank = match p.scope {
        PolicyScope::Repo => 2,
        PolicyScope::Workspace => 1,
        PolicyScope::Tenant => 0,
    };
    let deny_rank = match p.effect {
        PolicyEffect::Deny => 1,
        PolicyEffect::Allow => 0,
    };
    (p.priority, scope_rank, deny_rank)
}

/// Evaluate the given list of `policies` against `ctx` for `action` on `resource_type`.
///
/// Evaluation order (HSI §2, abac-policy-engine.md):
/// 1. **Immutable Deny policies first** — evaluated before any priority-based policy.
///    If any immutable Deny's conditions match, the request is denied immediately.
///    These cannot be overridden by any Allow regardless of priority.
/// 2. **Priority-based evaluation** — remaining policies sorted highest-priority-first,
///    then most-specific scope. First match wins (Allow or Deny).
/// 3. **Default deny** — if no policy matches, the request is denied.
pub fn evaluate(
    mut policies: Vec<Policy>,
    ctx: &AttributeContext,
    action: &str,
    resource_type: &str,
) -> EvalResult {
    let t0 = std::time::Instant::now();

    // Action, resource.type, and env.time are evaluation inputs (§Attributes)
    // owned by this function: inject them so conditions can reference them
    // uniformly regardless of caller. `env.time` is only set when absent so a
    // dry-run caller can pin a specific evaluation time. The context is small
    // (a few dozen entries built per request); the clone is not on any hot
    // loop.
    let mut ctx = ctx.clone();
    ctx.set("action", action);
    ctx.set("resource.type", resource_type);
    if !ctx.has("env.time") {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        ctx.set_number("env.time", now as i64);
    }

    // Filter to enabled policies that apply to this action/resource_type.
    policies.retain(|p| p.enabled && p.applies_to(action, resource_type));

    let total = policies.len() as u32;

    // Step 1: Check immutable Deny policies first (before any priority sorting).
    // These cannot be overridden — if any match, deny immediately.
    for policy in &policies {
        if policy.immutable
            && policy.effect == PolicyEffect::Deny
            && eval_policy_conditions(policy, &ctx)
        {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            return EvalResult {
                effect: PolicyEffect::Deny,
                matched_policy: Some(policy.id.to_string()),
                evaluated_count: total,
                evaluation_ms: ms,
            };
        }
    }

    // Step 2: Priority-based evaluation of remaining (non-immutable) policies.
    let mut remaining: Vec<_> = policies
        .iter()
        .filter(|p| !(p.immutable && p.effect == PolicyEffect::Deny))
        .collect();

    remaining.sort_by(|a, b| {
        let ka = policy_sort_key(a);
        let kb = policy_sort_key(b);
        kb.cmp(&ka)
    });

    for policy in &remaining {
        if eval_policy_conditions(policy, &ctx) {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            return EvalResult {
                effect: policy.effect.clone(),
                matched_policy: Some(policy.id.to_string()),
                evaluated_count: total,
                evaluation_ms: ms,
            };
        }
    }

    // Step 3: No match → default deny.
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    EvalResult {
        effect: PolicyEffect::Deny,
        matched_policy: None,
        evaluated_count: total,
        evaluation_ms: ms,
    }
}

/// Build a `PolicyDecision` record from an `EvalResult` plus request context.
pub fn build_decision(
    result: &EvalResult,
    subject_id: &str,
    subject_type: &str,
    action: &str,
    resource_type: &str,
    resource_id: &str,
) -> PolicyDecision {
    PolicyDecision {
        request_id: uuid::Uuid::new_v4().to_string(),
        subject_id: subject_id.to_string(),
        subject_type: subject_type.to_string(),
        action: action.to_string(),
        resource_type: resource_type.to_string(),
        resource_id: resource_id.to_string(),
        decision: result.effect.clone(),
        matched_policy: result.matched_policy.as_deref().map(gyre_common::Id::new),
        evaluated_policies: result.evaluated_count,
        evaluation_ms: result.evaluation_ms,
        evaluated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::Id;

    fn allow_policy(priority: u32, conditions: Vec<Condition>) -> Policy {
        Policy {
            id: Id::new(format!("p-{priority}")),
            name: format!("test-allow-{priority}"),
            description: String::new(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority,
            effect: PolicyEffect::Allow,
            conditions,
            actions: vec!["*".to_string()],
            resource_types: vec!["*".to_string()],
            enabled: true,
            built_in: false,
            immutable: false,
            created_by: Id::new("system"),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn deny_policy(priority: u32, conditions: Vec<Condition>) -> Policy {
        Policy {
            effect: PolicyEffect::Deny,
            ..allow_policy(priority, conditions)
        }
    }

    fn immutable_deny_policy(conditions: Vec<Condition>) -> Policy {
        Policy {
            immutable: true,
            ..deny_policy(999, conditions)
        }
    }

    #[test]
    fn no_policies_default_deny() {
        let ctx = AttributeContext::default();
        let result = evaluate(vec![], &ctx, "read", "repo");
        assert_eq!(result.effect, PolicyEffect::Deny);
        assert!(result.matched_policy.is_none());
    }

    #[test]
    fn single_allow_no_conditions_grants() {
        let ctx = AttributeContext::default();
        let p = allow_policy(10, vec![]);
        let result = evaluate(vec![p], &ctx, "read", "repo");
        assert_eq!(result.effect, PolicyEffect::Allow);
        assert!(result.matched_policy.is_some());
    }

    #[test]
    fn condition_equals_matches() {
        let mut ctx = AttributeContext::default();
        ctx.set("subject.workspace_role", "Developer");

        let cond = Condition {
            attribute: "subject.workspace_role".to_string(),
            operator: ConditionOp::Equals,
            value: ConditionValue::String("Developer".to_string()),
        };
        let result = evaluate(vec![allow_policy(10, vec![cond])], &ctx, "push", "repo");
        assert_eq!(result.effect, PolicyEffect::Allow);
    }

    #[test]
    fn condition_equals_mismatches_denies() {
        let mut ctx = AttributeContext::default();
        ctx.set("subject.workspace_role", "Viewer");

        let cond = Condition {
            attribute: "subject.workspace_role".to_string(),
            operator: ConditionOp::Equals,
            value: ConditionValue::String("Developer".to_string()),
        };
        let result = evaluate(vec![allow_policy(10, vec![cond])], &ctx, "push", "repo");
        // Condition doesn't match; no policy matches; default deny.
        assert_eq!(result.effect, PolicyEffect::Deny);
    }

    #[test]
    fn deny_overrides_allow_at_same_priority_and_scope() {
        // §Policy Language: "First match wins (deny takes precedence over
        // allow within the same priority)" / §Policy Composition rule 3.
        // Equal priority, equal scope: the Deny must be evaluated first even
        // when the Allow appears earlier in the list.
        let ctx = AttributeContext::default();
        let allow = allow_policy(100, vec![]);
        let deny = deny_policy(100, vec![]);
        // Both helpers mint id "p-100"; give the deny a distinct id.
        let mut deny = deny;
        deny.id = Id::new("p-100-deny");
        let result = evaluate(vec![allow, deny], &ctx, "write", "task");
        assert_eq!(result.effect, PolicyEffect::Deny);
        assert_eq!(result.matched_policy.as_deref(), Some("p-100-deny"));
    }

    #[test]
    fn higher_priority_deny_beats_lower_priority_allow() {
        let ctx = AttributeContext::default();
        let low_allow = allow_policy(5, vec![]);
        let high_deny = deny_policy(100, vec![]);
        let result = evaluate(vec![low_allow, high_deny], &ctx, "write", "task");
        assert_eq!(result.effect, PolicyEffect::Deny);
    }

    #[test]
    fn in_operator_matches_list() {
        let mut ctx = AttributeContext::default();
        ctx.set("subject.workspace_role", "Developer");
        let cond = Condition {
            attribute: "subject.workspace_role".to_string(),
            operator: ConditionOp::In,
            value: ConditionValue::StringList(vec![
                "Owner".to_string(),
                "Admin".to_string(),
                "Developer".to_string(),
            ]),
        };
        let result = evaluate(vec![allow_policy(10, vec![cond])], &ctx, "push", "repo");
        assert_eq!(result.effect, PolicyEffect::Allow);
    }

    #[test]
    fn not_in_operator_denies_when_in_list() {
        let mut ctx = AttributeContext::default();
        ctx.set("subject.workspace_role", "Viewer");
        let cond = Condition {
            attribute: "subject.workspace_role".to_string(),
            operator: ConditionOp::NotIn,
            value: ConditionValue::StringList(vec!["Owner".to_string(), "Admin".to_string()]),
        };
        // NotIn matches (Viewer is not in the list) → Allow.
        let result = evaluate(vec![allow_policy(10, vec![cond])], &ctx, "push", "repo");
        assert_eq!(result.effect, PolicyEffect::Allow);
    }

    #[test]
    fn exists_operator() {
        let mut ctx = AttributeContext::default();
        // "repo_scope" does NOT exist yet → Exists returns false → condition unmet → no match.
        let cond = Condition {
            attribute: "subject.repo_scope".to_string(),
            operator: ConditionOp::Exists,
            value: ConditionValue::Null,
        };
        let result = evaluate(vec![deny_policy(10, vec![cond])], &ctx, "push", "repo");
        assert_eq!(result.effect, PolicyEffect::Deny); // default deny (condition not met → policy didn't match, but default is deny anyway)
                                                       // Now add the attribute.
        ctx.set("subject.repo_scope", "repo:X");
        let cond2 = Condition {
            attribute: "subject.repo_scope".to_string(),
            operator: ConditionOp::Exists,
            value: ConditionValue::Null,
        };
        let result2 = evaluate(vec![allow_policy(10, vec![cond2])], &ctx, "push", "repo");
        assert_eq!(result2.effect, PolicyEffect::Allow);
    }

    #[test]
    fn contains_operator_on_list() {
        let mut ctx = AttributeContext::default();
        ctx.set_list(
            "subject.groups",
            vec!["infra".to_string(), "dev".to_string()],
        );
        let cond = Condition {
            attribute: "subject.groups".to_string(),
            operator: ConditionOp::Contains,
            value: ConditionValue::String("infra".to_string()),
        };
        let result = evaluate(vec![allow_policy(10, vec![cond])], &ctx, "read", "spec");
        assert_eq!(result.effect, PolicyEffect::Allow);
    }

    #[test]
    fn action_filter_ignores_non_matching_actions() {
        let ctx = AttributeContext::default();
        let mut p = allow_policy(10, vec![]);
        p.actions = vec!["approve".to_string()];
        // Policy only applies to "approve", not "read".
        let result = evaluate(vec![p], &ctx, "read", "spec");
        assert_eq!(result.effect, PolicyEffect::Deny); // default deny
    }

    #[test]
    fn merge_agent_jwt_claims_allowlisted_to_agent_claim_names() {
        // Only the four §Attributes "Agent OIDC claim" names may be defined
        // by a raw claim. Before the allowlist this test fails: the claim
        // set defined subject.workspace_role (a membership-sourced
        // attribute) and subject.groups, so a policy keyed on
        // subject.workspace_role matched a caller who holds no membership —
        // the review round-2 spoofing channel.
        let mut ctx = AttributeContext::default();
        let claims = serde_json::json!({
            "workspace_role": "Admin",
            "workspace_ids": ["ws-1"],
            "team_ids": ["team-x"],
            "groups": ["infra", "dev"],
            "global_role": "Admin",
            "persona": "repo-orchestrator",
            "repo_scope": "repo-9",
            "attestation_level": 2
        });
        ctx.merge_agent_jwt_claims(&claims);

        // Allowlisted agent-claim names merge with their types.
        assert_eq!(
            ctx.get("subject.persona"),
            Some(&AttrValue::Single("repo-orchestrator".to_string()))
        );
        assert_eq!(
            ctx.get("subject.repo_scope"),
            Some(&AttrValue::Single("repo-9".to_string()))
        );
        assert_eq!(
            ctx.get("subject.attestation_level"),
            Some(&AttrValue::Number(2))
        );

        // Membership-sourced and identity names are NEVER claim-definable.
        assert!(!ctx.has("subject.workspace_role"));
        assert!(!ctx.has("subject.workspace_ids"));
        assert!(!ctx.has("subject.team_ids"));
        assert!(!ctx.has("subject.global_role"));
        assert!(!ctx.has("subject.groups"));

        // Behavioral assertion: an Allow keyed on subject.workspace_role
        // must NOT match the claim-injected value — the attribute is
        // absent, so the condition fails closed and default deny stands.
        let allow_admin = Policy {
            id: Id::new("allow-admin"),
            name: "admin-allow".to_string(),
            description: String::new(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 900,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.workspace_role".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::String("Admin".to_string()),
            }],
            actions: vec!["write".to_string()],
            resource_types: vec!["*".to_string()],
            enabled: true,
            built_in: false,
            immutable: false,
            created_by: Id::new("test"),
            created_at: 0,
            updated_at: 0,
        };
        let result = evaluate(vec![allow_admin], &ctx, "write", "task");
        assert_eq!(
            result.effect,
            PolicyEffect::Deny,
            "a workspace_role claim must not satisfy a workspace_role-keyed Allow"
        );
    }

    #[test]
    fn merge_agent_jwt_claims_never_overrides_existing_attributes() {
        // The middleware sets auth-extractor identity facts before merging
        // claims. A claim sharing an attribute's name must NOT override the
        // extractor-resolved value — and with the allowlist, non-claim
        // names do not even enter the context. Before the insert-only fix
        // this test fails: HashMap::insert let the claim overwrite the
        // resolved "ReadOnly" with "Admin", so the readonly Allow no
        // longer matched and the request fell to default deny.
        let mut ctx = AttributeContext::default();
        ctx.set("subject.global_role", "ReadOnly");
        ctx.set("subject.type", "user");
        // An extractor- or normalizer-set allowlisted name also stands.
        ctx.set("subject.persona", "gate-approver");
        let claims = serde_json::json!({
            "global_role": "Admin",
            "type": "agent",
            "tenant_id": "tenant-a",
            "persona": "spoofed-persona"
        });
        ctx.merge_agent_jwt_claims(&claims);

        // Direct attribute assertions: extractor/normalizer facts stand,
        // non-claim names never merge.
        assert_eq!(
            ctx.get("subject.global_role"),
            Some(&AttrValue::Single("ReadOnly".to_string()))
        );
        assert_eq!(
            ctx.get("subject.type"),
            Some(&AttrValue::Single("user".to_string()))
        );
        assert!(!ctx.has("subject.tenant_id"));
        assert_eq!(
            ctx.get("subject.persona"),
            Some(&AttrValue::Single("gate-approver".to_string()))
        );

        // Behavioral assertion: an Allow keyed on the extractor value must
        // match. If the claim overwrote global_role to "Admin", the Allow
        // would not match and default deny would flip the decision.
        let allow_readonly = Policy {
            id: Id::new("allow-readonly"),
            name: "readonly-allow".to_string(),
            description: String::new(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 900,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.global_role".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::String("ReadOnly".to_string()),
            }],
            actions: vec!["read".to_string()],
            resource_types: vec!["*".to_string()],
            enabled: true,
            built_in: false,
            immutable: false,
            created_by: Id::new("test"),
            created_at: 0,
            updated_at: 0,
        };
        let result = evaluate(vec![allow_readonly], &ctx, "read", "repo");
        assert_eq!(
            result.effect,
            PolicyEffect::Allow,
            "claim-injected global_role must not override the extractor-resolved value"
        );
    }

    // --- Immutable Deny policy tests (HSI §2) ---------------------------------

    #[test]
    fn immutable_deny_blocks_even_when_high_priority_allow_matches() {
        // A high-priority Allow at p=1000 cannot override an immutable Deny.
        let mut ctx = AttributeContext::default();
        ctx.set("subject.type", "agent");

        // Immutable deny: agents cannot approve specs.
        let mut immutable = immutable_deny_policy(vec![Condition {
            attribute: "subject.type".to_string(),
            operator: ConditionOp::Equals,
            value: ConditionValue::String("agent".to_string()),
        }]);
        immutable.actions = vec!["approve".to_string()];
        immutable.resource_types = vec!["spec".to_string()];

        // High-priority allow (simulating system-full-access at p=1000).
        let mut system_allow = allow_policy(1000, vec![]);
        system_allow.actions = vec!["approve".to_string()];
        system_allow.resource_types = vec!["spec".to_string()];

        let result = evaluate(vec![system_allow, immutable], &ctx, "approve", "spec");
        // Immutable deny must win regardless of the p=1000 Allow.
        assert_eq!(result.effect, PolicyEffect::Deny);
        assert!(result.matched_policy.is_some());
    }

    #[test]
    fn immutable_deny_only_triggers_when_conditions_match() {
        // Immutable deny does NOT trigger when conditions don't match.
        let mut ctx = AttributeContext::default();
        ctx.set("subject.type", "user"); // user, not agent

        let mut immutable = immutable_deny_policy(vec![Condition {
            attribute: "subject.type".to_string(),
            operator: ConditionOp::NotEquals,
            value: ConditionValue::String("user".to_string()),
        }]);
        immutable.actions = vec!["approve".to_string()];
        immutable.resource_types = vec!["spec".to_string()];

        let mut allow = allow_policy(50, vec![]);
        allow.actions = vec!["approve".to_string()];
        allow.resource_types = vec!["spec".to_string()];

        let result = evaluate(vec![allow, immutable], &ctx, "approve", "spec");
        // User approving: immutable condition (not_equals "user") doesn't match → Allow wins.
        assert_eq!(result.effect, PolicyEffect::Allow);
    }

    #[test]
    fn builtin_require_human_spec_approval_blocks_agent_approve() {
        // Integration-level: simulate the builtin:require-human-spec-approval policy.
        let mut ctx = AttributeContext::default();
        ctx.set("subject.type", "agent");
        ctx.set("subject.id", "worker-42");

        let builtin_policies = gyre_domain::builtin_policies("system");
        let spec_approval = builtin_policies
            .into_iter()
            .find(|p| p.name == "builtin:require-human-spec-approval")
            .expect("builtin:require-human-spec-approval must exist");

        assert!(spec_approval.immutable, "must be immutable");

        // Add a high-priority allow to simulate system-full-access at p=1000.
        let mut system_allow = allow_policy(1000, vec![]);
        system_allow.actions = vec!["approve".to_string()];
        system_allow.resource_types = vec!["spec".to_string()];

        let result = evaluate(vec![system_allow, spec_approval], &ctx, "approve", "spec");
        assert_eq!(result.effect, PolicyEffect::Deny);
    }

    #[test]
    fn system_full_access_matches_by_id_not_type() {
        // system-full-access now matches subject.id == "gyre-system-token", not subject.type.
        // The merge processor (type: "system", id: "merge-processor") must NOT be allowed.
        let builtin_policies = gyre_domain::builtin_policies("system");
        let system_access = builtin_policies
            .into_iter()
            .find(|p| p.name == "system-full-access")
            .expect("system-full-access must exist");

        // The condition must be on subject.id, not subject.type.
        assert_eq!(system_access.conditions.len(), 1);
        assert_eq!(system_access.conditions[0].attribute, "subject.id");
        assert_eq!(
            system_access.conditions[0].value,
            gyre_domain::ConditionValue::String("gyre-system-token".to_string())
        );
    }

    // --- TASK-129: §Built-In Policies table rows --------------------------------

    /// The domain builtin set must contain every §Built-In Policies table
    /// row by name (the seeding filter in abac_middleware selects from this
    /// set — a missing row means the policy can never be seeded).
    #[test]
    fn builtin_set_contains_spec_table_rows() {
        let names: Vec<String> = gyre_domain::builtin_policies("system")
            .iter()
            .map(|p| p.name.clone())
            .collect();
        for required in [
            "system-full-access",
            "tenant-isolation",
            "agent-repo-scope",
            "workspace-membership-required",
            "persona-human-approval",
            "builtin:require-human-spec-approval",
            "default-deny",
        ] {
            // default-deny is owned by the m34 seed list; the rest live in
            // the domain set.
            if required != "default-deny" {
                assert!(
                    names.iter().any(|n| n == required),
                    "§Built-In Policies table row '{required}' must exist in the domain builtin set"
                );
            }
        }
    }

    /// `agent-repo-scope` seed must match the §Policy Examples semantics
    /// and actually enforce containment: enabled (the §Built-In Policies
    /// table lists containment as a shipped invariant, not an opt-in),
    /// dynamic-reference conditions (`subject.repo_scope not_equals
    /// $resource.repo_id` — an Exists-only condition would deny
    /// repo-scoped agents EVERYTHING, since worker tokens always carry a
    /// repo_id claim), and priority above the m34 `agent-scoped-access`
    /// role Allow (700), because composition is higher-priority-wins.
    /// Killing directions: the pre-fix seed (enabled: false, Exists
    /// condition, priority 100) fails the enabled, condition, and priority
    /// assertions respectively.
    #[test]
    fn agent_repo_scope_seed_matches_spec_example() {
        let builtins = gyre_domain::builtin_policies("system");
        let p = builtins
            .iter()
            .find(|p| p.name == "agent-repo-scope")
            .expect("agent-repo-scope must exist in the domain builtin set")
            .clone();

        assert!(
            p.enabled,
            "containment is a shipped invariant (§Built-In Policies), not an opt-in"
        );
        assert_eq!(p.effect, PolicyEffect::Deny);
        assert_eq!(p.actions, vec!["*".to_string()]);
        assert_eq!(p.resource_types, vec!["*".to_string()]);
        assert_eq!(p.conditions.len(), 2);
        assert_eq!(p.conditions[0].attribute, "subject.type");
        assert_eq!(p.conditions[0].operator, ConditionOp::Equals);
        assert_eq!(
            p.conditions[0].value,
            ConditionValue::String("agent".to_string())
        );
        assert_eq!(p.conditions[1].attribute, "subject.repo_scope");
        assert_eq!(p.conditions[1].operator, ConditionOp::NotEquals);
        assert_eq!(
            p.conditions[1].value,
            ConditionValue::String("$resource.repo_id".to_string()),
            "the spec's dynamic reference — an Exists-only condition would \
             deny repo-scoped agents everything (worker tokens always carry \
             a repo_id claim)"
        );
        assert!(
            p.priority > 700,
            "the containment Deny must outrank the m34 agent-scoped-access \
             role Allow (priority 700): composition is higher-priority-wins, \
             so a Deny at the §Policy Examples priority 100 enforces nothing"
        );
    }

    /// Live-set behavior of the seeded `agent-repo-scope` against the REAL
    /// m34 role Allow: in-scope access passes through to the Allow,
    /// out-of-scope access is denied BY the containment builtin (matched
    /// by id, not the default-deny catchall), and non-repo-addressed
    /// resources fall through (the `$resource.repo_id` reference fails
    /// closed, so containment governs only repo-scoped resources — the
    /// spec example's exact semantics).
    #[test]
    fn agent_repo_scope_enforced_over_role_allow() {
        let mut set = gyre_domain::builtin_policies("system");
        set.extend(crate::abac_middleware::m34_builtin_policies());

        let mut ctx = AttributeContext::default();
        ctx.set("subject.type", "agent");
        ctx.set("subject.global_role", "Agent");
        ctx.set("subject.tenant_id", "t-1");
        ctx.set("subject.repo_scope", "repo-a");
        ctx.set("resource.repo_id", "repo-a");
        ctx.set("resource.tenant_id", "t-1");

        // In-scope read: the containment Deny does not match
        // (repo_scope == resource.repo_id); the role Allow (700) grants.
        let r = evaluate(set.clone(), &ctx, "read", "repo");
        assert_eq!(r.effect, PolicyEffect::Allow);
        assert_eq!(
            r.matched_policy.as_deref(),
            Some("builtin-agent-scoped-access")
        );

        // Out-of-scope read: denied BY the containment builtin — the
        // killing case for a disabled/enabled-but-ineffective seed.
        ctx.set("resource.repo_id", "repo-b");
        let r = evaluate(set.clone(), &ctx, "read", "repo");
        assert_eq!(r.effect, PolicyEffect::Deny);
        assert_eq!(
            r.matched_policy.as_deref(),
            Some("builtin-agent-repo-scope"),
            "the denial must come from the containment builtin, not the \
             default-deny catchall"
        );

        // Non-repo-addressed resource: `$resource.repo_id` does not
        // resolve, the NotEquals condition fails closed, the role Allow
        // grants — containment only governs repo-scoped resources.
        let mut ctx2 = AttributeContext::default();
        ctx2.set("subject.type", "agent");
        ctx2.set("subject.global_role", "Agent");
        ctx2.set("subject.tenant_id", "t-1");
        ctx2.set("subject.repo_scope", "repo-a");
        let r = evaluate(set, &ctx2, "read", "spec");
        assert_eq!(r.effect, PolicyEffect::Allow);
    }

    /// `tenant-isolation`: a subject whose tenant differs from the
    /// resource's tenant is denied — even when a same-tenant Allow would
    /// otherwise grant, and even for a global Admin role (the Allow at 900
    /// is below the builtin Deny at 950). Same-tenant access passes through
    /// to the rest of the policy set.
    #[test]
    fn tenant_isolation_denies_cross_tenant_even_for_admin() {
        let builtins = gyre_domain::builtin_policies("system");
        let tenant_isolation = builtins
            .iter()
            .find(|p| p.name == "tenant-isolation")
            .expect("tenant-isolation must exist")
            .clone();

        // Cross-tenant subject: an Admin (role allow at 900 would grant
        // without the builtin).
        let mut ctx = AttributeContext::default();
        ctx.set("subject.type", "user");
        ctx.set("subject.tenant_id", "tenant-acme");
        ctx.set("resource.tenant_id", "tenant-other");

        let admin_allow = allow_policy(900, vec![Condition {
            attribute: "subject.global_role".to_string(),
            operator: ConditionOp::Equals,
            value: ConditionValue::String("Admin".to_string()),
        }]);
        let mut ctx_admin = ctx.clone();
        ctx_admin.set("subject.global_role", "Admin");

        let result = evaluate(
            vec![admin_allow.clone(), tenant_isolation.clone()],
            &ctx_admin,
            "read",
            "task",
        );
        assert_eq!(
            result.effect,
            PolicyEffect::Deny,
            "cross-tenant access must be denied even for a global Admin role"
        );
        assert_eq!(
            result.matched_policy.as_deref(),
            Some("builtin-tenant-isolation")
        );

        // Same tenant: the builtin does not match; the Admin allow grants.
        let mut ctx_same = ctx;
        ctx_same.set("resource.tenant_id", "tenant-acme");
        ctx_same.set("subject.global_role", "Admin");
        let result = evaluate(vec![admin_allow, tenant_isolation], &ctx_same, "read", "task");
        assert_eq!(
            result.effect,
            PolicyEffect::Allow,
            "same-tenant access must fall through to the role allow"
        );
    }

    /// `tenant-isolation` fails closed when either tenant attribute is
    /// absent: the Exists conditions must not match, so evaluation falls
    /// through to the rest of the policy set (no fabricated tenant).
    #[test]
    fn tenant_isolation_fails_closed_on_missing_attributes() {
        let builtins = gyre_domain::builtin_policies("system");
        let tenant_isolation = builtins
            .iter()
            .find(|p| p.name == "tenant-isolation")
            .unwrap()
            .clone();

        // resource.tenant_id unpopulated (collection route / no entity
        // lookup): the builtin must NOT match — an Allow may grant.
        let mut ctx = AttributeContext::default();
        ctx.set("subject.tenant_id", "tenant-acme");
        let allow = allow_policy(500, vec![]);
        let result = evaluate(vec![allow, tenant_isolation], &ctx, "read", "task");
        assert_eq!(
            result.effect,
            PolicyEffect::Allow,
            "absent resource.tenant_id must fail the Exists condition, not fabricate a tenant"
        );
    }

    /// `system-full-access` (priority 1000) outranks `tenant-isolation`
    /// (950): the global token identity spans tenants by design — it is the
    /// server itself, resolving the bootstrap tenant while administering
    /// every tenant's resources.
    #[test]
    fn system_full_access_outranks_tenant_isolation() {
        let builtins = gyre_domain::builtin_policies("system");
        let get = |name: &str| {
            builtins
                .iter()
                .find(|p| p.name == name)
                .unwrap()
                .clone()
        };

        let mut ctx = AttributeContext::default();
        ctx.set("subject.id", "gyre-system-token");
        ctx.set("subject.tenant_id", "default");
        ctx.set("resource.tenant_id", "tenant-acme");

        let result = evaluate(
            vec![get("tenant-isolation"), get("system-full-access")],
            &ctx,
            "write",
            "task",
        );
        assert_eq!(
            result.effect,
            PolicyEffect::Allow,
            "the global token identity must be allowed across tenants (priority 1000 > 950)"
        );
        assert_eq!(
            result.matched_policy.as_deref(),
            Some("builtin-system-access")
        );
    }

    /// `workspace-membership-required`: a user who is not a member of the
    /// addressed workspace is denied (the middleware derives
    /// `subject.member_of_resource_workspace` from the real membership
    /// store); a member falls through to the role allows. Agents carry no
    /// user membership, so the subject.type condition keys them out.
    #[test]
    fn workspace_membership_required_denies_non_member_users() {
        let builtins = gyre_domain::builtin_policies("system");
        let membership_required = builtins
            .iter()
            .find(|p| p.name == "workspace-membership-required")
            .unwrap()
            .clone();

        let dev_allow = allow_policy(800, vec![Condition {
            attribute: "subject.global_role".to_string(),
            operator: ConditionOp::Equals,
            value: ConditionValue::String("Developer".to_string()),
        }]);

        // Non-member user: boolean false → denied even with the Developer
        // role allow at 800 (the builtin Deny sits at 940).
        let mut non_member = AttributeContext::default();
        non_member.set("subject.type", "user");
        non_member.set("subject.global_role", "Developer");
        non_member.set_bool("subject.member_of_resource_workspace", false);
        let result = evaluate(
            vec![dev_allow.clone(), membership_required.clone()],
            &non_member,
            "read",
            "task",
        );
        assert_eq!(
            result.effect,
            PolicyEffect::Deny,
            "a non-member user must be denied workspace resources regardless of global role"
        );
        assert_eq!(
            result.matched_policy.as_deref(),
            Some("builtin-workspace-membership-required")
        );

        // Member: boolean true → falls through to the Developer allow.
        let mut member = non_member;
        member.set_bool("subject.member_of_resource_workspace", true);
        let result = evaluate(vec![dev_allow, membership_required], &member, "read", "task");
        assert_eq!(
            result.effect,
            PolicyEffect::Allow,
            "a member must fall through to the role allow"
        );
    }

    /// `persona-human-approval`: non-user subjects cannot approve personas,
    /// outranking the agent role allow (700) that would otherwise permit
    /// the approve action.
    #[test]
    fn persona_human_approval_denies_agents() {
        let builtins = gyre_domain::builtin_policies("system");
        let human_only = builtins
            .iter()
            .find(|p| p.name == "persona-human-approval")
            .unwrap()
            .clone();

        let mut agent_allow = allow_policy(700, vec![Condition {
            attribute: "subject.global_role".to_string(),
            operator: ConditionOp::Equals,
            value: ConditionValue::String("Agent".to_string()),
        }]);
        agent_allow.actions = vec!["read".to_string(), "approve".to_string()];

        // Agent approving a persona → denied by the builtin (950 > 700).
        let mut agent = AttributeContext::default();
        agent.set("subject.type", "agent");
        agent.set("subject.global_role", "Agent");
        let result = evaluate(
            vec![agent_allow.clone(), human_only.clone()],
            &agent,
            "approve",
            "persona",
        );
        assert_eq!(
            result.effect,
            PolicyEffect::Deny,
            "agents must not approve personas"
        );
        assert_eq!(
            result.matched_policy.as_deref(),
            Some("builtin-persona-human-approval")
        );

        // User approving a persona → allowed by the role allow.
        let mut user = AttributeContext::default();
        user.set("subject.type", "user");
        user.set("subject.global_role", "Agent");
        let result = evaluate(vec![agent_allow, human_only], &user, "approve", "persona");
        assert_eq!(
            result.effect,
            PolicyEffect::Allow,
            "human users keep the role-allowed approve action on personas"
        );
    }

    /// §Performance: "Target: <1ms per evaluation. With cached policies and
    /// precomputed attributes, this is achievable." Verifies the ENGINE's
    /// per-evaluation latency against the full built-in policy set (m34's
    /// 12 policies + the 6 domain splices — the exact set a fresh server
    /// seeds) with a fully populated attribute context. The mean over 200
    /// evaluations must stay under 1ms; using the mean (not a single run)
    /// keeps the assertion robust to scheduler noise in debug builds. A
    /// regressed engine (e.g. per-policy store lookups, quadratic condition
    /// evaluation) blows through this budget by orders of magnitude.
    #[test]
    fn evaluation_meets_latency_target() {
        // The exact seeded set: m34 builtins + the domain splice.
        let mut policies = crate::abac_middleware::m34_builtin_policies();
        policies.extend(crate::abac_middleware::domain_builtin_policies());
        assert!(
            policies.len() >= 15,
            "expected the full seeded builtin set, got {}",
            policies.len()
        );

        // Fully populated context: subject, resource, and environment
        // attributes, mirroring the middleware's extraction.
        let mut ctx = AttributeContext::default();
        ctx.set("subject.type", "user");
        ctx.set("subject.id", "member-user");
        ctx.set("subject.tenant_id", "tenant-acme");
        ctx.set("subject.global_role", "Developer");
        ctx.set("subject.workspace_role", "Developer");
        ctx.set_list(
            "subject.workspace_ids",
            vec!["ws-1".to_string(), "ws-2".to_string()],
        );
        ctx.set_list("subject.team_ids", vec!["team-platform".to_string()]);
        ctx.set("resource.id", "task-42");
        ctx.set("resource.workspace_id", "ws-1");
        ctx.set("resource.repo_id", "repo-1");
        ctx.set("resource.tenant_id", "tenant-acme");
        ctx.set("resource.owner", "user:alice");
        ctx.set("resource.approval_status", "pending");
        ctx.set("env.ip", "10.0.0.5");
        ctx.set_number("env.budget_remaining", 600);
        ctx.set("env.main_health", "green");

        // Warmup (first call pays context-clone and sort setup).
        let _ = evaluate(policies.clone(), &ctx, "read", "task");

        const RUNS: usize = 200;
        let mut total_ms = 0.0f64;
        for _ in 0..RUNS {
            let result = evaluate(policies.clone(), &ctx, "read", "task");
            total_ms += result.evaluation_ms;
        }
        let mean_ms = total_ms / RUNS as f64;
        assert!(
            mean_ms < 1.0,
            "mean evaluation latency {mean_ms:.3}ms over {RUNS} runs with {} policies \
             must stay under the 1ms spec target (§Performance)",
            policies.len()
        );
    }

    // --- TASK-061: Attestation chain attribute evaluation ---

    #[test]
    fn not_equals_bool_condition_compares_booleans() {
        // NotEquals on Bool attributes (used by
        // builtin:require-signed-authorization's unless-clause).
        let cond = Condition {
            attribute: "subject.attestation_chain_present".to_string(),
            operator: ConditionOp::NotEquals,
            value: ConditionValue::Bool(true),
        };
        let mut p = deny_policy(100, vec![cond]);
        p.actions = vec!["merge".to_string()];
        p.resource_types = vec!["attestation".to_string()];

        // Chain present (true != true is false) → policy does not match →
        // a companion allow decides.
        let mut present = AttributeContext::default();
        present.set_bool("subject.attestation_chain_present", true);
        let allow = allow_policy(50, vec![]);
        let result = evaluate(vec![p.clone(), allow], &present, "merge", "attestation");
        assert_eq!(result.effect, PolicyEffect::Allow);

        // Chain explicitly absent-marked (false != true) → deny matches.
        let mut absent = AttributeContext::default();
        absent.set_bool("subject.attestation_chain_present", false);
        let result = evaluate(vec![p], &absent, "merge", "attestation");
        assert_eq!(result.effect, PolicyEffect::Deny);
        assert!(result.matched_policy.is_some());
    }

    #[test]
    fn builtin_signed_authorization_satisfied_by_chain_presence() {
        // §7.2: "Deny unless a valid attestation chain exists for the
        // submitted work" — the shipped builtin pair must let attested
        // work through and deny chain-less work.
        let builtins: Vec<Policy> = crate::abac_middleware::m34_builtin_policies()
            .into_iter()
            .filter(|p| {
                p.name == "builtin:require-signed-authorization"
                    || p.name == "builtin:attested-authorization"
            })
            .collect();
        assert_eq!(builtins.len(), 2, "both builtins must exist");

        // Attested agent work: immutable deny's unless-clause is false, the
        // attested-authorization Allow (priority 150) matches.
        let mut attested = AttributeContext::default();
        attested.set("subject.type", "agent");
        attested.set_bool("subject.attestation_chain_present", true);
        let result = evaluate(builtins.clone(), &attested, "merge", "attestation");
        assert_eq!(result.effect, PolicyEffect::Allow);
        assert_eq!(
            result.matched_policy,
            Some("builtin-attested-authorization".to_string())
        );

        // Chain-less agent work: the immutable deny fires and cannot be
        // overridden by the attested Allow. The attribute is set explicitly
        // to false (not merely absent): eval_condition fails closed on
        // missing attributes, so an absent chain attribute would leave the
        // deny's `!= true` condition unevaluated and fall through to the
        // anonymous default deny (matched_policy: None) instead of
        // attributing the decision to this policy. Production sets the
        // attribute explicitly whenever it runs the attestation-scoped
        // evaluation (git_http.rs / merge_processor.rs), and the neighboring
        // not_equals_bool_condition_compares_booleans models absence the
        // same way.
        let mut unattested = AttributeContext::default();
        unattested.set("subject.type", "agent");
        unattested.set_bool("subject.attestation_chain_present", false);
        let result = evaluate(builtins, &unattested, "merge", "attestation");
        assert_eq!(result.effect, PolicyEffect::Deny);
        assert_eq!(
            result.matched_policy,
            Some("builtin-require-signed-authorization".to_string())
        );
    }

    #[test]
    fn user_deny_overrides_attested_authorization_allow() {
        // The attested-authorization Allow sits at priority 150, below the
        // user band (200-299), so a user deny still restricts attested work.
        let builtins: Vec<Policy> = crate::abac_middleware::m34_builtin_policies()
            .into_iter()
            .filter(|p| {
                p.name == "builtin:require-signed-authorization"
                    || p.name == "builtin:attested-authorization"
            })
            .collect();
        let mut user_deny = deny_policy(
            200,
            vec![Condition {
                attribute: "subject.chain_depth".to_string(),
                operator: ConditionOp::GreaterThan,
                value: ConditionValue::Number(0),
            }],
        );
        user_deny.actions = vec!["merge".to_string()];
        user_deny.resource_types = vec!["attestation".to_string()];

        let mut policies = builtins;
        policies.push(user_deny);

        let mut deep_chain = AttributeContext::default();
        deep_chain.set("subject.type", "agent");
        deep_chain.set_bool("subject.attestation_chain_present", true);
        deep_chain.set_number("subject.chain_depth", 2);
        let result = evaluate(policies, &deep_chain, "merge", "attestation");
        assert_eq!(result.effect, PolicyEffect::Deny);
        assert_eq!(result.matched_policy, Some("p-200".to_string()));
    }

    #[test]
    fn chain_depth_greater_than_denies_deep_chains() {
        let mut ctx = AttributeContext::default();
        ctx.set("subject.type", "agent");
        ctx.set_number("subject.chain_depth", 7);

        let cond = Condition {
            attribute: "subject.chain_depth".to_string(),
            operator: ConditionOp::GreaterThan,
            value: ConditionValue::Number(5),
        };
        let mut p = deny_policy(100, vec![cond]);
        p.actions = vec!["push".to_string()];
        p.resource_types = vec!["attestation".to_string()];

        let result = evaluate(vec![p], &ctx, "push", "attestation");
        assert_eq!(result.effect, PolicyEffect::Deny);
        assert!(result.matched_policy.is_some());
    }

    #[test]
    fn chain_depth_not_greater_than_allows_shallow_chains() {
        let mut ctx = AttributeContext::default();
        ctx.set("subject.type", "agent");
        ctx.set_number("subject.chain_depth", 3);

        let cond = Condition {
            attribute: "subject.chain_depth".to_string(),
            operator: ConditionOp::GreaterThan,
            value: ConditionValue::Number(5),
        };
        let mut deny = deny_policy(100, vec![cond]);
        deny.actions = vec!["push".to_string()];
        deny.resource_types = vec!["attestation".to_string()];

        let mut allow = allow_policy(10, vec![]);
        allow.actions = vec!["push".to_string()];
        allow.resource_types = vec!["attestation".to_string()];

        let result = evaluate(vec![deny, allow], &ctx, "push", "attestation");
        // chain_depth=3 does NOT match >5, so deny policy doesn't fire; allow matches.
        assert_eq!(result.effect, PolicyEffect::Allow);
    }

    #[test]
    fn root_signer_equals_condition_evaluates() {
        let mut ctx = AttributeContext::default();
        ctx.set("subject.type", "agent");
        ctx.set("subject.root_signer", "user:jsell");

        let cond = Condition {
            attribute: "subject.root_signer".to_string(),
            operator: ConditionOp::Equals,
            value: ConditionValue::String("user:jsell".to_string()),
        };
        let mut p = allow_policy(50, vec![cond]);
        p.actions = vec!["merge".to_string()];
        p.resource_types = vec!["attestation".to_string()];

        let result = evaluate(vec![p], &ctx, "merge", "attestation");
        assert_eq!(result.effect, PolicyEffect::Allow);
    }

    #[test]
    fn constraint_count_less_than_condition_evaluates() {
        let mut ctx = AttributeContext::default();
        ctx.set("subject.type", "agent");
        ctx.set_number("subject.constraint_count", 3);

        let cond = Condition {
            attribute: "subject.constraint_count".to_string(),
            operator: ConditionOp::LessThan,
            value: ConditionValue::Number(10),
        };
        let mut p = allow_policy(50, vec![cond]);
        p.actions = vec!["push".to_string()];
        p.resource_types = vec!["attestation".to_string()];

        let result = evaluate(vec![p], &ctx, "push", "attestation");
        assert_eq!(result.effect, PolicyEffect::Allow);
    }

    // --- Dynamic references (§Conditions) -----------------------------------

    #[test]
    fn dynamic_reference_resolves_resource_attr_at_eval_time() {
        // "$resource.repo_id" in a condition value is replaced by the
        // resource attribute at evaluation time. Used for e.g. "subject's
        // repo scope must contain the repo they act on".
        let mut ctx = AttributeContext::default();
        ctx.set("subject.id", "agent-7");
        ctx.set_list(
            "subject.repo_scope",
            vec!["repo:alpha".to_string(), "repo:beta".to_string()],
        );
        ctx.set("resource.repo_id", "repo:beta");

        let cond = Condition {
            attribute: "subject.repo_scope".to_string(),
            operator: ConditionOp::Contains,
            value: ConditionValue::String("$resource.repo_id".to_string()),
        };
        let result = evaluate(
            vec![allow_policy(10, vec![cond.clone()])],
            &ctx,
            "read",
            "repo",
        );
        assert_eq!(result.effect, PolicyEffect::Allow);

        // Same policy, different resource: the reference resolves to the
        // resource in the evaluated context, not a baked-in value.
        ctx.set("resource.repo_id", "repo:gamma");
        let result = evaluate(vec![allow_policy(10, vec![cond])], &ctx, "read", "repo");
        assert_eq!(result.effect, PolicyEffect::Deny);
    }

    #[test]
    fn dynamic_reference_subject_attr_resolves_at_eval_time() {
        // "$subject.tenant_id" as a condition value compares the resource's
        // tenant against the subject's tenant.
        let mut ctx = AttributeContext::default();
        ctx.set("subject.tenant_id", "tenant-a");
        ctx.set("resource.tenant_id", "tenant-a");

        let cond = Condition {
            attribute: "resource.tenant_id".to_string(),
            operator: ConditionOp::Equals,
            value: ConditionValue::String("$subject.tenant_id".to_string()),
        };
        let result = evaluate(
            vec![allow_policy(10, vec![cond.clone()])],
            &ctx,
            "read",
            "repo",
        );
        assert_eq!(result.effect, PolicyEffect::Allow);

        // Cross-tenant resource: reference resolves to tenant-a, mismatch denies.
        ctx.set("resource.tenant_id", "tenant-b");
        let result = evaluate(vec![allow_policy(10, vec![cond])], &ctx, "read", "repo");
        assert_eq!(result.effect, PolicyEffect::Deny);
    }

    #[test]
    fn dynamic_reference_unresolvable_fails_closed() {
        // No resource.repo_id in the context: the reference does not resolve
        // and the Allow condition cannot be established — must NOT grant.
        let mut ctx = AttributeContext::default();
        ctx.set_list("subject.repo_scope", vec!["repo:alpha".to_string()]);

        let cond = Condition {
            attribute: "subject.repo_scope".to_string(),
            operator: ConditionOp::Contains,
            value: ConditionValue::String("$resource.repo_id".to_string()),
        };
        let result = evaluate(vec![allow_policy(10, vec![cond])], &ctx, "read", "repo");
        assert_eq!(result.effect, PolicyEffect::Deny);

        // Killing case for fail-open mutations (review round 1: the Contains
        // case above cannot distinguish fail-closed from a resolver that
        // fabricates an empty string — both yield Deny). Here the operator's
        // outcome FLIPS under that mutation: `subject.type NotEquals
        // $resource.missing` is TRUE against a fabricated "" ("agent" != ""),
        // so a fail-open resolver would ALLOW. Only fail-closed resolution
        // — the condition cannot be established — yields Deny.
        let mut ctx2 = AttributeContext::default();
        ctx2.set("subject.type", "agent");

        let cond2 = Condition {
            attribute: "subject.type".to_string(),
            operator: ConditionOp::NotEquals,
            value: ConditionValue::String("$resource.missing".to_string()),
        };
        let result2 = evaluate(vec![allow_policy(10, vec![cond2])], &ctx2, "read", "repo");
        assert_eq!(
            result2.effect,
            PolicyEffect::Deny,
            "an Allow policy with an unresolvable $reference must not grant — \
             a fabricated empty-string resolution would flip NotEquals to true"
        );
    }

    #[test]
    fn dynamic_reference_number_and_list_resolution() {
        // A numeric reference preserves type: subject.chain_depth > $resource.min_depth.
        let mut ctx = AttributeContext::default();
        ctx.set_number("subject.chain_depth", 7);
        ctx.set_number("resource.min_depth", 5);

        let cond = Condition {
            attribute: "subject.chain_depth".to_string(),
            operator: ConditionOp::GreaterThan,
            value: ConditionValue::String("$resource.min_depth".to_string()),
        };
        let result = evaluate(
            vec![allow_policy(10, vec![cond])],
            &ctx,
            "push",
            "attestation",
        );
        assert_eq!(result.effect, PolicyEffect::Allow);

        // A list-valued reference in scalar (StringList element) position
        // does not resolve — no scalar is fabricated from a list. The
        // Allow condition cannot be established and must fail closed.
        let mut ctx2 = AttributeContext::default();
        ctx2.set("subject.workspace_role", "Developer");
        ctx2.set_list(
            "resource.allowed_roles",
            vec!["Owner".to_string(), "Developer".to_string()],
        );
        let cond2 = Condition {
            attribute: "subject.workspace_role".to_string(),
            operator: ConditionOp::In,
            value: ConditionValue::StringList(vec!["$resource.allowed_roles".to_string()]),
        };
        let result2 = evaluate(vec![allow_policy(10, vec![cond2])], &ctx2, "push", "repo");
        assert_eq!(result2.effect, PolicyEffect::Deny);
    }

    #[test]
    fn dynamic_reference_scalar_number_coerces_for_string_compare() {
        // A numeric reference used in scalar string position coerces to its
        // string form (resolve_dynamic_scalar), matching the String-typed
        // JSON claim convention.
        let mut ctx = AttributeContext::default();
        ctx.set("resource.owner_id", "user-42");
        ctx.set("subject.id", "user-42");

        let cond = Condition {
            attribute: "resource.owner_id".to_string(),
            operator: ConditionOp::Equals,
            value: ConditionValue::String("$subject.id".to_string()),
        };
        let result = evaluate(vec![allow_policy(10, vec![cond])], &ctx, "delete", "repo");
        assert_eq!(result.effect, PolicyEffect::Allow);
    }

    // --- Spec §Policy Examples: each documented example, verbatim ------------

    /// Helper: build a policy mirroring the spec's YAML example shape.
    fn spec_example(
        name: &str,
        scope: PolicyScope,
        priority: u32,
        effect: PolicyEffect,
        actions: &[&str],
        resource_types: &[&str],
        conditions: Vec<Condition>,
    ) -> Policy {
        Policy {
            id: Id::new(format!("spec-{name}")),
            name: name.to_string(),
            description: String::new(),
            scope,
            scope_id: None,
            priority,
            effect,
            conditions,
            actions: actions.iter().map(|s| s.to_string()).collect(),
            resource_types: resource_types.iter().map(|s| s.to_string()).collect(),
            enabled: true,
            built_in: false,
            immutable: false,
            created_by: Id::new("spec"),
            created_at: 0,
            updated_at: 0,
        }
    }

    /// Tenant-level example: "agents can only access their scoped repo" —
    /// Deny when `subject.type == agent` AND `subject.repo_scope !=
    /// $resource.repo_id`. The dynamic reference is the example's core
    /// mechanism; both the scoped agent (no deny) and the out-of-scope agent
    /// (deny) must evaluate correctly.
    #[test]
    fn spec_example_agent_repo_scope() {
        let p = spec_example(
            "agent-repo-scope",
            PolicyScope::Tenant,
            100,
            PolicyEffect::Deny,
            &["*"],
            &["*"],
            vec![
                Condition {
                    attribute: "subject.type".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("agent".to_string()),
                },
                Condition {
                    attribute: "subject.repo_scope".to_string(),
                    operator: ConditionOp::NotEquals,
                    value: ConditionValue::String("$resource.repo_id".to_string()),
                },
            ],
        );

        // Agent acting inside its scoped repo: repo_scope == resource.repo_id,
        // the NotEquals condition fails, the Deny does not match. The paired
        // Allow at priority 50 grants.
        let mut ctx = AttributeContext::default();
        ctx.set("subject.type", "agent");
        ctx.set("subject.repo_scope", "repo-gyre-server");
        ctx.set("resource.repo_id", "repo-gyre-server");
        let allow = allow_policy(50, vec![]);
        let result = evaluate(vec![p.clone(), allow.clone()], &ctx, "read", "repo");
        assert_eq!(result.effect, PolicyEffect::Allow);

        // Agent acting outside its scoped repo: repo_scope != resource.repo_id
        // → the Deny matches at priority 100 and wins over the Allow at 50.
        ctx.set("resource.repo_id", "repo-other");
        let result = evaluate(vec![p, allow], &ctx, "read", "repo");
        assert_eq!(result.effect, PolicyEffect::Deny);

        // A user is untouched by this policy: only the Allow applies.
        let mut ctx = AttributeContext::default();
        ctx.set("subject.type", "user");
        let result = evaluate(
            vec![spec_example(
                "agent-repo-scope",
                PolicyScope::Tenant,
                100,
                PolicyEffect::Deny,
                &["*"],
                &["*"],
                vec![
                    Condition {
                        attribute: "subject.type".to_string(),
                        operator: ConditionOp::Equals,
                        value: ConditionValue::String("agent".to_string()),
                    },
                    Condition {
                        attribute: "subject.repo_scope".to_string(),
                        operator: ConditionOp::NotEquals,
                        value: ConditionValue::String("$resource.repo_id".to_string()),
                    },
                ],
            )],
            &ctx,
            "read",
            "repo",
        );
        // No Allow in this set → default deny, but the important part is the
        // deny is NOT the example policy (its subject.type condition failed).
        assert_eq!(result.effect, PolicyEffect::Deny);
        assert_eq!(result.matched_policy, None);
    }

    /// Workspace-level example: "only Owner/Admin can manage personas" —
    /// Deny write/delete on persona when workspace_role not_in [Owner, Admin].
    #[test]
    fn spec_example_persona_management() {
        let p = spec_example(
            "persona-management",
            PolicyScope::Workspace,
            90,
            PolicyEffect::Deny,
            &["write", "delete"],
            &["persona"],
            vec![Condition {
                attribute: "subject.workspace_role".to_string(),
                operator: ConditionOp::NotIn,
                value: ConditionValue::StringList(vec!["Owner".to_string(), "Admin".to_string()]),
            }],
        );
        let allow = allow_policy(50, vec![]);

        // Developer managing a persona: not in [Owner, Admin] → deny.
        let mut ctx = AttributeContext::default();
        ctx.set("subject.workspace_role", "Developer");
        let result = evaluate(vec![p.clone(), allow.clone()], &ctx, "write", "persona");
        assert_eq!(result.effect, PolicyEffect::Deny);

        // Admin managing a persona: NotIn fails → allow.
        ctx.set("subject.workspace_role", "Admin");
        let result = evaluate(vec![p.clone(), allow.clone()], &ctx, "write", "persona");
        assert_eq!(result.effect, PolicyEffect::Allow);

        // Owner: allowed.
        ctx.set("subject.workspace_role", "Owner");
        let result = evaluate(vec![p.clone(), allow.clone()], &ctx, "delete", "persona");
        assert_eq!(result.effect, PolicyEffect::Allow);

        // Resource-type filter: the policy only applies to personas — a
        // Developer task write is unaffected by it.
        let result = evaluate(vec![p, allow], &ctx, "write", "task");
        assert_eq!(result.effect, PolicyEffect::Allow);
    }

    /// Workspace-level example: "viewers can't spawn agents" — Deny spawn on
    /// agent when workspace_role == Viewer.
    #[test]
    fn spec_example_viewer_no_spawn() {
        let p = spec_example(
            "viewer-no-spawn",
            PolicyScope::Workspace,
            90,
            PolicyEffect::Deny,
            &["spawn"],
            &["agent"],
            vec![Condition {
                attribute: "subject.workspace_role".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::String("Viewer".to_string()),
            }],
        );
        let allow = allow_policy(50, vec![]);

        let mut ctx = AttributeContext::default();
        ctx.set("subject.workspace_role", "Viewer");
        let result = evaluate(vec![p.clone(), allow.clone()], &ctx, "spawn", "agent");
        assert_eq!(result.effect, PolicyEffect::Deny);

        ctx.set("subject.workspace_role", "Developer");
        let result = evaluate(vec![p, allow], &ctx, "spawn", "agent");
        assert_eq!(result.effect, PolicyEffect::Allow);
    }

    /// Repo-level example: "only approved personas can be used as gate
    /// agents" — Deny gate_review on mr when subject.type == agent AND
    /// subject.attestation_level < 3.
    #[test]
    fn spec_example_gate_approved_persona() {
        let p = spec_example(
            "gate-approved-persona",
            PolicyScope::Repo,
            80,
            PolicyEffect::Deny,
            &["gate_review"],
            &["mr"],
            vec![
                Condition {
                    attribute: "subject.type".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("agent".to_string()),
                },
                Condition {
                    attribute: "subject.attestation_level".to_string(),
                    operator: ConditionOp::LessThan,
                    value: ConditionValue::Number(3),
                },
            ],
        );
        let allow = allow_policy(50, vec![]);

        // Level-2 agent reviewing a gate: < 3 → deny.
        let mut ctx = AttributeContext::default();
        ctx.set("subject.type", "agent");
        ctx.set_number("subject.attestation_level", 2);
        let result = evaluate(vec![p.clone(), allow.clone()], &ctx, "gate_review", "mr");
        assert_eq!(result.effect, PolicyEffect::Deny);

        // Level-3 agent: not < 3 → allow.
        ctx.set_number("subject.attestation_level", 3);
        let result = evaluate(vec![p, allow], &ctx, "gate_review", "mr");
        assert_eq!(result.effect, PolicyEffect::Allow);
    }

    /// Tenant-level example: "deny all by default" — priority 0, no
    /// conditions, always matches. With only an Allow present, the higher
    /// priority Allow wins; with no Allow, the default-deny policy is the
    /// match (matched_policy set, distinct from no-match default deny).
    #[test]
    fn spec_example_default_deny() {
        let p = spec_example(
            "default-deny",
            PolicyScope::Tenant,
            0,
            PolicyEffect::Deny,
            &["*"],
            &["*"],
            vec![],
        );

        // No other policy: the default-deny example matches.
        let ctx = AttributeContext::default();
        let result = evaluate(vec![p.clone()], &ctx, "read", "repo");
        assert_eq!(result.effect, PolicyEffect::Deny);
        assert_eq!(result.matched_policy.as_deref(), Some("spec-default-deny"));

        // An explicit Allow outranks the priority-0 default deny.
        let allow = allow_policy(10, vec![]);
        let result = evaluate(vec![p, allow], &ctx, "read", "repo");
        assert_eq!(result.effect, PolicyEffect::Allow);
    }
}
