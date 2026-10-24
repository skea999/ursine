//! Bluetooth adapter and device views.

use serde::{Deserialize, Serialize};

/// Bluetooth adapter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BluetoothAdapter {
    pub address: String,
    pub name: String,
    pub powered: bool,
    pub discoverable: bool,
}

/// Known Bluetooth device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BluetoothDevice {
    pub address: String,
    pub name: String,
    pub connected: bool,
    pub paired: bool,
    pub trusted: bool,
}

/// Adapter plus known devices.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BluetoothView {
    #[serde(default)]
    pub adapter: BluetoothAdapter,
    #[serde(default)]
    pub devices: Vec<BluetoothDevice>,
}

/// Device operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BluetoothOp {
    Pair,
    Connect,
    Disconnect,
    Trust,
    Untrust,
    Forget,
}
