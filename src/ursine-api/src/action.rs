//! Per-module action payloads.
//!
//! One flat enum keeps the wire format simple; `dispatch` validates that the
//! `(ModuleId, Action)` pair is legal. Variants cover the v1 spec surface even
//! where the handler still returns `NotImplemented`.

use serde::{Deserialize, Serialize};

use crate::ModuleId;

/// Serialized with an internal `action` tag: `{"action":"list","query":null}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    // -- system --
    Status,

    // -- config --
    /// Write a commented `/etc/ursine/config.toml` from a capability scan.
    Init,
    /// Return the current daemon configuration module switches.
    GetConfig,
    /// Persist one module switch (`"auto"`, true or false) and apply it live.
    SetModuleSwitch {
        module: ModuleId,
        value: crate::TriState,
    },

    // -- capabilities --
    /// List every capability with availability, reason and fix.
    List,
    /// Re-run detection now.
    Refresh,

    // -- packages.nix --
    /// List installed packages in the target user's profile.
    ListInstalled,
    /// Search nixpkgs (slow: UI shows progress).
    Search {
        query: String,
    },
    /// Install a package into the target user's profile.
    Install {
        package: String,
    },
    /// Remove a package from the target user's profile.
    Remove {
        package: String,
    },
    /// Remove every package from the profile.
    RemoveAll,
    /// Remove all packages whose name matches a regex.
    RemoveRegex {
        pattern: String,
    },
    /// Upgrade one package (or all when `all`).
    Upgrade {
        package: Option<String>,
        all: bool,
    },
    /// Upgrade all packages whose name matches a regex.
    UpgradeRegex {
        pattern: String,
    },
    /// Profile history (generations).
    Generations,
    /// Roll the profile back to a previous generation (default: previous).
    Rollback {
        generation: Option<u32>,
    },
    /// Delete old profile generations (destructive). Dry-run supported.
    WipeHistory {
        older_than: Option<String>,
        dry_run: bool,
    },
    /// Show what changed between generations (human-readable closure diff).
    DiffClosures {
        generation: Option<u32>,
    },
    /// Garbage-collect the whole nix store (unreachable paths only).
    StoreGc,
    /// Prune profile generations, keeping the last `keep` (dry_run = no changes).
    PruneGenerations {
        keep: u32,
        dry_run: bool,
    },

    // -- snapshots (btrbk) --
    /// List btrbk-managed snapshots.
    ListSnapshots,
    /// btrbk stats output.
    Stats,
    /// Run a snapshot-only btrbk pass for a config section.
    CreateSnapshot {
        section: Option<String>,
    },
    /// Restore a snapshot to a target. `dry_run` returns the preview.
    RestoreSnapshot {
        snapshot: String,
        target: String,
        dry_run: bool,
    },
    /// Parsed view of `/etc/btrbk/btrbk.conf`.
    GetBtrbkConfig,
    /// Write a commented default btrbk.conf from detected subvolumes.
    InitBtrbkConfig,

    // -- users --
    /// List human users (uid >= 1000).
    ListUsers,
    /// Create a user; `admin` adds wheel membership.
    CreateUser {
        name: String,
        fullname: String,
        admin: bool,
    },
    /// Delete a user (confirmation enforced by the frontend).
    DeleteUser {
        uid: u32,
        remove_files: bool,
    },
    /// Set a user's password (value never logged or audited).
    SetPassword {
        uid: u32,
        new_password: String,
    },
    /// List system groups.
    ListGroups,
    /// Replace a user's supplementary groups.
    SetGroups {
        uid: u32,
        groups: Vec<String>,
    },

    // -- niri --
    /// Schema-typed view of the user's config.kdl.
    GetNiriConfig,
    /// Run `niri validate` against a proposed config (no write).
    ValidateNiriConfig {
        proposed: Option<String>,
    },
    /// Apply one section of form values (schema-driven).
    SetNiriSection {
        section: String,
        value: serde_json::Value,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_tag_is_present_and_snake_case() {
        let json = serde_json::to_value(&Action::ListInstalled).unwrap();
        assert_eq!(json["action"], "list_installed");
    }

    #[test]
    fn config_switch_payload_roundtrips() {
        let a = Action::SetModuleSwitch {
            module: ModuleId::PackagesNix,
            value: crate::TriState::On,
        };
        let s = serde_json::to_string(&a).unwrap();
        let back: Action = serde_json::from_str(&s).unwrap();
        assert_eq!(a, back);
        assert!(s.contains("\"action\":\"set_module_switch\""));
        assert!(s.contains("\"module\":\"packages_nix\""));
        assert!(s.contains("\"value\":true"));
    }

    #[test]
    fn payload_roundtrip() {
        let a = Action::Install {
            package: "hello".into(),
        };
        let s = serde_json::to_string(&a).unwrap();
        let back: Action = serde_json::from_str(&s).unwrap();
        assert_eq!(a, back);

        let r = Action::RestoreSnapshot {
            snapshot: "@snap".into(),
            target: "/mnt/restore".into(),
            dry_run: true,
        };
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Action>(&s).unwrap(), r);
    }
}
