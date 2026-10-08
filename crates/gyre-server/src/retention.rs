//! Data retention policies: define max age per data type and run cleanup.
//!
//! Implements business-continuity.md §5: seven data types with spec defaults,
//! policies persisted via the KV store (survive restart), and a nightly
//! cleanup job that hard-deletes data past policy age.
//!
//! - `activity_events` — lives in `TelemetryBuffer`, a bounded ring buffer
//!   (per-workspace + per-server caps, gyre-common/src/message.rs). The
//!   ring caps bound steady-state volume; the 90-day age policy is enforced
//!   by `TelemetryBuffer::purge_older_than` driven from `run_cleanup`.
//! - `agent_logs` — in-memory `HashMap<agent, Vec<String>>` with canonical
//!   `[ts] message` lines (api/agent_logs.rs). Entries past the cutoff are
//!   drained. "Compress after 7 days" (spec) is a size optimization, not a
//!   retention requirement — not implemented; the 30-day purge is.
//! - `audit_events` / `analytics_events` — hard DELETE via the repository
//!   ports (`delete_older_than`), SQLite + Postgres adapters.
//! - `snapshots` — tiered file policy `24h×24 + 7d×7 + 4w×4` on
//!   `snapshot_dir()` files, oldest-first deletion beyond tier caps.
//! - `attestations` — never purged (`max_age_days: u64::MAX`); the
//!   attestation store has no delete path (non-repudiation).
//! - `notifications` — read (resolved or dismissed) older than
//!   `max_age_days_read` (default 90d), unread older than `max_age_days`
//!   (default 365d).

use anyhow::Result;
use gyre_ports::KvJsonStore;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::AppState;

/// KV namespace + key the policy list is persisted under.
const KV_NAMESPACE: &str = "retention";
const KV_KEY: &str = "policies";

/// Snapshot tier policy (business-continuity.md §5 + §1): keep at most 24
/// snapshots with mtime within the last 24h, 7 within the last 7 days, and 4
/// within the last 4 weeks; delete everything else. These are the spec
/// defaults — the live values live on the `snapshots` policy row.
pub const DEFAULT_SNAPSHOT_TIER_24H_KEEP: usize = 24;
pub const DEFAULT_SNAPSHOT_TIER_7D_KEEP: usize = 7;
pub const DEFAULT_SNAPSHOT_TIER_4W_KEEP: usize = 4;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct RetentionPolicy {
    pub data_type: String,
    pub max_age_days: u64,
    /// Read-notification cutoff in days. Only meaningful for `notifications`:
    /// read (resolved or dismissed) notifications are purged after this many
    /// days, unread ones after `max_age_days`. `None` elsewhere.
    #[serde(default)]
    pub max_age_days_read: Option<u64>,
    /// Snapshot tier counts. Only meaningful for `snapshots`: at most
    /// `keep_24h` snapshots younger than 24h, `keep_7d` younger than 7d,
    /// `keep_4w` younger than 4 weeks are kept (newest first); everything
    /// else is deleted. `None` elsewhere.
    #[serde(default)]
    pub snapshot_tiers: Option<SnapshotTiers>,
}

/// Tiered snapshot retention counts (business-continuity.md §5 "24h×24 +
/// 7d×7 + 4w×4"), configurable via the `snapshots` row of
/// `PUT /api/v1/admin/retention`.
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct SnapshotTiers {
    pub keep_24h: usize,
    pub keep_7d: usize,
    pub keep_4w: usize,
}

impl SnapshotTiers {
    /// The spec defaults: 24 hourly + 7 daily + 4 weekly.
    pub const fn spec_default() -> Self {
        Self {
            keep_24h: DEFAULT_SNAPSHOT_TIER_24H_KEEP,
            keep_7d: DEFAULT_SNAPSHOT_TIER_7D_KEEP,
            keep_4w: DEFAULT_SNAPSHOT_TIER_4W_KEEP,
        }
    }
}

/// Default retention policies — the seven data types from
/// business-continuity.md §5.
pub fn default_policies() -> Vec<RetentionPolicy> {
    vec![
        RetentionPolicy {
            data_type: "activity_events".to_string(),
            max_age_days: 90,
            max_age_days_read: None,
            snapshot_tiers: None,
        },
        RetentionPolicy {
            data_type: "agent_logs".to_string(),
            max_age_days: 30,
            max_age_days_read: None,
            snapshot_tiers: None,
        },
        RetentionPolicy {
            data_type: "audit_events".to_string(),
            max_age_days: 365,
            max_age_days_read: None,
            snapshot_tiers: None,
        },
        // Enforced structurally by the tiered snapshot policy, not by age.
        RetentionPolicy {
            data_type: "snapshots".to_string(),
            max_age_days: u64::MAX,
            max_age_days_read: None,
            snapshot_tiers: Some(SnapshotTiers::spec_default()),
        },
        // Never purged (non-repudiation).
        RetentionPolicy {
            data_type: "attestations".to_string(),
            max_age_days: u64::MAX,
            max_age_days_read: None,
            snapshot_tiers: None,
        },
        RetentionPolicy {
            data_type: "notifications".to_string(),
            max_age_days: 365,
            max_age_days_read: Some(90),
            snapshot_tiers: None,
        },
        RetentionPolicy {
            data_type: "analytics_events".to_string(),
            max_age_days: 365,
            max_age_days_read: None,
            snapshot_tiers: None,
        },
    ]
}

