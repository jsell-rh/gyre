use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{Review, ReviewComment, ReviewDecision};
use gyre_ports::ReviewRepository;

use super::{open_conn, SqliteStorage};

fn decision_to_str(d: &ReviewDecision) -> &'static str {
    match d {
        ReviewDecision::Approved => "Approved",
        ReviewDecision::ChangesRequested => "ChangesRequested",
    }
}

fn str_to_decision(s: &str) -> Result<ReviewDecision> {
    match s {
        "Approved" => Ok(ReviewDecision::Approved),
        "ChangesRequested" => Ok(ReviewDecision::ChangesRequested),
        other => Err(anyhow!("unknown review decision: {}", other)),
    }
}

fn row_to_comment(row: &rusqlite::Row<'_>) -> Result<ReviewComment> {
    Ok(ReviewComment {
        id: Id::new(row.get::<_, String>(0)?),
        merge_request_id: Id::new(row.get::<_, String>(1)?),
        author_agent_id: row.get(2)?,
        body: row.get(3)?,
        file_path: row.get(4)?,
        line_number: row.get::<_, Option<i64>>(5)?.map(|v| v as u32),
        created_at: row.get::<_, i64>(6)? as u64,
    })
}

fn row_to_review(row: &rusqlite::Row<'_>) -> Result<Review> {
    let decision_str: String = row.get(3)?;
    Ok(Review {
        id: Id::new(row.get::<_, String>(0)?),
        merge_request_id: Id::new(row.get::<_, String>(1)?),
        reviewer_agent_id: row.get(2)?,
        decision: str_to_decision(&decision_str)?,
        body: row.get(4)?,
        created_at: row.get::<_, i64>(5)? as u64,
    })
}

