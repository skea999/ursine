//! firewalld zones and rules.

use serde::{Deserialize, Serialize};

/// One firewalld zone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirewallZone {
    pub name: String,
    pub default: bool,
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub ports: Vec<String>,
    /// Zone target (`default`, `ACCEPT`, `REJECT`, `DROP`).
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub interfaces: Vec<String>,
    #[serde(default)]
    pub rich_rules: Vec<String>,
}

/// Live firewall state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirewallStatus {
    pub running: bool,
    pub default_zone: String,
    #[serde(default)]
    pub zones: Vec<FirewallZone>,
    /// Filtering on the default zone (target `ACCEPT` = off).
    #[serde(default)]
    pub enabled: bool,
    /// Service definitions under `/etc/firewalld/services`.
    #[serde(default)]
    pub custom_services: Vec<String>,
}