/// Retention policy store, optionally backed by a KV store.
///
/// `new()` seeds the spec defaults in memory; `init()` loads previously
/// persisted policies from the KV store (or persists defaults on first boot).
/// `update`/`set_policy` persist best-effort so admin edits survive restart.
#[derive(Clone)]
pub struct RetentionStore {
    policies: Arc<RwLock<Vec<RetentionPolicy>>>,
    /// Placeholder (`MemKvStore`) until `init` installs the real store.
    kv: Arc<RwLock<Arc<dyn KvJsonStore>>>,
}

impl RetentionStore {
    pub fn new() -> Self {
        Self {
            policies: Arc::new(RwLock::new(default_policies())),
            kv: Arc::new(RwLock::new(Arc::new(crate::mem::MemKvStore::default()) as Arc<dyn KvJsonStore>)),
        }
    }

    /// Load policies from the KV store, or persist the spec defaults on
    /// first boot. Called once from main after `build_state`.
    pub async fn init(&self, kv: Arc<dyn KvJsonStore>) {
        match kv.kv_get(KV_NAMESPACE, KV_KEY).await {
            Ok(Some(json)) => {
                if let Ok(policies) = serde_json::from_str::<Vec<RetentionPolicy>>(&json) {
                    if !policies.is_empty() {
                        *self.policies.write() = policies;
                    }
                }
            }
            _ => {}
        }
        *self.kv.write() = kv;
        // First boot or empty/corrupt blob: persist whatever we now hold
        // (loaded policies or defaults).
        self.persist().await;
    }

    async fn persist(&self) {
        let json = match serde_json::to_string(&*self.policies.read()) {
            Ok(json) => json,
            Err(_) => return, // RetentionPolicy is plain data; cannot fail
        };
        let kv = Arc::clone(&self.kv.read());
        if let Err(e) = kv.kv_set(KV_NAMESPACE, KV_KEY, json).await {
            tracing::warn!(error = %e, "failed to persist retention policies");
        }
    }

    pub fn list(&self) -> Vec<RetentionPolicy> {
        self.policies.read().clone()
    }

    /// Replace all policies. The admin PUT handler calls
    /// [`validate_policies`] before this — the store itself does not
    /// re-validate, so any other caller must too.
    pub fn update(&self, new_policies: Vec<RetentionPolicy>) {
        *self.policies.write() = new_policies;
        self.persist_best_effort();
    }

    pub fn set_policy(&self, data_type: &str, max_age_days: u64) {
        let mut policies = self.policies.write();
        if let Some(p) = policies.iter_mut().find(|p| p.data_type == data_type) {
            p.max_age_days = max_age_days;
        } else {
            policies.push(RetentionPolicy {
                data_type: data_type.to_string(),
                max_age_days,
                max_age_days_read: None,
                snapshot_tiers: None,
            });
        }
        drop(policies);
        self.persist_best_effort();
    }

    /// Fire-and-forget persistence for the sync mutation paths.
    /// `update`/`set_policy` keep their sync signatures (admin handlers);
    /// the KV write is spawned onto the current runtime, if any. Outside a
    /// runtime (plain unit tests) the write is skipped — the in-memory
    /// policies stay authoritative for the process lifetime anyway.
    fn persist_best_effort(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let policies = self.policies.read().clone();
        let kv = Arc::clone(&self.kv.read());
        tokio::spawn(async move {
            if let Ok(json) = serde_json::to_string(&policies) {
                if let Err(e) = kv.kv_set(KV_NAMESPACE, KV_KEY, json).await {
                    tracing::warn!(error = %e, "failed to persist retention policies");
                }
            }
        });
    }
}

impl RetentionStore {
    /// Run retention cleanup against the live stores in `state`.
    ///
    /// Idempotent: every purge is a "delete rows/files older than cutoff"
    /// operation, so a second consecutive run deletes nothing new.
    pub async fn run_cleanup(&self, state: &AppState) -> Result<()> {
        let now = now_secs();
        for policy in self.list() {
            // u64::MAX means "never purge" (attestations; snapshots are
            // tier-enforced instead of age-enforced below).
            let max_age_secs = policy.max_age_days.saturating_mul(86_400);
            let cutoff = now.saturating_sub(max_age_secs);

            let purged = match policy.data_type.as_str() {
                "audit_events" => state.audit.delete_older_than(cutoff).await?,
                "analytics_events" => state.analytics.delete_older_than(cutoff).await?,
                "notifications" => {
                    let read_cutoff = match policy.max_age_days_read {
                        Some(days) => {
                            let s = days.saturating_mul(86_400);
                            now.saturating_sub(s)
                        }
                        None => cutoff,
                    };
                    state
                        .notifications
                        .delete_older_than(read_cutoff, cutoff)
                        .await?
                }
                "agent_logs" => purge_agent_logs(state, cutoff).await,
                "snapshots" => {
                    // Tier counts come from the policy row (configurable
                    // via PUT /admin/retention); u64::MAX spec default
                    // never triggers the age path.
                    let tiers = policy
                        .snapshot_tiers
                        .unwrap_or_else(SnapshotTiers::spec_default);
                    purge_snapshots(tiers).await
                }
                "attestations" => {
                    // Never purged (non-repudiation, business-continuity §5).
                    // The AttestationRepository port has no delete path.
                    info!(data_type = "attestations", "retention: never purge");
                    0
                }
                "activity_events" => {
                    // Activity events live in the TelemetryBuffer ring
                    // (per-workspace + per-server caps). The buffer is
                    // count-bounded, not age-bounded, so the 90-day policy
                    // is enforced by an explicit age eviction sweep here.
                    // Message.created_at is epoch milliseconds; cutoff is
                    // seconds.
                    state.telemetry_buffer.purge_older_than(cutoff * 1000) as u64
                }
                _ => 0,
            };
            if purged > 0 || policy.max_age_days != u64::MAX {
                info!(
                    data_type = %policy.data_type,
                    purged,
                    "retention cleanup applied"
                );
            }
        }
        Ok(())
    }
}

