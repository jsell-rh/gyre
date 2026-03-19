use gyre_common::Id;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeQueueEntryStatus {
    Queued,
    Processing,
    Merged,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeQueueEntry {
    pub id: Id,
    pub merge_request_id: Id,
    /// Higher value = higher priority. Critical=100, High=75, Medium=50, Low=25.
    pub priority: u32,
    pub status: MergeQueueEntryStatus,
    pub enqueued_at: u64,
    pub processed_at: Option<u64>,
    pub error_message: Option<String>,
}

impl MergeQueueEntry {
    pub fn new(id: Id, merge_request_id: Id, priority: u32, enqueued_at: u64) -> Self {
        Self {
            id,
            merge_request_id,
            priority,
            status: MergeQueueEntryStatus::Queued,
            enqueued_at,
            processed_at: None,
            error_message: None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            MergeQueueEntryStatus::Merged
                | MergeQueueEntryStatus::Failed
                | MergeQueueEntryStatus::Cancelled
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry() -> MergeQueueEntry {
        MergeQueueEntry::new(Id::new("e1"), Id::new("mr1"), 50, 1000)
    }

    #[test]
    fn new_entry_is_queued() {
        let e = make_entry();
        assert_eq!(e.status, MergeQueueEntryStatus::Queued);
        assert!(e.processed_at.is_none());
        assert!(e.error_message.is_none());
        assert_eq!(e.priority, 50);
    }

    #[test]
    fn terminal_states() {
        let mut e = make_entry();
        assert!(!e.is_terminal());
        e.status = MergeQueueEntryStatus::Processing;
        assert!(!e.is_terminal());
        e.status = MergeQueueEntryStatus::Merged;
        assert!(e.is_terminal());
        e.status = MergeQueueEntryStatus::Failed;
        assert!(e.is_terminal());
        e.status = MergeQueueEntryStatus::Cancelled;
        assert!(e.is_terminal());
    }
}
