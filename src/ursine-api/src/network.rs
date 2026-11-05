//! `NetworkManager` devices, connections and Wi-Fi.

use serde::{Deserialize, Serialize};

/// One network interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkDevice {
    pub device: String,
    pub kind: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    #[serde(default)]
    pub ip4: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    #[serde(default)]
    pub dns: Vec<String>,
    /// IPv4 settings of the device's connection profile (when known).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv4_settings: Option<NetworkIpv4Settings>,
}

/// IPv4 configuration of one connection profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkIpv4Settings {
    /// `NetworkManager` method (`auto`, `manual`, …).
    pub method: String,
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    #[serde(default)]
    pub dns: Vec<String>,
}

/// One visible Wi-Fi network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: u8,
    pub security: String,
    pub active: bool,
}

/// One saved connection profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkProfile {
    pub name: String,
    /// `NetworkManager` type (`802-11-wireless`, `vpn`, `wireguard`, …).
    pub kind: String,
    pub active: bool,
}

/// `NetworkManager` state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkView {
    pub wifi_enabled: bool,
    #[serde(default)]
    pub devices: Vec<NetworkDevice>,
    #[serde(default)]
    pub wifi: Vec<WifiNetwork>,
    /// Saved connection profiles (for forget and VPN controls).
    #[serde(default)]
    pub profiles: Vec<NetworkProfile>,
}
