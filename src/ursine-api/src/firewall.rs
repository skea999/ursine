//! firewalld zones and rules.

use serde::{Deserialize, Serialize};

/// One firewalld zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirewallZone {
    pub name: String,
    pub default: bool,
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub ports: Vec<String>,
}

/// Live firewall state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirewallStatus {
    pub running: bool,
    pub default_zone: String,
    #[serde(default)]
    pub zones: Vec<FirewallZone>,
}