#[async_trait]
impl ReviewRepository for SqliteStorage {
    async fn add_comment(&self, comment: &ReviewComment) -> Result<()> {
        let path = self.db_path();
        let c = comment.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "INSERT INTO review_comments (id, merge_request_id, author_agent_id, body,
                                              file_path, line_number, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    c.id.as_str(),
                    c.merge_request_id.as_str(),
                    c.author_agent_id,
                    c.body,
                    c.file_path,
                    c.line_number.map(|v| v as i64),
                    c.created_at as i64,
                ],
            )
            .context("insert review_comment")?;
            Ok(())
        })
        .await?
    }

    async fn list_comments(&self, mr_id: &Id) -> Result<Vec<ReviewComment>> {
        let path = self.db_path();
        let mr_id = mr_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<ReviewComment>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, merge_request_id, author_agent_id, body, file_path, line_number,
                        created_at
                 FROM review_comments WHERE merge_request_id = ?1 ORDER BY created_at",
            )?;
            let rows = stmt.query_map([mr_id.as_str()], |row| Ok(row_to_comment(row).unwrap()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn submit_review(&self, review: &Review) -> Result<()> {
        let path = self.db_path();
        let r = review.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "INSERT INTO reviews (id, merge_request_id, reviewer_agent_id, decision, body,
                                      created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    r.id.as_str(),
                    r.merge_request_id.as_str(),
                    r.reviewer_agent_id,
                    decision_to_str(&r.decision),
                    r.body,
                    r.created_at as i64,
                ],
            )
            .context("insert review")?;
            Ok(())
        })
        .await?
    }

    async fn list_reviews(&self, mr_id: &Id) -> Result<Vec<Review>> {
        let path = self.db_path();
        let mr_id = mr_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Review>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, merge_request_id, reviewer_agent_id, decision, body, created_at
                 FROM reviews WHERE merge_request_id = ?1 ORDER BY created_at",
            )?;
            let rows = stmt.query_map([mr_id.as_str()], |row| Ok(row_to_review(row).unwrap()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn is_approved(&self, mr_id: &Id) -> Result<bool> {
        let reviews = self.list_reviews(mr_id).await?;
        if reviews.is_empty() {
            return Ok(false);
        }
        let has_changes_requested = reviews
            .iter()
            .any(|r| r.decision == ReviewDecision::ChangesRequested);
        if has_changes_requested {
            return Ok(false);
        }
        let has_approval = reviews
            .iter()
            .any(|r| r.decision == ReviewDecision::Approved);
        Ok(has_approval)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use gyre_domain::{MergeRequest, Project, Repository};
    use gyre_ports::{MergeRequestRepository, ProjectRepository, RepoRepository};
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    async fn seed_mr(s: &SqliteStorage, mr_id: &str) {
        let p = Project::new(Id::new("p1"), "proj".to_string(), 1000);
        let _ = ProjectRepository::create(s, &p).await;
        let r = Repository::new(
            Id::new("r1"),
            Id::new("p1"),
            "repo".to_string(),
            "/repo".to_string(),
            1000,
        );
        let _ = RepoRepository::create(s, &r).await;
        let mr = MergeRequest::new(
            Id::new(mr_id),
            Id::new("r1"),
            "Test MR",
            "feat/x",
            "main",
            1000,
        );
        MergeRequestRepository::create(s, &mr).await.unwrap();
    }

    fn make_comment(id: &str, mr_id: &str) -> ReviewComment {
        ReviewComment::new(Id::new(id), Id::new(mr_id), "agent-1", "Looks good", 1000)
    }

    fn make_review(id: &str, mr_id: &str, decision: ReviewDecision) -> Review {
        Review::new(Id::new(id), Id::new(mr_id), "agent-1", decision, 1000)
    }

    #[tokio::test]
    async fn add_and_list_comments() {
        let (_tmp, s) = setup();
        seed_mr(&s, "mr1").await;
        let c = make_comment("c1", "mr1");
        ReviewRepository::add_comment(&s, &c).await.unwrap();

        let comments = ReviewRepository::list_comments(&s, &Id::new("mr1"))
            .await
            .unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].body, "Looks good");
        assert_eq!(comments[0].author_agent_id, "agent-1");
    }

    #[tokio::test]
    async fn comment_with_file_and_line() {
        let (_tmp, s) = setup();
        seed_mr(&s, "mr1").await;
        let mut c = make_comment("c1", "mr1");
        c.file_path = Some("src/main.rs".to_string());
        c.line_number = Some(42);
        ReviewRepository::add_comment(&s, &c).await.unwrap();

        let comments = ReviewRepository::list_comments(&s, &Id::new("mr1"))
            .await
            .unwrap();
        assert_eq!(comments[0].file_path.as_deref(), Some("src/main.rs"));
        assert_eq!(comments[0].line_number, Some(42));
    }

    #[tokio::test]
    async fn list_comments_empty_mr() {
        let (_tmp, s) = setup();
        seed_mr(&s, "mr1").await;
        let comments = ReviewRepository::list_comments(&s, &Id::new("mr1"))
            .await
            .unwrap();
        assert!(comments.is_empty());
    }

    #[tokio::test]
    async fn submit_and_list_reviews() {
        let (_tmp, s) = setup();
        seed_mr(&s, "mr1").await;
        let r = make_review("r1", "mr1", ReviewDecision::Approved);
        ReviewRepository::submit_review(&s, &r).await.unwrap();

        let reviews = ReviewRepository::list_reviews(&s, &Id::new("mr1"))
            .await
            .unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].decision, ReviewDecision::Approved);
    }

    #[tokio::test]
    async fn is_approved_with_approval() {
        let (_tmp, s) = setup();
        seed_mr(&s, "mr1").await;
        let r = make_review("r1", "mr1", ReviewDecision::Approved);
        ReviewRepository::submit_review(&s, &r).await.unwrap();

        assert!(ReviewRepository::is_approved(&s, &Id::new("mr1"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn is_approved_with_changes_requested() {
        let (_tmp, s) = setup();
        seed_mr(&s, "mr1").await;
        let r = make_review("r1", "mr1", ReviewDecision::ChangesRequested);
        ReviewRepository::submit_review(&s, &r).await.unwrap();

        assert!(!ReviewRepository::is_approved(&s, &Id::new("mr1"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn is_not_approved_when_empty() {
        let (_tmp, s) = setup();
        seed_mr(&s, "mr1").await;
        assert!(!ReviewRepository::is_approved(&s, &Id::new("mr1"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn approval_blocked_by_changes_requested() {
        let (_tmp, s) = setup();
        seed_mr(&s, "mr1").await;
        ReviewRepository::submit_review(&s, &make_review("r1", "mr1", ReviewDecision::Approved))
            .await
            .unwrap();
        ReviewRepository::submit_review(
            &s,
            &make_review("r2", "mr1", ReviewDecision::ChangesRequested),
        )
        .await
        .unwrap();

        assert!(!ReviewRepository::is_approved(&s, &Id::new("mr1"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn multiple_comments_ordered() {
        let (_tmp, s) = setup();
        seed_mr(&s, "mr1").await;
        let mut c1 = make_comment("c1", "mr1");
        c1.created_at = 1000;
        let mut c2 = make_comment("c2", "mr1");
        c2.created_at = 2000;
        ReviewRepository::add_comment(&s, &c1).await.unwrap();
        ReviewRepository::add_comment(&s, &c2).await.unwrap();

        let comments = ReviewRepository::list_comments(&s, &Id::new("mr1"))
            .await
            .unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].id.as_str(), "c1");
        assert_eq!(comments[1].id.as_str(), "c2");
    }
}
