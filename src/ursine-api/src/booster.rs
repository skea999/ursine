//! Booster initramfs payloads (SPEC-booster).
//!
//! Booster has no configuration CLI: the daemon edits `/etc/booster.yaml`
//! surgically and regenerates through the distro's own kernel hooks. All
//! values are the raw YAML scalar text (booster lists are comma-joined
//! scalars, booleans are `true`/`false`/`on`/`off` tokens).

use serde::{Deserialize, Serialize};

/// Known keys of `/etc/booster.yaml` as read from disk (`None` = absent).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BoosterConfig {
    /// File present on disk (a fresh install may have none).
    pub exists: bool,
    /// Full file contents, byte-identical.
    pub raw: String,
    /// Chain step `booster_regenerate` will run (computed live).
    #[serde(default)]
    pub regenerate_step: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compression: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub universal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vconsole: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_lvm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_mdraid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append_all_modaliases: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount_timeout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_timeout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_delay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_files: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modules: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modules_force_load: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "network.dhcp")]
    pub network_dhcp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "network.ip")]
    pub network_ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "network.gateway")]
    pub network_gateway: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "network.dns_servers")]
    pub network_dns_servers: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "network.interfaces")]
    pub network_interfaces: Option<String>,
}

/// Partial update of the known keys. `None` leaves a key untouched;
/// `clear` removes the named known keys (their lines disappear).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BoosterConfigPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compression: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub universal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vconsole: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_lvm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_mdraid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append_all_modaliases: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount_timeout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_timeout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_delay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_files: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modules: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modules_force_load: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "network.dhcp")]
    pub network_dhcp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "network.ip")]
    pub network_ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "network.gateway")]
    pub network_gateway: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "network.dns_servers")]
    pub network_dns_servers: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "network.interfaces")]
    pub network_interfaces: Option<String>,
    /// Known key names (or `network.<key>`) to remove from the file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clear: Vec<String>,
}

/// Result of `booster_regenerate`: the chain step that ran plus its
/// merged stdout/stderr (hook output included).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoosterRegenerateReport {
    /// Human label of the step (`apk fix kernel-hooks`, ...).
    pub step: String,
    /// Combined process output, trimmed.
    pub output: String,
}
