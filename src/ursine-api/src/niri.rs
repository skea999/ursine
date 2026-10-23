//! Niri config view shared by daemon and GUI.

use serde::{Deserialize, Serialize};

/// Form values for the managed niri sections.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NiriValues {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_gaps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animations_off: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub touchpad_tap: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub touchpad_natural_scroll: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub touchpad_dwt: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_theme: Option<String>,
}

/// Parsed view of a user's niri config.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NiriConfigView {
    pub path: String,
    pub exists: bool,
    pub content: String,
    #[serde(default)]
    pub values: NiriValues,
}
