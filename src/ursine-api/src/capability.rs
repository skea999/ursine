//! Runtime capability types.
//!
//! A capability is a fact about the machine, detected live and NEVER
//! persisted (it would go stale on the first `apk add`).

use serde::{Deserialize, Serialize};

/// Everything the daemon can probe on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// btrbk binary present.
    Btrbk,
    /// btrfs filesystem in use.
    Btrfs,
    /// Nix store + nix binary present.
    Nix,
    /// Alpine apk binary present.
    Apk,
    /// Flatpak binary present.
    Flatpak,
    /// CUPS is running and lpstat is available.
    Cups,
    /// SANE scanner setup is available.
    Sane,
    /// firewalld is available.
    Firewalld,
    /// org.freedesktop.Accounts reachable on D-Bus.
    Accountsservice,
    /// niri binary + user config present.
    Niri,
    /// PID 1 is dinit.
    Dinit,
}

impl Capability {
    /// Every capability the daemon probes, in display order.
    pub const ALL: [Self; 11] = [
        Self::Btrbk,
        Self::Btrfs,
        Self::Nix,
        Self::Apk,
        Self::Flatpak,
        Self::Cups,
        Self::Sane,
        Self::Firewalld,
        Self::Accountsservice,
        Self::Niri,
        Self::Dinit,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Btrbk => "btrbk",
            Self::Btrfs => "btrfs",
            Self::Nix => "nix",
            Self::Apk => "apk",
            Self::Flatpak => "flatpak",
            Self::Cups => "cups",
            Self::Sane => "sane",
            Self::Firewalld => "firewalld",
            Self::Accountsservice => "accountsservice",
            Self::Niri => "niri",
            Self::Dinit => "dinit",
        }
    }
}

/// Detection result with UI-actionable context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityReport {
    pub capability: Capability,
    pub available: bool,
    /// Why not, when unavailable (human string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Exact command that would enable it (e.g. `apk add btrbk`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

impl CapabilityReport {
    pub fn available(capability: Capability) -> Self {
        Self {
            capability,
            available: true,
            reason: None,
            fix: None,
        }
    }

    pub fn unavailable(
        capability: Capability,
        reason: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Self {
            capability,
            available: false,
            reason: Some(reason.into()),
            fix: Some(fix.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_capabilities_are_listed_once() {
        assert_eq!(Capability::ALL.len(), 11);
        assert!(Capability::ALL.contains(&Capability::Apk));
        assert!(Capability::ALL.contains(&Capability::Flatpak));
    }

    #[test]
    fn report_skips_none_fields() {
        let r = CapabilityReport::available(Capability::Nix);
        let v = serde_json::to_value(&r).unwrap();
        assert!(!v.as_object().unwrap().contains_key("reason"));
        assert!(!v.as_object().unwrap().contains_key("fix"));
    }

    #[test]
    fn unavailable_report_carries_fix() {
        let r = CapabilityReport::unavailable(
            Capability::Btrbk,
            "btrbk not installed",
            "apk add btrbk",
        );
        let s = serde_json::to_string(&r).unwrap();
        let back: CapabilityReport = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
        assert_eq!(back.fix.as_deref(), Some("apk add btrbk"));
    }
}
