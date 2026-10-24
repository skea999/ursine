//! SANE scanner discovery and airscan configuration.

use serde::{Deserialize, Serialize};

/// One SANE scanner discovered by `scanimage -L`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scanner {
    pub id: String,
    pub description: String,
}

/// One manually configured airscan device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AirscanDevice {
    pub name: String,
    pub address: String,
    pub protocol: String,
}

/// View of `/etc/sane.d/airscan.conf`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScannerConfigView {
    pub path: String,
    pub exists: bool,
    pub discovery_disabled: bool,
    #[serde(default)]
    pub devices: Vec<AirscanDevice>,
    #[serde(default)]
    pub detected: Vec<Scanner>,
}
