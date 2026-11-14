use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{
    AuditEventType, JudgmentEntry, JudgmentType, UserNotificationPreference, UserToken,
};
use gyre_ports::{
    JudgmentLedgerRepository, UserNotificationPreferenceRepository, UserTokenRepository,
};
use std::sync::Arc;

use super::audit::AuditEventRow;
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

/// Judgment-ledger source-1 row: one `spec_approvals` row left-joined to its
/// spec-ledger workspace attribution. Named struct rather than a positional
/// tuple — a 9-column positional carrier is the TASK-048 F1 field-swap class
/// (check-unnamed-tuple-carriers).
#[derive(Queryable)]
struct SpecApprovalJudgmentRow {
    spec_path: String,
    approved_at: Option<i64>,
    revoked_at: Option<i64>,
    revoked_by: Option<String>,
    revocation_reason: Option<String>,
    rejected_at: Option<i64>,
    rejected_by: Option<String>,
    rejected_reason: Option<String>,
    workspace_id: Option<String>,
}

/// Builds the source-1 (spec approvals & rejections) judgment query.
/// Factored out of `list_for_user` so the generated PG statement shape is
/// testable without a PG server (postgres/search.rs precedent): the sqlite
/// sibling module carries the behavioral tests against a real SQLite file,
/// and these builders pin the mirrored predicate shape — attribution keys,
/// the workspace join filter (a real SQL filter, never a stamped param),
/// the since predicates — where a PG-side drift would otherwise surface
/// only in production deployments.
fn spec_approval_judgment_query<
    'a,
>(
    approver: &'a str,
    workspace: Option<&'a str>,
    since: i64,
) -> impl diesel::query_dsl::LoadQuery<'a, diesel::pg::PgConnection, SpecApprovalJudgmentRow>
   + diesel::query_builder::QueryFragment<diesel::pg::Pg> {
    use crate::schema::{spec_approvals, spec_ledger_entries};
    let mut query = spec_approvals::table
        .left_join(
            spec_ledger_entries::table
                .on(spec_ledger_entries::path.eq(spec_approvals::spec_path)),
        )
        .filter(
            spec_approvals::approver_id
                .eq(approver)
                .or(spec_approvals::rejected_by.eq(approver))
                .or(spec_approvals::revoked_by.eq(approver)),
        )
        .filter(
            spec_approvals::approved_at
                .ge(since)
                .or(spec_approvals::revoked_at.ge(since))
                .or(spec_approvals::rejected_at.ge(since)),
        )
        .select((
            spec_approvals::spec_path,
            spec_approvals::approved_at,
            spec_approvals::revoked_at,
            spec_approvals::revoked_by,
            spec_approvals::revocation_reason,
            spec_approvals::rejected_at,
            spec_approvals::rejected_by,
            spec_approvals::rejected_reason,
            spec_ledger_entries::workspace_id.nullable(),
        ))
        .into_boxed();
    if let Some(w) = workspace {
        query = query.filter(spec_ledger_entries::workspace_id.eq(w));
    }
    query
}

