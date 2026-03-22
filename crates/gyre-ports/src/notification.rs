use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::Notification;

#[async_trait]
pub trait NotificationRepository: Send + Sync {
    async fn create(&self, notification: &Notification) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<Notification>>;
    async fn list_by_user(&self, user_id: &Id, unread_only: bool) -> Result<Vec<Notification>>;
    async fn count_unread(&self, user_id: &Id) -> Result<u64>;
    async fn mark_read(&self, id: &Id, now: u64) -> Result<()>;
    async fn mark_all_read(&self, user_id: &Id, now: u64) -> Result<()>;
}
