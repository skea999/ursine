//! Failed-login rate limiting, in RAM, per peer uid.
//!
//! Window: one minute, sliding (queue of attempt timestamps).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::clock::Clock;

/// Per-uid sliding-window limiter.
pub struct RateLimiter {
    max_per_min: u32,
    clock: Arc<dyn Clock>,
    attempts: std::sync::Mutex<HashMap<u32, VecDeque<u64>>>,
}

const WINDOW_MILLIS: u64 = 60_000;

impl RateLimiter {
    pub fn new(max_per_min: u32, clock: Arc<dyn Clock>) -> Self {
        Self {
            max_per_min,
            clock,
            attempts: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Record/check an attempt. `false` = limited (deny).
    pub fn allow(&self, uid: u32) -> bool {
        let now = self.clock.now_millis();
        let mut map = self.attempts.lock().expect("ratelimit lock");
        let queue = map.entry(uid).or_default();
        while queue
            .front()
            .is_some_and(|t| now.saturating_sub(*t) >= WINDOW_MILLIS)
        {
            queue.pop_front();
        }
        if queue.len() as u32 >= self.max_per_min {
            return false;
        }
        queue.push_back(now);
        true
    }

    /// Drop the uid's history (called after a successful login).
    pub fn reset(&self, uid: u32) {
        self.attempts.lock().expect("ratelimit lock").remove(&uid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;

    #[test]
    fn limits_within_window_and_recovers_after() {
        let clock = ManualClock::new(0);
        let rl = RateLimiter::new(3, clock.clone());
        assert!(rl.allow(1000));
        assert!(rl.allow(1000));
        assert!(rl.allow(1000));
        assert!(!rl.allow(1000), "4th attempt within a minute is denied");
        assert!(rl.allow(1001), "other uid unaffected");

        clock.advance(WINDOW_MILLIS + 1);
        assert!(rl.allow(1000), "window slid, allowed again");
    }

    #[test]
    fn successful_login_resets_history() {
        let clock = ManualClock::new(0);
        let rl = RateLimiter::new(2, clock.clone());
        assert!(rl.allow(5));
        assert!(rl.allow(5));
        assert!(!rl.allow(5));
        rl.reset(5);
        assert!(rl.allow(5));
    }
}
