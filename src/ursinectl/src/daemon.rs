//! In-process daemon construction: same wiring as `ursined`, minus the
//! socket and the PAM handshake (the root gate replaces authentication).
//!
//! Job logs live under `/run/ursinectl/jobs`, NOT `/run/ursine/jobs`: the
//! registry purges "orphaned" logs at startup and must never touch the
//! running daemon's job files.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ursine_core::audit::AuditLog;
use ursine_core::auth::{AuthError, Authenticator, PeerInfo};
use ursine_core::capabilities::Registry;
use ursine_core::clock::{Clock, SystemClock};
use ursine_core::dispatch::Daemon;
use ursine_core::jobs::JobRegistry;
use ursine_core::ratelimit::RateLimiter;
use ursine_core::session::TokenStore;

/// No authentication path exists in the CLI: the root gate replaced it.
/// Any (hypothetical) `auth` request is rejected outright — the stub
/// authenticator of `ursined` must never appear in a root binary.
struct NoAuth;

impl Authenticator for NoAuth {
    fn authenticate(
        &self,
        _peer: &PeerInfo,
        _user: &str,
        _password: &str,
    ) -> Result<ursine_api::Principal, AuthError> {
        Err(AuthError::Invalid)
    }
}

/// Build the shared daemon around `config_path` (missing file → defaults,
/// same as `ursined`).
pub fn build(config_path: &std::path::Path) -> Result<Daemon, String> {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let config = ursine_core::config::load(config_path).map_err(|e| e.to_string())?;
    let audit = Arc::new(
        AuditLog::open(config.audit_path.clone(), clock.clone()).map_err(|e| e.to_string())?,
    );
    let jobs_dir = PathBuf::from("/run/ursinectl/jobs");
    let jobs =
        JobRegistry::new(jobs_dir, Arc::clone(&audit), clock.clone()).map_err(|e| e.to_string())?;
    let ttl = Duration::from_secs(u64::from(config.token_ttl_min) * 60);
    let rate = config.login_rate_limit_per_min;
    let auth: Arc<dyn Authenticator> = Arc::new(NoAuth);
    Ok(Daemon {
        caps: Registry::system(),
        config_path: config_path.to_path_buf(),
        sessions: TokenStore::new(ttl, clock.clone()),
        ratelimit: RateLimiter::new(rate, clock),
        auth_permits: Arc::new(tokio::sync::Semaphore::new(
            ursine_core::dispatch::MAX_INFLIGHT_AUTH,
        )),
        audit,
        jobs,
        auth,
        config: tokio::sync::RwLock::new(config),
    })
}
