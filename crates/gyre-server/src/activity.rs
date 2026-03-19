use gyre_common::ActivityEventData;
use std::collections::VecDeque;
use std::sync::{Arc, RwLock};

const CAPACITY: usize = 1000;

/// In-memory ring buffer of activity events, shared across handlers.
#[derive(Clone)]
pub struct ActivityStore {
    events: Arc<RwLock<VecDeque<ActivityEventData>>>,
}

impl Default for ActivityStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivityStore {
    pub fn new() -> Self {
        Self {
            events: Arc::new(RwLock::new(VecDeque::with_capacity(CAPACITY))),
        }
    }

    /// Record an event, evicting the oldest if at capacity.
    pub fn record(&self, event: ActivityEventData) {
        let mut guard = self.events.write().unwrap();
        if guard.len() == CAPACITY {
            guard.pop_front();
        }
        guard.push_back(event);
    }

    /// Return events matching the optional filters.
    pub fn query(&self, since: Option<u64>, limit: Option<usize>) -> Vec<ActivityEventData> {
        let guard = self.events.read().unwrap();
        let limit = limit.unwrap_or(50).min(CAPACITY);
        guard
            .iter()
            .filter(|e| since.is_none_or(|ts| e.timestamp > ts))
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(id: &str, ts: u64) -> ActivityEventData {
        ActivityEventData {
            event_id: id.to_string(),
            agent_id: "agent1".to_string(),
            event_type: gyre_common::AgEventType::StateChanged,
            description: "test event".to_string(),
            timestamp: ts,
        }
    }

    #[test]
    fn record_and_query() {
        let store = ActivityStore::new();
        store.record(make_event("e1", 100));
        store.record(make_event("e2", 200));

        let events = store.query(None, None);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_id, "e1");
        assert_eq!(events[1].event_id, "e2");
    }

    #[test]
    fn query_with_since_filter() {
        let store = ActivityStore::new();
        store.record(make_event("e1", 100));
        store.record(make_event("e2", 200));
        store.record(make_event("e3", 300));

        let events = store.query(Some(150), None);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_id, "e2");
    }

    #[test]
    fn capacity_bound() {
        let store = ActivityStore::new();
        for i in 0u64..1001 {
            store.record(make_event(&format!("e{i}"), i));
        }
        let events = store.query(None, Some(CAPACITY + 10));
        assert_eq!(events.len(), CAPACITY);
        // First event should be e1 (e0 evicted)
        assert_eq!(events[0].event_id, "e1");
    }
}