/// The seven data types business-continuity.md §5 defines retention for.
/// `PUT /api/v1/admin/retention` must cover exactly these — a policy list
/// that omits a type silently disables enforcement for it.
pub const RETENTION_DATA_TYPES: [&str; 7] = [
    "activity_events",
    "agent_logs",
    "audit_events",
    "snapshots",
    "attestations",
    "notifications",
    "analytics_events",
];

/// Validate a full replacement policy list for `PUT /api/v1/admin/retention`.
///
/// Rules (spec §5 "configurable" presumes a valid, complete policy):
/// - exactly the 7 spec data types, each exactly once;
/// - `max_age_days >= 1` for age-enforced types (0 would purge everything
///   on the next nightly run);
/// - `notifications` must set `max_age_days_read` (the 90d-read/365d-unread
///   split is spec-mandatory) and it must be `<= max_age_days`;
/// - `snapshots` must set tier counts, each >= 1.
/// - `attestations` must keep `max_age_days == u64::MAX` (never purge is
///   spec-mandated for non-repudiation).
pub fn validate_policies(policies: &[RetentionPolicy]) -> Result<(), String> {
    use std::collections::HashSet;

    let mut seen: HashSet<&str> = HashSet::new();
    for p in policies {
        if !RETENTION_DATA_TYPES.contains(&p.data_type.as_str()) {
            return Err(format!(
                "unknown data_type '{}' (expected one of: {})",
                p.data_type,
                RETENTION_DATA_TYPES.join(", ")
            ));
        }
        if !seen.insert(&p.data_type) {
            return Err(format!("duplicate data_type '{}'", p.data_type));
        }
        match p.data_type.as_str() {
            "attestations" => {
                if p.max_age_days != u64::MAX {
                    return Err(
                        "attestations retention is 'Forever' (non-repudiation); max_age_days \
                         must be u64::MAX"
                            .to_string(),
                    );
                }
            }
            "snapshots" => {
                let tiers = p.snapshot_tiers.as_ref().ok_or_else(|| {
                    "snapshots policy must set snapshot_tiers (keep_24h, keep_7d, keep_4w)"
                        .to_string()
                })?;
                if tiers.keep_24h == 0 || tiers.keep_7d == 0 || tiers.keep_4w == 0 {
                    return Err(
                        "snapshot tier counts must each be >= 1 (0 would delete all snapshots)"
                            .to_string(),
                    );
                }
            }
            "notifications" => {
                if p.max_age_days < 1 {
                    return Err("notifications max_age_days must be >= 1".to_string());
                }
                let read_days = p.max_age_days_read.ok_or_else(|| {
                    "notifications policy must set max_age_days_read (read/unread split)"
                        .to_string()
                })?;
                if read_days < 1 {
                    return Err("notifications max_age_days_read must be >= 1".to_string());
                }
                if read_days > p.max_age_days {
                    return Err(
                        "notifications max_age_days_read must be <= max_age_days (read \
                         notifications are purged sooner than unread)"
                            .to_string(),
                    );
                }
            }
            _ => {
                if p.max_age_days < 1 {
                    return Err(format!("{} max_age_days must be >= 1", p.data_type));
                }
            }
        }
    }

    let missing: Vec<&str> = RETENTION_DATA_TYPES
        .iter()
        .filter(|t| !seen.contains(**t))
        .copied()
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "missing required data_type(s): {} (a policy list that omits a type \
             silently disables enforcement for it)",
            missing.join(", ")
        ));
    }
    Ok(())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Drain agent-log lines whose `[ts]` prefix is older than `cutoff`.
/// Lines without a parseable prefix are kept (conservative — cannot prove
/// age, do not delete). Returns lines purged.
async fn purge_agent_logs(state: &AppState, cutoff: u64) -> u64 {
    let mut logs = state.agent_logs.lock().await;
    let mut purged = 0u64;
    for buf in logs.values_mut() {
        let before = buf.len();
        buf.retain(|line| !agent_log_line_is_old(line, cutoff));
        purged += (before - buf.len()) as u64;
    }
    purged
}

/// A line is old when it starts with `[ts]` and ts < cutoff.
fn agent_log_line_is_old(line: &str, cutoff: u64) -> bool {
    let Some(ts) = line
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .and_then(|(ts, _)| ts.trim().parse::<u64>().ok())
    else {
        return false;
    };
    ts < cutoff
}

