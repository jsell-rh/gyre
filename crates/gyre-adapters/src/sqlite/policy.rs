use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::policy::{Condition, Policy, PolicyDecision, PolicyEffect, PolicyScope};
use gyre_ports::PolicyRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::{policies, policy_decisions};

fn scope_to_str(s: &PolicyScope) -> &'static str {
    match s {
        PolicyScope::Tenant => "tenant",
        PolicyScope::Workspace => "workspace",
        PolicyScope::Repo => "repo",
    }
}

fn str_to_scope(s: &str) -> Result<PolicyScope> {
    match s {
        "tenant" => Ok(PolicyScope::Tenant),
        "workspace" => Ok(PolicyScope::Workspace),
        "repo" => Ok(PolicyScope::Repo),
        other => Err(anyhow!("unknown policy scope: {}", other)),
    }
}

fn effect_to_str(e: &PolicyEffect) -> &'static str {
    match e {
        PolicyEffect::Allow => "allow",
        PolicyEffect::Deny => "deny",
    }
}

fn str_to_effect(s: &str) -> Result<PolicyEffect> {
    match s {
        "allow" => Ok(PolicyEffect::Allow),
        "deny" => Ok(PolicyEffect::Deny),
        other => Err(anyhow!("unknown policy effect: {}", other)),
    }
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = policies)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct PolicyRow {
    id: String,
    name: String,
    description: String,
    scope: String,
    scope_id: Option<String>,
    priority: i32,
    effect: String,
    conditions: String,
    actions: String,
    resource_types: String,
    enabled: i32,
    built_in: i32,
    created_by: String,
    created_at: i64,
    updated_at: i64,
}

impl PolicyRow {
    fn into_policy(self) -> Result<Policy> {
        let conditions: Vec<Condition> = serde_json::from_str(&self.conditions).unwrap_or_default();
        let actions: Vec<String> = serde_json::from_str(&self.actions).unwrap_or_default();
        let resource_types: Vec<String> =
            serde_json::from_str(&self.resource_types).unwrap_or_default();
        Ok(Policy {
            id: Id::new(self.id),
            name: self.name,
            description: self.description,
            scope: str_to_scope(&self.scope)?,
            scope_id: self.scope_id,
            priority: self.priority as u32,
            effect: str_to_effect(&self.effect)?,
            conditions,
            actions,
            resource_types,
            enabled: self.enabled != 0,
            built_in: self.built_in != 0,
            created_by: self.created_by,
            created_at: self.created_at as u64,
            updated_at: self.updated_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = policies)]
struct NewPolicyRow {
    id: String,
    name: String,
    description: String,
    scope: String,
    scope_id: Option<String>,
    priority: i32,
    effect: String,
    conditions: String,
    actions: String,
    resource_types: String,
    enabled: i32,
    built_in: i32,
    created_by: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = policy_decisions)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct PolicyDecisionRow {
    request_id: String,
    subject_id: String,
    subject_type: String,
    action: String,
    resource_type: String,
    resource_id: String,
    decision: String,
    matched_policy: Option<String>,
    evaluated_policies: i32,
    evaluation_ms: f64,
    evaluated_at: i64,
}

impl PolicyDecisionRow {
    fn into_decision(self) -> Result<PolicyDecision> {
        Ok(PolicyDecision {
            request_id: self.request_id,
            subject_id: self.subject_id,
            subject_type: self.subject_type,
            action: self.action,
            resource_type: self.resource_type,
            resource_id: self.resource_id,
            decision: str_to_effect(&self.decision)?,
            matched_policy: self.matched_policy.map(Id::new),
            evaluated_policies: self.evaluated_policies as u32,
            evaluation_ms: self.evaluation_ms,
            evaluated_at: self.evaluated_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = policy_decisions)]
struct NewPolicyDecisionRow<'a> {
    request_id: &'a str,
    subject_id: &'a str,
    subject_type: &'a str,
    action: &'a str,
    resource_type: &'a str,
    resource_id: &'a str,
    decision: &'a str,
    matched_policy: Option<&'a str>,
    evaluated_policies: i32,
    evaluation_ms: f64,
    evaluated_at: i64,
}

#[async_trait]
impl PolicyRepository for SqliteStorage {
    async fn create(&self, policy: &Policy) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let p = policy.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewPolicyRow {
                id: p.id.as_str().to_string(),
                name: p.name.clone(),
                description: p.description.clone(),
                scope: scope_to_str(&p.scope).to_string(),
                scope_id: p.scope_id.clone(),
                priority: p.priority as i32,
                effect: effect_to_str(&p.effect).to_string(),
                conditions: serde_json::to_string(&p.conditions)?,
                actions: serde_json::to_string(&p.actions)?,
                resource_types: serde_json::to_string(&p.resource_types)?,
                enabled: p.enabled as i32,
                built_in: p.built_in as i32,
                created_by: p.created_by.clone(),
                created_at: p.created_at as i64,
                updated_at: p.updated_at as i64,
            };
            diesel::insert_into(policies::table)
                .values(&row)
                .on_conflict(policies::id)
                .do_update()
                .set((
                    policies::name.eq(&row.name),
                    policies::description.eq(&row.description),
                    policies::scope.eq(&row.scope),
                    policies::scope_id.eq(&row.scope_id),
                    policies::priority.eq(row.priority),
                    policies::effect.eq(&row.effect),
                    policies::conditions.eq(&row.conditions),
                    policies::actions.eq(&row.actions),
                    policies::resource_types.eq(&row.resource_types),
                    policies::enabled.eq(row.enabled),
                    policies::updated_at.eq(row.updated_at),
                ))
                .execute(&mut *conn)
                .context("insert policy")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &str) -> Result<Option<Policy>> {
        let pool = Arc::clone(&self.pool);
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Policy>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = policies::table
                .find(&id)
                .first::<PolicyRow>(&mut *conn)
                .optional()
                .context("find policy by id")?;
            result.map(PolicyRow::into_policy).transpose()
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Policy>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<Policy>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = policies::table
                .order(policies::priority.desc())
                .load::<PolicyRow>(&mut *conn)
                .context("list policies")?;
            rows.into_iter().map(PolicyRow::into_policy).collect()
        })
        .await?
    }

