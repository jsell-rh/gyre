use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::spec_approval::ApprovalTransitionError;
use gyre_domain::SpecApproval;
use gyre_ports::SpecApprovalRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::spec_approvals;

#[derive(Queryable, Selectable)]
#[diesel(table_name = spec_approvals)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct SpecApprovalRow {
    id: String,
    spec_path: String,
    spec_sha: String,
    approver_id: String,
    signature: Option<String>,
    approved_at: Option<i64>,
    revoked_at: Option<i64>,
    revoked_by: Option<String>,
    revocation_reason: Option<String>,
    rejected_at: Option<i64>,
    rejected_reason: Option<String>,
    rejected_by: Option<String>,
}

impl SpecApprovalRow {
    fn into_approval(self) -> SpecApproval {
        SpecApproval {
            id: Id::new(self.id),
            spec_path: self.spec_path,
            spec_sha: self.spec_sha,
            approver_id: self.approver_id,
            signature: self.signature,
            approved_at: self.approved_at.map(|v| v as u64),
            revoked_at: self.revoked_at.map(|v| v as u64),
            revoked_by: self.revoked_by,
            revocation_reason: self.revocation_reason,
            rejected_at: self.rejected_at.map(|v| v as u64),
            rejected_reason: self.rejected_reason,
            rejected_by: self.rejected_by,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = spec_approvals)]
struct NewSpecApprovalRow<'a> {
    id: &'a str,
    spec_path: &'a str,
    spec_sha: &'a str,
    approver_id: &'a str,
    signature: Option<&'a str>,
    approved_at: Option<i64>,
    revoked_at: Option<i64>,
    revoked_by: Option<&'a str>,
    revocation_reason: Option<&'a str>,
    rejected_at: Option<i64>,
    rejected_reason: Option<&'a str>,
    rejected_by: Option<&'a str>,
}

impl SqliteStorage {
    /// Apply a domain transition (approve/revoke/reject) to a stored entry:
    /// load, validate via the domain's transition rules, persist the full row
    /// so mutual exclusivity (clearing other timestamp columns) is guaranteed.
    /// Returns Ok(None) when the entry does not exist.
    async fn transition(
        &self,
        id: &Id,
        apply: impl FnOnce(&mut SpecApproval) -> Result<(), ApprovalTransitionError> + Send + 'static,
    ) -> Result<Option<()>, ApprovalTransitionError> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<()>, ApprovalTransitionError> {
            let mut conn = pool
                .get()
                .map_err(|e| ApprovalTransitionError::Storage(e.to_string()))?;
            let row = spec_approvals::table
                .find(id.as_str())
                .first::<SpecApprovalRow>(&mut conn)
                .optional()
                .map_err(|e| ApprovalTransitionError::Storage(e.to_string()))?;
            let Some(row) = row else {
                return Ok(None);
            };
            let mut approval = row.into_approval();
            apply(&mut approval)?;
            upsert_row(&mut conn, &approval)
                .map_err(|e| ApprovalTransitionError::Storage(e.to_string()))?;
            Ok(Some(()))
        })
        .await
        .map_err(|e| ApprovalTransitionError::Storage(e.to_string()))?
    }
}
/// Persist the full row for an approval (used after in-domain transitions).
fn upsert_row(conn: &mut SqliteConnection, a: &SpecApproval) -> Result<()> {
    let row = NewSpecApprovalRow {
        id: a.id.as_str(),
        spec_path: &a.spec_path,
        spec_sha: &a.spec_sha,
        approver_id: &a.approver_id,
        signature: a.signature.as_deref(),
        approved_at: a.approved_at.map(|v| v as i64),
        revoked_at: a.revoked_at.map(|v| v as i64),
        revoked_by: a.revoked_by.as_deref(),
        revocation_reason: a.revocation_reason.as_deref(),
        rejected_at: a.rejected_at.map(|v| v as i64),
        rejected_reason: a.rejected_reason.as_deref(),
        rejected_by: a.rejected_by.as_deref(),
    };
    diesel::replace_into(spec_approvals::table)
        .values(&row)
        .execute(conn)
        .context("upsert spec approval")?;
    Ok(())
}