/// Apply the tiered snapshot policy to the snapshot directory. Tier counts
/// come from the `snapshots` retention policy row. Non-snapshot files are
/// left alone. Returns files deleted.
async fn purge_snapshots_in(dir: std::path::PathBuf, tiers: SnapshotTiers) -> u64 {
    let mut files: Vec<(String, u64)> = Vec::new();
    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(_) => return 0, // no snapshot dir yet — nothing to purge
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        let Ok(mtime) = meta.modified() else {
            continue;
        };
        let mtime_secs = mtime
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        files.push((name.to_string(), mtime_secs));
    }

    let now = now_secs();
    let victims = snapshot_tier_deletions(now, &files, tiers);
    for name in &victims {
        if let Err(e) = tokio::fs::remove_file(dir.join(name)).await {
            tracing::warn!(file = %name, error = %e, "failed to delete snapshot");
        }
    }
    victims.len() as u64
}

/// Apply the tiered snapshot policy to the configured snapshot directory.
async fn purge_snapshots(tiers: SnapshotTiers) -> u64 {
    purge_snapshots_in(crate::snapshot::snapshot_dir(), tiers).await
}

/// Pure tiered-policy decision: given `now`, snapshot files `(name, mtime)`,
/// and tier counts, return the file names to delete. Keep at most
/// `tiers.keep_24h` snapshots with mtime in the last 24h, at most
/// `tiers.keep_7d` within the last 7 days, at most `tiers.keep_4w` within
/// the last 4 weeks; delete the rest, oldest first.
fn snapshot_tier_deletions(
    now_secs: u64,
    files: &[(String, u64)],
    tiers: SnapshotTiers,
) -> Vec<String> {
    // Sort newest-first; within a tier, the newest entries are kept.
    let mut sorted: Vec<&(String, u64)> = files.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1)); // newest first

    let mut deletions = Vec::new();
    let mut kept_24h = 0usize;
    let mut kept_7d = 0usize;
    let mut kept_4w = 0usize;
    let day = 86_400u64;

    for (name, mtime) in sorted {
        let age = now_secs.saturating_sub(*mtime);
        if age <= day && kept_24h < tiers.keep_24h {
            kept_24h += 1;
        } else if age <= 7 * day && kept_7d < tiers.keep_7d {
            kept_7d += 1;
        } else if age <= 28 * day && kept_4w < tiers.keep_4w {
            kept_4w += 1;
        } else {
            deletions.push(name.clone());
        }
    }
    deletions
}

#[cfg(test)]
mod tests {
    use crate::jobs::next_daily_run_secs;
    use super::*;
    use crate::mem::test_state;
    use gyre_common::{Id, Notification, NotificationType};
    use gyre_domain::{AnalyticsEvent, AttestationBundle, AuditEvent, AuditEventType, MergeAttestation};
    use std::sync::Arc;

    const DAY: u64 = 86_400;

    fn now() -> u64 {
        now_secs()
    }

    #[test]
    fn default_policies_present() {
        let policies = default_policies();
        assert_eq!(policies.len(), 7);
        let types: Vec<&str> = policies.iter().map(|p| p.data_type.as_str()).collect();
        for expected in [
            "activity_events",
            "agent_logs",
            "audit_events",
            "snapshots",
            "attestations",
            "notifications",
            "analytics_events",
        ] {
            assert!(types.contains(&expected), "missing policy {expected}");
        }
        // No cost_entries policy — spec lists exactly these 7 types.
        assert!(!types.contains(&"cost_entries"));
        // Notifications: 365d unread / 90d read split (§5).
        let notif = policies.iter().find(|p| p.data_type == "notifications").unwrap();
        assert_eq!(notif.max_age_days, 365);
        assert_eq!(notif.max_age_days_read, Some(90));
        // Attestations and snapshots are never age-purged.
        assert_eq!(
            policies.iter().find(|p| p.data_type == "attestations").unwrap().max_age_days,
            u64::MAX
        );
        // Snapshots carry the spec tier defaults 24h×24 + 7d×7 + 4w×4.
        assert_eq!(
            policies
                .iter()
                .find(|p| p.data_type == "snapshots")
                .unwrap()
                .snapshot_tiers,
            Some(SnapshotTiers::spec_default())
        );
        assert_eq!(SnapshotTiers::spec_default().keep_24h, 24);
        assert_eq!(SnapshotTiers::spec_default().keep_7d, 7);
        assert_eq!(SnapshotTiers::spec_default().keep_4w, 4);
    }

    #[test]
    fn update_policies() {
        let store = RetentionStore::new();
        let mut policies = default_policies();
        policies[0].max_age_days = 10;
        store.update(policies);
        assert_eq!(store.list()[0].max_age_days, 10);
    }

    #[test]
    fn set_existing_policy() {
        let store = RetentionStore::new();
        store.set_policy("audit_events", 30);
        let p = store.list().into_iter().find(|p| p.data_type == "audit_events").unwrap();
        assert_eq!(p.max_age_days, 30);
    }

    #[test]
    fn set_new_policy() {
        let store = RetentionStore::new();
        store.set_policy("custom_type", 7);
        let p = store.list().into_iter().find(|p| p.data_type == "custom_type").unwrap();
        assert_eq!(p.max_age_days, 7);
    }

    #[tokio::test]
    async fn policies_persist_across_store_reinit() {
        let kv: Arc<dyn KvJsonStore> = Arc::new(crate::mem::MemKvStore::default());
        let store = RetentionStore::new();
        store.init(Arc::clone(&kv)).await;
        store.set_policy("audit_events", 42);
        // Give the spawned persist task a chance to run.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // A second store booting against the same KV must load 42, not the default.
        let store2 = RetentionStore::new();
        store2.init(kv).await;
        let p = store2
            .list()
            .into_iter()
            .find(|p| p.data_type == "audit_events")
            .unwrap();
        assert_eq!(p.max_age_days, 42);
    }

