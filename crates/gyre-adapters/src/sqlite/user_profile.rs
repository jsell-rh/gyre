use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{JudgmentEntry, JudgmentType, UserNotificationPreference, UserToken};
use gyre_ports::{
    JudgmentLedgerRepository, UserNotificationPreferenceRepository, UserTokenRepository,
};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::{user_notification_preferences, user_tokens};

// ─── User Notification Preferences ──────────────────────────────────────────

#[derive(Queryable, Selectable)]
#[diesel(table_name = user_notification_preferences)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
impl UserNotificationPreferenceRepository for SqliteStorage {
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
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
impl UserTokenRepository for SqliteStorage {
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
impl JudgmentLedgerRepository for SqliteStorage {
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
                    .left_join(spec_ledger_entries::table)
                    .on(spec_ledger_entries::path.eq(spec_approvals::spec_path))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use gyre_domain::{User, UserNotificationPreference};
    use gyre_ports::UserRepository;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    fn make_user(id: &str) -> User {
        User::new(Id::new(id), format!("ext-{id}"), format!("user-{id}"), 1000)
    }

    #[tokio::test]
    async fn notification_prefs_roundtrip() {
        let (_tmp, s) = setup();
        let u = make_user("u1");
        UserRepository::create(&s, &u).await.unwrap();

        let pref = UserNotificationPreference::new(u.id.clone(), "spec_approved", true);
        UserNotificationPreferenceRepository::upsert(&s, &pref)
            .await
            .unwrap();

        let prefs = UserNotificationPreferenceRepository::list_for_user(&s, &u.id)
            .await
            .unwrap();
        assert_eq!(prefs.len(), 1);
        assert_eq!(prefs[0].notification_type, "spec_approved");
        assert!(prefs[0].enabled);
    }

    #[tokio::test]
    async fn notification_pref_upsert_updates() {
        let (_tmp, s) = setup();
        let u = make_user("u2");
        UserRepository::create(&s, &u).await.unwrap();

        let pref = UserNotificationPreference::new(u.id.clone(), "spec_approved", true);
        UserNotificationPreferenceRepository::upsert(&s, &pref)
            .await
            .unwrap();

        let disabled = UserNotificationPreference::new(u.id.clone(), "spec_approved", false);
        UserNotificationPreferenceRepository::upsert(&s, &disabled)
            .await
            .unwrap();

        let prefs = UserNotificationPreferenceRepository::list_for_user(&s, &u.id)
            .await
            .unwrap();
        assert_eq!(prefs.len(), 1);
        assert!(!prefs[0].enabled);
    }

    #[tokio::test]
    async fn token_create_list_delete() {
        let (_tmp, s) = setup();
        let u = make_user("u3");
        UserRepository::create(&s, &u).await.unwrap();

        let token = UserToken::new(Id::new("tok1"), u.id.clone(), "ci-token", "hash-abc", 1000);
        UserTokenRepository::create(&s, &token).await.unwrap();

        let list = UserTokenRepository::list_for_user(&s, &u.id).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "ci-token");
        // token_hash is stored but not exposed in API — verify it's in DB.
        assert_eq!(list[0].token_hash, "hash-abc");

        UserTokenRepository::delete(&s, &Id::new("tok1"), &u.id)
            .await
            .unwrap();
        let list = UserTokenRepository::list_for_user(&s, &u.id).await.unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn token_delete_wrong_user_no_op() {
        let (_tmp, s) = setup();
        let u1 = make_user("u4");
        let u2 = make_user("u5");
        UserRepository::create(&s, &u1).await.unwrap();
        UserRepository::create(&s, &u2).await.unwrap();

        let token = UserToken::new(Id::new("tok2"), u1.id.clone(), "tok", "h", 1000);
        UserTokenRepository::create(&s, &token).await.unwrap();

        // Attempt to delete with wrong user — should be no-op (scoped delete).
        UserTokenRepository::delete(&s, &Id::new("tok2"), &u2.id)
            .await
            .unwrap();

        let list = UserTokenRepository::list_for_user(&s, &u1.id)
            .await
            .unwrap();
        assert_eq!(
            list.len(),
            1,
            "token should still exist after wrong-user delete"
        );
    }

