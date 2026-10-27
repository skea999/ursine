//! Printer and print-job views.

use serde::{Deserialize, Serialize};

/// One CUPS printer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Printer {
    pub name: String,
    pub state: String,
    pub is_default: bool,
    pub accepting: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// One queued print job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrintJob {
    pub id: String,
    pub printer: String,
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}
