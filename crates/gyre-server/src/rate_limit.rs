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
}

impl RateLimiter {
    pub fn new(rate_per_sec: u64) -> Arc<Self> {
        let now = now_secs();
        Arc::new(Self {
            rate: rate_per_sec,
            tokens: AtomicU64::new(rate_per_sec),
            last_refill_sec: AtomicU64::new(now),
        })
    }

    /// Try to consume one token. Returns true if allowed, false if rate exceeded.
    pub fn try_acquire(&self) -> bool {
        let now = now_secs();
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
}
