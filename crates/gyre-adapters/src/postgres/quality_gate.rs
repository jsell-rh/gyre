use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{GateResult, GateStatus, GateType, QualityGate};
use gyre_ports::{GateResultRepository, QualityGateRepository};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::{gate_results, quality_gates};

#[derive(Queryable, Selectable)]
#[diesel(table_name = quality_gates)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct QualityGateRow {
    id: String,
    repo_id: String,
    name: String,
    gate_type: String,
    command: Option<String>,
    required_approvals: Option<i32>,
    persona: Option<String>,
    required: i32,
    created_at: i64,
}

impl QualityGateRow {
    fn into_gate(self) -> QualityGate {
        let gate_type = match self.gate_type.as_str() {
            "test_command" => GateType::TestCommand,
            "lint_command" => GateType::LintCommand,
            "required_approvals" => GateType::RequiredApprovals,
            "agent_review" => GateType::AgentReview,
            "agent_validation" => GateType::AgentValidation,
            "trace_capture" => GateType::TraceCapture,
            _ => GateType::TestCommand,
        };
        QualityGate {
            id: Id::new(self.id),
            repo_id: Id::new(self.repo_id),
            name: self.name,
            gate_type,
            command: self.command,
            required_approvals: self.required_approvals.map(|v| v as u32),
            persona: self.persona,
            required: self.required != 0,
            created_at: self.created_at as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = quality_gates)]
struct NewQualityGateRow<'a> {
    id: &'a str,
    repo_id: &'a str,
    name: &'a str,
    gate_type: &'a str,
    command: Option<&'a str>,
    required_approvals: Option<i32>,
    persona: Option<&'a str>,
    required: i32,
    created_at: i64,
}

fn gate_type_str(gt: &GateType) -> &'static str {
    match gt {
        GateType::TestCommand => "test_command",
        GateType::LintCommand => "lint_command",
        GateType::RequiredApprovals => "required_approvals",
        GateType::AgentReview => "agent_review",
        GateType::AgentValidation => "agent_validation",
        GateType::TraceCapture => "trace_capture",
    }
}

#[async_trait]
impl QualityGateRepository for PgStorage {
    async fn save(&self, gate: &QualityGate) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let g = gate.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewQualityGateRow {
                id: g.id.as_str(),
                repo_id: g.repo_id.as_str(),
                name: &g.name,
                gate_type: gate_type_str(&g.gate_type),
                command: g.command.as_deref(),
                required_approvals: g.required_approvals.map(|v| v as i32),
                persona: g.persona.as_deref(),
                required: if g.required { 1 } else { 0 },
                created_at: g.created_at as i64,
            };
            diesel::insert_into(quality_gates::table)
                .values(&row)
                .on_conflict(quality_gates::id)
                .do_update()
                .set((
                    quality_gates::name.eq(&row.name),
                    quality_gates::gate_type.eq(row.gate_type),
                    quality_gates::command.eq(row.command),
                    quality_gates::required_approvals.eq(row.required_approvals),
                    quality_gates::persona.eq(row.persona),
                    quality_gates::required.eq(row.required),
                ))
                .execute(&mut *conn)
                .context("upsert quality gate")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &str) -> Result<Option<QualityGate>> {
        let pool = Arc::clone(&self.pool);
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<QualityGate>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = quality_gates::table
                .find(&id)
                .first::<QualityGateRow>(&mut *conn)
                .optional()
                .context("find quality gate by id")?;
            Ok(row.map(QualityGateRow::into_gate))
        })
        .await?
    }

    async fn list_by_repo_id(&self, repo_id: &str) -> Result<Vec<QualityGate>> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<QualityGate>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = quality_gates::table
                .filter(quality_gates::repo_id.eq(&repo_id))
                .order(quality_gates::created_at.asc())
                .load::<QualityGateRow>(&mut *conn)
                .context("list quality gates by repo")?;
            Ok(rows.into_iter().map(QualityGateRow::into_gate).collect())
        })
        .await?
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(quality_gates::table.find(&id))
                .execute(&mut *conn)
                .context("delete quality gate")?;
            Ok(())
        })
        .await?
    }
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = gate_results)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct GateResultRow {
    id: String,
    gate_id: String,
    mr_id: String,
    status: String,
    output: Option<String>,
    started_at: Option<i64>,
    finished_at: Option<i64>,
}

