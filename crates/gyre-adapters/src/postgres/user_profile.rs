use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{JudgmentEntry, JudgmentType, UserNotificationPreference, UserToken};
use gyre_ports::{
    JudgmentLedgerRepository, UserNotificationPreferenceRepository, UserTokenRepository,
};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::{user_notification_preferences, user_tokens};

// ─── User Notification Preferences ──────────────────────────────────────────

#[derive(Queryable, Selectable)]
#[diesel(table_name = user_notification_preferences)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct NotifPrefRow {
    user_id: String,
    notification_type: String,
    enabled: i32,
}

#[derive(Insertable)]
#[diesel(table_name = user_notification_preferences)]
struct NotifPrefRecord<'a> {
    user_id: &'a str,
    notification_type: &'a str,
    enabled: i32,
}

impl From<NotifPrefRow> for UserNotificationPreference {
    fn from(r: NotifPrefRow) -> Self {
        UserNotificationPreference::new(Id::new(r.user_id), r.notification_type, r.enabled != 0)
    }
}

#[async_trait]
impl UserNotificationPreferenceRepository for PgStorage {
    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<UserNotificationPreference>> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<UserNotificationPreference>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = user_notification_preferences::table
                .filter(user_notification_preferences::user_id.eq(&uid))
                .load::<NotifPrefRow>(&mut *conn)
                .context("list notification preferences")?;
            Ok(rows.into_iter().map(Into::into).collect())
        })
        .await?
    }

    async fn upsert(&self, pref: &UserNotificationPreference) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let p = pref.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = NotifPrefRecord {
                user_id: p.user_id.as_str(),
                notification_type: &p.notification_type,
                enabled: if p.enabled { 1 } else { 0 },
            };
            diesel::insert_into(user_notification_preferences::table)
                .values(&record)
                .on_conflict((
                    user_notification_preferences::user_id,
                    user_notification_preferences::notification_type,
                ))
                .do_update()
                .set(user_notification_preferences::enabled.eq(record.enabled))
                .execute(&mut *conn)
                .context("upsert notification preference")?;
            Ok(())
        })
        .await?
    }

    async fn upsert_batch(&self, prefs: &[UserNotificationPreference]) -> Result<()> {
        for pref in prefs {
            self.upsert(pref).await?;
        }
        Ok(())
    }
}

// ─── User Tokens ─────────────────────────────────────────────────────────────

#[derive(Queryable, Selectable)]
#[diesel(table_name = user_tokens)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct UserTokenRow {
    id: String,
    user_id: String,
    name: String,
    token_hash: String,
    created_at: i64,
    last_used_at: Option<i64>,
    expires_at: Option<i64>,
}

#[derive(Insertable)]
#[diesel(table_name = user_tokens)]
struct UserTokenRecord<'a> {
    id: &'a str,
    user_id: &'a str,
    name: &'a str,
    token_hash: &'a str,
    created_at: i64,
    last_used_at: Option<i64>,
    expires_at: Option<i64>,
}

impl From<UserTokenRow> for UserToken {
    fn from(r: UserTokenRow) -> Self {
        let mut t = UserToken::new(
            Id::new(r.id),
            Id::new(r.user_id),
            r.name,
            r.token_hash,
            r.created_at as u64,
        );
        t.last_used_at = r.last_used_at.map(|v| v as u64);
        t.expires_at = r.expires_at.map(|v| v as u64);
        t
    }
}