    async fn list_by_scope(
        &self,
        scope: &PolicyScope,
        scope_id: Option<&str>,
    ) -> Result<Vec<Policy>> {
        let pool = Arc::clone(&self.pool);
        let scope_str = scope_to_str(scope).to_string();
        let scope_id = scope_id.map(String::from);
        tokio::task::spawn_blocking(move || -> Result<Vec<Policy>> {
            let mut conn = pool.get().context("get db connection")?;
            let mut query = policies::table
                .filter(policies::scope.eq(&scope_str))
                .order(policies::priority.desc())
                .into_boxed();
            if let Some(sid) = scope_id {
                query = query.filter(policies::scope_id.eq(sid));
            } else {
                query = query.filter(policies::scope_id.is_null());
            }
            let rows = query
                .load::<PolicyRow>(&mut *conn)
                .context("list policies by scope")?;
            rows.into_iter().map(PolicyRow::into_policy).collect()
        })
        .await?
    }

    async fn update(&self, policy: &Policy) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let p = policy.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(policies::table.find(p.id.as_str()))
                .set((
                    policies::name.eq(&p.name),
                    policies::description.eq(&p.description),
                    policies::scope.eq(scope_to_str(&p.scope)),
                    policies::scope_id.eq(&p.scope_id),
                    policies::priority.eq(p.priority as i32),
                    policies::effect.eq(effect_to_str(&p.effect)),
                    policies::conditions.eq(serde_json::to_string(&p.conditions)?),
                    policies::actions.eq(serde_json::to_string(&p.actions)?),
                    policies::resource_types.eq(serde_json::to_string(&p.resource_types)?),
                    policies::enabled.eq(p.enabled as i32),
                    policies::updated_at.eq(p.updated_at as i64),
                ))
                .execute(&mut *conn)
                .context("update policy")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(policies::table.find(&id))
                .execute(&mut *conn)
                .context("delete policy")?;
            Ok(())
        })
        .await?
    }

    async fn record_decision(&self, decision: &PolicyDecision) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let d = decision.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let matched = d.matched_policy.as_ref().map(|id| id.as_str().to_string());
            let row = NewPolicyDecisionRow {
                request_id: &d.request_id,
                subject_id: &d.subject_id,
                subject_type: &d.subject_type,
                action: &d.action,
                resource_type: &d.resource_type,
                resource_id: &d.resource_id,
                decision: effect_to_str(&d.decision),
                matched_policy: matched.as_deref(),
                evaluated_policies: d.evaluated_policies as i32,
                evaluation_ms: d.evaluation_ms,
                evaluated_at: d.evaluated_at as i64,
            };
            diesel::insert_into(policy_decisions::table)
                .values(&row)
                .on_conflict(policy_decisions::request_id)
                .do_nothing()
                .execute(&mut *conn)
                .context("insert policy decision")?;
            Ok(())
        })
        .await?
    }

    async fn list_decisions(
        &self,
        subject_id: Option<&str>,
        resource_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PolicyDecision>> {
        let pool = Arc::clone(&self.pool);
        let subject_id = subject_id.map(String::from);
        let resource_type = resource_type.map(String::from);
        tokio::task::spawn_blocking(move || -> Result<Vec<PolicyDecision>> {
            let mut conn = pool.get().context("get db connection")?;
            let mut query = policy_decisions::table
                .order(policy_decisions::evaluated_at.desc())
                .into_boxed();
            if let Some(sid) = subject_id {
                query = query.filter(policy_decisions::subject_id.eq(sid));
            }
            if let Some(rt) = resource_type {
                query = query.filter(policy_decisions::resource_type.eq(rt));
            }
            let rows = query
                .limit(limit as i64)
                .load::<PolicyDecisionRow>(&mut *conn)
                .context("list policy decisions")?;
            rows.into_iter()
                .map(PolicyDecisionRow::into_decision)
                .collect()
        })
        .await?
    }
}
