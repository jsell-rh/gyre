//! Token-bucket rate limiter for HTTP request handling.
//!
//! Returns HTTP 429 immediately when the rate is exceeded,
//! rather than blocking. Rate is configurable via `GYRE_RATE_LIMIT`
//! (requests per second, default: 100).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Wall-clock source (unix seconds). Injectable so tests can freeze or script
/// time and verify refill behavior deterministically instead of racing the
/// real clock across a second boundary.
pub type NowSecsFn = Box<dyn Fn() -> u64 + Send + Sync>;

pub struct RateLimiter {
    rate: u64,
    tokens: AtomicU64,
    last_refill_sec: AtomicU64,
    now_secs: NowSecsFn,
}

impl RateLimiter {
    /// Construct with the real wall clock.
    pub fn new(rate_per_sec: u64) -> Arc<Self> {
        Self::with_clock(rate_per_sec, Box::new(system_now_secs))
    }

    /// Construct with a custom clock (unix seconds). Production code uses
    /// [`RateLimiter::new`]; tests inject a fixed or scripted clock so the
    /// refill-at-second-boundary behavior is exercised deterministically.
    pub fn with_clock(rate_per_sec: u64, now_secs: NowSecsFn) -> Arc<Self> {
        let now = now_secs();
        Arc::new(Self {
            rate: rate_per_sec,
            tokens: AtomicU64::new(rate_per_sec),
            last_refill_sec: AtomicU64::new(now),
            now_secs,
        })
    }

    /// Try to consume one token. Returns true if allowed, false if rate exceeded.
    pub fn try_acquire(&self) -> bool {
        let now = (self.now_secs)();
        let last = self.last_refill_sec.load(Ordering::Relaxed);
        if now > last {
            // New second — refill to full rate.
            if self
                .last_refill_sec
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                self.tokens.store(self.rate, Ordering::Relaxed);
            }
        }
        // Try to consume a token.
        let mut current = self.tokens.load(Ordering::Relaxed);
        loop {
            if current == 0 {
                return false;
            }
            match self.tokens.compare_exchange_weak(
                current,
                current - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }
}

fn system_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    /// Clock frozen at a fixed unix second: `try_acquire` can never cross a
    /// second boundary, so token exhaustion is deterministic.
    fn frozen_clock() -> NowSecsFn {
        Box::new(|| 1_700_000_000)
    }

    #[test]
    fn allows_up_to_rate() {
        let rl = RateLimiter::with_clock(5, frozen_clock());
        for _ in 0..5 {
            assert!(rl.try_acquire(), "should be allowed within rate");
        }
    }

    #[test]
    fn rejects_when_exhausted() {
        let rl = RateLimiter::with_clock(2, frozen_clock());
        assert!(rl.try_acquire());
        assert!(rl.try_acquire());
        assert!(!rl.try_acquire(), "should reject when rate exceeded");
    }

    #[test]
    fn zero_rate_always_rejects() {
        let rl = RateLimiter::with_clock(0, frozen_clock());
        assert!(!rl.try_acquire());
    }

    #[test]
    fn refills_to_full_rate_on_new_second() {
        // Scripted clock: the test advances the shared tick between acquires,
        // so the second-boundary refill is exercised deterministically.
        let tick = Arc::new(AtomicU64::new(1_700_000_000));
        let clock = {
            let tick = Arc::clone(&tick);
            Box::new(move || tick.load(Ordering::Relaxed))
        };
        let rl = RateLimiter::with_clock(1, clock);
        // Rate of 1: first acquire in the current second succeeds.
        assert!(rl.try_acquire());
        // Same second: bucket is empty.
        assert!(
            !rl.try_acquire(),
            "second acquire in the same second must be rejected"
        );
        // New second: bucket refills to the full rate.
        tick.store(1_700_000_001, Ordering::Relaxed);
        assert!(rl.try_acquire(), "new second must refill the bucket");
        assert!(!rl.try_acquire(), "refill restores the rate, not more");
    }
}
