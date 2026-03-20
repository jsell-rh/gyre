use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{Review, ReviewComment, ReviewDecision};
use gyre_ports::ReviewRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::{review_comments, reviews};

#[derive(Queryable, Selectable)]
#[diesel(table_name = review_comments)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
#[diesel(check_for_backend(diesel::pg::Pg))]
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
impl ReviewRepository for PgStorage {
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
