use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{Review, ReviewComment, ReviewDecision};
use gyre_ports::ReviewRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::{review_comments, reviews};

#[derive(Queryable, Selectable)]
#[diesel(table_name = review_comments)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct ReviewCommentRow {
    id: String,
    merge_request_id: String,
    author_agent_id: String,
    body: String,
    file_path: Option<String>,
    line_number: Option<i32>,
    created_at: i64,
}

impl From<ReviewCommentRow> for ReviewComment {
    fn from(r: ReviewCommentRow) -> Self {
        ReviewComment {
            id: Id::new(r.id),
            merge_request_id: Id::new(r.merge_request_id),
            author_agent_id: r.author_agent_id,
            body: r.body,
            file_path: r.file_path,
            line_number: r.line_number.map(|v| v as u32),
            created_at: r.created_at as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = review_comments)]
struct ReviewCommentRecord<'a> {
    id: &'a str,
    merge_request_id: &'a str,
    author_agent_id: &'a str,
    body: &'a str,
    file_path: Option<&'a str>,
    line_number: Option<i32>,
    created_at: i64,
}

impl<'a> From<&'a ReviewComment> for ReviewCommentRecord<'a> {
    fn from(c: &'a ReviewComment) -> Self {
        ReviewCommentRecord {
            id: c.id.as_str(),
            merge_request_id: c.merge_request_id.as_str(),
            author_agent_id: &c.author_agent_id,
            body: &c.body,
            file_path: c.file_path.as_deref(),
            line_number: c.line_number.map(|v| v as i32),
            created_at: c.created_at as i64,
        }
    }
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = reviews)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct ReviewRow {
    id: String,
    merge_request_id: String,
    reviewer_agent_id: String,
    decision: String,
    body: Option<String>,
    created_at: i64,
}

impl ReviewRow {
    fn into_review(self) -> Result<Review> {
        let decision = match self.decision.as_str() {
            "Approved" => Ok(ReviewDecision::Approved),
            "ChangesRequested" => Ok(ReviewDecision::ChangesRequested),
            other => Err(anyhow!("unknown review decision: {}", other)),
        }?;
        Ok(Review {
            id: Id::new(self.id),
            merge_request_id: Id::new(self.merge_request_id),
            reviewer_agent_id: self.reviewer_agent_id,
            decision,
            body: self.body,
            created_at: self.created_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = reviews)]
struct ReviewRecord<'a> {
    id: &'a str,
    merge_request_id: &'a str,
    reviewer_agent_id: &'a str,
    decision: &'a str,
    body: Option<&'a str>,
    created_at: i64,
}

fn decision_to_str(d: &ReviewDecision) -> &'static str {
    match d {
        ReviewDecision::Approved => "Approved",
        ReviewDecision::ChangesRequested => "ChangesRequested",
    }
}

#[async_trait]
impl ReviewRepository for SqliteStorage {
    async fn add_comment(&self, comment: &ReviewComment) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let c = comment.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = ReviewCommentRecord::from(&c);
            diesel::insert_into(review_comments::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert review_comment")?;
            Ok(())
        })
        .await?
    }

    async fn list_comments(&self, mr_id: &Id) -> Result<Vec<ReviewComment>> {
        let pool = Arc::clone(&self.pool);
        let mr_id = mr_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<ReviewComment>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = review_comments::table
                .filter(review_comments::merge_request_id.eq(mr_id.as_str()))
                .order(review_comments::created_at.asc())
                .load::<ReviewCommentRow>(&mut *conn)
                .context("list review_comments")?;
            Ok(rows.into_iter().map(ReviewComment::from).collect())
        })
        .await?
    }

    async fn submit_review(&self, review: &Review) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let r = review.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let decision = decision_to_str(&r.decision);
            let record = ReviewRecord {
                id: r.id.as_str(),
                merge_request_id: r.merge_request_id.as_str(),
                reviewer_agent_id: &r.reviewer_agent_id,
                decision,
                body: r.body.as_deref(),
                created_at: r.created_at as i64,
            };
            diesel::insert_into(reviews::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert review")?;
            Ok(())
        })
        .await?
    }

    async fn list_reviews(&self, mr_id: &Id) -> Result<Vec<Review>> {
        let pool = Arc::clone(&self.pool);
        let mr_id = mr_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Review>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = reviews::table
                .filter(reviews::merge_request_id.eq(mr_id.as_str()))
                .order(reviews::created_at.asc())
                .load::<ReviewRow>(&mut *conn)
                .context("list reviews")?;
            rows.into_iter().map(|r| r.into_review()).collect()
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
    use gyre_domain::{MergeRequest, Repository};
    use gyre_ports::{MergeRequestRepository, RepoRepository};
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    async fn seed_mr(s: &SqliteStorage, mr_id: &str) {
        let r = Repository::new(
            Id::new("r1"),
            Id::new("ws1"),
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
