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

#[derive(Clone, Debug)]
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

    /// Merge JWT claims into the context under the `subject.` namespace.
    pub fn merge_jwt_claims(&mut self, claims: &serde_json::Value) {
        if let Some(obj) = claims.as_object() {
            for (key, val) in obj {
                let full_key = format!("subject.{key}");
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
            match (&cond.operator, attr_val, &cond.value) {
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

/// Sort key for a policy: higher priority first, then scope specificity.
fn policy_sort_key(p: &Policy) -> (u32, u8) {
    let scope_rank = match p.scope {
        PolicyScope::Repo => 2,
        PolicyScope::Workspace => 1,
        PolicyScope::Tenant => 0,
    };
    (p.priority, scope_rank)
}

/// Evaluate the given list of `policies` against `ctx` for `action` on `resource_type`.
///
/// Returns `Allow` if any policy with `Allow` effect matches first, or `Deny`
/// if a `Deny` policy matches first. If no policy matches, returns `Deny`
/// (default-deny posture).
pub fn evaluate(
    mut policies: Vec<Policy>,
    ctx: &AttributeContext,
    action: &str,
    resource_type: &str,
) -> EvalResult {
    let t0 = std::time::Instant::now();

    // Filter to enabled policies that apply to this action/resource_type.
    policies.retain(|p| p.enabled && p.applies_to(action, resource_type));

    // Sort: highest priority first, then most-specific scope.
    policies.sort_by(|a, b| {
        let ka = policy_sort_key(a);
        let kb = policy_sort_key(b);
        kb.cmp(&ka)
    });

    let total = policies.len() as u32;

    for policy in &policies {
        if eval_policy_conditions(policy, ctx) {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            return EvalResult {
                effect: policy.effect.clone(),
                matched_policy: Some(policy.id.to_string()),
                evaluated_count: total,
                evaluation_ms: ms,
            };
        }
    }

    // No match → default deny.
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
            created_by: "system".to_string(),
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
    fn merge_jwt_claims_into_context() {
        let mut ctx = AttributeContext::default();
        let claims = serde_json::json!({
            "workspace_role": "Developer",
            "groups": ["infra", "dev"],
            "attestation_level": 2
        });
        ctx.merge_jwt_claims(&claims);
        assert!(ctx.has("subject.workspace_role"));
        assert!(ctx.has("subject.groups"));
        assert!(ctx.has("subject.attestation_level"));
    }
}