impl GateResultRow {
    fn into_result(self) -> GateResult {
        let status = match self.status.as_str() {
            "pending" => GateStatus::Pending,
            "running" => GateStatus::Running,
            "passed" => GateStatus::Passed,
            "failed" => GateStatus::Failed,
            _ => GateStatus::Pending,
        };
        GateResult {
            id: Id::new(self.id),
            gate_id: Id::new(self.gate_id),
            mr_id: Id::new(self.mr_id),
            status,
            output: self.output,
            started_at: self.started_at.map(|v| v as u64),
            finished_at: self.finished_at.map(|v| v as u64),
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = gate_results)]
struct NewGateResultRow<'a> {
    id: &'a str,
    gate_id: &'a str,
    mr_id: &'a str,
    status: &'a str,
    output: Option<&'a str>,
    started_at: Option<i64>,
    finished_at: Option<i64>,
}

fn gate_status_str(s: &GateStatus) -> &'static str {
    match s {
        GateStatus::Pending => "pending",
        GateStatus::Running => "running",
        GateStatus::Passed => "passed",
        GateStatus::Failed => "failed",
    }
}

#[async_trait]
impl GateResultRepository for PgStorage {
    async fn save(&self, result: &GateResult) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let r = result.clone();
        let status_str = gate_status_str(&r.status).to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewGateResultRow {
                id: r.id.as_str(),
                gate_id: r.gate_id.as_str(),
                mr_id: r.mr_id.as_str(),
                status: &status_str,
                output: r.output.as_deref(),
                started_at: r.started_at.map(|v| v as i64),
                finished_at: r.finished_at.map(|v| v as i64),
            };
            diesel::insert_into(gate_results::table)
                .values(&row)
                .on_conflict(gate_results::id)
                .do_update()
                .set((
                    gate_results::status.eq(row.status),
                    gate_results::output.eq(row.output),
                    gate_results::started_at.eq(row.started_at),
                    gate_results::finished_at.eq(row.finished_at),
                ))
                .execute(&mut *conn)
                .context("upsert gate result")?;
            Ok(())
        })
        .await?
    }

    async fn update_status(
        &self,
        id: &str,
        status: GateStatus,
        started_at: Option<u64>,
        finished_at: Option<u64>,
        output: Option<String>,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.to_string();
        let status_str = gate_status_str(&status).to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(gate_results::table.find(&id))
                .set((
                    gate_results::status.eq(&status_str),
                    gate_results::started_at.eq(started_at.map(|v| v as i64)),
                    gate_results::finished_at.eq(finished_at.map(|v| v as i64)),
                    gate_results::output.eq(output),
                ))
                .execute(&mut *conn)
                .context("update gate result status")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &str) -> Result<Option<GateResult>> {
        let pool = Arc::clone(&self.pool);
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<GateResult>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = gate_results::table
                .find(&id)
                .first::<GateResultRow>(&mut *conn)
                .optional()
                .context("find gate result by id")?;
            Ok(row.map(GateResultRow::into_result))
        })
        .await?
    }

    async fn list_by_mr_id(&self, mr_id: &str) -> Result<Vec<GateResult>> {
        let pool = Arc::clone(&self.pool);
        let mr_id = mr_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<GateResult>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = gate_results::table
                .filter(gate_results::mr_id.eq(&mr_id))
                .load::<GateResultRow>(&mut *conn)
                .context("list gate results by mr_id")?;
            Ok(rows.into_iter().map(GateResultRow::into_result).collect())
        })
        .await?
    }
}
