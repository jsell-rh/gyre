use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{
    JudgmentEntry, JudgmentType, UserNotificationPreference, UserSession, UserToken,
};
use gyre_ports::{
    JudgmentLedgerRepository, SessionRepository, UserNotificationPreferenceRepository,
    UserTokenRepository,
};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::{user_notification_preferences, user_sessions, user_tokens};

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
        use crate::schema::spec_approvals;
        let pool = Arc::clone(&self.pool);
        let approver = approver_id.to_string();
        let ws_id = workspace_id.map(|id| id.as_str().to_string());
        let jtype = judgment_type;
        let since_ts = since;
        let lim = limit as i64;
        let off = offset as i64;

        tokio::task::spawn_blocking(move || -> Result<Vec<JudgmentEntry>> {
            let mut conn = pool.get().context("get db connection")?;
            let mut entries: Vec<JudgmentEntry> = Vec::new();

            // Only include spec_approvals entries if type filter allows it.
            let include_approvals = jtype
                .as_ref()
                .map(|t| matches!(t, JudgmentType::SpecApproval | JudgmentType::SpecRejection))
                .unwrap_or(true);

            if include_approvals {
                let mut q = spec_approvals::table
                    .filter(spec_approvals::approver_id.eq(&approver))
                    .into_boxed();

                if let Some(since_ts) = since_ts {
                    q = q.filter(spec_approvals::approved_at.ge(since_ts as i64));
                }

                let rows = q
                    .order(spec_approvals::approved_at.desc())
                    .select((
                        spec_approvals::id,
                        spec_approvals::spec_path,
                        spec_approvals::approved_at,
                        spec_approvals::revoked_at,
                        spec_approvals::revocation_reason,
                    ))
                    .load::<(String, String, i64, Option<i64>, Option<String>)>(&mut *conn)
                    .context("load spec_approvals for judgment ledger")?;

                for (_, spec_path, approved_at, revoked_at, revocation_reason) in rows {
                    // If revoked, it's a rejection entry; otherwise an approval.
                    let jt = if revoked_at.is_some() {
                        // Only include if filter allows rejections.
                        if jtype
                            .as_ref()
                            .map(|t| !matches!(t, JudgmentType::SpecApproval))
                            .unwrap_or(true)
                        {
                            JudgmentType::SpecRejection
                        } else {
                            continue;
                        }
                    } else if jtype
                        .as_ref()
                        .map(|t| !matches!(t, JudgmentType::SpecRejection))
                        .unwrap_or(true)
                    {
                        JudgmentType::SpecApproval
                    } else {
                        continue;
                    };
                    entries.push(JudgmentEntry::new(
                        jt,
                        spec_path,
                        ws_id.as_deref().map(Id::new),
                        approved_at as u64,
                        revocation_reason,
                    ));
                }
            }

            // Sort all entries by timestamp descending, apply limit+offset.
            entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            let entries = entries
                .into_iter()
                .skip(off as usize)
                .take(lim as usize)
                .collect();
            Ok(entries)
        })
        .await?
    }
}

// ─── User Sessions ───────────────────────────────────────────────────────────

#[derive(Queryable, Selectable)]
#[diesel(table_name = user_sessions)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct UserSessionRow {
    id: String,
    user_id: String,
    token_hash: String,
    ip_address: String,
    user_agent: String,
    created_at: i64,
    last_active_at: i64,
    expires_at: i64,
    revoked: i32,
}

#[derive(Insertable)]
#[diesel(table_name = user_sessions)]
struct UserSessionRecord<'a> {
    id: &'a str,
    user_id: &'a str,
    token_hash: &'a str,
    ip_address: &'a str,
    user_agent: &'a str,
    created_at: i64,
    last_active_at: i64,
    expires_at: i64,
    revoked: i32,
}

impl From<UserSessionRow> for UserSession {
    fn from(r: UserSessionRow) -> Self {
        UserSession {
            id: Id::new(r.id),
            user_id: Id::new(r.user_id),
            token_hash: r.token_hash,
            ip_address: r.ip_address,
            user_agent: r.user_agent,
            created_at: r.created_at as u64,
            last_active_at: r.last_active_at as u64,
            expires_at: r.expires_at as u64,
            revoked: r.revoked != 0,
        }
    }
}

