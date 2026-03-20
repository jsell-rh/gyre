use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{User, UserRole};
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
}

impl From<UserRow> for User {
    fn from(r: UserRow) -> Self {
        User {
            id: Id::new(r.id),
            external_id: r.external_id,
            name: r.name,
            email: r.email,
            roles: json_to_roles(&r.roles),
            created_at: r.created_at as u64,
            updated_at: r.updated_at as u64,
        }
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
            let roles = roles_to_json(&u.roles);
            let record = UserRecord {
                id: u.id.as_str(),
                external_id: &u.external_id,
                name: &u.name,
                email: u.email.as_deref(),
                roles,
                created_at: u.created_at as i64,
                updated_at: u.updated_at as i64,
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
            let roles = roles_to_json(&u.roles);
            diesel::update(users::table.find(u.id.as_str()))
                .set((
                    users::external_id.eq(&u.external_id),
                    users::name.eq(&u.name),
                    users::email.eq(u.email.as_deref()),
                    users::roles.eq(&roles),
                    users::updated_at.eq(u.updated_at as i64),
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
        assert_eq!(found.name, "alice");
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
