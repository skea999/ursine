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
}

/// One pending APK update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApkUpdate {
    pub name: String,
    pub installed: String,
    pub available: String,
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
