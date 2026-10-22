//! RAM-only session tokens with sliding TTL. Lost on restart — by design.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ursine_api::{Principal, SessionToken};

use crate::clock::Clock;

struct Session {
    principal: Principal,
    last_seen_millis: u64,
}

/// In-memory token store.
pub struct TokenStore {
    ttl: Duration,
    clock: Arc<dyn Clock>,
    sessions: std::sync::Mutex<HashMap<String, Session>>,
}

impl std::fmt::Debug for TokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenStore")
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

impl TokenStore {
    pub fn new(ttl: Duration, clock: Arc<dyn Clock>) -> Self {
        Self {
            ttl,
            clock,
            sessions: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Issue a fresh 256-bit token for the principal. Returns (token, ttl secs).
    ///
    /// # Panics
    ///
    /// Panics only if the OS RNG fails, which is unrecoverable for the daemon.
    #[allow(clippy::expect_used)] // OS RNG failure cannot be handled here
    pub fn issue(&self, principal: Principal) -> (SessionToken, u64) {
        let mut raw = [0u8; 32];
        getrandom::getrandom(&mut raw).expect("os rng must work");
        let mut token = String::with_capacity(64);
        for byte in raw {
            use std::fmt::Write as _;
            let _ = write!(token, "{byte:02x}");
        }

        let now = self.clock.now_millis();
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions.retain(|_, s| !self.expired(s, now));
        sessions.insert(
            token.clone(),
            Session {
                principal,
                last_seen_millis: now,
            },
        );
        (SessionToken::new(token), self.ttl.as_secs())
    }

    /// Validate a token and slide its TTL. Returns the principal when valid.
    pub fn validate(&self, token: &SessionToken) -> Option<Principal> {
        let now = self.clock.now_millis();
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions.retain(|_, s| !self.expired(s, now));
        let session = sessions.get_mut(token.as_str())?;
        session.last_seen_millis = now;
        Some(session.principal.clone())
    }

    /// Number of live sessions (tests/diagnostics).
    pub fn len(&self) -> usize {
        let now = self.clock.now_millis();
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions.values().filter(|s| !self.expired(s, now)).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn expired(&self, session: &Session, now: u64) -> bool {
        now.saturating_sub(session.last_seen_millis)
            > u64::try_from(self.ttl.as_millis()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;

    fn store(clock: &Arc<ManualClock>) -> TokenStore {
        TokenStore::new(Duration::from_secs(600), clock.clone())
    }

    #[test]
    fn issue_and_validate() {
        let clock = ManualClock::new(0);
        let store = store(&clock);
        let (token, ttl) = store.issue(Principal::local("skea999", vec!["wheel".into()]));
        assert_eq!(ttl, 600);
        assert_eq!(token.as_str().len(), 64, "256-bit hex token");
        let p = store.validate(&token).unwrap();
        assert_eq!(p.user, "skea999");
    }

    #[test]
    fn ttl_slides_on_activity() {
        let clock = ManualClock::new(0);
        let store = store(&clock);
        let (token, _) = store.issue(Principal::local("a", vec![]));
        clock.advance(500_000);
        assert!(store.validate(&token).is_some(), "still inside 10 min");
        clock.advance(500_000); // 1000s total from issue, 500s from last use
        assert!(store.validate(&token).is_some(), "TTL slid");
        clock.advance(601_000);
        assert!(store.validate(&token).is_none(), "expired after idle > TTL");
    }

    #[test]
    fn unknown_token_is_none_and_expired_are_purged() {
        let clock = ManualClock::new(0);
        let store = store(&clock);
        let (t1, _) = store.issue(Principal::local("a", vec![]));
        let (t2, _) = store.issue(Principal::local("b", vec![]));
        assert!(store.validate(&SessionToken::new("nope")).is_none());
        clock.advance(1_000_000);
        let (t3, _) = store.issue(Principal::local("c", vec![]));
        assert_eq!(store.len(), 1, "expired purged on issue");
        assert!(store.validate(&t1).is_none());
        assert!(store.validate(&t2).is_none());
        assert!(store.validate(&t3).is_some());
    }
}