#[async_trait]
impl UserTokenRepository for PgStorage {
    async fn create(&self, token: &UserToken) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let t = token.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = UserTokenRecord {
                id: t.id.as_str(),
                user_id: t.user_id.as_str(),
                name: &t.name,
                token_hash: &t.token_hash,
                created_at: t.created_at as i64,
                last_used_at: t.last_used_at.map(|v| v as i64),
                expires_at: t.expires_at.map(|v| v as i64),
            };
            diesel::insert_into(user_tokens::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert user token")?;
            Ok(())
        })
        .await?
    }

    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<UserToken>> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<UserToken>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = user_tokens::table
                .filter(user_tokens::user_id.eq(&uid))
                .order(user_tokens::created_at.desc())
                .load::<UserTokenRow>(&mut *conn)
                .context("list user tokens")?;
            Ok(rows.into_iter().map(Into::into).collect())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<UserToken>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<UserToken>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = user_tokens::table
                .find(id.as_str())
                .first::<UserTokenRow>(&mut *conn)
                .optional()
                .context("find user token by id")?;
            Ok(row.map(Into::into))
        })
        .await?
    }

    async fn find_by_hash(&self, token_hash: &str) -> Result<Option<UserToken>> {
        let pool = Arc::clone(&self.pool);
        let hash = token_hash.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<UserToken>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = user_tokens::table
                .filter(user_tokens::token_hash.eq(&hash))
                .first::<UserTokenRow>(&mut *conn)
                .optional()
                .context("find user token by hash")?;
            Ok(row.map(Into::into))
        })
        .await?
    }

    async fn touch(&self, id: &Id, last_used_at: u64) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(user_tokens::table.find(id.as_str()))
                .set(user_tokens::last_used_at.eq(last_used_at as i64))
                .execute(&mut *conn)
                .context("touch user token")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id, user_id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            // Scoped delete: only delete if token belongs to the requesting user.
            diesel::delete(
                user_tokens::table
                    .filter(user_tokens::id.eq(id.as_str()))
                    .filter(user_tokens::user_id.eq(uid.as_str())),
            )
            .execute(&mut *conn)
            .context("delete user token")?;
            Ok(())
        })
        .await?
    }
}

// ─── Judgment Ledger ─────────────────────────────────────────────────────────

/// Audit event types that carry judgment semantics (HSI §12 Judgment Ledger
/// Sources). Written by the gate-override, trust-transition and meta-spec
/// publish flows; human attribution via `audit_events.user_id`.
const JUDGMENT_AUDIT_TYPES: [&str; 3] = ["gate_override", "trust_change", "meta_spec_publish"];

