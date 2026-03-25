use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_ports::MetaSpecSetRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::meta_spec_sets;

#[derive(Queryable, Selectable)]
#[diesel(table_name = meta_spec_sets)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct MetaSpecSetRow {
    #[allow(dead_code)]
    workspace_id: String,
    json: String,
    #[allow(dead_code)]
    updated_at: i64,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = meta_spec_sets)]
struct UpsertMetaSpecSetRow<'a> {
    workspace_id: &'a str,
    json: &'a str,
    updated_at: i64,
}

#[async_trait]
impl MetaSpecSetRepository for SqliteStorage {
    async fn get(&self, workspace_id: &Id) -> Result<Option<String>> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<String>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = meta_spec_sets::table
                .find(ws_id.as_str())
                .first::<MetaSpecSetRow>(&mut *conn)
                .optional()
                .context("find meta_spec_set by workspace_id")?;
            Ok(result.map(|r| r.json))
        })
        .await?
    }

    async fn upsert(&self, workspace_id: &Id, json: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        let json = json.to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = UpsertMetaSpecSetRow {
                workspace_id: ws_id.as_str(),
                json: json.as_str(),
                updated_at: now,
            };
            diesel::insert_into(meta_spec_sets::table)
                .values(&row)
                .on_conflict(meta_spec_sets::workspace_id)
                .do_update()
                .set(&row)
                .execute(&mut *conn)
                .context("upsert meta_spec_set")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, workspace_id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(meta_spec_sets::table.find(ws_id.as_str()))
                .execute(&mut *conn)
                .context("delete meta_spec_set")?;
            Ok(())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let (_tmp, s) = setup();
        let result = MetaSpecSetRepository::get(&s, &Id::new("ws-1"))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn upsert_and_get() {
        let (_tmp, s) = setup();
        let json = r#"{"workspace_id":"ws-1","personas":{},"principles":[]}"#;
        MetaSpecSetRepository::upsert(&s, &Id::new("ws-1"), json)
            .await
            .unwrap();
        let got = MetaSpecSetRepository::get(&s, &Id::new("ws-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got, json);
    }

    #[tokio::test]
    async fn upsert_overwrites() {
        let (_tmp, s) = setup();
        MetaSpecSetRepository::upsert(&s, &Id::new("ws-1"), r#"{"v":1}"#)
            .await
            .unwrap();
        MetaSpecSetRepository::upsert(&s, &Id::new("ws-1"), r#"{"v":2}"#)
            .await
            .unwrap();
        let got = MetaSpecSetRepository::get(&s, &Id::new("ws-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got, r#"{"v":2}"#);
    }

    #[tokio::test]
    async fn delete_removes() {
        let (_tmp, s) = setup();
        MetaSpecSetRepository::upsert(&s, &Id::new("ws-1"), r#"{"v":1}"#)
            .await
            .unwrap();
        MetaSpecSetRepository::delete(&s, &Id::new("ws-1"))
            .await
            .unwrap();
        let got = MetaSpecSetRepository::get(&s, &Id::new("ws-1"))
            .await
            .unwrap();
        assert!(got.is_none());
    }
}
