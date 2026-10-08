use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{GlobalRole, User, UserPreferences, UserRole};
use gyre_ports::{ApiKeyRepository, UserRepository};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::{api_keys, users};

fn roles_to_json(roles: &[UserRole]) -> String {
    let strs: Vec<&str> = roles.iter().map(|r| r.as_str()).collect();
    serde_json::to_string(&strs).unwrap_or_else(|_| "[]".to_string())
}

fn json_to_roles(s: &str) -> Vec<UserRole> {
    let strs: Vec<String> = serde_json::from_str(s).unwrap_or_default();
    strs.iter().filter_map(|s| UserRole::from_str(s)).collect()
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = users)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct UserRow {
    id: String,
    external_id: String,
    name: String,
    email: Option<String>,
    roles: String,
    created_at: i64,
    updated_at: i64,
    display_name: Option<String>,
    timezone: Option<String>,
    locale: Option<String>,
    username: String,
    avatar_url: Option<String>,
    preferences: Option<String>,
    last_login_at: Option<i64>,
    tenant_id: Option<String>,
    global_role: String,
}

impl From<UserRow> for User {
    fn from(r: UserRow) -> Self {
        let preferences = r
            .preferences
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let global_role = match r.global_role.as_str() {
            "TenantAdmin" => GlobalRole::TenantAdmin,
            _ => GlobalRole::Member,
        };
        let mut u = User::new_sso(
            Id::new(r.id.clone()),
            r.external_id.clone(),
            r.username.clone(),
            r.name.clone(),
            r.created_at as u64,
        );
        u.email = r.email;
        u.roles = json_to_roles(&r.roles);
        u.updated_at = r.updated_at as u64;
        u.display_name = r
            .display_name
            .unwrap_or_else(|| r.name.clone());
        u.timezone = r.timezone.unwrap_or_else(|| "UTC".to_string());
        u.locale = r.locale.unwrap_or_else(|| "en".to_string());
        u.avatar_url = r.avatar_url;
        u.preferences = preferences;
        u.last_login_at = r.last_login_at.map(|v| v as u64);
        u.tenant_id = r.tenant_id.map(Id::new);
        u.global_role = global_role;
        u
    }
}

fn prefs_to_json(prefs: &UserPreferences) -> String {
    serde_json::to_string(prefs).unwrap_or_else(|_| "{}".to_string())
}

fn global_role_to_str(role: &GlobalRole) -> &'static str {
    match role {
        GlobalRole::TenantAdmin => "TenantAdmin",
        GlobalRole::Member => "Member",
    }
}

#[derive(Insertable)]
#[diesel(table_name = users)]
struct UserRecord<'a> {
    id: &'a str,
    external_id: &'a str,
    name: &'a str,
    email: Option<&'a str>,
    roles: String,
    created_at: i64,
    updated_at: i64,
    display_name: Option<&'a str>,
    timezone: Option<&'a str>,
    locale: Option<&'a str>,
    username: &'a str,
    avatar_url: Option<&'a str>,
    preferences: String,
    last_login_at: Option<i64>,
    tenant_id: Option<&'a str>,
    global_role: String,
}

#[derive(Insertable)]
#[diesel(table_name = api_keys)]
struct ApiKeyRecord<'a> {
    key: &'a str,
    user_id: &'a str,
    name: &'a str,
    created_at: i64,
}

