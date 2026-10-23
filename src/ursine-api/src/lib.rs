//! `ursine-api` — the shared contract between `ursined` and its frontends.
//!
//! Pure types only: no IO, no async, no logic beyond parsing. Everything a
//! frontend can send or receive has a type here, so protocol drift is a
//! compile error (see `docs/specs/SPEC-protocol.md`).

pub mod action;
pub mod capability;
pub mod config;
pub mod envelope;
pub mod error;
pub mod module;
pub mod nix;
pub mod principal;
pub mod snapshot;
pub mod timeouts;
pub mod token;

pub use action::Action;
pub use capability::{Capability, CapabilityReport};
pub use config::{ModulesConfig, TriState, UrsineConfig};
pub use envelope::{Request, RequestBody, Response, ResponseBody, PROTOCOL_VERSION};
pub use error::{ApiError, ErrorCode};
pub use module::ModuleId;
pub use nix::{
    installable_from_attr, is_safe_installable_atom, normalize_installable, validate_installable,
    CommandOutcome, InstalledPackage, SearchResult,
};
pub use principal::{AsUser, Principal, PrincipalOrigin};
pub use snapshot::{BtrbkConfigView, BtrbkSettings, Snapshot, SnapshotStats};
pub use token::SessionToken;
