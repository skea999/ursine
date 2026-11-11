//! Shared daemon operation budgets, in seconds.
//!
//! Frontends derive their socket deadline from these values plus the daemon's
//! write timeout, so a slow but successful operation is never reported as a
//! failure. Daemon and client must not carry private copies.

/// `nix profile` read operations (list, history, diff).
pub const PROFILE_SECS: u64 = 60;
/// Generation-history read inside a combined prune request.
pub const HISTORY_SECS: u64 = 60;
/// `nix search`.
pub const SEARCH_SECS: u64 = 300;
/// On-demand nix upgrade check (`list_nix_updates`): the dry-run
/// re-fetches every unlocked flake with tarballTtl=0.
pub const UPDATE_CHECK_SECS: u64 = 300;
/// Profile mutations (install, remove, upgrade, rollback, wipe).
pub const MUTATE_SECS: u64 = 900;
/// Store-wide garbage collection.
pub const GC_SECS: u64 = 3600;
/// Socket write budget after the daemon finishes an operation.
pub const WRITE_SECS: u64 = 30;