    fn audit_event(id: &str, ts: u64) -> AuditEvent {
        AuditEvent::new(
            Id::new(id),
            AuditEventType::FileAccess,
            Some(Id::new("agent-1")),
            None,
            None,
            None,
            None,
            "agent".to_string(),
            Some("agent-1".to_string()),
            gyre_domain::AuditOutcome::Success,
            serde_json::json!({ "path": "/tmp/x", "pid": 123 }),
            None,
            None,
            ts,
        )
    }

    fn analytics_event(id: &str, ts: u64) -> AnalyticsEvent {
        AnalyticsEvent::new(
            Id::new(id),
            "test_event",
            Some("agent-1".to_string()),
            serde_json::json!({}),
            ts,
        )
    }

    fn notification(id: &str, created_at: i64, read: bool) -> Notification {
        let mut n = Notification::new(
            Id::new(id),
            Id::new("ws-1"),
            Id::new("user-1"),
            NotificationType::GateFailure,
            "title",
            "tenant-1",
            created_at,
        );
        if read {
            n.resolved_at = Some(created_at + 10);
        }
        n
    }

    /// Seed every in-memory store with old (beyond policy) and new (within
    /// policy) data plus an ancient attestation that must survive.
    async fn seed_state(state: &AppState) {
        let t = now();
        // audit: 365d policy — purge older than 365d.
        gyre_ports::AuditRepository::record(&*state.audit, &audit_event("a-old", t - 400 * DAY))
            .await
            .unwrap();
        gyre_ports::AuditRepository::record(&*state.audit, &audit_event("a-new", t - 100 * DAY))
            .await
            .unwrap();
        // analytics: 365d policy.
        gyre_ports::AnalyticsRepository::record(
            &*state.analytics,
            &analytics_event("an-old", t - 400 * DAY),
        )
        .await
        .unwrap();
        gyre_ports::AnalyticsRepository::record(
            &*state.analytics,
            &analytics_event("an-new", t - 100 * DAY),
        )
        .await
        .unwrap();
        // notifications: read older than 90d purged; unread older than 365d
        // purged; read-but-recent and unread-recent kept.
        gyre_ports::NotificationRepository::create(
            &*state.notifications,
            &notification("n-old-read", (t - 100 * DAY) as i64, true),
        )
        .await
        .unwrap();
        gyre_ports::NotificationRepository::create(
            &*state.notifications,
            &notification("n-old-unread", (t - 400 * DAY) as i64, false),
        )
        .await
        .unwrap();
        gyre_ports::NotificationRepository::create(
            &*state.notifications,
            &notification("n-new-read", (t - 10 * DAY) as i64, true),
        )
        .await
        .unwrap();
        gyre_ports::NotificationRepository::create(
            &*state.notifications,
            &notification("n-new-unread", (t - 10 * DAY) as i64, false),
        )
        .await
        .unwrap();
        // agent_logs: 30d policy — one old line, one new line, one unprefixed.
        state.agent_logs.lock().await.insert(
            "agent-1".to_string(),
            vec![
                format!("[{}] old line", t - 40 * DAY),
                format!("[{}] new line", t - DAY),
                "no prefix line".to_string(),
            ],
        );
        // attestation: ancient, must never be purged.
        let bundle = AttestationBundle {
            attestation: MergeAttestation {
                attestation_version: 1,
                mr_id: "mr-ancient".to_string(),
                merge_commit_sha: "deadbeef".to_string(),
                merged_at: t - 10_000 * DAY,
                gate_results: vec![],
                spec_ref: None,
                spec_fully_approved: false,
                author_agent_id: None,
                conversation_sha: None,
                completion_summary: None,
                meta_specs_used: vec![],
            },
            signature: "sig".to_string(),
            signing_key_id: "kid".to_string(),
            deprecation_notice: None,
        };
        state.attestation_store.save("mr-ancient", &bundle).await.unwrap();
    }

    #[tokio::test]
    async fn run_cleanup_purges_old_and_keeps_new() {
        let state = test_state();
        seed_state(&state).await;

        state.retention_store.run_cleanup(&state).await.unwrap();

        // audit: old gone, new kept.
        let audit = gyre_ports::AuditRepository::query(&*state.audit, &gyre_ports::AuditQueryFilter { limit: 100, ..Default::default() })
            .await
            .unwrap();
        let ids: Vec<&str> = audit.iter().map(|e| e.id.as_str()).collect();
        assert!(!ids.contains(&"a-old"), "old audit event must be purged");
        assert!(ids.contains(&"a-new"), "new audit event must be kept");

        // analytics: old gone, new kept.
        let analytics =
            gyre_ports::AnalyticsRepository::query(&*state.analytics, None, None, 100)
                .await
                .unwrap();
        let ids: Vec<&str> = analytics.iter().map(|e| e.id.as_str()).collect();
        assert!(!ids.contains(&"an-old"), "old analytics event must be purged");
        assert!(ids.contains(&"an-new"), "new analytics event must be kept");

        // notifications: old-read and old-unread purged; both new kept.
        let notifs = gyre_ports::NotificationRepository::list_recent(&*state.notifications, 100)
            .await
            .unwrap();
        let ids: Vec<&str> = notifs.iter().map(|n| n.id.as_str()).collect();
        assert!(!ids.contains(&"n-old-read"), "old read notification must be purged");
        assert!(!ids.contains(&"n-old-unread"), "old unread notification must be purged");
        assert!(ids.contains(&"n-new-read"), "recent read notification must be kept");
        assert!(ids.contains(&"n-new-unread"), "recent unread notification must be kept");

        // agent_logs: old line drained; new line and unprefixed line kept.
        let logs = state.agent_logs.lock().await;
        let lines = logs.get("agent-1").unwrap();
        assert_eq!(lines.len(), 2, "old log line must be purged, others kept: {lines:?}");
        assert!(lines.iter().any(|l| l.contains("new line")));
        assert!(lines.iter().any(|l| l.contains("no prefix line")));
        drop(logs);

        // attestation: ancient bundle survives (never purged).
        assert!(
            state.attestation_store.find_by_mr_id("mr-ancient").await.unwrap().is_some(),
            "attestations must never be purged"
        );
    }