#[async_trait]
impl UserRepository for SqliteStorage {
    async fn create(&self, user: &User) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let u = user.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            // Port contract: fail if id, external_id, or username already
            // exists. username is enforced by idx_users_username; the
            // explicit checks give a precise error instead of a raw
            // constraint violation.
            let dup: Option<String> = users::table
                .filter(
                    users::id
                        .eq(u.id.as_str())
                        .or(users::external_id.eq(u.external_id.as_str()))
                        .or(users::username.eq(u.username.as_str())),
                )
                .select(users::id)
                .first::<String>(&mut *conn)
                .optional()
                .context("check duplicate user")?;
            if let Some(existing_id) = dup {
                anyhow::bail!(
                    "user already exists (conflicting id {existing_id}) for username {}, external_id {}",
                    u.username,
                    u.external_id
                );
            }
            let roles = roles_to_json(&u.roles);
            let record = UserRecord {
                id: u.id.as_str(),
                external_id: &u.external_id,
                name: &u.display_name,
                email: u.email.as_deref(),
                roles,
                created_at: u.created_at as i64,
                updated_at: u.updated_at as i64,
                display_name: Some(u.display_name.as_str()),
                timezone: Some(u.timezone.as_str()),
                locale: Some(u.locale.as_str()),
                username: &u.username,
                avatar_url: u.avatar_url.as_deref(),
                preferences: prefs_to_json(&u.preferences),
                last_login_at: u.last_login_at.map(|v| v as i64),
                tenant_id: u.tenant_id.as_ref().map(|t| t.as_str()),
                global_role: global_role_to_str(&u.global_role).to_string(),
            };
            diesel::insert_into(users::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert user")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<User>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<User>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = users::table
                .find(id.as_str())
                .first::<UserRow>(&mut *conn)
                .optional()
                .context("find user by id")?;
            Ok(result.map(User::from))
        })
        .await?
    }

    async fn find_by_external_id(&self, external_id: &str) -> Result<Option<User>> {
        let pool = Arc::clone(&self.pool);
        let ext_id = external_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<User>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = users::table
                .filter(users::external_id.eq(ext_id.as_str()))
                .first::<UserRow>(&mut *conn)
                .optional()
                .context("find user by external_id")?;
            Ok(result.map(User::from))
        })
        .await?
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>> {
        let pool = Arc::clone(&self.pool);
        let uname = username.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<User>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = users::table
                .filter(users::username.eq(uname.as_str()))
                .first::<UserRow>(&mut *conn)
                .optional()
                .context("find user by username")?;
            Ok(result.map(User::from))
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<User>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<User>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = users::table
                .order(users::created_at.asc())
                .load::<UserRow>(&mut *conn)
                .context("list users")?;
            Ok(rows.into_iter().map(User::from).collect())
        })
        .await?
    }

    async fn update(&self, user: &User) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let u = user.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            // Port contract: username and external_id are immutable after
            // creation (user-management.md §Username vs Display Name).
            let existing: Option<UserRow> = users::table
                .find(u.id.as_str())
                .first::<UserRow>(&mut *conn)
                .optional()
                .context("load user for update")?;
            let existing = existing.ok_or_else(|| {
                anyhow::anyhow!("cannot update user {}: not found", u.id)
            })?;
            if existing.username != u.username {
                anyhow::bail!(
                    "username is immutable: cannot change {} to {}",
                    existing.username,
                    u.username
                );
            }
            if existing.external_id != u.external_id {
                anyhow::bail!(
                    "external_id is immutable: cannot change {} to {}",
                    existing.external_id,
                    u.external_id
                );
            }
            let roles = roles_to_json(&u.roles);
            diesel::update(users::table.find(u.id.as_str()))
                .set((
                    users::name.eq(&u.display_name),
                    users::email.eq(u.email.as_deref()),
                    users::roles.eq(&roles),
                    users::updated_at.eq(u.updated_at as i64),
                    users::display_name.eq(Some(u.display_name.as_str())),
                    users::timezone.eq(Some(u.timezone.as_str())),
                    users::locale.eq(Some(u.locale.as_str())),
                    users::avatar_url.eq(u.avatar_url.as_deref()),
                    users::preferences.eq(prefs_to_json(&u.preferences)),
                    users::last_login_at.eq(u.last_login_at.map(|v| v as i64)),
                    users::tenant_id.eq(u.tenant_id.as_ref().map(|t| t.as_str())),
                    users::global_role.eq(global_role_to_str(&u.global_role)),
                ))
                .execute(&mut *conn)
                .context("update user")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(users::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete user")?;
            Ok(())
        })
        .await?
    }
}

