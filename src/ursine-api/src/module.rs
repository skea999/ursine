//! Module identifiers: the functional areas the daemon exposes.

use serde::{Deserialize, Serialize};

/// One variant per v1 module (plus internal ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleId {
    /// Daemon status, version, uptime.
    System,
    /// Daemon configuration (`config.init`, reload).
    Config,
    /// Runtime capability detection.
    Capabilities,
    /// Nix user profiles (the flagship).
    PackagesNix,
    /// btrbk snapshot management.
    Snapshots,
    /// Users and groups via accountsservice.
    Users,
    /// niri compositor configuration.
    Niri,
}

impl ModuleId {
    /// Stable wire/audit name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Config => "config",
            Self::Capabilities => "capabilities",
            Self::PackagesNix => "packages_nix",
            Self::Snapshots => "snapshots",
            Self::Users => "users",
            Self::Niri => "niri",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_names_are_stable_snake_case() {
        assert_eq!(
            serde_json::to_string(&ModuleId::PackagesNix).unwrap(),
            "\"packages_nix\""
        );
        assert_eq!(serde_json::to_string(&ModuleId::Niri).unwrap(), "\"niri\"");
    }

    #[test]
    fn roundtrip_all_variants() {
        for m in [
            ModuleId::System,
            ModuleId::Config,
            ModuleId::Capabilities,
            ModuleId::PackagesNix,
            ModuleId::Snapshots,
            ModuleId::Users,
            ModuleId::Niri,
        ] {
            let s = serde_json::to_string(&m).unwrap();
            let back: ModuleId = serde_json::from_str(&s).unwrap();
            assert_eq!(m, back);
            assert_eq!(s.trim_matches('"'), m.as_str());
        }
    }
}
