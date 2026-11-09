//! Token-bucket rate limiter for HTTP request handling.
//!
//! Returns HTTP 429 immediately when the rate is exceeded,
//! rather than blocking. Rate is configurable via `GYRE_RATE_LIMIT`
//! (requests per second, default: 100).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct RateLimiter {
    rate: u64,
    tokens: AtomicU64,
    last_refill_sec: AtomicU64,
    /// Wall-clock source (seconds since epoch). Injectable so tests can
    /// freeze time; the per-second refill otherwise makes exhaustion
    /// assertions depend on where requests land relative to a second
    /// boundary.
    clock: fn() -> u64,
}

impl RateLimiter {
    pub fn new(rate_per_sec: u64) -> Arc<Self> {
        Self::with_clock(rate_per_sec, now_secs)
    }

    /// Like [`new`], but with an injected clock. A constant clock freezes
    /// the bucket (no refill), making exhaustion deterministic in tests.
    pub fn with_clock(rate_per_sec: u64, clock: fn() -> u64) -> Arc<Self> {
        Arc::new(Self {
            rate: rate_per_sec,
            tokens: AtomicU64::new(rate_per_sec),
            last_refill_sec: AtomicU64::new(clock()),
            clock,
        })
    }

    /// Try to consume one token. Returns true if allowed, false if rate exceeded.
    pub fn try_acquire(&self) -> bool {
        let now = (self.clock)();
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

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_rate() {
        let rl = RateLimiter::new(5);
        for _ in 0..5 {
            assert!(rl.try_acquire(), "should be allowed within rate");
        }
    }

    #[test]
    fn rejects_when_exhausted() {
        let rl = RateLimiter::new(2);
        assert!(rl.try_acquire());
        assert!(rl.try_acquire());
        assert!(!rl.try_acquire(), "should reject when rate exceeded");
    }

    #[test]
    fn zero_rate_always_rejects() {
        let rl = RateLimiter::new(0);
        assert!(!rl.try_acquire());
    }

    #[test]
    fn refills_to_full_rate_each_second() {
        // Injectable clock: deterministic refill coverage without sleeping
        // across a real second boundary.
        static CLOCK: AtomicU64 = AtomicU64::new(1_700_000_000);
        let clock = || CLOCK.load(Ordering::Relaxed);
        let rl = RateLimiter::with_clock(2, clock);
        assert!(rl.try_acquire());
        assert!(rl.try_acquire());
        assert!(!rl.try_acquire(), "exhausted within the current second");
        CLOCK.store(1_700_000_001, Ordering::Relaxed);
        assert!(rl.try_acquire(), "bucket refills to full rate next second");
        assert!(rl.try_acquire());
        assert!(
            !rl.try_acquire(),
            "refill is to the full rate, not one token per second"
        );
    }

    #[test]
    fn frozen_clock_keeps_bucket_exhausted() {
        let rl = RateLimiter::with_clock(1, || 1_700_000_000);
        assert!(rl.try_acquire());
        assert!(
            !rl.try_acquire(),
            "with a frozen clock the bucket never refills"
        );
    }
}