#[async_trait]
impl SessionRepository for SqliteStorage {
    async fn create(&self, session: &UserSession) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let s = session.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = UserSessionRecord {
                id: s.id.as_str(),
                user_id: s.user_id.as_str(),
                token_hash: &s.token_hash,
                ip_address: &s.ip_address,
                user_agent: &s.user_agent,
                created_at: s.created_at as i64,
                last_active_at: s.last_active_at as i64,
                expires_at: s.expires_at as i64,
                revoked: if s.revoked { 1 } else { 0 },
            };
            diesel::insert_into(user_sessions::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert user_session")?;
            Ok(())
        })
        .await?
    }

    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<UserSession>> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<UserSession>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = user_sessions::table
                .filter(user_sessions::user_id.eq(&uid))
                .order(user_sessions::created_at.desc())
                .load::<UserSessionRow>(&mut *conn)
                .context("list user_sessions")?;
            Ok(rows.into_iter().map(Into::into).collect())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<UserSession>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<UserSession>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = user_sessions::table
                .find(id.as_str())
                .first::<UserSessionRow>(&mut *conn)
                .optional()
                .context("find user_session by id")?;
            Ok(row.map(Into::into))
        })
        .await?
    }

    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<UserSession>> {
        let pool = Arc::clone(&self.pool);
        let hash = token_hash.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<UserSession>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = user_sessions::table
                .filter(user_sessions::token_hash.eq(&hash))
                .first::<UserSessionRow>(&mut *conn)
                .optional()
                .context("find user_session by token hash")?;
            Ok(row.map(Into::into))
        })
        .await?
    }

    async fn find_by_credential_and_device(
        &self,
        user_id: &Id,
        credential_hash: &str,
        ip_address: &str,
        user_agent: &str,
    ) -> Result<Option<UserSession>> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.as_str().to_string();
        let cred = credential_hash.to_string();
        let ip = ip_address.to_string();
        let ua = user_agent.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<UserSession>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = user_sessions::table
                .filter(user_sessions::user_id.eq(&uid))
                .filter(user_sessions::token_hash.eq(&cred))
                .filter(user_sessions::ip_address.eq(&ip))
                .filter(user_sessions::user_agent.eq(&ua))
                .first::<UserSessionRow>(&mut *conn)
                .optional()
                .context("find user_session by credential+device")?;
            Ok(row.map(Into::into))
        })
        .await?
    }

    async fn touch(&self, id: &Id, last_active_at: u64) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(user_sessions::table.find(id.as_str()))
                .set(user_sessions::last_active_at.eq(last_active_at as i64))
                .execute(&mut *conn)
                .context("touch user_session")?;
            Ok(())
        })
        .await?
    }

    async fn revoke(&self, id: &Id, user_id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            // Scoped revoke: only mark revoked if the session belongs to the
            // requesting user. Idempotent — already-revoked rows stay revoked.
            diesel::update(
                user_sessions::table
                    .filter(user_sessions::id.eq(id.as_str()))
                    .filter(user_sessions::user_id.eq(uid.as_str())),
            )
            .set(user_sessions::revoked.eq(1))
            .execute(&mut *conn)
            .context("revoke user_session")?;
            Ok(())
        })
        .await?
    }

    async fn revoke_all_for_user(&self, user_id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(
                user_sessions::table
                    .filter(user_sessions::user_id.eq(&uid))
                    .filter(user_sessions::revoked.eq(0)),
            )
            .set(user_sessions::revoked.eq(1))
            .execute(&mut *conn)
            .context("revoke all user_sessions for user")?;
            Ok(())
        })
        .await?
    }

    async fn delete_expired_before(&self, cutoff: u64) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let n = diesel::delete(user_sessions::table)
                .filter(user_sessions::expires_at.lt(cutoff as i64))
                .execute(&mut *conn)
                .context("delete expired user_sessions")?;
            Ok(n as u64)
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

    // ── SessionRepository (user-management.md §Session Management) ─────────

    fn make_session(id: &str, user_id: &Id, token_hash: &str, expires_at: u64) -> UserSession {
        UserSession::new(
            Id::new(id),
            user_id.clone(),
            token_hash,
            "127.0.0.1",
            "gyre-test/1.0",
            1_000,
            expires_at,
        )
    }

    #[tokio::test]
    async fn session_create_find_roundtrip() {
        let (_tmp, s) = setup();
        let u = make_user("s-u1");
        UserRepository::create(&s, &u).await.unwrap();

        let sess = make_session("sess-1", &u.id, "hash-1", 2_000);
        SessionRepository::create(&s, &sess).await.unwrap();

        let found = SessionRepository::find_by_id(&s, &Id::new("sess-1"))
            .await
            .unwrap()
            .expect("session must be findable by id");
        assert_eq!(found.token_hash, "hash-1");
        assert_eq!(found.user_agent, "gyre-test/1.0");
        assert_eq!(found.ip_address, "127.0.0.1");
        assert!(!found.revoked);

        let by_hash = SessionRepository::find_by_token_hash(&s, "hash-1")
            .await
            .unwrap()
            .expect("session must be findable by token hash");
        assert_eq!(by_hash.id, Id::new("sess-1"));
    }

    #[tokio::test]
    async fn session_create_duplicate_id_fails() {
        let (_tmp, s) = setup();
        let u = make_user("s-u2");
        UserRepository::create(&s, &u).await.unwrap();

        let sess = make_session("sess-dup", &u.id, "hash-dup", 2_000);
        SessionRepository::create(&s, &sess).await.unwrap();
        // Port contract: create fails if a session with the same id exists.
        assert!(
            SessionRepository::create(&s, &sess).await.is_err(),
            "duplicate session id must fail (PRIMARY KEY guard)"
        );
    }

    #[tokio::test]
    async fn session_find_by_credential_and_device_matches_all_four() {
        let (_tmp, s) = setup();
        let u = make_user("s-u3");
        UserRepository::create(&s, &u).await.unwrap();

        let sess = make_session("sess-dev", &u.id, "cred-hash", 2_000);
        SessionRepository::create(&s, &sess).await.unwrap();

        // Exact (user, credential, ip, user-agent) → found.
        let found = SessionRepository::find_by_credential_and_device(
            &s, &u.id, "cred-hash", "127.0.0.1", "gyre-test/1.0",
        )
        .await
        .unwrap()
        .expect("exact device tuple must match");
        assert_eq!(found.id, Id::new("sess-dev"));

        // Any differing component (different device) → not found; it is a
        // new session, not the same one.
        assert!(
            SessionRepository::find_by_credential_and_device(
                &s, &u.id, "cred-hash", "10.0.0.1", "gyre-test/1.0",
            )
            .await
            .unwrap()
            .is_none(),
            "different ip must not match the session"
        );
        assert!(
            SessionRepository::find_by_credential_and_device(
                &s, &u.id, "cred-hash", "127.0.0.1", "other-agent/9",
            )
            .await
            .unwrap()
            .is_none(),
            "different user-agent must not match the session"
        );
    }

    #[tokio::test]
    async fn session_touch_updates_last_active() {
        let (_tmp, s) = setup();
        let u = make_user("s-u4");
        UserRepository::create(&s, &u).await.unwrap();

        let sess = make_session("sess-touch", &u.id, "hash-touch", 2_000);
        SessionRepository::create(&s, &sess).await.unwrap();

        SessionRepository::touch(&s, &Id::new("sess-touch"), 1_500)
            .await
            .unwrap();
        let after = SessionRepository::find_by_id(&s, &Id::new("sess-touch"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            after.last_active_at, 1_500,
            "touch must persist last_active_at"
        );
    }

    #[tokio::test]
    async fn session_revoke_is_scoped_to_owner() {
        let (_tmp, s) = setup();
        let u1 = make_user("s-u5");
        let u2 = make_user("s-u6");
        UserRepository::create(&s, &u1).await.unwrap();
        UserRepository::create(&s, &u2).await.unwrap();

        let sess = make_session("sess-rev", &u1.id, "hash-rev", 2_000);
        SessionRepository::create(&s, &sess).await.unwrap();

        // Wrong owner: revoke is a no-op (scoped), not an error.
        SessionRepository::revoke(&s, &Id::new("sess-rev"), &u2.id)
            .await
            .unwrap();
        let still = SessionRepository::find_by_id(&s, &Id::new("sess-rev"))
            .await
            .unwrap()
            .unwrap();
        assert!(!still.revoked, "wrong-owner revoke must not revoke");

        // Right owner: revoked.
        SessionRepository::revoke(&s, &Id::new("sess-rev"), &u1.id)
            .await
            .unwrap();
        let revoked = SessionRepository::find_by_id(&s, &Id::new("sess-rev"))
            .await
            .unwrap()
            .unwrap();
        assert!(revoked.revoked, "owner revoke must revoke");

        // Idempotent: re-revoke succeeds.
        SessionRepository::revoke(&s, &Id::new("sess-rev"), &u1.id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn session_revoke_all_and_delete_expired() {
        let (_tmp, s) = setup();
        let u = make_user("s-u7");
        UserRepository::create(&s, &u).await.unwrap();
        let other = make_user("s-u8");
        UserRepository::create(&s, &other).await.unwrap();

        let s1 = make_session("sess-a1", &u.id, "h-a1", 2_000);
        let s2 = make_session("sess-a2", &u.id, "h-a2", 9_000);
        let keep = make_session("sess-b1", &other.id, "h-b1", 2_000);
        SessionRepository::create(&s, &s1).await.unwrap();
        SessionRepository::create(&s, &s2).await.unwrap();
        SessionRepository::create(&s, &keep).await.unwrap();

        // Revoke-all touches only the target user's sessions.
        SessionRepository::revoke_all_for_user(&s, &u.id)
            .await
            .unwrap();
        let mine = SessionRepository::list_for_user(&s, &u.id).await.unwrap();
        assert!(
            mine.iter().all(|x| x.revoked),
            "revoke-all must revoke every session of the user"
        );
        let others = SessionRepository::list_for_user(&s, &other.id)
            .await
            .unwrap();
        assert!(
            others.iter().all(|x| !x.revoked),
            "revoke-all must not touch other users' sessions"
        );

        // Retention cleanup deletes only sessions expired before cutoff.
        let deleted = SessionRepository::delete_expired_before(&s, 5_000)
            .await
            .unwrap();
        assert_eq!(deleted, 2, "expired rows (sess-a1, sess-b1) must be deleted");
        let remaining: Vec<_> = SessionRepository::list_for_user(&s, &u.id)
            .await
            .unwrap()
            .into_iter()
            .chain(SessionRepository::list_for_user(&s, &other.id).await.unwrap())
            .collect();
        assert!(
            remaining.iter().all(|x| x.expires_at >= 5_000),
            "no row expired before the cutoff may survive"
        );
        assert_eq!(remaining.len(), 1, "only the unexpired session remains");
    }
}
