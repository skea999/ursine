//! Snapshot (btrbk) result types shared by daemon and frontends.

use serde::{Deserialize, Serialize};

/// One btrbk-managed snapshot, parsed from `btrbk --format raw list snapshots`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub source_subvolume: String,
    pub snapshot_subvolume: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
}

/// One row of `btrbk --format raw stats`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotStats {
    pub source_subvolume: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_subvolume: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_status: Option<String>,
    pub snapshots: u64,
    pub backups: u64,
    pub correlated: u64,
    pub orphaned: u64,
    pub incomplete: u64,
}

/// Read-only view of `/etc/btrbk/btrbk.conf`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtrbkConfigView {
    pub path: String,
    pub exists: bool,
    pub content: String,
}
