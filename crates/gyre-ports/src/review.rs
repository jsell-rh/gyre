use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{Review, ReviewComment};

#[async_trait]
pub trait ReviewRepository: Send + Sync {
    async fn add_comment(&self, comment: &ReviewComment) -> Result<()>;
    async fn list_comments(&self, mr_id: &Id) -> Result<Vec<ReviewComment>>;
    async fn submit_review(&self, review: &Review) -> Result<()>;
    async fn list_reviews(&self, mr_id: &Id) -> Result<Vec<Review>>;
    /// Returns true if at least one reviewer has approved and none has requested changes.
    async fn is_approved(&self, mr_id: &Id) -> Result<bool>;
}
