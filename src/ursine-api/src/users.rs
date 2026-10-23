//! User and group views shared by daemon and GUI.

use serde::{Deserialize, Serialize};

/// A human account for the Users panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSummary {
    pub uid: u32,
    pub name: String,
    pub fullname: String,
    pub shell: String,
    pub home: String,
    #[serde(default)]
    pub groups: Vec<String>,
}