    // ── HSI §12 judgment ledger aggregation ─────────────────────────────────

    use gyre_common::NotificationType;
    use gyre_domain::{
        ApprovalStatus, AuditEvent, AuditEventType, AuditOutcome, JudgmentType, Notification,
        SpecApproval, SpecLedgerEntry,
    };
    use gyre_ports::{
        AuditRepository, JudgmentLedgerRepository, NotificationRepository, SpecApprovalRepository,
        SpecLedgerRepository,
    };
    use serde_json::json;

    fn ledger_entry(path: &str, ws: Option<&str>) -> SpecLedgerEntry {
        SpecLedgerEntry {
            path: path.into(),
            title: path.into(),
            owner: "owner".into(),
            kind: None,
            current_sha: "abc".into(),
            approval_mode: "human".into(),
            approval_status: ApprovalStatus::Approved,
            linked_tasks: vec![],
            linked_mrs: vec![],
            drift_status: "clean".into(),
            created_at: 900,
            updated_at: 900,
            repo_id: None,
            workspace_id: ws.map(str::to_string),
        }
    }

    fn audit_ev(
        id: &str,
        et: AuditEventType,
        user: &str,
        ws: Option<&str>,
        ts: u64,
        detail: serde_json::Value,
    ) -> AuditEvent {
        AuditEvent::new(
            Id::new(id),
            et,
            None,
            Some(Id::new(user)),
            None,
            ws.map(Id::new),
            None,
            "judgment".to_string(),
            Some(id.to_string()),
            AuditOutcome::Success,
            detail,
            None,
            None,
            ts,
        )
    }