#[async_trait]
impl JudgmentLedgerRepository for PgStorage {
    async fn list_for_user(
        &self,
        approver_id: &str,
        workspace_id: Option<&Id>,
        judgment_type: Option<JudgmentType>,
        since: Option<u64>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<JudgmentEntry>> {
        use crate::schema::{audit_events, spec_approvals, spec_ledger_entries};
        let pool = Arc::clone(&self.pool);
        let approver = approver_id.to_string();
        let ws = workspace_id.map(|id| id.as_str().to_string());
        let jt = judgment_type;

        tokio::task::spawn_blocking(move || -> Result<Vec<JudgmentEntry>> {
            let mut conn = pool.get().context("get db connection")?;
            let since_i = since.unwrap_or(0) as i64;
            let mut entries: Vec<JudgmentEntry> = Vec::new();

            // A `?type=` filter narrows which physical sources are consulted:
            // approval/rejection live in spec_approvals; gate/trust/meta-spec
            // live in audit_events.
            let want_approvals = jt.is_none_or(|t| {
                matches!(t, JudgmentType::SpecApproval | JudgmentType::SpecRejection)
            });
            let want_audit = jt.is_none_or(|t| {
                matches!(
                    t,
                    JudgmentType::GateOverride | JudgmentType::TrustGrant | JudgmentType::MetaSpec
                )
            });

            // ── Source 1: spec approvals & rejections ─────────────────────────
            // Workspace attribution via LEFT JOIN on the spec ledger entry
            // (spec_approvals has no workspace column). When a workspace
            // filter is set, entries with unknown workspace are excluded
            // (SQL `workspace_id = ?` drops NULLs on the left-joined side).
            if want_approvals {
                let mut query = spec_approvals::table
                    .left_join(
                        spec_ledger_entries::table
                            .on(spec_ledger_entries::path.eq(spec_approvals::spec_path)),
                    )
                    .filter(spec_approvals::approver_id.eq(&approver))
                    .filter(spec_approvals::approved_at.ge(since_i))
                    .select((
                        spec_approvals::spec_path,
                        spec_approvals::approved_at,
                        spec_approvals::revoked_at,
                        spec_approvals::revocation_reason,
                        spec_approvals::rejected_at,
                        spec_approvals::rejected_reason,
                        spec_ledger_entries::workspace_id.nullable(),
                    ))
                    .into_boxed();
                if let Some(w) = &ws {
                    query = query.filter(spec_ledger_entries::workspace_id.eq(w));
                }
                let rows: Vec<(
                    String,
                    i64,
                    Option<i64>,
                    Option<String>,
                    Option<i64>,
                    Option<String>,
                    Option<String>,
                )> = query
                    .load(&mut *conn)
                    .context("judgment ledger: spec_approvals")?;

                for (
                    path,
                    approved_at,
                    revoked_at,
                    revoke_reason,
                    rejected_at,
                    reject_reason,
                    wid,
                ) in rows
                {
                    // revoked_at/rejected_at indicate a rejection.
                    let rejected = revoked_at.is_some() || rejected_at.is_some();
                    let jtype = if rejected {
                        JudgmentType::SpecRejection
                    } else {
                        JudgmentType::SpecApproval
                    };
                    if jt.is_some_and(|f| f != jtype) {
                        continue;
                    }
                    let detail = if rejected {
                        revoke_reason.or(reject_reason)
                    } else {
                        None
                    };
                    entries.push(JudgmentEntry::new(
                        jtype,
                        path,
                        wid.map(Id::new),
                        approved_at.max(0) as u64,
                        detail,
                    ));
                }
            }

            // ── Source 2: audit-carried judgments (gate / trust / meta-spec) ──
            if want_audit {
                let mut query = audit_events::table
                    .filter(audit_events::user_id.eq(&approver))
                    .filter(audit_events::event_type.eq_any(JUDGMENT_AUDIT_TYPES))
                    .filter(audit_events::timestamp.ge(since_i))
                    .select((
                        audit_events::event_type,
                        audit_events::workspace_id,
                        audit_events::resource_id,
                        audit_events::detail,
                        audit_events::timestamp,
                    ))
                    .into_boxed();
                if let Some(w) = &ws {
                    query = query.filter(audit_events::workspace_id.eq(w));
                }
                let rows: Vec<(String, Option<String>, Option<String>, String, i64)> = query
                    .load(&mut *conn)
                    .context("judgment ledger: audit_events")?;

                for (etype, wid, resource_id, detail_raw, ts) in rows {
                    let detail: serde_json::Value =
                        serde_json::from_str(&detail_raw).unwrap_or(serde_json::Value::Null);
                    let jtype = match etype.as_str() {
                        "gate_override" => JudgmentType::GateOverride,
                        "trust_change" => JudgmentType::TrustGrant,
                        _ => JudgmentType::MetaSpec,
                    };
                    if jt.is_some_and(|f| f != jtype) {
                        continue;
                    }
                    let str_field = |k: &str| -> Option<String> {
                        detail.get(k).and_then(|v| v.as_str()).map(str::to_string)
                    };
                    let (entity_ref, entry_detail) = match jtype {
                        JudgmentType::GateOverride => (
                            str_field("mr_id").or(resource_id).unwrap_or_default(),
                            format!(
                                "gate {} overridden: {} -> {}",
                                str_field("gate_type").unwrap_or_else(|| "unknown".into()),
                                str_field("from_status").unwrap_or_else(|| "failed".into()),
                                str_field("to_status").unwrap_or_else(|| "overridden".into()),
                            ),
                        ),
                        JudgmentType::TrustGrant => (
                            str_field("workspace_id").or(wid.clone()).unwrap_or_default(),
                            format!(
                                "trust {} -> {}",
                                str_field("from").unwrap_or_default(),
                                str_field("to").unwrap_or_default(),
                            ),
                        ),
                        _ => (
                            str_field("name").or(resource_id).unwrap_or_default(),
                            format!(
                                "{} v{}",
                                str_field("kind").unwrap_or_default(),
                                detail.get("version").and_then(|v| v.as_u64()).unwrap_or(0),
                            ),
                        ),
                    };
                    entries.push(JudgmentEntry::new(
                        jtype,
                        entity_ref,
                        wid.map(Id::new),
                        ts.max(0) as u64,
                        Some(entry_detail),
                    ));
                }
            }

            // Reverse-chronological across sources; entity_ref tie-break keeps
            // pagination deterministic.
            entries.sort_by(|a, b| {
                b.timestamp
                    .cmp(&a.timestamp)
                    .then_with(|| b.entity_ref.cmp(&a.entity_ref))
            });
            Ok(entries
                .into_iter()
                .skip(offset as usize)
                .take(limit as usize)
                .collect())
        })
        .await?
    }
}
