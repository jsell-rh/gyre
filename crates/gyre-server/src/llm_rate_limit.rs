//! Per-user, per-workspace sliding-window rate limiter for LLM endpoints.
//!
//! Implements the rate limiting spec from `specs/system/ui-layout.md` §2:
//! - 10 requests per user per workspace per 60-second sliding window
//! - In-memory VecDeque of Instants, one deque per (user_id, workspace_id) pair
//! - Entries evicted after 60 s of inactivity by the background cleanup task
//! - 429 responses include `Retry-After: <seconds>` header
//!
//! # Usage in handlers
//!
//! ```ignore
//! use crate::llm_rate_limit::{check_rate_limit, rate_limited_response, LLM_RATE_LIMIT, LLM_WINDOW_SECS};
//!
//! let mut limiter = state.llm_rate_limiter.lock().await;
//! if let Err(retry_after) = check_rate_limit(&mut limiter, &user_id, &workspace_id, LLM_RATE_LIMIT, LLM_WINDOW_SECS) {
//!     return Err(rate_limited_response(retry_after));
//! }
//! drop(limiter); // release lock before making the LLM call
//! ```

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

/// Maximum LLM requests per user per workspace per window.
pub const LLM_RATE_LIMIT: usize = 10;

/// Sliding window duration in seconds.
pub const LLM_WINDOW_SECS: u64 = 60;

/// In-memory rate limiter state: (user_id, workspace_id) → timestamps of recent requests.
pub type LlmRateLimiterMap = HashMap<(String, String), VecDeque<Instant>>;

/// Check whether a request from `(user_id, workspace_id)` is within the rate limit.
///
/// - Drains timestamps older than `window_secs` from the front of the deque.
/// - If `deque.len() >= limit`, returns `Err(retry_after_secs)` (seconds until the oldest
///   timestamp falls out of the window).
/// - Otherwise, pushes `Instant::now()` and returns `Ok(())`.
pub fn check_rate_limit(
    limiter: &mut LlmRateLimiterMap,
    user_id: &str,
    workspace_id: &str,
    limit: usize,
    window_secs: u64,
) -> Result<(), u64> {
    let window = Duration::from_secs(window_secs);
    let now = Instant::now();
    let key = (user_id.to_string(), workspace_id.to_string());
    let deque = limiter.entry(key).or_default();

    // Drop timestamps that have fallen outside the window.
    while let Some(&front) = deque.front() {
        if now.duration_since(front) >= window {
            deque.pop_front();
        } else {
            break;
        }
    }

    if deque.len() >= limit {
        // Oldest timestamp is still inside the window; tell the caller how long to wait.
        let oldest = deque.front().copied().expect("deque non-empty");
        let elapsed = now.duration_since(oldest);
        let retry_after = window_secs.saturating_sub(elapsed.as_secs());
        return Err(retry_after.max(1));
    }

    deque.push_back(now);
    Ok(())
}

/// Build an HTTP 429 response with `Retry-After` header.
pub fn rate_limited_response(retry_after_secs: u64) -> Response {
    let headers = [("Retry-After", retry_after_secs.to_string())];
    (
        StatusCode::TOO_MANY_REQUESTS,
        headers,
        Json(json!({
            "error": "rate limit exceeded",
            "retry_after": retry_after_secs
        })),
    )
        .into_response()
}

/// Background cleanup: remove map entries whose deque is empty after draining old timestamps.
/// Call this from a `tokio::spawn` loop every `window_secs` seconds.
pub fn evict_stale_entries(limiter: &mut LlmRateLimiterMap, window_secs: u64) {
    let window = Duration::from_secs(window_secs);
    let now = Instant::now();
    limiter.retain(|_, deque| {
        while let Some(&front) = deque.front() {
            if now.duration_since(front) >= window {
                deque.pop_front();
            } else {
                break;
            }
        }
        !deque.is_empty()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_map() -> LlmRateLimiterMap {
        HashMap::new()
    }

    #[test]
    fn allows_requests_within_limit() {
        let mut map = make_map();
        for i in 0..10 {
            assert!(
                check_rate_limit(&mut map, "user1", "ws1", 10, 60).is_ok(),
                "request {i} should be allowed"
            );
        }
    }

    #[test]
    fn rejects_at_limit() {
        let mut map = make_map();
        for _ in 0..10 {
            check_rate_limit(&mut map, "user1", "ws1", 10, 60).unwrap();
        }
        let result = check_rate_limit(&mut map, "user1", "ws1", 10, 60);
        assert!(result.is_err(), "11th request should be rejected");
        let retry_after = result.unwrap_err();
        assert!(retry_after >= 1, "retry_after must be at least 1 second");
        assert!(retry_after <= 60, "retry_after must not exceed window");
    }

    #[test]
    fn different_users_are_independent() {
        let mut map = make_map();
        for _ in 0..10 {
            check_rate_limit(&mut map, "user1", "ws1", 10, 60).unwrap();
        }
        // user2 in same workspace is unaffected.
        assert!(check_rate_limit(&mut map, "user2", "ws1", 10, 60).is_ok());
    }

    #[test]
    fn different_workspaces_are_independent() {
        let mut map = make_map();
        for _ in 0..10 {
            check_rate_limit(&mut map, "user1", "ws1", 10, 60).unwrap();
        }
        // Same user in different workspace is unaffected.
        assert!(check_rate_limit(&mut map, "user1", "ws2", 10, 60).is_ok());
    }

    #[test]
    fn evict_removes_empty_entries() {
        let mut map = make_map();
        // Seed with a zero-duration window so everything is immediately stale.
        let key = ("user1".to_string(), "ws1".to_string());
        let mut deque = VecDeque::new();
        // Push an Instant that is already 2 seconds old (sleep-free: use past-biased Instant).
        // We can't easily fake Instant, so instead use a 1-second window.
        deque.push_back(Instant::now());
        map.insert(key.clone(), deque);

        // Immediately evict with window = 0 (any timestamp is "old").
        evict_stale_entries(&mut map, 0);
        assert!(map.is_empty(), "stale entry should be evicted");
    }

    #[test]
    fn evict_preserves_active_entries() {
        let mut map = make_map();
        check_rate_limit(&mut map, "user1", "ws1", 10, 60).unwrap();
        // 60-second window: entry is fresh, should not be evicted.
        evict_stale_entries(&mut map, 60);
        assert!(!map.is_empty(), "fresh entry should be preserved");
    }

    #[test]
    fn retry_after_is_nonzero() {
        let mut map = make_map();
        for _ in 0..10 {
            check_rate_limit(&mut map, "u", "w", 10, 60).unwrap();
        }
        let err = check_rate_limit(&mut map, "u", "w", 10, 60).unwrap_err();
        assert!(err >= 1);
    }

    #[test]
    fn window_zero_always_allows_after_eviction() {
        // With window=0, every timestamp is immediately outside the window.
        let mut map = make_map();
        // First call seeds the deque, but window=0 means the just-pushed Instant
        // will be evicted on the *next* call (Duration::from_secs(0) check).
        // Actually with window=0: now.duration_since(front) >= Duration::ZERO is always true,
        // so it always drains before checking len. Verify unlimited allows.
        assert!(check_rate_limit(&mut map, "u", "w", 10, 0).is_ok());
        // Second call drains the one entry from the previous call immediately.
        assert!(check_rate_limit(&mut map, "u", "w", 10, 0).is_ok());
    }
}