    async fn seed_judgments(s: &SqliteStorage) {
        UserRepository::create(s, &make_user("u1")).await.unwrap();
        UserRepository::create(s, &make_user("u9")).await.unwrap();
        // Approvals: one in ws1, one with no workspace attribution.
        SpecLedgerRepository::save(s, &ledger_entry("specs/a.md", Some("ws1")))
            .await
            .unwrap();
        SpecLedgerRepository::save(s, &ledger_entry("specs/b.md", None))
            .await
            .unwrap();
        SpecApprovalRepository::create(
            s,
            &SpecApproval::new(Id::new("ap1"), "specs/a.md", "sha1", "u1", 1000),
        )
        .await
        .unwrap();
        SpecApprovalRepository::create(
            s,
            &SpecApproval::new(Id::new("ap2"), "specs/b.md", "sha2", "u1", 1100),
        )
        .await
        .unwrap();
        // Rejection (rejected_at only) with workspace attribution.
        let mut rej = SpecApproval::new(Id::new("ap3"), "specs/a.md", "sha1b", "u1", 1150);
        rej.reject("not coherent", Id::new("u1"), 1160);
        SpecApprovalRepository::create(s, &rej).await.unwrap();
        // A foreign user's approval must never leak into u1's ledger.
        SpecApprovalRepository::create(
            s,
            &SpecApproval::new(Id::new("ap9"), "specs/a.md", "sha9", "u9", 1190),
        )
        .await
        .unwrap();
        // Audit-carried judgments.
        AuditRepository::record(
            s,
            &audit_ev(
                "g1",
                AuditEventType::GateOverride,
                "u1",
                Some("ws1"),
                1500,
                json!({"mr_id": "mr-9", "gate_type": "test_command", "from_status": "failed", "to_status": "overridden"}),
            ),
        )
        .await
        .unwrap();
        AuditRepository::record(
            s,
            &audit_ev(
                "t1",
                AuditEventType::TrustChange,
                "u1",
                Some("ws1"),
                1600,
                json!({"workspace_id": "ws1", "from": "Supervised", "to": "Guided"}),
            ),
        )
        .await
        .unwrap();
        AuditRepository::record(
            s,
            &audit_ev(
                "m1",
                AuditEventType::MetaSpecPublish,
                "u1",
                None,
                1700,
                json!({"kind": "meta:persona", "name": "orchestrator", "version": 2}),
            ),
        )
        .await
        .unwrap();
        // Foreign user's judgment.
        AuditRepository::record(
            s,
            &audit_ev(
                "g9",
                AuditEventType::GateOverride,
                "u9",
                Some("ws1"),
                1800,
                json!({"mr_id": "mr-x"}),
            ),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn judgment_ledger_merges_sources_reverse_chron() {
        let (_tmp, s) = setup();
        seed_judgments(&s).await;
        let entries =
            JudgmentLedgerRepository::list_for_user(&s, "u1", None, None, None, 50, 0)
                .await
                .unwrap();
        // 3 approvals/rejections (not u9's) + 3 audit events (not u9's).
        assert_eq!(entries.len(), 6, "got {:?}", entries.len());
        let types: Vec<_> = entries.iter().map(|e| e.judgment_type.clone()).collect();
        // Descending timestamp across the merged stream.
        let ts: Vec<u64> = entries.iter().map(|e| e.timestamp).collect();
        assert!(ts.windows(2).all(|w| w[0] >= w[1]), "not sorted: {ts:?}");
        assert_eq!(entries[0].judgment_type, JudgmentType::MetaSpec);
        assert_eq!(entries[0].entity_ref, "orchestrator");
        assert_eq!(entries[0].detail.as_deref(), Some("meta:persona v2"));
        assert_eq!(types[1], JudgmentType::TrustGrant);
        assert_eq!(entries[1].entity_ref, "ws1");
        assert_eq!(
            entries[1].detail.as_deref(),
            Some("trust Supervised -> Guided")
        );
        assert_eq!(types[2], JudgmentType::GateOverride);
        assert_eq!(entries[2].entity_ref, "mr-9");
        assert_eq!(
            entries[2].detail.as_deref(),
            Some("gate test_command overridden: failed -> overridden")
        );
        // Rejection classification + detail.
        let rej = entries
            .iter()
            .find(|e| e.judgment_type == JudgmentType::SpecRejection)
            .expect("rejected approval surfaces as SpecRejection");
        assert_eq!(rej.entity_ref, "specs/a.md");
        assert_eq!(rej.detail.as_deref(), Some("not coherent"));
        assert_eq!(rej.workspace_id, Some(Id::new("ws1")));
        // No foreign-user entries.
        assert!(!entries.iter().any(|e| e.entity_ref == "specs/a.md" && e.timestamp == 1190));
    }

    #[tokio::test]
    async fn judgment_ledger_workspace_filter_drops_unattributed() {
        let (_tmp, s) = setup();
        seed_judgments(&s).await;
        let entries = JudgmentLedgerRepository::list_for_user(
            &s,
            "u1",
            Some(&Id::new("ws1")),
            None,
            None,
            50,
            0,
        )
        .await
        .unwrap();
        // specs/b.md approval (NULL workspace) and the global meta-spec publish
        // are excluded; approval+rejection+gate+trust remain.
        assert_eq!(entries.len(), 4, "got {:?}", entries.len());
        assert!(entries.iter().all(|e| e.workspace_id == Some(Id::new("ws1"))));
    }

    #[tokio::test]
    async fn judgment_ledger_type_filter_selects_source() {
        let (_tmp, s) = setup();
        seed_judgments(&s).await;
        let gates = JudgmentLedgerRepository::list_for_user(
            &s,
            "u1",
            None,
            Some(JudgmentType::GateOverride),
            None,
            50,
            0,
        )
        .await
        .unwrap();
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].entity_ref, "mr-9");
        let approvals = JudgmentLedgerRepository::list_for_user(
            &s,
            "u1",
            None,
            Some(JudgmentType::SpecApproval),
            None,
            50,
            0,
        )
        .await
        .unwrap();
        // Only non-revoked/non-rejected approvals.
        assert_eq!(approvals.len(), 2);
        assert!(approvals.iter().all(|e| e.judgment_type == JudgmentType::SpecApproval));
    }

