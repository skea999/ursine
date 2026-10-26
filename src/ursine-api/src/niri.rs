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
    pub touchpad_accel_speed: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub touchpad_accel_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mouse_accel_speed: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mouse_accel_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mouse_natural_scroll: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_theme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard_layout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard_repeat_delay: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard_repeat_rate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard_track_layout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard_numlock: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_border_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_focus_ring_width: Option<u32>,
    #[serde(default)]
    pub layout_struts: NiriStruts,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotkey_overlay_skip_at_startup: Option<bool>,
    #[serde(default)]
    pub outputs: Vec<NiriOutput>,
    #[serde(default)]
    pub environment: Vec<NiriEnvVar>,
    #[serde(default)]
    pub spawn_at_startup: Vec<NiriSpawn>,
    #[serde(default)]
    pub keybinds: Vec<NiriKeybind>,
    #[serde(default)]
    pub gestures: NiriGestures,
}

/// One niri key binding.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NiriKeybind {
    pub key: String,
    #[serde(default)]
    pub actions: Vec<NiriBindAction>,
}

/// One action inside a key binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NiriBindAction {
    pub name: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// Managed niri gesture settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NiriGestures {
    #[serde(default)]
    pub hot_corners: Vec<String>,
    #[serde(default)]
    pub hot_corners_off: bool,
    #[serde(default)]
    pub dnd_edge_view_scroll: NiriEdgeGesture,
    #[serde(default)]
    pub dnd_edge_workspace_switch: NiriEdgeGesture,
}

/// Edge-scroll gesture tuning.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NiriEdgeGesture {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_speed: Option<u32>,
}

/// One managed `spawn-at-startup` command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NiriSpawn {
    pub command: Vec<String>,
}

/// One managed niri environment variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NiriEnvVar {
    pub name: String,
    pub value: String,
}

/// One managed niri output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NiriOutput {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<NiriPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_refresh_rate: Option<bool>,
}

/// Physical position of an output in the layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NiriPosition {
    pub x: i32,
    pub y: i32,
}

/// Logical-pixel struts around the layout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NiriStruts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<u32>,
}

/// Parsed view of a user's niri config.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NiriConfigView {
    pub path: String,
    pub exists: bool,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub values: NiriValues,
}