#[async_trait]
impl ApiKeyRepository for SqliteStorage {
    async fn create(&self, key: &str, user_id: &Id, name: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let key = key.to_string();
        let user_id = user_id.clone();
        let name = name.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let record = ApiKeyRecord {
                key: key.as_str(),
                user_id: user_id.as_str(),
                name: name.as_str(),
                created_at: now,
            };
            diesel::insert_into(api_keys::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert api_key")?;
            Ok(())
        })
        .await?
    }

    async fn find_user_id(&self, key: &str) -> Result<Option<Id>> {
        let pool = Arc::clone(&self.pool);
        let key = key.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Id>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = api_keys::table
                .find(key.as_str())
                .select(api_keys::user_id)
                .first::<String>(&mut *conn)
                .optional()
                .context("find api_key user_id")?;
            Ok(result.map(Id::new))
        })
        .await?
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let key = key.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(api_keys::table.find(key.as_str()))
                .execute(&mut *conn)
                .context("delete api_key")?;
            Ok(())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use gyre_domain::UserRole;
    use gyre_ports::{ApiKeyRepository, UserRepository};
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    fn make_user(id: &str, ext_id: &str, name: &str) -> User {
        User::new(Id::new(id), ext_id, name, 1000)
    }

    #[tokio::test]
    async fn create_and_find_by_id() {
        let (_tmp, s) = setup();
        let u = make_user("u1", "ext-1", "alice");
        UserRepository::create(&s, &u).await.unwrap();
        let found = UserRepository::find_by_id(&s, &u.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.display_name, "alice");
        assert_eq!(found.external_id, "ext-1");
    }

    #[tokio::test]
    async fn find_by_external_id() {
        let (_tmp, s) = setup();
        let u = make_user("u1", "keycloak-sub-123", "bob");
        UserRepository::create(&s, &u).await.unwrap();
        let found = s
            .find_by_external_id("keycloak-sub-123")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, u.id);
    }

    #[tokio::test]
    async fn find_by_external_id_missing() {
        let (_tmp, s) = setup();
        assert!(s
            .find_by_external_id("nonexistent")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn update_roles() {
        let (_tmp, s) = setup();
        let mut u = make_user("u1", "ext-1", "carol");
        UserRepository::create(&s, &u).await.unwrap();
        u.roles = vec![UserRole::Admin, UserRole::Developer];
        u.updated_at = 2000;
        UserRepository::update(&s, &u).await.unwrap();
        let found = UserRepository::find_by_id(&s, &u.id)
            .await
            .unwrap()
            .unwrap();
        assert!(found.roles.contains(&UserRole::Admin));
        assert!(found.roles.contains(&UserRole::Developer));
    }

    #[tokio::test]
    async fn user_entity_fields_round_trip() {
        // user-management.md §User Entity: every new column must survive a
        // create → find cycle. Before task-120 the adapter dropped username,
        // avatar_url, preferences, last_login_at, tenant_id, and global_role
        // on every round-trip (UserRow/UserRecord lacked the columns).
        let (_tmp, s) = setup();
        let mut u = User::new_sso(Id::new("u1"), "ext-1", "jsell", "Jordan Sell", 1000);
        u.avatar_url = Some("https://example.com/a.png".to_string());
        u.tenant_id = Some(Id::new("tenant-a"));
        u.global_role = GlobalRole::TenantAdmin;
        u.last_login_at = Some(1999);
        u.preferences.ui_density = gyre_domain::UiDensity::Compact;
        u.preferences.code_font_size = 16;
        u.preferences.diff_view = gyre_domain::DiffView::Unified;
        u.preferences.theme = gyre_domain::Theme::Dark;
        u.preferences.activity_feed_scope = gyre_domain::FeedScope::All;
        UserRepository::create(&s, &u).await.unwrap();

        let found = UserRepository::find_by_id(&s, &u.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.username, "jsell", "username must persist");
        assert_eq!(found.display_name, "Jordan Sell");
        assert_eq!(found.avatar_url.as_deref(), Some("https://example.com/a.png"));
        assert_eq!(found.tenant_id, Some(Id::new("tenant-a")));
        assert_eq!(found.global_role, GlobalRole::TenantAdmin);
        assert_eq!(found.last_login_at, Some(1999));
        assert_eq!(found.preferences, u.preferences, "preferences must persist as JSON");

        // find_by_username resolves the unique handle.
        let by_name = s.find_by_username("jsell").await.unwrap().unwrap();
        assert_eq!(by_name.id, u.id);
        assert!(s.find_by_username("nobody").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn username_unique_across_users() {
        // Spec: username is unique. A second user with the same handle must
        // be rejected by create (port contract + idx_users_username).
        let (_tmp, s) = setup();
        let a = User::new_sso(Id::new("u1"), "ext-1", "jsell", "Jordan Sell", 1000);
        let b = User::new_sso(Id::new("u2"), "ext-2", "jsell", "Other Person", 1000);
        UserRepository::create(&s, &a).await.unwrap();
        let err = UserRepository::create(&s, &b).await;
        assert!(err.is_err(), "duplicate username must be rejected");
    }

    #[tokio::test]
    async fn username_immutable_on_update() {
        // Spec: username is immutable after creation. update() must reject
        // a handle change even when everything else is valid.
        let (_tmp, s) = setup();
        let mut u = User::new_sso(Id::new("u1"), "ext-1", "jsell", "Jordan Sell", 1000);
        UserRepository::create(&s, &u).await.unwrap();
        u.username = "renamed".to_string();
        let err = UserRepository::update(&s, &u).await;
        assert!(err.is_err(), "username change must be rejected");

        // external_id is equally immutable.
        let mut u2 = User::new_sso(Id::new("u2"), "ext-2", "alice", "Alice", 1000);
        UserRepository::create(&s, &u2).await.unwrap();
        u2.username = "alice".to_string();
        u2.external_id = "ext-hacked".to_string();
        assert!(UserRepository::update(&s, &u2).await.is_err());
    }

    #[tokio::test]
    async fn update_persists_login_stamp_and_preferences() {
        // record_login + preferences updates must persist through update().
        let (_tmp, s) = setup();
        let mut u = User::new_sso(Id::new("u1"), "ext-1", "jsell", "Jordan Sell", 1000);
        UserRepository::create(&s, &u).await.unwrap();
        u.record_login(2500);
        u.preferences.theme = gyre_domain::Theme::Dark;
        u.display_name = "J. Sell".to_string();
        UserRepository::update(&s, &u).await.unwrap();
        let found = UserRepository::find_by_id(&s, &u.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.last_login_at, Some(2500));
        assert_eq!(found.updated_at, 2500);
        assert_eq!(found.preferences.theme, gyre_domain::Theme::Dark);
        assert_eq!(found.display_name, "J. Sell");
        assert_eq!(found.username, "jsell", "username untouched by update");
    }

    #[tokio::test]
    async fn api_key_create_and_find() {
        let (_tmp, s) = setup();
        let u = make_user("u1", "ext-1", "dave");
        UserRepository::create(&s, &u).await.unwrap();
        ApiKeyRepository::create(&s, "gyre_test_key_123", &u.id, "ci-key")
            .await
            .unwrap();
        let found_id = s.find_user_id("gyre_test_key_123").await.unwrap().unwrap();
        assert_eq!(found_id, u.id);
    }

    #[tokio::test]
    async fn api_key_not_found() {
        let (_tmp, s) = setup();
        assert!(s.find_user_id("no-such-key").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn api_key_delete() {
        let (_tmp, s) = setup();
        let u = make_user("u1", "ext-1", "eve");
        UserRepository::create(&s, &u).await.unwrap();
        ApiKeyRepository::create(&s, "gyre_key_abc", &u.id, "temp")
            .await
            .unwrap();
        ApiKeyRepository::delete(&s, "gyre_key_abc").await.unwrap();
        assert!(s.find_user_id("gyre_key_abc").await.unwrap().is_none());
    }
}
