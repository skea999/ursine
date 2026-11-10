//! `AppArmor` payloads (SPEC-apparmor).

use serde::{Deserialize, Serialize};

/// One loaded profile with its mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparmorProfile {
    pub name: String,
    /// `enforce` or `complain` (as reported by aa-status).
    pub mode: String,
}

/// One profile file under `/etc/apparmor.d`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparmorFile {
    /// File name (basename).
    pub name: String,
    /// Absolute path.
    pub path: String,
    /// Present in `/etc/apparmor.d/disable/` (skipped by reload-all).
    pub disabled: bool,
    /// A loaded profile of this name exists.
    pub loaded: bool,
    /// Mode of the matching loaded profile (`enforce`/`complain`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// Everything the `AppArmor` panel shows in one view.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ApparmorView {
    /// Kernel module loaded.
    pub enabled: bool,
    /// `AppArmor` securityfs mounted.
    pub mounted: bool,
    pub profiles: Vec<ApparmorProfile>,
    pub files: Vec<ApparmorFile>,
    pub processes_defined: usize,
    pub processes_enforce: usize,
    pub processes_complain: usize,
    /// `show_notifications="yes"` in /etc/apparmor/notify.conf.
    pub notifications_configured: bool,
    /// aa-notify autostart entry installed for the session user.
    pub notifications_autostart: bool,
    /// aa-notify binary present.
    pub aa_notify_present: bool,
}

/// Raw profile file content for the editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparmorProfileFile {
    pub path: String,
    pub content: String,
}

/// Raw command output (unconfined list, features-abi, decode).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparmorOutput {
    pub output: String,
}

/// Result of a syntax check (`apparmor_parser -d`): the parser's own
/// text comes back either way so the GUI can show the exact error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparmorCheck {
    pub ok: bool,
    pub output: String,
}
