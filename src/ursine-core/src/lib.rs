//! `ursine-core` — all daemon logic: config, capabilities, auth, sessions,
//! audit, socket server and the module handlers. Frontends never depend
//! on this crate (they see `ursine-api` only).
//!
//! Binding rules: no polkit, no database, stateless; capabilities are
//! detected live and never persisted; tokens live in RAM only.

pub mod audit;
pub mod auth;
pub mod capabilities;
pub mod clock;
pub mod config;
pub mod dispatch;
pub mod error;
pub mod modules;
pub mod ratelimit;
pub mod session;
pub mod socket;

pub use dispatch::Daemon;
pub use error::CoreError;
