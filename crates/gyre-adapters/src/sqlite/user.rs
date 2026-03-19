use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{User, UserRole};
use gyre_ports::{ApiKeyRepository, UserRepository};

use super::{open_conn, SqliteStorage};

fn roles_to_json(roles: &[UserRole]) -> String {
    let strs: Vec<&str> = roles.iter().map(|r| r.as_str()).collect();
    serde_json::to_string(&strs).unwrap_or_else(|_| "[]".to_string())
}

fn json_to_roles(s: &str) -> Vec<UserRole> {
    let strs: Vec<String> = serde_json::from_str(s).unwrap_or_default();
    strs.iter().filter_map(|s| UserRole::from_str(s)).collect()
}

fn row_to_user(row: &rusqlite::Row<'_>) -> Result<User> {
    let roles_json: String = row.get(4)?;
    Ok(User {
        id: Id::new(row.get::<_, String>(0)?),
        external_id: row.get(1)?,
        name: row.get(2)?,
        email: row.get(3)?,
        roles: json_to_roles(&roles_json),
        created_at: row.get::<_, i64>(5)? as u64,
        updated_at: row.get::<_, i64>(6)? as u64,
    })
}

#[async_trait]
impl UserRepository for SqliteStorage {
    async fn create(&self, user: &User) -> Result<()> {
        let path = self.db_path();
        let u = user.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "INSERT INTO users (id, external_id, name, email, roles, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    u.id.as_str(),
                    u.external_id,
                    u.name,
                    u.email,
                    roles_to_json(&u.roles),
                    u.created_at as i64,
                    u.updated_at as i64,
                ],
            )
            .context("insert user")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<User>> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<User>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, external_id, name, email, roles, created_at, updated_at
                 FROM users WHERE id = ?1",
            )?;
            let mut rows = stmt.query([id.as_str()])?;
            if let Some(row) = rows.next()? {
                Ok(Some(row_to_user(row)?))
            } else {
                Ok(None)
            }
        })
        .await?
    }

    async fn find_by_external_id(&self, external_id: &str) -> Result<Option<User>> {
        let path = self.db_path();
        let ext_id = external_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<User>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, external_id, name, email, roles, created_at, updated_at
                 FROM users WHERE external_id = ?1",
            )?;
            let mut rows = stmt.query([&ext_id])?;
            if let Some(row) = rows.next()? {
                Ok(Some(row_to_user(row)?))
            } else {
                Ok(None)
            }
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<User>> {
        let path = self.db_path();
        tokio::task::spawn_blocking(move || -> Result<Vec<User>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, external_id, name, email, roles, created_at, updated_at
                 FROM users ORDER BY created_at",
            )?;
            let rows = stmt.query_map([], |row| Ok(row_to_user(row).unwrap()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn update(&self, user: &User) -> Result<()> {
        let path = self.db_path();
        let u = user.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "UPDATE users SET external_id=?1, name=?2, email=?3, roles=?4, updated_at=?5
                 WHERE id=?6",
                rusqlite::params![
                    u.external_id,
                    u.name,
                    u.email,
                    roles_to_json(&u.roles),
                    u.updated_at as i64,
                    u.id.as_str(),
                ],
            )
            .context("update user")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute("DELETE FROM users WHERE id=?1", [id.as_str()])
                .context("delete user")?;
            Ok(())
        })
        .await?
    }
}

#[async_trait]
impl ApiKeyRepository for SqliteStorage {
    async fn create(&self, key: &str, user_id: &Id, name: &str) -> Result<()> {
        let path = self.db_path();
        let key = key.to_string();
        let user_id = user_id.clone();
        let name = name.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            conn.execute(
                "INSERT INTO api_keys (key, user_id, name, created_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![key, user_id.as_str(), name, now],
            )
            .context("insert api_key")?;
            Ok(())
        })
        .await?
    }

    async fn find_user_id(&self, key: &str) -> Result<Option<Id>> {
        let path = self.db_path();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Id>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare("SELECT user_id FROM api_keys WHERE key = ?1")?;
            let mut rows = stmt.query([&key])?;
            if let Some(row) = rows.next()? {
                Ok(Some(Id::new(row.get::<_, String>(0)?)))
            } else {
                Ok(None)
            }
        })
        .await?
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.db_path();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute("DELETE FROM api_keys WHERE key=?1", [&key])
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