/// Builds the source-2 (audit-carried: gate / trust / meta-spec) judgment
/// query. Same factoring rationale as [`spec_approval_judgment_query`].
fn audit_judgment_query<'a>(
    approver: &'a str,
    workspace: Option<&'a str>,
    since: i64,
) -> impl diesel::query_dsl::LoadQuery<'a, diesel::pg::PgConnection, AuditEventRow>
   + diesel::query_builder::QueryFragment<diesel::pg::Pg> {
    use crate::schema::audit_events;
    let mut query = audit_events::table
        .filter(audit_events::user_id.eq(approver))
        .filter(audit_events::event_type.eq_any(JUDGMENT_AUDIT_TYPES))
        .filter(audit_events::timestamp.ge(since))
        .select(AuditEventRow::as_select())
        .into_boxed();
    if let Some(w) = workspace {
        query = query.filter(audit_events::workspace_id.eq(w));
    }
    query
}

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
            // The spec approval ledger derives status from which timestamp
            // column is non-null and keeps them mutually exclusive: the
            // judgment timestamp is whichever column fired. Pending entries
            // (all NULL) match no timestamp comparison and drop out here.
            //
            // Attribution (HSI §12): an approval is the approver's judgment
            // (`approver_id` = canonical user Id, written by the approve
            // endpoint); a rejection/revocation is the declining human's
            // judgment (`rejected_by`/`revoked_by` = canonical user Id). A
            // row whose approval was withdrawn by someone else therefore
            // surfaces in the withdrawer's ledger, not the original
            // approver's — the WHERE matches any of the three keys and the
            // row loop below re-checks the right one per status.
            if want_approvals {
                let rows: Vec<SpecApprovalJudgmentRow> =
                    spec_approval_judgment_query(&approver, ws.as_deref(), since_i)
                        .load(&mut *conn)
                        .context("judgment ledger: spec_approvals")?;

                for SpecApprovalJudgmentRow {
                    spec_path: path,
                    approved_at,
                    revoked_at,
                    revoked_by,
                    revocation_reason: revoke_reason,
                    rejected_at,
                    rejected_by,
                    rejected_reason: reject_reason,
                    workspace_id: wid,
                } in rows
                {
                    // revoked_at/rejected_at indicate a rejection; its
                    // judgment timestamp is the withdrawal/decline time
                    // (mutual exclusivity cleared approved_at).
                    let rejected = revoked_at.is_some() || rejected_at.is_some();
                    if rejected {
                        // The rejection belongs to the human who declined or
                        // withdrew it — not to the original approver.
                        if revoked_by.as_deref() != Some(approver.as_str())
                            && rejected_by.as_deref() != Some(approver.as_str())
                        {
                            continue;
                        }
                    }
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
                    let judgment_ts = if rejected {
                        rejected_at.or(revoked_at)
                    } else {
                        approved_at
                    }
                    .unwrap_or(0)
                    .max(0) as u64;
                    entries.push(JudgmentEntry::new(
                        jtype,
                        path,
                        wid.map(Id::new),
                        judgment_ts,
                        detail,
                    ));
                }
            }

            // ── Source 2: audit-carried judgments (gate / trust / meta-spec) ──
            // Rows are materialized through the shared AuditEventRow read
            // path (AuditRepository query/since_timestamp use the same one)
            // so the log-compression job's detail_zstd payload — written
            // when a row ages past 7 days, blanking `detail` — decompresses
            // transparently here too. Reading `detail` directly would
            // surface null details for every older judgment.
            if want_audit {
                let rows: Vec<AuditEventRow> = audit_judgment_query(&approver, ws.as_deref(), since_i)
                    .load(&mut *conn)
                    .context("judgment ledger: audit_events")?;

                for row in rows {
                    let ev = row
                        .into_event()
                        .context("judgment ledger: materialize audit event")?;
                    let jtype = match ev.event_type {
                        AuditEventType::GateOverride => JudgmentType::GateOverride,
                        AuditEventType::TrustChange => JudgmentType::TrustGrant,
                        _ => JudgmentType::MetaSpec,
                    };
                    if jt.is_some_and(|f| f != jtype) {
                        continue;
                    }
                    let detail = ev.detail;
                    let resource_id = ev.resource_id;
                    let wid = ev.workspace_id.clone();
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
                            str_field("workspace_id")
                                .or_else(|| wid.as_ref().map(|w| w.as_str().to_string()))
                                .unwrap_or_default(),
                            format!(
                                "trust {} -> {}",
                                str_field("from").unwrap_or_default(),
                                str_field("to").unwrap_or_default(),
                            ),
                        ),
                        _ => (
                            str_field("name")
                                .or(resource_id.clone())
                                .unwrap_or_default(),
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
                        wid,
                        ev.timestamp,
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
    //! SQL-shape regression tests for the judgment-ledger query builders.
    //! The repo has no PG server harness (postgres/search.rs precedent), so
    //! these capture the generated statement shape — the surface where a
    //! PG-side drift from the sqlite sibling (which carries the behavioral
    //! tests against a real SQLite file) would otherwise surface only in
    //! production deployments. The behavioral semantics (union, sort,
    //! attribution re-check, pagination) are exercised by the sqlite
    //! module's suite; these pin the PG statement the mirror emits.

    use super::*;

    fn spec_approval_sql(approver: &str, workspace: Option<&str>) -> String {
        diesel::debug_query::<diesel::pg::Pg, _>(
            &spec_approval_judgment_query(approver, workspace, 0),
        )
        .to_string()
    }

    fn audit_sql(approver: &str, workspace: Option<&str>) -> String {
        diesel::debug_query::<diesel::pg::Pg, _>(&audit_judgment_query(approver, workspace, 0))
            .to_string()
    }

    fn where_clause(sql: &str) -> &str {
        sql.split_once(" WHERE ").map(|(_, rest)| rest).unwrap_or("")
    }

    #[test]
    fn spec_approval_query_filters_workspace_in_sql_not_by_stamping() {
        // HSI §12: ?workspace_id= must FILTER (drop non-matching rows in the
        // WHERE clause), never stamp the param onto returned entries. A
        // stamp-only regression shows up here as a missing predicate.
        let filtered_sql = spec_approval_sql("u1", Some("ws-1"));
        let filtered = where_clause(&filtered_sql);
        assert!(
            filtered.contains("\"spec_ledger_entries\".\"workspace_id\" = $"),
            "workspace filter missing from WHERE: {filtered}"
        );
        let unfiltered_sql = spec_approval_sql("u1", None);
        let unfiltered = where_clause(&unfiltered_sql);
        assert!(
            !unfiltered.contains("workspace_id = $"),
            "no workspace filter requested but WHERE carries one: {unfiltered}"
        );
    }

    #[test]
    fn spec_approval_query_joins_ledger_for_workspace_attribution() {
        // Workspace attribution comes from the spec ledger entry — dropping
        // the join makes every judgment workspace-less and the ?workspace_id=
        // filter match nothing.
        let sql = spec_approval_sql("u1", None);
        assert!(
            sql.contains("LEFT OUTER JOIN \"spec_ledger_entries\""),
            "workspace attribution join missing: {sql}"
        );
        assert!(
            sql.contains("\"spec_ledger_entries\".\"path\" = \"spec_approvals\".\"spec_path\""),
            "join predicate missing: {sql}"
        );
    }

    #[test]
    fn spec_approval_query_matches_all_three_attribution_keys() {
        // HSI §12 attribution: an approval is the approver's judgment; a
        // rejection/revocation is the declining human's. The WHERE must
        // match all three keys — dropping one hides that judgment class
        // from its owner's ledger.
        let sql = spec_approval_sql("u1", None);
        let w = where_clause(&sql);
        for col in [
            "\"spec_approvals\".\"approver_id\" = $",
            "\"spec_approvals\".\"rejected_by\" = $",
            "\"spec_approvals\".\"revoked_by\" = $",
        ] {
            assert!(w.contains(col), "attribution key missing: {col} in {w}");
        }
    }

    #[test]
    fn spec_approval_query_applies_since_to_all_three_timestamps() {
        // ?since= must bound whichever timestamp column fired (mutual
        // exclusivity: exactly one is set per status).
        let sql = spec_approval_sql("u1", None);
        let w = where_clause(&sql);
        for col in [
            "\"spec_approvals\".\"approved_at\" >= $",
            "\"spec_approvals\".\"revoked_at\" >= $",
            "\"spec_approvals\".\"rejected_at\" >= $",
        ] {
            assert!(w.contains(col), "since predicate missing: {col} in {w}");
        }
    }

    #[test]
    fn audit_query_scopes_user_types_and_workspace() {
        // Human attribution via audit_events.user_id; judgment semantics via
        // the event_type allow-list; workspace filter in SQL.
        let filtered_sql = audit_sql("u1", Some("ws-1"));
        let filtered = where_clause(&filtered_sql);
        assert!(
            filtered.contains("\"audit_events\".\"user_id\" = $"),
            "user attribution missing: {filtered}"
        );
        assert!(
            filtered.contains("\"audit_events\".\"event_type\" = ANY("),
            "judgment event-type allow-list missing: {filtered}"
        );
        assert!(
            filtered.contains("\"audit_events\".\"timestamp\" >= $"),
            "since predicate missing: {filtered}"
        );
        assert!(
            filtered.contains("\"audit_events\".\"workspace_id\" = $"),
            "workspace filter missing: {filtered}"
        );

        let unfiltered_sql = audit_sql("u1", None);
        let unfiltered = where_clause(&unfiltered_sql);
        assert!(
            !unfiltered.contains("workspace_id = $"),
            "no workspace filter requested but WHERE carries one: {unfiltered}"
        );
    }

    #[test]
    fn judgment_audit_type_list_matches_sqlite_sibling() {
        // Repo policy: the sqlite and postgres adapters stay in parity. The
        // sqlite module's behavioral tests exercise its own list; this pins
        // that the PG mirror aggregates the SAME HSI §12 judgment sources —
        // a list edited on one side only silently drops a judgment class
        // from PG deployments.
        assert_eq!(
            JUDGMENT_AUDIT_TYPES,
            crate::sqlite::user_profile::JUDGMENT_AUDIT_TYPES,
            "postgres/sqlite judgment audit-type lists drifted"
        );
    }
}
