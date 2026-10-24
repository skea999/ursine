//! Init service views (`OpenRC` today, dinit-ready).

use serde::{Deserialize, Serialize};

/// One init service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Service {
    pub name: String,
    pub state: String,
    pub enabled: bool,
    pub init: String,
}

/// Service operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceOp {
    Start,
    Stop,
    Restart,
    Enable,
    Disable,
}
