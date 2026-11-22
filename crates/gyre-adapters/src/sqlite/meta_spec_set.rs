use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_ports::{MetaSpecSetRepository, TenantMetaSpecSetRepository};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::{meta_spec_sets, tenant_meta_spec_sets};

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

#[derive(Queryable, Selectable)]
#[diesel(table_name = tenant_meta_spec_sets)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct TenantMetaSpecSetRow {
    #[allow(dead_code)]
    tenant_id: String,
    json: String,
    #[allow(dead_code)]
    updated_at: i64,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = tenant_meta_spec_sets)]
struct UpsertTenantMetaSpecSetRow<'a> {
    tenant_id: &'a str,
    json: &'a str,
    updated_at: i64,
}

#[async_trait]
impl TenantMetaSpecSetRepository for SqliteStorage {
    async fn get(&self, tenant_id: &Id) -> Result<Option<String>> {
        let pool = Arc::clone(&self.pool);
        let tid = tenant_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<String>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = tenant_meta_spec_sets::table
                .filter(tenant_meta_spec_sets::tenant_id.eq(tid.as_str()))
                .first::<TenantMetaSpecSetRow>(&mut *conn)
                .optional()
                .context("find tenant_meta_spec_set by tenant_id")?;
            Ok(result.map(|r| r.json))
        })
        .await?
    }

    async fn upsert(&self, tenant_id: &Id, json: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let tid = tenant_id.as_str().to_string();
        let json = json.to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = UpsertTenantMetaSpecSetRow {
                tenant_id: tid.as_str(),
                json: json.as_str(),
                updated_at: now,
            };
            diesel::insert_into(tenant_meta_spec_sets::table)
                .values(&row)
                .on_conflict(tenant_meta_spec_sets::tenant_id)
                .do_update()
                .set(&row)
                .execute(&mut *conn)
                .context("upsert tenant_meta_spec_set")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, tenant_id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let tid = tenant_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(tenant_meta_spec_sets::table.find(tid.as_str()))
                .execute(&mut *conn)
                .context("delete tenant_meta_spec_set")?;
            Ok(())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use gyre_ports::{MetaSpecSetRepository, TenantMetaSpecSetRepository};
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

    #[tokio::test]
    async fn tenant_get_missing_returns_none() {
        let (_tmp, s) = setup();
        let result = TenantMetaSpecSetRepository::get(&s, &Id::new("tenant-1"))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn tenant_upsert_and_get() {
        let (_tmp, s) = setup();
        let json = r#"{"tenant_id":"tenant-1","personas":{},"principles":[]}"#;
        TenantMetaSpecSetRepository::upsert(&s, &Id::new("tenant-1"), json)
            .await
            .unwrap();
        let got = TenantMetaSpecSetRepository::get(&s, &Id::new("tenant-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got, json);
    }

    #[tokio::test]
    async fn tenant_upsert_overwrites() {
        let (_tmp, s) = setup();
        TenantMetaSpecSetRepository::upsert(&s, &Id::new("tenant-1"), r#"{"v":1}"#)
            .await
            .unwrap();
        TenantMetaSpecSetRepository::upsert(&s, &Id::new("tenant-1"), r#"{"v":2}"#)
            .await
            .unwrap();
        let got = TenantMetaSpecSetRepository::get(&s, &Id::new("tenant-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got, r#"{"v":2}"#);
    }

    #[tokio::test]
    async fn tenant_delete_removes() {
        let (_tmp, s) = setup();
        TenantMetaSpecSetRepository::upsert(&s, &Id::new("tenant-1"), r#"{"v":1}"#)
            .await
            .unwrap();
        TenantMetaSpecSetRepository::delete(&s, &Id::new("tenant-1"))
            .await
            .unwrap();
        let got = TenantMetaSpecSetRepository::get(&s, &Id::new("tenant-1"))
            .await
            .unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn tenant_set_is_independent_of_workspace_set() {
        let (_tmp, s) = setup();
        TenantMetaSpecSetRepository::upsert(&s, &Id::new("t"), r#"{"who":"tenant"}"#)
            .await
            .unwrap();
        MetaSpecSetRepository::upsert(&s, &Id::new("t"), r#"{"who":"workspace"}"#)
            .await
            .unwrap();
        assert_eq!(
            TenantMetaSpecSetRepository::get(&s, &Id::new("t"))
                .await
                .unwrap()
                .unwrap(),
            r#"{"who":"tenant"}"#
        );
        assert_eq!(
            MetaSpecSetRepository::get(&s, &Id::new("t"))
                .await
                .unwrap()
                .unwrap(),
            r#"{"who":"workspace"}"#
        );
    }
}
