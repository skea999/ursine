//! Alpine and Flatpak package views.

use serde::{Deserialize, Serialize};

/// One APK package row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApkPackage {
    /// Full `name-version` identifier accepted by `apk add`.
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub protected: bool,
    /// Repository tail (e.g. `main`) the installed version came from
    /// (from `apk policy`; None for locally installed packages).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

/// One pending APK update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApkUpdate {
    pub name: String,
    pub installed: String,
    pub available: String,
}

/// One APK repository line (`apk-repositories(5)`: `[@tag] url`).
/// `tag = Some("edge")` means the line is `@edge <url>` — a MASKED
/// repository that apk ignores unless a world entry pins `pkg@edge`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApkRepository {
    /// Everything after the optional tag (URL, optional components).
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// One configured Flatpak remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatpakRemote {
    pub name: String,
    pub url: String,
}

/// Resolved Flatpak ref details for one remote (`flatpak remote-info`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatpakInfo {
    pub app_id: String,
    pub remote: String,
    /// Human-readable name from the remote summary (best effort).
    pub name: String,
    pub version: String,
    /// Size as printed by flatpak (e.g. `129.7 MB`).
    pub download_size: String,
    /// Size as printed by flatpak (e.g. `345.7 MB`).
    pub installed_size: String,
    pub runtime: String,
}

/// One environment variable override (`[Environment] NAME=VALUE`; an
/// empty value unsets the variable in the sandbox).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatpakEnvVar {
    pub name: String,
    pub value: String,
}

/// Editable Flatpak override state for one app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatpakPermissions {
    pub app_id: String,
    /// Whether an override file exists for this app.
    pub exists: bool,
    pub sockets: Vec<String>,
    pub devices: Vec<String>,
    pub filesystems: Vec<String>,
    pub env: Vec<FlatpakEnvVar>,
    /// Raw `flatpak info --show-permissions` (effective sandbox view).
    pub effective: String,
}

/// What a Flatpak ref is: an application or a runtime.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlatpakRefKind {
    #[default]
    App,
    Runtime,
}

/// One Flatpak application row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatpakPackage {
    pub app_id: String,
    pub name: String,
    pub version: String,
    pub branch: String,
    pub origin: String,
    pub system: bool,
    #[serde(default)]
    pub kind: FlatpakRefKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatpak_row_without_kind_deserializes_as_app() {
        // Old daemons omit `kind`: rows must still parse for new clients.
        let row: FlatpakPackage = serde_json::from_value(serde_json::json!({
            "app_id": "org.mozilla.firefox",
            "name": "Firefox",
            "version": "158.0.1",
            "branch": "stable",
            "origin": "flathub",
            "system": true
        }))
        .unwrap();
        assert_eq!(row.kind, FlatpakRefKind::App);
        let runtime: FlatpakPackage = serde_json::from_value(serde_json::json!({
            "app_id": "org.freedesktop.Platform",
            "name": "Freedesktop",
            "version": "26.08",
            "branch": "26.08",
            "origin": "flathub",
            "system": false,
            "kind": "runtime"
        }))
        .unwrap();
        assert_eq!(runtime.kind, FlatpakRefKind::Runtime);
    }
}
