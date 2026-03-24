use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_domain::ContainerAuditRecord;
use gyre_ports::ContainerAuditRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::container_audit_records;

#[derive(Queryable, Selectable)]
#[diesel(table_name = container_audit_records)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct ContainerAuditRow {
    agent_id: String,
    container_id: String,
    image: String,
    image_hash: Option<String>,
    runtime: String,
    started_at: i64,
    stopped_at: Option<i64>,
    exit_code: Option<i32>,
}

impl ContainerAuditRow {
    fn into_record(self) -> ContainerAuditRecord {
        ContainerAuditRecord {
            agent_id: self.agent_id,
            container_id: self.container_id,
            image: self.image,
            image_hash: self.image_hash,
            runtime: self.runtime,
            started_at: self.started_at as u64,
            stopped_at: self.stopped_at.map(|v| v as u64),
            exit_code: self.exit_code,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = container_audit_records)]
struct NewContainerAuditRow<'a> {
    agent_id: &'a str,
    container_id: &'a str,
    image: &'a str,
    image_hash: Option<&'a str>,
    runtime: &'a str,
    started_at: i64,
    stopped_at: Option<i64>,
    exit_code: Option<i32>,
}

#[async_trait]
impl ContainerAuditRepository for SqliteStorage {
    async fn find_by_agent_id(&self, agent_id: &str) -> Result<Option<ContainerAuditRecord>> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<ContainerAuditRecord>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = container_audit_records::table
                .find(&agent_id)
                .first::<ContainerAuditRow>(&mut *conn)
                .optional()
                .context("find container audit by agent_id")?;
            Ok(row.map(ContainerAuditRow::into_record))
        })
        .await?
    }

    async fn save(&self, record: &ContainerAuditRecord) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let r = record.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewContainerAuditRow {
                agent_id: &r.agent_id,
                container_id: &r.container_id,
                image: &r.image,
                image_hash: r.image_hash.as_deref(),
                runtime: &r.runtime,
                started_at: r.started_at as i64,
                stopped_at: r.stopped_at.map(|v| v as i64),
                exit_code: r.exit_code,
            };
            diesel::insert_into(container_audit_records::table)
                .values(&row)
                .on_conflict(container_audit_records::agent_id)
                .do_update()
                .set((
                    container_audit_records::container_id.eq(row.container_id),
                    container_audit_records::image.eq(row.image),
                    container_audit_records::image_hash.eq(row.image_hash),
                    container_audit_records::runtime.eq(row.runtime),
                    container_audit_records::started_at.eq(row.started_at),
                    container_audit_records::stopped_at.eq(row.stopped_at),
                    container_audit_records::exit_code.eq(row.exit_code),
                ))
                .execute(&mut *conn)
                .context("upsert container audit record")?;
            Ok(())
        })
        .await?
    }

    async fn update_exit(
        &self,
        agent_id: &str,
        exit_code: Option<i32>,
        stopped_at: Option<u64>,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(container_audit_records::table.find(&agent_id))
                .set((
                    container_audit_records::exit_code.eq(exit_code),
                    container_audit_records::stopped_at.eq(stopped_at.map(|v| v as i64)),
                ))
                .execute(&mut *conn)
                .context("update container audit exit info")?;
            Ok(())
        })
        .await?
    }
}
