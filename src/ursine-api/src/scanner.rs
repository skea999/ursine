//! Scanner devices and scan options.

use serde::{Deserialize, Serialize};

/// One SANE scanner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scanner {
    pub id: String,
    pub description: String,
}

/// Result of a completed scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanResult {
    pub path: String,
}
