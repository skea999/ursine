//! Injectable time source so session/ratelimit logic is unit-testable.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Milliseconds since an arbitrary epoch (monotonic enough for TTLs).
pub trait Clock: Send + Sync {
    fn now_millis(&self) -> u64;
}

/// Real clock backed by `std::time::Instant` since process start.
#[derive(Debug)]
pub struct SystemClock {
    start: std::time::Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// Test clock the caller advances by hand.
#[derive(Debug, Default)]
pub struct ManualClock {
    now: AtomicU64,
}

impl ManualClock {
    pub fn new(start_millis: u64) -> Arc<Self> {
        Arc::new(Self {
            now: AtomicU64::new(start_millis),
        })
    }

    pub fn advance(&self, millis: u64) {
        self.now.fetch_add(millis, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_millis(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_advances() {
        let c = ManualClock::new(1000);
        assert_eq!(c.now_millis(), 1000);
        c.advance(500);
        assert_eq!(c.now_millis(), 1500);
    }
}
