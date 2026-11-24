//! `ursine-core` — all daemon logic: config, capabilities, auth, sessions,
//! audit, socket server and the module handlers. Frontends never depend
//! on this crate (they see `ursine-api` only).
//!
//! Binding rules: no polkit, no database, stateless; capabilities are
//! detected live and never persisted; tokens live in RAM only.

pub mod audit;
pub mod auth;
pub mod backup_configs;
pub mod capabilities;
pub mod clock;
pub mod config;
pub mod dispatch;
pub mod error;
pub mod jobs;
pub mod modules;
pub mod ratelimit;
pub mod session;
pub mod socket;

pub(crate) mod atomic;
pub(crate) mod tooling;

// One wiring surface for the two binaries (`ursined`, `ursinectl`): they
// import the daemon parts here instead of repeating the module paths.
pub use audit::AuditLog;
pub use auth::Authenticator;
pub use capabilities::Registry;
pub use clock::{Clock, SystemClock};
pub use dispatch::Daemon;
pub use error::CoreError;
pub use jobs::JobRegistry;
pub use ratelimit::RateLimiter;
pub use session::{SessionInfo, TokenStore};
