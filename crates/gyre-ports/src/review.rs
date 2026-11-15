use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{Review, ReviewComment};

#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait ReviewRepository: Send + Sync {
    async fn add_comment(&self, comment: &ReviewComment) -> Result<()>;
    async fn list_comments(&self, mr_id: &Id) -> Result<Vec<ReviewComment>>;
    async fn submit_review(&self, review: &Review) -> Result<()>;
    async fn list_reviews(&self, mr_id: &Id) -> Result<Vec<Review>>;
    /// List a reviewer's reviews, newest first (public profile activity
    /// feed + stats: user-management.md §User Profile Page).
    async fn list_by_reviewer(&self, reviewer_agent_id: &str) -> Result<Vec<Review>>;
    /// Returns true if at least one reviewer has approved and none has requested changes.
    async fn is_approved(&self, mr_id: &Id) -> Result<bool>;
}
