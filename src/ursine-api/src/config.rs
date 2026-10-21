//! Daemon configuration model (`/etc/ursine/config.toml`).
//!
//! Per-module switches are tri-state: `"auto"` (detection decides), `true`
//! (force, warn if missing), `false` (off even if present).

use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// `"auto" | true | false` on the wire and in the TOML file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriState {
    /// Detection decides (default).
    #[default]
    Auto,
    /// Force on; warn when the capability is missing.
    On,
    /// Force off.
    Off,
}

impl TriState {
    pub const fn is_auto(self) -> bool {
        matches!(self, Self::Auto)
    }

    /// Stable human/audit spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::On => "true",
            Self::Off => "false",
        }
    }
}

impl Serialize for TriState {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => s.serialize_str("auto"),
            Self::On => s.serialize_bool(true),
            Self::Off => s.serialize_bool(false),
        }
    }
}

impl<'de> Deserialize<'de> for TriState {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Vis;
        impl serde::de::Visitor<'_> for Vis {
            type Value = TriState;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("\"auto\", true or false")
            }
            fn visit_bool<E>(self, v: bool) -> Result<TriState, E> {
                Ok(if v { TriState::On } else { TriState::Off })
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<TriState, E> {
                match v {
                    "auto" => Ok(TriState::Auto),
                    other => Err(E::custom(format!(
                        "invalid tri-state {other:?}: want \"auto\", true or false"
                    ))),
                }
            }
        }
        d.deserialize_any(Vis)
    }
}

/// Per-module switches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModulesConfig {
    pub packages_nix: TriState,
    pub snapshots: TriState,
    pub users: TriState,
    pub niri: TriState,
}

impl Default for ModulesConfig {
    fn default() -> Self {
        Self {
            packages_nix: TriState::Auto,
            snapshots: TriState::Auto,
            users: TriState::Auto,
            niri: TriState::Auto,
        }
    }
}

fn default_socket_path() -> PathBuf {
    PathBuf::from("/run/ursine.sock")
}

fn default_audit_path() -> PathBuf {
    PathBuf::from("/var/log/ursine/audit.jsonl")
}

fn default_token_ttl_min() -> u32 {
    10
}

fn default_login_rate_limit_per_min() -> u32 {
    5
}

/// Full daemon configuration. All fields have sane defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UrsineConfig {
    #[serde(default = "default_socket_path")]
    pub socket_path: PathBuf,
    #[serde(default = "default_audit_path")]
    pub audit_path: PathBuf,
    /// Session token TTL, sliding, minutes. Default 10.
    #[serde(default = "default_token_ttl_min")]
    pub token_ttl_min: u32,
    /// Failed logins allowed per uid per minute.
    #[serde(default = "default_login_rate_limit_per_min")]
    pub login_rate_limit_per_min: u32,
    #[serde(default)]
    pub modules: ModulesConfig,
}

impl Default for UrsineConfig {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path(),
            audit_path: default_audit_path(),
            token_ttl_min: default_token_ttl_min(),
            login_rate_limit_per_min: default_login_rate_limit_per_min(),
            modules: ModulesConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tri_state_wire_values() {
        assert_eq!(serde_json::to_string(&TriState::Auto).unwrap(), "\"auto\"");
        assert_eq!(serde_json::to_string(&TriState::On).unwrap(), "true");
        assert_eq!(serde_json::to_string(&TriState::Off).unwrap(), "false");
    }

    #[test]
    fn tri_state_parses_all_forms() {
        assert_eq!(
            serde_json::from_str::<TriState>("\"auto\"").unwrap(),
            TriState::Auto
        );
        assert_eq!(
            serde_json::from_str::<TriState>("true").unwrap(),
            TriState::On
        );
        assert_eq!(
            serde_json::from_str::<TriState>("false").unwrap(),
            TriState::Off
        );
        assert!(serde_json::from_str::<TriState>("\"on\"").is_err());
    }

    #[test]
    fn defaults_are_auto_with_10_min_ttl() {
        let c = UrsineConfig::default();
        assert!(c.modules.snapshots.is_auto());
        assert_eq!(c.token_ttl_min, 10);
        assert_eq!(c.socket_path, PathBuf::from("/run/ursine.sock"));
    }

    #[test]
    fn toml_roundtrip_partial_file() {
        let c: UrsineConfig =
            toml::from_str("token_ttl_min = 30\n[modules]\nsnapshots = false\n").unwrap();
        assert_eq!(c.token_ttl_min, 30);
        assert_eq!(c.modules.snapshots, TriState::Off);
        assert!(c.modules.niri.is_auto());
    }
}
