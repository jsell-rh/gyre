use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_ports::RolloutPolicyRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::rollout_policies;

#[derive(Queryable, Selectable)]
#[diesel(table_name = rollout_policies)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct RolloutPolicyRow {
    #[allow(dead_code)]
    workspace_id: String,
    json: String,
    #[allow(dead_code)]
    updated_at: i64,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = rollout_policies)]
struct UpsertRolloutPolicyRow<'a> {
    workspace_id: &'a str,
    json: &'a str,
    updated_at: i64,
}

#[async_trait]
impl RolloutPolicyRepository for SqliteStorage {
    async fn get(&self, workspace_id: &Id) -> Result<Option<String>> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<String>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = rollout_policies::table
                .find(ws_id.as_str())
                .first::<RolloutPolicyRow>(&mut *conn)
                .optional()
                .context("find rollout_policy by workspace_id")?;
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
            let row = UpsertRolloutPolicyRow {
                workspace_id: ws_id.as_str(),
                json: json.as_str(),
                updated_at: now,
            };
            diesel::insert_into(rollout_policies::table)
                .values(&row)
                .on_conflict(rollout_policies::workspace_id)
                .do_update()
                .set(&row)
                .execute(&mut *conn)
                .context("upsert rollout_policy")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, workspace_id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(rollout_policies::table.find(ws_id.as_str()))
                .execute(&mut *conn)
                .context("delete rollout_policy")?;
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

    fn policy_json(strategy: &str) -> String {
        format!(
            r#"{{"strategy":"{strategy}","max_concurrent_agents":5,"priority":"normal","auto_merge":false}}"#
        )
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let (_tmp, s) = setup();
        let result = RolloutPolicyRepository::get(&s, &Id::new("ws-1"))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn upsert_and_get() {
        let (_tmp, s) = setup();
        let json = policy_json("rolling");
        RolloutPolicyRepository::upsert(&s, &Id::new("ws-1"), &json)
            .await
            .unwrap();
        let got = RolloutPolicyRepository::get(&s, &Id::new("ws-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got, json);
    }

    #[tokio::test]
    async fn upsert_overwrites() {
        let (_tmp, s) = setup();
        RolloutPolicyRepository::upsert(&s, &Id::new("ws-1"), &policy_json("rolling"))
            .await
            .unwrap();
        RolloutPolicyRepository::upsert(&s, &Id::new("ws-1"), &policy_json("immediate"))
            .await
            .unwrap();
        let got = RolloutPolicyRepository::get(&s, &Id::new("ws-1"))
            .await
            .unwrap()
            .unwrap();
        assert!(
            got.contains("\"immediate\""),
            "expected overwrite, got {got}"
        );
    }

    #[tokio::test]
    async fn delete_reverts_to_missing() {
        let (_tmp, s) = setup();
        RolloutPolicyRepository::upsert(&s, &Id::new("ws-1"), &policy_json("rolling"))
            .await
            .unwrap();
        RolloutPolicyRepository::delete(&s, &Id::new("ws-1"))
            .await
            .unwrap();
        let got = RolloutPolicyRepository::get(&s, &Id::new("ws-1"))
            .await
            .unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn stored_json_round_trips_domain_policy() {
        // The row is opaque JSON to the adapter, but it must deserialize to
        // the domain type the server layer expects — otherwise the §7
        // policy GET/API path silently fails at parse time.
        let (_tmp, s) = setup();
        let policy = gyre_domain::RolloutPolicy {
            strategy: gyre_domain::RolloutStrategy::Rolling,
            max_concurrent_agents: 7,
            priority: gyre_domain::ReconciliationPriority::High,
            auto_merge: true,
        };
        let json = serde_json::to_string(&policy).unwrap();
        RolloutPolicyRepository::upsert(&s, &Id::new("ws-1"), &json)
            .await
            .unwrap();
        let got = RolloutPolicyRepository::get(&s, &Id::new("ws-1"))
            .await
            .unwrap()
            .unwrap();
        let parsed: gyre_domain::RolloutPolicy = serde_json::from_str(&got).unwrap();
        assert_eq!(parsed, policy);
    }
}
