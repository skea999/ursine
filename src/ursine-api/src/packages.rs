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
}

/// One pending APK update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApkUpdate {
    pub name: String,
    pub installed: String,
    pub available: String,
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

/// One Flatpak application row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatpakPackage {
    pub app_id: String,
    pub name: String,
    pub version: String,
    pub branch: String,
    pub origin: String,
    pub system: bool,
}