    #[tokio::test]
    async fn run_cleanup_is_idempotent() {
        let state = test_state();
        seed_state(&state).await;

        state.retention_store.run_cleanup(&state).await.unwrap();
        let after_first = gyre_ports::NotificationRepository::list_recent(&*state.notifications, 100)
            .await
            .unwrap()
            .len();
        let audit_first = gyre_ports::AuditRepository::query(&*state.audit, &gyre_ports::AuditQueryFilter { limit: 100, ..Default::default() })
            .await
            .unwrap()
            .len();

        // Second consecutive run must not delete anything new.
        state.retention_store.run_cleanup(&state).await.unwrap();
        let after_second = gyre_ports::NotificationRepository::list_recent(&*state.notifications, 100)
            .await
            .unwrap()
            .len();
        let audit_second = gyre_ports::AuditRepository::query(&*state.audit, &gyre_ports::AuditQueryFilter { limit: 100, ..Default::default() })
            .await
            .unwrap()
            .len();
        assert_eq!(after_first, after_second);
        assert_eq!(audit_first, audit_second);
    }

    #[test]
    fn agent_log_line_is_old_boundaries() {
        let cutoff = 1000;
        assert!(agent_log_line_is_old("[999] x", cutoff));
        assert!(!agent_log_line_is_old("[1000] x", cutoff), "cutoff is exclusive");
        assert!(!agent_log_line_is_old("[1001] x", cutoff));
        assert!(!agent_log_line_is_old("no prefix", cutoff), "unparseable prefix kept");
        assert!(!agent_log_line_is_old("[abc] x", cutoff), "non-numeric ts kept");
        assert!(!agent_log_line_is_old("", cutoff));
    }

    #[test]
    fn snapshot_tiering_keeps_24h_24_7d_7_4w_4() {
        let now = 1_000 * DAY;
        let mut files = Vec::new();
        // 30 files aged 0..29 hours. The newest 24 fill the 24h tier; the 6
        // oldest (24h–29h) spill toward the 7d tier.
        for i in 0..30 {
            files.push((format!("recent-{i}"), now - i * 3_600));
        }
        // 4 files aged 3–6 days.
        for i in 3..7 {
            files.push((format!("week-{i}"), now - i * DAY));
        }
        // 6 files aged 15,17,19,21,23,25 days (all within the 4w tier).
        for i in 0..6 {
            files.push((format!("month-{i}"), now - (15 + 2 * i) * DAY));
        }
        // 3 files older than 4 weeks — always deleted.
        for i in 0..3 {
            files.push((format!("ancient-{i}"), now - (40 + i) * DAY));
        }

        // Expected survivors (35 = 24 + 7 + 4):
        // - 24h tier (24): recent-0..recent-23
        // - 7d tier (7): recent-24..recent-29 (the hour-aged spillover) + week-3
        // - 4w tier (4): week-4, week-5, week-6, month-0 (newest first)
        // Deleted (8): month-1..month-5, ancient-0..2
        let deletions = snapshot_tier_deletions(now, &files, SnapshotTiers::spec_default());
        assert_eq!(deletions.len(), 8, "deleted: {deletions:?}");
        for name in ["month-1", "month-2", "month-3", "month-4", "month-5"] {
            assert!(deletions.contains(&name.to_string()), "{name} should be deleted");
        }
        for i in 0..3 {
            assert!(deletions.contains(&format!("ancient-{i}")));
        }
        for i in 0..24 {
            assert!(!deletions.contains(&format!("recent-{i}")), "recent-{i} must survive");
        }
        for name in ["week-3", "week-4", "week-5", "week-6", "month-0"] {
            assert!(!deletions.contains(&name.to_string()), "{name} must survive");
        }
    }

    #[test]
    fn snapshot_tiering_empty_and_small_sets() {
        assert!(snapshot_tier_deletions(0, &[], SnapshotTiers::spec_default()).is_empty());
        // A handful of recent files — all kept.
        let files: Vec<(String, u64)> = (0..5).map(|i| (format!("f{i}"), 100 - i)).collect();
        assert!(
            snapshot_tier_deletions(100, &files, SnapshotTiers::spec_default()).is_empty()
        );
    }