    #[tokio::test]
    async fn judgment_ledger_pagination_across_merged_stream() {
        let (_tmp, s) = setup();
        seed_judgments(&s).await;
        let all = JudgmentLedgerRepository::list_for_user(&s, "u1", None, None, None, 50, 0)
            .await
            .unwrap();
        let page = JudgmentLedgerRepository::list_for_user(&s, "u1", None, None, None, 2, 2)
            .await
            .unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].timestamp, all[2].timestamp);
        assert_eq!(page[1].timestamp, all[3].timestamp);
    }

    #[tokio::test]
    async fn notifications_exclude_types_applies_to_list_and_count() {
        let (_tmp, s) = setup();
        let u = make_user("u1");
        UserRepository::create(&s, &u).await.unwrap();
        for (i, t) in [
            NotificationType::GateFailure,
            NotificationType::AgentCompleted,
            NotificationType::TrustSuggestion,
        ]
        .iter()
        .enumerate()
        {
            let n = Notification::new(
                Id::new(format!("n{i}")),
                Id::new("ws1"),
                u.id.clone(),
                t.clone(),
                "t",
                "t1",
                1000 + i as i64,
            );
            NotificationRepository::create(&s, &n).await.unwrap();
        }
        let all =
            NotificationRepository::list_for_user(&s, &u.id, None, None, None, &[], 50, 0)
                .await
                .unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(
            NotificationRepository::count_unresolved(&s, &u.id, None, &[])
                .await
                .unwrap(),
            3
        );
        let filtered = NotificationRepository::list_for_user(
            &s,
            &u.id,
            None,
            None,
            None,
            &["GateFailure"],
            50,
            0,
        )
        .await
        .unwrap();
        assert_eq!(filtered.len(), 2);
        assert!(!filtered
            .iter()
            .any(|n| n.notification_type == NotificationType::GateFailure));
        assert_eq!(
            NotificationRepository::count_unresolved(&s, &u.id, None, &["GateFailure"])
                .await
                .unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn record_login_debounce_and_update() {
        let (_tmp, s) = setup();
        let u = make_user("u1");
        UserRepository::create(&s, &u).await.unwrap();
        let got = UserRepository::find_by_id(&s, &u.id).await.unwrap().unwrap();
        assert_eq!(got.oidc_issuer, None);
        assert_eq!(got.last_login_at, None);

        // First login records.
        UserRepository::record_login(&s, &u.id, "https://idp.test", 1000, 60)
            .await
            .unwrap();
        let got = UserRepository::find_by_id(&s, &u.id).await.unwrap().unwrap();
        assert_eq!(got.oidc_issuer.as_deref(), Some("https://idp.test"));
        assert_eq!(got.last_login_at, Some(1000));

        // Within the debounce window: must NOT advance.
        UserRepository::record_login(&s, &u.id, "https://idp.test", 1030, 60)
            .await
            .unwrap();
        let got = UserRepository::find_by_id(&s, &u.id).await.unwrap().unwrap();
        assert_eq!(got.last_login_at, Some(1000), "in-window write leaked through");

        // Past the window: advances.
        UserRepository::record_login(&s, &u.id, "https://idp.test", 1061, 60)
            .await
            .unwrap();
        let got = UserRepository::find_by_id(&s, &u.id).await.unwrap().unwrap();
        assert_eq!(got.last_login_at, Some(1061));

        // update() must preserve auth-provider fields.
        let mut edited = got.clone();
        edited.display_name = "renamed".into();
        UserRepository::update(&s, &edited).await.unwrap();
        let got = UserRepository::find_by_id(&s, &u.id).await.unwrap().unwrap();
        assert_eq!(got.oidc_issuer.as_deref(), Some("https://idp.test"));
        assert_eq!(got.last_login_at, Some(1061));
    }
}
