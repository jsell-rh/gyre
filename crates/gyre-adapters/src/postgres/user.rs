use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{GlobalRole, User, UserPreferences, UserRole};
use gyre_ports::{ApiKeyRepository, UserRepository};
use std::sync::Arc;

use super::PgStorage;
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
#[diesel(check_for_backend(diesel::pg::Pg))]
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
        u.display_name = r.display_name.unwrap_or_else(|| r.name.clone());
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
impl UserRepository for PgStorage {
    async fn create(&self, user: &User) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let u = user.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            // Port contract: fail if id, external_id, or username already exists.
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
            let existing = existing
                .ok_or_else(|| anyhow::anyhow!("cannot update user {}: not found", u.id))?;
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
impl ApiKeyRepository for PgStorage {
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