    #[test]
    fn snapshot_tiering_honors_configured_counts() {
        // A custom (non-default) tier set must change the decision —
        // this is the pure-function half of F2 (configurable retention).
        let now = 1_000 * DAY;
        let mut files = Vec::new();
        for i in 0..10 {
            files.push((format!("recent-{i}"), now - i * 3_600));
        }
        // keep_24h = 2: only the 2 newest hour-aged files survive.
        let tiers = SnapshotTiers { keep_24h: 2, keep_7d: 7, keep_4w: 4 };
        let deletions = snapshot_tier_deletions(now, &files, tiers);
        assert_eq!(deletions.len(), 8, "deleted: {deletions:?}");
        assert!(!deletions.contains(&"recent-0".to_string()));
        assert!(!deletions.contains(&"recent-1".to_string()));
        assert!(deletions.contains(&"recent-2".to_string()));
        assert!(deletions.contains(&"recent-9".to_string()));
    }

    #[test]
    fn next_daily_run_secs_matches_wall_clock() {
        let day = 86_400u64;
        // 01:00 UTC, job at 02:00 → 1h away.
        assert_eq!(next_daily_run_secs(day + 3_600, 2), 3_600);
        // 03:00 UTC, job at 02:00 → next day's 02:00, 23h away.
        assert_eq!(next_daily_run_secs(day + 3 * 3_600, 2), 23 * 3_600);
        // Exactly 02:00 → next run is a full day away.
        assert_eq!(next_daily_run_secs(day + 2 * 3_600, 2), day);
        // 23:59 UTC, job at 00:00 → 1 minute away.
        assert_eq!(next_daily_run_secs(2 * day - 60, 0), 60);
    }

    // ── F3: disk-level snapshot purge (real tempdir, real mtimes) ────────

    /// Create a file with a specific mtime (seconds since epoch) in `dir`.
    fn make_file(dir: &std::path::Path, name: &str, mtime_secs: u64) {
        let path = dir.join(name);
        std::fs::write(&path, b"{}").unwrap();
        let mtime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(mtime_secs);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }

    fn remaining_names(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[tokio::test]
    async fn purge_snapshots_on_disk_both_directions() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let now = now_secs();

        // Real snapshot names as create_snapshot writes them (<unix_secs>.json),
        // plus a non-.json file that must survive.
        for i in 0..30u64 {
            // 30 files aged 0..29 hours: newest 24 fill the 24h tier, 6 spill.
            make_file(&dir, &format!("{}.json", now - i * 3_600), now - i * 3_600);
        }
        for i in 3..7u64 {
            make_file(&dir, &format!("{}.json", now - i * DAY), now - i * DAY);
        }
        for i in 0..6u64 {
            make_file(&dir, &format!("{}.json", now - (15 + 2 * i) * DAY), now - (15 + 2 * i) * DAY);
        }
        for i in 0..3u64 {
            make_file(&dir, &format!("{}.json", now - (40 + i) * DAY), now - (40 + i) * DAY);
        }
        make_file(&dir, "README.txt", now - 99 * DAY);

        let purged = purge_snapshots_in(dir.clone(), SnapshotTiers::spec_default()).await;
        assert_eq!(purged, 8, "month-1..5 + ancient-0..2 deleted");

        let left = remaining_names(&dir);
        // 35 survivors = 24 + 7 + 4 snapshot tiers + README.txt.
        assert_eq!(left.len(), 36, "survivors: {left:?}");
        assert!(left.contains(&"README.txt".to_string()), "non-json files survive");
        for i in [1u64, 2, 3, 4, 5] {
            assert!(
                !left.contains(&format!("{}.json", now - (15 + 2 * i) * DAY)),
                "month-{i} must be deleted"
            );
        }
        for i in 0..3u64 {
            assert!(
                !left.contains(&format!("{}.json", now - (40 + i) * DAY)),
                "ancient-{i} must be deleted"
            );
        }
        for i in 0..24u64 {
            assert!(left.contains(&format!("{}.json", now - i * 3_600)));
        }
        for i in [3u64, 4, 5, 6] {
            assert!(left.contains(&format!("{}.json", now - i * DAY)));
        }

        // Idempotent on disk: a second run deletes nothing new.
        let purged_again = purge_snapshots_in(dir.clone(), SnapshotTiers::spec_default()).await;
        assert_eq!(purged_again, 0);
        assert_eq!(remaining_names(&dir).len(), 36);
    }

