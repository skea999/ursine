//! RAM-only session tokens with sliding TTL. Lost on restart — by design.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ursine_api::{Principal, SessionToken};

use crate::clock::Clock;

struct Session {
    principal: Principal,
    /// The credential may read and nothing else: `handle_call` refuses every
    /// mutating action of this session.
    read_only: bool,
    /// Frontend that opened the session; carried into the audit log so a line
    /// can be attributed to `ursinectl` or `ursine-gui`, not just a user.
    client: Option<String>,
    last_seen_millis: u64,
}

/// What the daemon knows about a validated token.
///
/// The principal alone cannot answer "may this credential write?", so the
/// session's scope travels with every call instead of being looked up again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    /// Authenticated user and its groups.
    pub principal: Principal,
    /// The credential may only read.
    pub read_only: bool,
    /// Frontend name supplied at authentication, when it supplied one.
    pub client: Option<String>,
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

    /// Lock the store and drop expired sessions in the same pass every
    /// operation needs; returns `now` plus the live map.
    fn live(&self) -> (u64, std::sync::MutexGuard<'_, HashMap<String, Session>>) {
        let now = self.clock.now_millis();
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions.retain(|_, s| !self.expired(s, now));
        (now, sessions)
    }

    /// Issue a fresh 256-bit token for the principal. Returns (token, ttl secs).
    ///
    /// `read_only` is fixed for the life of the session: a client that wants
    /// write access authenticates again, so a scope mistake cannot be healed
    /// by a later request on the same token.
    ///
    /// # Panics
    ///
    /// Panics only if the OS RNG fails, which is unrecoverable for the daemon.
    #[allow(clippy::expect_used)] // OS RNG failure cannot be handled here
    pub fn issue(
        &self,
        principal: Principal,
        read_only: bool,
        client: Option<String>,
    ) -> (SessionToken, u64) {
        let mut raw = [0u8; 32];
        getrandom::getrandom(&mut raw).expect("os rng must work");
        let mut token = String::with_capacity(64);
        for byte in raw {
            use std::fmt::Write as _;
            let _ = write!(token, "{byte:02x}");
        }

        let (now, mut sessions) = self.live();
        sessions.insert(
            token.clone(),
            Session {
                principal,
                read_only,
                client,
                last_seen_millis: now,
            },
        );
        (SessionToken::new(token), self.ttl.as_secs())
    }

    /// Validate a token and slide its TTL. Returns the session when valid.
    pub fn validate(&self, token: &SessionToken) -> Option<SessionInfo> {
        let (now, mut sessions) = self.live();
        let session = sessions.get_mut(token.as_str())?;
        session.last_seen_millis = now;
        Some(SessionInfo {
            principal: session.principal.clone(),
            read_only: session.read_only,
            client: session.client.clone(),
        })
    }

    /// Number of live sessions (tests/diagnostics).
    pub fn len(&self) -> usize {
        let (now, sessions) = self.live();
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
        let (token, ttl) = store.issue(
            Principal::local("skea999", vec!["wheel".into()]),
            false,
            None,
        );
        assert_eq!(ttl, 600);
        assert_eq!(token.as_str().len(), 64, "256-bit hex token");
        let info = store.validate(&token).unwrap();
        assert_eq!(info.principal.user, "skea999");
        assert!(!info.read_only);
        assert!(info.client.is_none());
    }

    #[test]
    fn validate_returns_the_scope_and_client_it_was_issued_with() {
        let clock = ManualClock::new(0);
        let store = store(&clock);
        let (token, _) = store.issue(
            Principal::local("skea999", vec!["wheel".into()]),
            true,
            Some("ursinectl".into()),
        );
        let info = store.validate(&token).unwrap();
        assert!(info.read_only, "a scoped credential keeps its scope");
        assert_eq!(info.client.as_deref(), Some("ursinectl"));
    }

    #[test]
    fn ttl_slides_on_activity() {
        let clock = ManualClock::new(0);
        let store = store(&clock);
        let (token, _) = store.issue(Principal::local("a", vec![]), false, None);
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
        let (t1, _) = store.issue(Principal::local("a", vec![]), false, None);
        let (t2, _) = store.issue(Principal::local("b", vec![]), false, None);
        assert!(store.validate(&SessionToken::new("nope")).is_none());
        clock.advance(1_000_000);
        let (t3, _) = store.issue(Principal::local("c", vec![]), false, None);
        assert_eq!(store.len(), 1, "expired purged on issue");
        assert!(store.validate(&t1).is_none());
        assert!(store.validate(&t2).is_none());
        assert!(store.validate(&t3).is_some());
    }
}
