//! Data retention policies: define max age per data type and run cleanup.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use tracing::info;


#[derive(Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    pub data_type: String,
    pub max_age_days: u64,
}

/// Default retention policies.
pub fn default_policies() -> Vec<RetentionPolicy> {
    vec![
        RetentionPolicy {
            data_type: "activity_events".to_string(),
            max_age_days: 90,
        },
        RetentionPolicy {
            data_type: "analytics_events".to_string(),
            max_age_days: 365,
        },
        RetentionPolicy {
            data_type: "cost_entries".to_string(),
            max_age_days: 365,
        },
    ]
}

/// In-memory retention policy store.
#[derive(Clone)]
pub struct RetentionStore {
    policies: Arc<RwLock<Vec<RetentionPolicy>>>,
}

impl RetentionStore {
    pub fn new() -> Self {
        Self {
            policies: Arc::new(RwLock::new(default_policies())),
        }
    }

    pub fn list(&self) -> Vec<RetentionPolicy> {
        self.policies.read().unwrap().clone()
    }

    pub fn update(&self, new_policies: Vec<RetentionPolicy>) {
        *self.policies.write().unwrap() = new_policies;
    }

    pub fn set_policy(&self, data_type: &str, max_age_days: u64) {
        let mut policies = self.policies.write().unwrap();
        if let Some(p) = policies.iter_mut().find(|p| p.data_type == data_type) {
            p.max_age_days = max_age_days;
        } else {
            policies.push(RetentionPolicy {
                data_type: data_type.to_string(),
                max_age_days,
            });
        }
    }

    /// Run cleanup based on current policies.
    /// Note: activity_events are now stored in TelemetryBuffer (ring buffer with automatic
    /// eviction) — no explicit cleanup needed. Analytics/cost entries are not yet purged here.
    pub async fn run_cleanup(&self) {
        let policies = self.list();
        for policy in &policies {
            info!(
                data_type = %policy.data_type,
                max_age_days = policy.max_age_days,
                "retention policy checked (no-op for ring-buffer backed stores)"
            );
        }
    }
}

impl Default for RetentionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policies_present() {
        let store = RetentionStore::new();
        let policies = store.list();
        assert!(policies.iter().any(|p| p.data_type == "activity_events"));
        assert!(policies.iter().any(|p| p.data_type == "analytics_events"));
        assert!(policies.iter().any(|p| p.data_type == "cost_entries"));
    }

    #[test]
    fn update_policies() {
        let store = RetentionStore::new();
        store.update(vec![RetentionPolicy {
            data_type: "activity_events".to_string(),
            max_age_days: 30,
        }]);
        let policies = store.list();
        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].max_age_days, 30);
    }

    #[test]
    fn set_existing_policy() {
        let store = RetentionStore::new();
        store.set_policy("activity_events", 14);
        let policies = store.list();
        let activity = policies
            .iter()
            .find(|p| p.data_type == "activity_events")
            .unwrap();
        assert_eq!(activity.max_age_days, 14);
    }

    #[test]
    fn set_new_policy() {
        let store = RetentionStore::new();
        store.set_policy("custom_data", 7);
        let policies = store.list();
        assert!(policies
            .iter()
            .any(|p| p.data_type == "custom_data" && p.max_age_days == 7));
    }

    #[tokio::test]
    async fn cleanup_runs_without_error() {
        let store = RetentionStore::new();
        store.set_policy("activity_events", 0);
        // TelemetryBuffer is a ring buffer with automatic eviction; cleanup is a no-op.
        store.run_cleanup().await;
    }
}