    #[tokio::test]
    async fn purge_snapshots_on_disk_missing_dir_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist");
        assert_eq!(purge_snapshots_in(missing, SnapshotTiers::spec_default()).await, 0);
    }

    #[tokio::test]
    async fn purge_snapshots_on_disk_honors_configured_tiers() {
        // F2 end-to-end: a custom tier set on the policy row changes the
        // real on-disk outcome.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let now = now_secs();
        for i in 0..10u64 {
            make_file(&dir, &format!("{}.json", now - i * 3_600), now - i * 3_600);
        }
        let purged = purge_snapshots_in(
            dir.clone(),
            SnapshotTiers { keep_24h: 2, keep_7d: 7, keep_4w: 4 },
        )
        .await;
        assert_eq!(purged, 8);
        assert_eq!(remaining_names(&dir).len(), 2);
    }

    // ── F1: activity events purged through run_cleanup ──────────────────

    #[tokio::test]
    async fn run_cleanup_purges_old_activity_events_from_telemetry_buffer() {
        use gyre_common::message::{Destination, Message, MessageOrigin};

        let state = test_state();
        let ws = Id::new("ws-act");
        let t_ms = now() * 1000;
        let old = t_ms - 91 * DAY * 1000; // 91 days old, policy is 90
        let new = t_ms - 10 * DAY * 1000;
        for (id, ts) in [("act-old", old), ("act-new", new)] {
            state.telemetry_buffer.push(Message {
                id: Id::new(id),
                tenant_id: Id::new("default"),
                from: MessageOrigin::Server,
                workspace_id: Some(ws.clone()),
                to: Destination::Workspace(ws.clone()),
                kind: gyre_common::MessageKind::ToolCallStart,
                payload: None,
                created_at: ts,
                signature: None,
                key_id: None,
                acknowledged: false,
            });
        }
        assert_eq!(state.telemetry_buffer.list_since(&ws, 0, 100).len(), 2);

        state.retention_store.run_cleanup(&state).await.unwrap();

        // Both directions: old evicted, new kept.
        let left = state.telemetry_buffer.list_since(&ws, 0, 100);
        assert_eq!(left.len(), 1, "old activity event must be purged");
        assert_eq!(left[0].id, Id::new("act-new"));

        // Idempotent.
        state.retention_store.run_cleanup(&state).await.unwrap();
        assert_eq!(state.telemetry_buffer.list_since(&ws, 0, 100).len(), 1);
    }

    #[tokio::test]
    async fn run_cleanup_honors_configured_activity_policy() {
        use gyre_common::message::{Destination, Message, MessageOrigin};

        // The 90d default is not hardcoded into the purge path: a PUT-shaped
        // policy change to 10 days must purge a 15-day-old event.
        let state = test_state();
        let mut policies = default_policies();
        for p in &mut policies {
            if p.data_type == "activity_events" {
                p.max_age_days = 10;
            }
        }
        state.retention_store.update(policies);

        let ws = Id::new("ws-cfg");
        let t_ms = now() * 1000;
        let event = Message {
            id: Id::new("act-15d"),
            tenant_id: Id::new("default"),
            from: MessageOrigin::Server,
            workspace_id: Some(ws.clone()),
            to: Destination::Workspace(ws.clone()),
            kind: gyre_common::MessageKind::ToolCallStart,
            payload: None,
            created_at: t_ms - 15 * DAY * 1000,
            signature: None,
            key_id: None,
            acknowledged: false,
        };
        state.telemetry_buffer.push(event);

        state.retention_store.run_cleanup(&state).await.unwrap();
        assert_eq!(
            state.telemetry_buffer.list_since(&ws, 0, 100).len(),
            0,
            "15-day-old event must be purged under a 10-day policy"
        );
    }

    // ── F4: PUT validation ───────────────────────────────────────────────

    #[test]
    fn validate_policies_accepts_defaults() {
        assert_eq!(validate_policies(&default_policies()), Ok(()));
    }

    #[test]
    fn validate_policies_rejects_missing_and_unknown_types() {
        // (a) partial list — omission silently disables enforcement.
        let mut partial = default_policies();
        partial.truncate(1);
        let err = validate_policies(&partial).unwrap_err();
        assert!(err.contains("missing required data_type"), "{err}");

        // (b) unknown type.
        let mut unknown = default_policies();
        unknown[0].data_type = "activity".to_string();
        let err = validate_policies(&unknown).unwrap_err();
        assert!(err.contains("unknown data_type"), "{err}");

        // (c) duplicate type.
        let mut dup = default_policies();
        dup[1].data_type = dup[0].data_type.clone();
        let err = validate_policies(&dup).unwrap_err();
        assert!(err.contains("duplicate data_type"), "{err}");
    }

    #[test]
    fn validate_policies_rejects_degenerate_values() {
        // max_age_days = 0 would purge everything on the next nightly run.
        let mut zero = default_policies();
        zero[0].max_age_days = 0;
        let err = validate_policies(&zero).unwrap_err();
        assert!(err.contains("max_age_days must be >= 1"), "{err}");

        // attestations must stay forever.
        let mut not_forever = default_policies();
        if let Some(p) = not_forever.iter_mut().find(|p| p.data_type == "attestations") {
            p.max_age_days = 365;
        }
        let err = validate_policies(&not_forever).unwrap_err();
        assert!(err.contains("Forever"), "{err}");

        // notifications must keep the read/unread split.
        let mut no_read = default_policies();
        if let Some(p) = no_read.iter_mut().find(|p| p.data_type == "notifications") {
            p.max_age_days_read = None;
        }
        let err = validate_policies(&no_read).unwrap_err();
        assert!(err.contains("max_age_days_read"), "{err}");

        // read cutoff after unread cutoff is nonsensical.
        let mut inverted = default_policies();
        if let Some(p) = inverted.iter_mut().find(|p| p.data_type == "notifications") {
            p.max_age_days_read = Some(400);
        }
        let err = validate_policies(&inverted).unwrap_err();
        assert!(err.contains("must be <= max_age_days"), "{err}");

        // snapshots must set tier counts, all >= 1.
        let mut no_tiers = default_policies();
        if let Some(p) = no_tiers.iter_mut().find(|p| p.data_type == "snapshots") {
            p.snapshot_tiers = None;
        }
        let err = validate_policies(&no_tiers).unwrap_err();
        assert!(err.contains("snapshot_tiers"), "{err}");
        let mut zero_tier = default_policies();
        if let Some(p) = zero_tier.iter_mut().find(|p| p.data_type == "snapshots") {
            p.snapshot_tiers = Some(SnapshotTiers { keep_24h: 0, keep_7d: 7, keep_4w: 4 });
        }
        let err = validate_policies(&zero_tier).unwrap_err();
        assert!(err.contains("tier counts must each be >= 1"), "{err}");
    }
}