#[async_trait]
impl SpecApprovalRepository for SqliteStorage {
    async fn create(&self, approval: &SpecApproval) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let a = approval.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewSpecApprovalRow {
                id: a.id.as_str(),
                spec_path: &a.spec_path,
                spec_sha: &a.spec_sha,
                approver_id: &a.approver_id,
                signature: a.signature.as_deref(),
                approved_at: a.approved_at.map(|v| v as i64),
                revoked_at: a.revoked_at.map(|v| v as i64),
                revoked_by: a.revoked_by.as_deref(),
                revocation_reason: a.revocation_reason.as_deref(),
                rejected_at: a.rejected_at.map(|v| v as i64),
                rejected_reason: a.rejected_reason.as_deref(),
                rejected_by: a.rejected_by.as_deref(),
            };
            // Port contract: create fails when an id already exists (duplicate
            // ids indicate a bug). The PK constraint raises; no silent no-op
            // (mem adapter enforces the same contract in code).
            diesel::insert_into(spec_approvals::table)
                .values(&row)
                .execute(&mut *conn)
                .context("insert spec approval")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<SpecApproval>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<SpecApproval>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = spec_approvals::table
                .find(id.as_str())
                .first::<SpecApprovalRow>(&mut *conn)
                .optional()
                .context("find spec approval by id")?;
            Ok(result.map(SpecApprovalRow::into_approval))
        })
        .await?
    }

    async fn list_by_path(&self, spec_path: &str) -> Result<Vec<SpecApproval>> {
        let pool = Arc::clone(&self.pool);
        let path = spec_path.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<SpecApproval>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = spec_approvals::table
                .filter(spec_approvals::spec_path.eq(&path))
                .order(spec_approvals::id.desc())
                .load::<SpecApprovalRow>(&mut *conn)
                .context("list spec approvals by path")?;
            Ok(rows
                .into_iter()
                .map(SpecApprovalRow::into_approval)
                .collect())
        })
        .await?
    }

    async fn list_active_by_path(&self, spec_path: &str) -> Result<Vec<SpecApproval>> {
        let pool = Arc::clone(&self.pool);
        let path = spec_path.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<SpecApproval>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = spec_approvals::table
                .filter(spec_approvals::spec_path.eq(&path))
                .filter(spec_approvals::revoked_at.is_null())
                .filter(spec_approvals::rejected_at.is_null())
                .filter(spec_approvals::approved_at.is_not_null())
                .order(spec_approvals::id.desc())
                .load::<SpecApprovalRow>(&mut *conn)
                .context("list active spec approvals by path")?;
            Ok(rows
                .into_iter()
                .map(SpecApprovalRow::into_approval)
                .collect())
        })
        .await?
    }

    async fn list_all(&self) -> Result<Vec<SpecApproval>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<SpecApproval>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = spec_approvals::table
                .order(spec_approvals::id.desc())
                .load::<SpecApprovalRow>(&mut *conn)
                .context("list all spec approvals")?;
            Ok(rows
                .into_iter()
                .map(SpecApprovalRow::into_approval)
                .collect())
        })
        .await?
    }

    async fn approve(&self, id: &Id, now: u64) -> Result<Option<()>, ApprovalTransitionError> {
        self.transition(id, move |a| a.approve(now)).await
    }
    async fn revoke(
        &self,
        id: &Id,
        revoked_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<Option<()>, ApprovalTransitionError> {
        let revoked_by = revoked_by.to_string();
        let reason = reason.to_string();
        self.transition(id, move |a| a.revoke(&revoked_by, &reason, now))
            .await
    }
    async fn reject(
        &self,
        id: &Id,
        rejected_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<Option<()>, ApprovalTransitionError> {
        let rejected_by = rejected_by.to_string();
        let reason = reason.to_string();
        self.transition(id, move |a| a.reject(&rejected_by, &reason, now))
            .await
    }

    async fn revoke_all_for_path(
        &self,
        spec_path: &str,
        revoked_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let path = spec_path.to_string();
        let revoked_by = revoked_by.to_string();
        let reason = reason.to_string();
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            // Apply the same domain transition per row: only Approved rows
            // are revocable; mutual exclusivity clears approved_at/rejected_*.
            let rows = spec_approvals::table
                .filter(spec_approvals::spec_path.eq(&path))
                .load::<SpecApprovalRow>(&mut *conn)
                .context("load spec approvals for revoke-all")?;
            let mut revoked = 0u64;
            for row in rows {
                let mut approval = row.into_approval();
                // Stale-invalidation semantics: an approval is stale once the
                // spec content changes. Rows already revoked/rejected keep
                // their terminal state; only Approved rows transition.
                if approval.status() == gyre_domain::spec_approval::ApprovalStatus::Approved
                    && approval.revoke(&revoked_by, &reason, now).is_ok()
                {
                    upsert_row(&mut conn, &approval)?;
                    revoked += 1;
                }
            }
            Ok(revoked)
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use tempfile::NamedTempFile;

    fn tmp_storage() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, storage)
    }

    fn sha(seed: char) -> String {
        std::iter::repeat(seed).take(40).collect()
    }

    #[tokio::test]
    async fn create_find_and_list_by_path() {
        let (_tmp, storage) = tmp_storage();
        let a1 = SpecApproval::new(Id::new("apr-1"), "system/design.md", sha('a'), "user:alice");
        let a2 = SpecApproval::new(Id::new("apr-2"), "system/design.md", sha('b'), "user:bob");
        let a3 = SpecApproval::new(Id::new("apr-3"), "system/other.md", sha('a'), "user:alice");
        storage.create(&a1).await.unwrap();
        storage.create(&a2).await.unwrap();
        storage.create(&a3).await.unwrap();

        // Multiple approvals for the same path (different SHAs) coexist.
        let by_path = storage.list_by_path("system/design.md").await.unwrap();
        assert_eq!(by_path.len(), 2);

        let found = storage.find_by_id(&Id::new("apr-1")).await.unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.spec_path, "system/design.md");
        assert_eq!(found.spec_sha, sha('a'));
        assert_eq!(found.status(), gyre_domain::spec_approval::ApprovalStatus::Pending);

        let missing = storage.find_by_id(&Id::new("nope")).await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn approve_persists_and_active_list_finds_only_approved() {
        let (_tmp, storage) = tmp_storage();
        let id = Id::new("apr-1");
        storage
            .create(&SpecApproval::new(
                id.clone(),
                "system/design.md",
                sha('a'),
                "user:alice",
            ))
            .await
            .unwrap();

        let res = storage.approve(&id, 1700000100).await.unwrap();
        assert_eq!(res, Some(()), "existing entry transitions");

        let active = storage.list_active_by_path("system/design.md").await.unwrap();
        assert_eq!(active.len(), 1);
        assert!(active[0].is_active());
        assert_eq!(active[0].approved_at, Some(1700000100));

        // Transitioning a missing entry returns Ok(None), not an error.
        let missing = storage.approve(&Id::new("nope"), 1).await.unwrap();
        assert_eq!(missing, None);
    }

    #[tokio::test]
    async fn revoke_enforces_transition_and_mutual_exclusivity() {
        let (_tmp, storage) = tmp_storage();
        let id = Id::new("apr-1");
        storage
            .create(&SpecApproval::new(
                id.clone(),
                "system/design.md",
                sha('a'),
                "user:alice",
            ))
            .await
            .unwrap();
        storage.approve(&id, 1700000100).await.unwrap().unwrap();

        // Revoking a Pending entry is invalid — but this one is Approved.
        // Revocation of an Approved entry clears approved_at (mutual exclusivity).
        storage
            .revoke(&id, "user:admin", "spec withdrawn", 1700000200)
            .await
            .unwrap()
            .unwrap();

        let reloaded = storage.find_by_id(&id).await.unwrap().unwrap();
        assert_eq!(reloaded.status(), gyre_domain::spec_approval::ApprovalStatus::Revoked);
        assert_eq!(reloaded.approved_at, None, "mutual exclusivity: approved_at cleared");
        assert_eq!(reloaded.revoked_at, Some(1700000200));
        assert_eq!(reloaded.revoked_by.as_deref(), Some("user:admin"));
        assert_eq!(reloaded.revocation_reason.as_deref(), Some("spec withdrawn"));
        assert!(!reloaded.is_active());

        // Already revoked — a second revoke is an invalid transition.
        let again = storage
            .revoke(&id, "user:admin", "again", 1700000300)
            .await;
        assert!(again.is_err(), "Revoked → Revoked must be rejected");

        // No longer in the active list.
        let active = storage.list_active_by_path("system/design.md").await.unwrap();
        assert!(active.is_empty());
    }

    #[tokio::test]
    async fn reject_only_from_pending_and_closes_lifecycle() {
        let (_tmp, storage) = tmp_storage();
        let id = Id::new("apr-1");
        storage
            .create(&SpecApproval::new(
                id.clone(),
                "system/design.md",
                sha('a'),
                "user:alice",
            ))
            .await
            .unwrap();

        storage
            .reject(&id, "user:reviewer", "not ready", 1700000100)
            .await
            .unwrap()
            .unwrap();
        let reloaded = storage.find_by_id(&id).await.unwrap().unwrap();
        assert_eq!(reloaded.status(), gyre_domain::spec_approval::ApprovalStatus::Rejected);
        assert_eq!(reloaded.rejected_by.as_deref(), Some("user:reviewer"));
        assert!(!reloaded.is_active());

        // Approved entries cannot be rejected (only revoked).
        let id2 = Id::new("apr-2");
        storage
            .create(&SpecApproval::new(
                id2.clone(),
                "system/design.md",
                sha('b'),
                "user:alice",
            ))
            .await
            .unwrap();
        storage.approve(&id2, 1700000100).await.unwrap().unwrap();
        let res = storage
            .reject(&id2, "user:reviewer", "no", 1700000200)
            .await;
        assert!(res.is_err(), "Approved → Rejected must be rejected");
    }

    #[tokio::test]
    async fn revoke_all_for_path_only_touches_approved_rows() {
        let (_tmp, storage) = tmp_storage();
        let sha_a = sha('a');
        let approved = Id::new("apr-approved");
        let pending = Id::new("apr-pending");
        let rejected = Id::new("apr-rejected");
        let other_path = Id::new("apr-other");
        for (id, path) in [
            (&approved, "system/design.md"),
            (&pending, "system/design.md"),
            (&rejected, "system/design.md"),
            (&other_path, "system/other.md"),
        ] {
            storage
                .create(&SpecApproval::new(
                    id.clone(),
                    path,
                    sha_a.clone(),
                    "user:alice",
                ))
                .await
                .unwrap();
        }
        storage.approve(&approved, 1700000100).await.unwrap().unwrap();
        storage
            .reject(&rejected, "user:reviewer", "no", 1700000100)
            .await
            .unwrap()
            .unwrap();
        storage.approve(&other_path, 1700000100).await.unwrap().unwrap();

        let count = storage
            .revoke_all_for_path("system/design.md", "system:spec-lifecycle", "spec modified", 1700000200)
            .await
            .unwrap();
        assert_eq!(count, 1, "only the Approved row for the path is revoked");

        let approved_row = storage.find_by_id(&approved).await.unwrap().unwrap();
        assert_eq!(
            approved_row.status(),
            gyre_domain::spec_approval::ApprovalStatus::Revoked
        );
        let pending_row = storage.find_by_id(&pending).await.unwrap().unwrap();
        assert_eq!(
            pending_row.status(),
            gyre_domain::spec_approval::ApprovalStatus::Pending,
            "Pending rows keep their state"
        );
        let rejected_row = storage.find_by_id(&rejected).await.unwrap().unwrap();
        assert_eq!(
            rejected_row.status(),
            gyre_domain::spec_approval::ApprovalStatus::Rejected,
            "Rejected rows keep their terminal state"
        );
        let other_row = storage.find_by_id(&other_path).await.unwrap().unwrap();
        assert_eq!(
            other_row.status(),
            gyre_domain::spec_approval::ApprovalStatus::Approved,
            "other paths untouched"
        );
    }

    #[tokio::test]
    async fn create_duplicate_id_fails() {
        let (_tmp, storage) = tmp_storage();
        let a = SpecApproval::new(Id::new("apr-dup"), "system/design.md", sha('a'), "user:alice");
        storage.create(&a).await.unwrap();
        // Same id must not silently replace the first row (port contract:
        // create fails if an entry with the same id already exists).
        let res = storage.create(&a).await;
        assert!(res.is_err(), "duplicate id create must fail");
        let rows = storage.list_by_path("system/design.md").await.unwrap();
        assert_eq!(rows.len(), 1);
    }
}
