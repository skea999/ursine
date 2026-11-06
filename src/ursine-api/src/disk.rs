//! Block-device views.

use serde::{Deserialize, Serialize};

/// Whitelisted filesystem types for `format_disk` — shared by the
/// daemon gate and the GUI dropdown (single source of truth).
pub const DISK_FSTYPES: [&str; 7] = ["ext4", "xfs", "btrfs", "vfat", "exfat", "f2fs", "ntfs"];

/// One block device or partition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Disk {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_percent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smart: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mountpoint: Option<String>,
    pub removable: bool,
    pub read_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// `list_disks` response: the device tree plus the destructive-ops
/// enablement flag from the daemon config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskView {
    #[serde(default)]
    pub disks: Vec<Disk>,
    /// `disks_format = true` in `/etc/ursine/config.toml`.
    #[serde(default)]
    pub format_enabled: bool,
}
