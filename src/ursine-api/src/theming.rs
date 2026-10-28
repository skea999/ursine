//! Theming (uniform desktop) types.
//!
//! The daemon reads the active shell's config (Wayle or `DankMaterialShell`)
//! and propagates one normalized theme to every toolkit and application
//! (see `docs/specs/SPEC-theming.md`). Frontends render these types only:
//! no theme logic lives outside the daemon.

use serde::{Deserialize, Serialize};

/// Which shell config the snapshot came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeSource {
    Wayle,
    Dms,
}

impl ThemeSource {
    /// Stable wire/audit name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wayle => "wayle",
            Self::Dms => "dms",
        }
    }
}

/// One propagation destination. The full list is fixed so every frontend
/// renders the same target rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemingTarget {
    Gtk3,
    Gtk4,
    Gtk2,
    Qt5,
    Qt6,
    Kde,
    Flatpak,
    Cosmic,
    Environment,
    Portal,
    Firefox,
    Vscodium,
    Alacritty,
    Kitty,
    Ghostty,
    Foot,
    Wezterm,
    Cava,
    Electron,
}

impl ThemingTarget {
    /// Every target, in display order.
    pub const ALL: [Self; 19] = [
        Self::Gtk3,
        Self::Gtk4,
        Self::Gtk2,
        Self::Qt5,
        Self::Qt6,
        Self::Kde,
        Self::Flatpak,
        Self::Cosmic,
        Self::Environment,
        Self::Portal,
        Self::Firefox,
        Self::Vscodium,
        Self::Alacritty,
        Self::Kitty,
        Self::Ghostty,
        Self::Foot,
        Self::Wezterm,
        Self::Cava,
        Self::Electron,
    ];

    /// Stable wire/audit name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gtk3 => "gtk3",
            Self::Gtk4 => "gtk4",
            Self::Gtk2 => "gtk2",
            Self::Qt5 => "qt5",
            Self::Qt6 => "qt6",
            Self::Kde => "kde",
            Self::Flatpak => "flatpak",
            Self::Cosmic => "cosmic",
            Self::Environment => "environment",
            Self::Portal => "portal",
            Self::Firefox => "firefox",
            Self::Vscodium => "vscodium",
            Self::Alacritty => "alacritty",
            Self::Kitty => "kitty",
            Self::Ghostty => "ghostty",
            Self::Foot => "foot",
            Self::Wezterm => "wezterm",
            Self::Cava => "cava",
            Self::Electron => "electron",
        }
    }
}

/// Availability of one target for the current user (detected, not applied).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetStatus {
    pub target: ThemingTarget,
    pub available: bool,
    /// Why unavailable / where it will write (human string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl TargetStatus {
    pub fn available(target: ThemingTarget, detail: impl Into<String>) -> Self {
        Self {
            target,
            available: true,
            detail: Some(detail.into()),
        }
    }

    pub fn unavailable(target: ThemingTarget, detail: impl Into<String>) -> Self {
        Self {
            target,
            available: false,
            detail: Some(detail.into()),
        }
    }
}

/// Result of `theme_detect`: is theming active, which source wins, and what
/// can be written for this user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemingStatus {
    /// False when neither Wayle nor DMS config exists (feature inactive).
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ThemeSource>,
    /// Per-target availability for the target user's home.
    pub targets: Vec<TargetStatus>,
}

/// Palette shared by every toolkit (`#rrggbb`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeColors {
    pub background: String,
    pub foreground: String,
    pub primary: String,
    pub secondary: String,
    pub surface: String,
    pub error: String,
    pub success: String,
    pub warning: String,
    pub outline: String,
}

/// Fonts shared by every toolkit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeFonts {
    pub ui: String,
    pub mono: String,
    pub weight: u16,
    pub scale: f64,
}

/// Cursor theme + size (GTK, env, KDE).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorTheme {
    pub name: String,
    pub size: u16,
}

/// Normalized theme from the winning shell source (`theme_get`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeSnapshot {
    pub source: ThemeSource,
    /// Shell theme name (DMS `currentThemeName`, Wayle provider/theme).
    pub name: String,
    pub dark: bool,
    pub colors: ThemeColors,
    pub fonts: ThemeFonts,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_radius: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_theme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<CursorTheme>,
}

/// What happened to one target during `theme_apply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyOutcome {
    /// Written (or rewritten identically).
    Written,
    /// Skipped: tool/config absent, or the shell owns this file.
    Skipped,
    /// Failed: IO/parse error (detail carries the reason).
    Failed,
}

/// Per-target apply result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetResult {
    pub target: ThemingTarget,
    pub outcome: ApplyOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Result of `theme_apply` (every target reported exactly once).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeApplyReport {
    pub results: Vec<TargetResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_wire_names_are_unique_snake_case() {
        let mut seen = std::collections::HashSet::new();
        for target in ThemingTarget::ALL {
            let s = serde_json::to_string(&target).unwrap();
            let name = target.as_str();
            assert_eq!(s.trim_matches('"'), name);
            assert!(seen.insert(name), "duplicate target name {name}");
        }
        assert_eq!(ThemingTarget::ALL.len(), 19);
    }

    #[test]
    fn snapshot_roundtrips_without_none_fields() {
        let snapshot = ThemeSnapshot {
            source: ThemeSource::Dms,
            name: "green".into(),
            dark: true,
            colors: ThemeColors {
                background: "#10140f".into(),
                foreground: "#e0e4db".into(),
                primary: "#4caf50".into(),
                secondary: "#2e7d32".into(),
                surface: "#1b5e20".into(),
                error: "#f2b8b5".into(),
                success: "#4caf50".into(),
                warning: "#dacb52".into(),
                outline: "#35a639".into(),
            },
            fonts: ThemeFonts {
                ui: "Inter Variable".into(),
                mono: "Fira Code".into(),
                weight: 400,
                scale: 1.0,
            },
            corner_radius: Some(12),
            icon_theme: None,
            cursor: Some(CursorTheme {
                name: "Adwaita".into(),
                size: 16,
            }),
        };
        let value = serde_json::to_value(&snapshot).unwrap();
        assert!(!value["icon_theme"].is_object());
        assert!(value.get("icon_theme").is_none());
        let back: ThemeSnapshot = serde_json::from_value(value).unwrap();
        assert_eq!(back, snapshot);
    }

    #[test]
    fn apply_report_roundtrips() {
        let report = ThemeApplyReport {
            results: vec![
                TargetResult {
                    target: ThemingTarget::Gtk3,
                    outcome: ApplyOutcome::Written,
                    detail: None,
                },
                TargetResult {
                    target: ThemingTarget::Kde,
                    outcome: ApplyOutcome::Skipped,
                    detail: Some("kwriteconfig5 not found".into()),
                },
            ],
        };
        let s = serde_json::to_string(&report).unwrap();
        let back: ThemeApplyReport = serde_json::from_str(&s).unwrap();
        assert_eq!(back, report);
        assert!(s.contains("\"written\""));
        assert!(s.contains("\"skipped\""));
    }

    #[test]
    fn theming_status_serializes_inactive_without_source() {
        let status = ThemingStatus {
            available: false,
            source: None,
            targets: vec![],
        };
        let value = serde_json::to_value(&status).unwrap();
        assert!(value.get("source").is_none());
        assert_eq!(value["available"], false);
    }
}
