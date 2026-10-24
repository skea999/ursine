//! Niri screen: schema-driven form for managed `config.kdl` sections.

use cosmic::iced::Length;
use cosmic::widget::{
    button, column, dropdown, row, scrollable, settings, text, text_input, toggler,
};
use cosmic::Element;
use serde_json::json;
use ursine_api::{NiriConfigView, NiriEnvVar, NiriKeybind, NiriOutput};

pub const BIND_ACTIONS: [&str; 16] = [
    "spawn",
    "close-window",
    "fullscreen-window",
    "toggle-windowed-fullscreen",
    "focus-column-left",
    "focus-column-right",
    "move-column-left",
    "move-column-right",
    "focus-workspace-down",
    "focus-workspace-up",
    "move-window-to-workspace-down",
    "move-window-to-workspace-up",
    "toggle-overview",
    "screenshot",
    "quit",
    "reload-config",
];

use crate::app::{EdgeField, GestureEdge, Message, StrutEdge};

#[derive(Default)]
pub struct NiriState {
    pub view: Option<NiriConfigView>,
    pub busy: bool,
    pub action_busy: bool,
    pub error: Option<String>,
    pub message: Option<String>,
    pub gaps: String,
    pub animations_off: bool,
    pub tap: bool,
    pub natural_scroll: bool,
    pub dwt: bool,
    pub cursor_size: String,
    pub cursor_theme: String,
    pub keyboard_layout: String,
    pub repeat_delay: String,
    pub repeat_rate: String,
    pub track_layout: String,
    pub numlock: bool,
    pub touchpad_accel_speed: String,
    pub touchpad_accel_profile: String,
    pub mouse_accel_speed: String,
    pub mouse_accel_profile: String,
    pub mouse_natural_scroll: bool,
    pub border_width: String,
    pub focus_ring_width: String,
    pub strut_left: String,
    pub strut_right: String,
    pub strut_top: String,
    pub strut_bottom: String,
    pub screenshot_path: String,
    pub hotkey_overlay_skip: bool,
    pub outputs: Vec<NiriOutput>,
    pub new_output: String,
    pub environment: Vec<NiriEnvVar>,
    pub new_env_name: String,
    pub new_env_value: String,
    pub spawn_text: Vec<String>,
    pub new_spawn: String,
    pub spawn_dirty: bool,
    pub keybinds: Vec<NiriKeybind>,
    pub keybind_actions: Vec<usize>,
    pub keybind_custom: Vec<String>,
    pub keybind_args: Vec<String>,
    pub keybind_dirty: bool,
    pub new_bind_key: String,
    pub new_bind_action: usize,
    pub new_bind_args: String,
    pub hot_corners: [bool; 4],
    pub hot_corners_off: bool,
    pub gesture_dirty: bool,
    pub capture: Option<usize>,
    pub edge_view_width: String,
    pub edge_view_height: String,
    pub edge_view_delay: String,
    pub edge_view_speed: String,
    pub edge_workspace_width: String,
    pub edge_workspace_height: String,
    pub edge_workspace_delay: String,
    pub edge_workspace_speed: String,
}

impl NiriState {
    pub fn apply(&mut self, view: NiriConfigView) {
        let values = &view.values;
        self.gaps = values
            .layout_gaps
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.animations_off = values.animations_off.unwrap_or(false);
        self.tap = values.touchpad_tap.unwrap_or(false);
        self.natural_scroll = values.touchpad_natural_scroll.unwrap_or(false);
        self.dwt = values.touchpad_dwt.unwrap_or(false);
        self.cursor_size = values
            .cursor_size
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.cursor_theme = values.cursor_theme.clone().unwrap_or_default();
        self.keyboard_layout = values.keyboard_layout.clone().unwrap_or_default();
        self.repeat_delay = values
            .keyboard_repeat_delay
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.repeat_rate = values
            .keyboard_repeat_rate
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.track_layout = values.keyboard_track_layout.clone().unwrap_or_default();
        self.numlock = values.keyboard_numlock.unwrap_or(false);
        self.touchpad_accel_speed = values
            .touchpad_accel_speed
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.touchpad_accel_profile = values.touchpad_accel_profile.clone().unwrap_or_default();
        self.mouse_accel_speed = values
            .mouse_accel_speed
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.mouse_accel_profile = values.mouse_accel_profile.clone().unwrap_or_default();
        self.mouse_natural_scroll = values.mouse_natural_scroll.unwrap_or(false);
        self.border_width = values
            .layout_border_width
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.focus_ring_width = values
            .layout_focus_ring_width
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.strut_left = values
            .layout_struts
            .left
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.strut_right = values
            .layout_struts
            .right
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.strut_top = values
            .layout_struts
            .top
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.strut_bottom = values
            .layout_struts
            .bottom
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.screenshot_path = values.screenshot_path.clone().unwrap_or_default();
        self.hotkey_overlay_skip = values.hotkey_overlay_skip_at_startup.unwrap_or(false);
        self.outputs.clone_from(&values.outputs);
        self.environment.clone_from(&values.environment);
        self.spawn_text = values
            .spawn_at_startup
            .iter()
            .map(|spawn| shell_join(&spawn.command))
            .collect();
        self.spawn_dirty = false;
        self.keybinds.clone_from(&values.keybinds);
        self.keybind_actions = self
            .keybinds
            .iter()
            .map(|bind| {
                bind.actions
                    .first()
                    .and_then(|action| BIND_ACTIONS.iter().position(|known| *known == action.name))
                    .unwrap_or(BIND_ACTIONS.len())
            })
            .collect();
        self.keybind_custom = self
            .keybinds
            .iter()
            .map(|bind| {
                bind.actions
                    .first()
                    .map(|action| {
                        if BIND_ACTIONS.contains(&action.name.as_str()) {
                            String::new()
                        } else {
                            action.name.clone()
                        }
                    })
                    .unwrap_or_default()
            })
            .collect();
        self.keybind_args = self
            .keybinds
            .iter()
            .map(|bind| {
                bind.actions
                    .first()
                    .map(|action| shell_join(&action.args))
                    .unwrap_or_default()
            })
            .collect();
        self.keybind_dirty = false;
        self.hot_corners = [
            values
                .gestures
                .hot_corners
                .iter()
                .any(|corner| corner == "top-left"),
            values
                .gestures
                .hot_corners
                .iter()
                .any(|corner| corner == "top-right"),
            values
                .gestures
                .hot_corners
                .iter()
                .any(|corner| corner == "bottom-left"),
            values
                .gestures
                .hot_corners
                .iter()
                .any(|corner| corner == "bottom-right"),
        ];
        self.hot_corners_off = values.gestures.hot_corners_off;
        self.edge_view_width = number_text(values.gestures.dnd_edge_view_scroll.trigger_width);
        self.edge_view_height = number_text(values.gestures.dnd_edge_view_scroll.trigger_height);
        self.edge_view_delay = number_text(values.gestures.dnd_edge_view_scroll.delay_ms);
        self.edge_view_speed = number_text(values.gestures.dnd_edge_view_scroll.max_speed);
        self.edge_workspace_width =
            number_text(values.gestures.dnd_edge_workspace_switch.trigger_width);
        self.edge_workspace_height =
            number_text(values.gestures.dnd_edge_workspace_switch.trigger_height);
        self.edge_workspace_delay = number_text(values.gestures.dnd_edge_workspace_switch.delay_ms);
        self.edge_workspace_speed =
            number_text(values.gestures.dnd_edge_workspace_switch.max_speed);
        self.gesture_dirty = false;
        self.capture = None;
        self.view = Some(view);
        self.error = None;
    }

    pub fn sections(&self) -> Result<Vec<(String, serde_json::Value)>, String> {
        let mut sections = Vec::new();
        if !self.gaps.trim().is_empty() {
            let gaps: u32 = self
                .gaps
                .trim()
                .parse()
                .map_err(|_| "Layout gaps must be a number 0..256.")?;
            if gaps > 256 {
                return Err("Layout gaps must be a number 0..256.".into());
            }
            sections.push(("layout".to_string(), layout_value(self, gaps)?));
        } else if has_layout_extras(self) {
            sections.push(("layout".to_string(), layout_value(self, 0)?));
        }
        if !self.keyboard_layout.trim().is_empty()
            || !self.repeat_delay.trim().is_empty()
            || !self.repeat_rate.trim().is_empty()
        {
            let mut value = serde_json::Map::new();
            if !self.keyboard_layout.trim().is_empty() {
                value.insert("layout".into(), json!(self.keyboard_layout.trim()));
            }
            if !self.repeat_delay.trim().is_empty() {
                value.insert(
                    "repeat_delay".into(),
                    json!(parse_number(&self.repeat_delay, "repeat delay", 5000)?),
                );
            }
            if !self.repeat_rate.trim().is_empty() {
                value.insert(
                    "repeat_rate".into(),
                    json!(parse_number(&self.repeat_rate, "repeat rate", 5000)?),
                );
            }
            if !self.track_layout.is_empty() {
                value.insert("track_layout".into(), json!(self.track_layout));
            }
            value.insert("numlock".into(), json!(self.numlock));
            sections.push((
                "input.keyboard".to_string(),
                serde_json::Value::Object(value),
            ));
        }
        if !self.screenshot_path.trim().is_empty() {
            sections.push((
                "screenshot".to_string(),
                json!({"path": self.screenshot_path.trim()}),
            ));
        }
        sections.push((
            "hotkey-overlay".to_string(),
            json!({"skip_at_startup": self.hotkey_overlay_skip}),
        ));
        if !self.outputs.is_empty() {
            sections.push(("outputs".to_string(), json!({"outputs": self.outputs})));
        }
        if !self.environment.is_empty() {
            sections.push(("environment".to_string(), json!({"vars": self.environment})));
        }
        if self.spawn_dirty {
            let mut commands = Vec::with_capacity(self.spawn_text.len());
            for text in &self.spawn_text {
                let command = shell_split(text)?;
                if !command.is_empty() {
                    commands.push(command);
                }
            }
            sections.push(("spawn".to_string(), json!({"commands": commands})));
        }
        if self.keybind_dirty {
            let mut binds = Vec::with_capacity(self.keybinds.len());
            for (index, bind) in self.keybinds.iter().enumerate() {
                let action = self
                    .keybind_actions
                    .get(index)
                    .and_then(|choice| BIND_ACTIONS.get(*choice))
                    .map(|name| (*name).to_string())
                    .or_else(|| self.keybind_custom.get(index).cloned())
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| "Choose an action for every keybind.".to_string())?;
                let args = self
                    .keybind_args
                    .get(index)
                    .filter(|text| !text.trim().is_empty())
                    .map(|text| shell_split(text))
                    .transpose()?
                    .unwrap_or_default();
                binds.push(json!({
                    "key": bind.key,
                    "actions": [{"name": action, "args": args}],
                }));
            }
            sections.push(("keybinds".to_string(), json!({"binds": binds})));
        }
        if self.gesture_dirty {
            let names = ["top-left", "top-right", "bottom-left", "bottom-right"];
            let hot_corners: Vec<&str> = names
                .iter()
                .zip(self.hot_corners)
                .filter_map(|(name, enabled)| enabled.then_some(*name))
                .collect();
            let mut gestures = serde_json::Map::new();
            gestures.insert("hot_corners".into(), json!(hot_corners));
            gestures.insert("hot_corners_off".into(), json!(self.hot_corners_off));
            for (key, edge) in [
                ("dnd_edge_view_scroll", GestureEdge::ViewScroll),
                ("dnd_edge_workspace_switch", GestureEdge::WorkspaceSwitch),
            ] {
                let mut values = serde_json::Map::new();
                for (field, raw, label) in [
                    (
                        EdgeField::TriggerWidth,
                        self.edge_value(edge, EdgeField::TriggerWidth),
                        "trigger width",
                    ),
                    (
                        EdgeField::TriggerHeight,
                        self.edge_value(edge, EdgeField::TriggerHeight),
                        "trigger height",
                    ),
                    (
                        EdgeField::Delay,
                        self.edge_value(edge, EdgeField::Delay),
                        "delay",
                    ),
                    (
                        EdgeField::MaxSpeed,
                        self.edge_value(edge, EdgeField::MaxSpeed),
                        "max speed",
                    ),
                ] {
                    if !raw.is_empty() {
                        let name = match field {
                            EdgeField::TriggerWidth => "trigger_width",
                            EdgeField::TriggerHeight => "trigger_height",
                            EdgeField::Delay => "delay_ms",
                            EdgeField::MaxSpeed => "max_speed",
                        };
                        values.insert(name.into(), json!(parse_number(raw, label, 100_000)?));
                    }
                }
                if !values.is_empty() {
                    gestures.insert(key.into(), serde_json::Value::Object(values));
                }
            }
            sections.push(("gestures".to_string(), serde_json::Value::Object(gestures)));
        }
        sections.push((
            "animations".to_string(),
            json!({"off": self.animations_off}),
        ));
        let mut touchpad = serde_json::Map::new();
        touchpad.insert("tap".into(), json!(self.tap));
        touchpad.insert("natural_scroll".into(), json!(self.natural_scroll));
        touchpad.insert("dwt".into(), json!(self.dwt));
        if !self.touchpad_accel_speed.trim().is_empty() {
            touchpad.insert(
                "accel_speed".into(),
                json!(parse_speed(&self.touchpad_accel_speed, "touchpad speed")?),
            );
        }
        if !self.touchpad_accel_profile.is_empty() {
            touchpad.insert("accel_profile".into(), json!(self.touchpad_accel_profile));
        }
        sections.push((
            "input.touchpad".to_string(),
            serde_json::Value::Object(touchpad),
        ));
        let mut mouse = serde_json::Map::new();
        if !self.mouse_accel_speed.trim().is_empty() {
            mouse.insert(
                "accel_speed".into(),
                json!(parse_speed(&self.mouse_accel_speed, "mouse speed")?),
            );
        }
        if !self.mouse_accel_profile.is_empty() {
            mouse.insert("accel_profile".into(), json!(self.mouse_accel_profile));
        }
        mouse.insert("natural_scroll".into(), json!(self.mouse_natural_scroll));
        sections.push(("input.mouse".to_string(), serde_json::Value::Object(mouse)));
        if !self.cursor_size.trim().is_empty() || !self.cursor_theme.trim().is_empty() {
            let mut value = serde_json::Map::new();
            if !self.cursor_size.trim().is_empty() {
                let size: u32 = self
                    .cursor_size
                    .trim()
                    .parse()
                    .map_err(|_| "Cursor size must be a number 8..128.")?;
                if !(8..=128).contains(&size) {
                    return Err("Cursor size must be a number 8..128.".into());
                }
                value.insert("size".into(), json!(size));
            }
            if !self.cursor_theme.trim().is_empty() {
                value.insert("theme".into(), json!(self.cursor_theme.trim()));
            }
            sections.push(("cursor".to_string(), serde_json::Value::Object(value)));
        }
        Ok(sections)
    }

    pub fn set_edge(&mut self, edge: GestureEdge, field: EdgeField, value: String) {
        let slot = match (edge, field) {
            (GestureEdge::ViewScroll, EdgeField::TriggerWidth) => &mut self.edge_view_width,
            (GestureEdge::ViewScroll, EdgeField::TriggerHeight) => &mut self.edge_view_height,
            (GestureEdge::ViewScroll, EdgeField::Delay) => &mut self.edge_view_delay,
            (GestureEdge::ViewScroll, EdgeField::MaxSpeed) => &mut self.edge_view_speed,
            (GestureEdge::WorkspaceSwitch, EdgeField::TriggerWidth) => {
                &mut self.edge_workspace_width
            }
            (GestureEdge::WorkspaceSwitch, EdgeField::TriggerHeight) => {
                &mut self.edge_workspace_height
            }
            (GestureEdge::WorkspaceSwitch, EdgeField::Delay) => &mut self.edge_workspace_delay,
            (GestureEdge::WorkspaceSwitch, EdgeField::MaxSpeed) => &mut self.edge_workspace_speed,
        };
        *slot = value;
    }

    fn edge_value(&self, edge: GestureEdge, field: EdgeField) -> &str {
        match (edge, field) {
            (GestureEdge::ViewScroll, EdgeField::TriggerWidth) => &self.edge_view_width,
            (GestureEdge::ViewScroll, EdgeField::TriggerHeight) => &self.edge_view_height,
            (GestureEdge::ViewScroll, EdgeField::Delay) => &self.edge_view_delay,
            (GestureEdge::ViewScroll, EdgeField::MaxSpeed) => &self.edge_view_speed,
            (GestureEdge::WorkspaceSwitch, EdgeField::TriggerWidth) => &self.edge_workspace_width,
            (GestureEdge::WorkspaceSwitch, EdgeField::TriggerHeight) => &self.edge_workspace_height,
            (GestureEdge::WorkspaceSwitch, EdgeField::Delay) => &self.edge_workspace_delay,
            (GestureEdge::WorkspaceSwitch, EdgeField::MaxSpeed) => &self.edge_workspace_speed,
        }
    }

    fn edge_input(
        &self,
        edge: GestureEdge,
        field: EdgeField,
        placeholder: &'static str,
    ) -> Element<'_, Message> {
        text_input(placeholder, self.edge_value(edge, field).to_string())
            .on_input(move |value| Message::NiriEdgeChanged(edge, field, value))
            .into()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2("Niri desktop").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe((!self.busy && !self.action_busy).then_some(Message::RefreshNiri))
                .into(),
        ])
        .spacing(12)
        .width(Length::Fill)
        .into()];

        if let Some(error) = &self.error {
            items.push(text(error.clone()).into());
        }
        if let Some(message) = &self.message {
            items.push(text(message.clone()).into());
        }
        if self.busy {
            items.push(text("Reading niri config…").into());
        } else if self.action_busy {
            items.push(text("Validating and writing niri config…").into());
        }

        if let Some(view) = &self.view {
            items.push(
                settings::section()
                    .title("Configuration")
                    .add(settings::item("Path", text(view.path.clone())))
                    .add(settings::item(
                        "State",
                        text(if view.exists {
                            "config.kdl exists; unknown nodes and comments are preserved"
                        } else {
                            "config.kdl is missing; saving creates it"
                        }),
                    ))
                    .add(settings::item(
                        "Validate current file",
                        button::text("niri validate").on_press_maybe(
                            (!self.action_busy).then_some(Message::ValidateNiriRequested),
                        ),
                    ))
                    .into(),
            );
        }

        items.push(
            settings::section()
                .title("Layout")
                .add(settings::item(
                    "Gaps preset",
                    dropdown(
                        vec!["unchanged", "0", "4", "8", "12", "16", "24", "32"],
                        ["", "0", "4", "8", "12", "16", "24", "32"]
                            .iter()
                            .position(|value| *value == self.gaps),
                        Message::NiriGapsPreset,
                    ),
                ))
                .add(settings::item(
                    "Gaps custom (overrides preset)",
                    text_input("16", self.gaps.clone()).on_input(Message::NiriGapsChanged),
                ))
                .add(settings::item(
                    "Border width (0..64, empty = unchanged)",
                    text_input("4", self.border_width.clone())
                        .on_input(Message::NiriBorderWidthChanged),
                ))
                .add(settings::item(
                    "Focus-ring width (0..64, empty = unchanged)",
                    text_input("4", self.focus_ring_width.clone())
                        .on_input(Message::NiriFocusRingWidthChanged),
                ))
                .add(settings::item(
                    "Struts left/right",
                    row(vec![
                        text_input("0", self.strut_left.clone())
                            .on_input(|value| Message::NiriStrutChanged(StrutEdge::Left, value))
                            .into(),
                        text_input("0", self.strut_right.clone())
                            .on_input(|value| Message::NiriStrutChanged(StrutEdge::Right, value))
                            .into(),
                    ])
                    .spacing(8),
                ))
                .add(settings::item(
                    "Struts top/bottom",
                    row(vec![
                        text_input("0", self.strut_top.clone())
                            .on_input(|value| Message::NiriStrutChanged(StrutEdge::Top, value))
                            .into(),
                        text_input("0", self.strut_bottom.clone())
                            .on_input(|value| Message::NiriStrutChanged(StrutEdge::Bottom, value))
                            .into(),
                    ])
                    .spacing(8),
                ))
                .into(),
        );

        items.push(
            settings::section()
                .title("Keyboard")
                .add(settings::item(
                    "Layout (e.g. it, us, it,us)",
                    text_input("us", self.keyboard_layout.clone())
                        .on_input(Message::NiriKeyboardLayoutChanged),
                ))
                .add(settings::item(
                    "Repeat delay (ms, empty = unchanged)",
                    text_input("600", self.repeat_delay.clone())
                        .on_input(Message::NiriRepeatDelayChanged),
                ))
                .add(settings::item(
                    "Repeat rate (chars/s, empty = unchanged)",
                    text_input("25", self.repeat_rate.clone())
                        .on_input(Message::NiriRepeatRateChanged),
                ))
                .add(settings::item(
                    "Track layout",
                    dropdown(
                        vec!["unchanged", "global", "window"],
                        ["", "global", "window"]
                            .iter()
                            .position(|value| *value == self.track_layout),
                        Message::NiriTrackLayoutChanged,
                    ),
                ))
                .add(settings::item(
                    "Num Lock at startup",
                    toggler(self.numlock).on_toggle(Message::NiriNumlockToggled),
                ))
                .into(),
        );

        items.push(
            settings::section()
                .title("Animations")
                .add(settings::item(
                    "Disable animations",
                    toggler(self.animations_off).on_toggle(Message::NiriAnimationsToggled),
                ))
                .into(),
        );

        items.push(
            settings::section()
                .title("Touchpad")
                .add(settings::item(
                    "Tap to click",
                    toggler(self.tap).on_toggle(Message::NiriTapToggled),
                ))
                .add(settings::item(
                    "Natural scroll",
                    toggler(self.natural_scroll).on_toggle(Message::NiriNaturalScrollToggled),
                ))
                .add(settings::item(
                    "Disable while typing",
                    toggler(self.dwt).on_toggle(Message::NiriDwtToggled),
                ))
                .add(settings::item(
                    "Acceleration speed",
                    dropdown(
                        vec!["unchanged", "-0.50", "-0.25", "0", "0.25", "0.50"],
                        ["", "-50", "-25", "0", "25", "50"]
                            .iter()
                            .position(|value| *value == self.touchpad_accel_speed),
                        Message::NiriTouchpadSpeedPreset,
                    ),
                ))
                .add(settings::item(
                    "Acceleration profile",
                    dropdown(
                        vec!["unchanged", "adaptive", "flat"],
                        ["", "adaptive", "flat"]
                            .iter()
                            .position(|value| *value == self.touchpad_accel_profile),
                        Message::NiriTouchpadProfileChanged,
                    ),
                ))
                .into(),
        );

        items.push(
            settings::section()
                .title("Mouse")
                .add(settings::item(
                    "Acceleration speed",
                    dropdown(
                        vec!["unchanged", "-0.50", "-0.25", "0", "0.25", "0.50"],
                        ["", "-50", "-25", "0", "25", "50"]
                            .iter()
                            .position(|value| *value == self.mouse_accel_speed),
                        Message::NiriMouseSpeedPreset,
                    ),
                ))
                .add(settings::item(
                    "Acceleration profile",
                    dropdown(
                        vec!["unchanged", "adaptive", "flat"],
                        ["", "adaptive", "flat"]
                            .iter()
                            .position(|value| *value == self.mouse_accel_profile),
                        Message::NiriMouseProfileChanged,
                    ),
                ))
                .add(settings::item(
                    "Natural scroll",
                    toggler(self.mouse_natural_scroll)
                        .on_toggle(Message::NiriMouseNaturalScrollToggled),
                ))
                .into(),
        );

        items.push(
            settings::section()
                .title("Cursor")
                .add(settings::item(
                    "Size preset",
                    dropdown(
                        vec!["unchanged", "16", "24", "32", "48", "64"],
                        ["", "16", "24", "32", "48", "64"]
                            .iter()
                            .position(|value| *value == self.cursor_size),
                        Message::NiriCursorSizePreset,
                    ),
                ))
                .add(settings::item(
                    "Size custom (8..128, overrides preset)",
                    text_input("24", self.cursor_size.clone())
                        .on_input(Message::NiriCursorSizeChanged),
                ))
                .add(settings::item(
                    "Theme (empty = leave unchanged)",
                    text_input("Adwaita", self.cursor_theme.clone())
                        .on_input(Message::NiriCursorThemeChanged),
                ))
                .into(),
        );

        let mut spawn = settings::section().title("Startup commands");
        if self.spawn_text.is_empty() {
            spawn = spawn.add(settings::item("Result", text("No startup commands.")));
        }
        for (index, command) in self.spawn_text.iter().enumerate() {
            spawn = spawn.add(settings::item(
                format!("Command {}", index + 1),
                row(vec![
                    text_input("sh -c '...'", command.clone())
                        .on_input(move |value| Message::NiriSpawnChanged(index, value))
                        .into(),
                    button::text("Remove")
                        .on_press(Message::NiriSpawnRemove(index))
                        .into(),
                ])
                .spacing(8),
            ));
        }
        spawn = spawn.add(settings::item(
            "Add command",
            row(vec![
                text_input("sh -c '...'", self.new_spawn.clone())
                    .on_input(Message::NiriSpawnInputChanged)
                    .into(),
                button::text("Add").on_press(Message::NiriSpawnAdd).into(),
            ])
            .spacing(8),
        ));
        items.push(spawn.into());

        let mut environment = settings::section().title("Environment");
        if self.environment.is_empty() {
            environment =
                environment.add(settings::item("Result", text("No environment entries.")));
        }
        for (index, var) in self.environment.iter().enumerate() {
            environment = environment.add(settings::item(
                format!("Variable {}", index + 1),
                row(vec![
                    text_input("NAME", var.name.clone())
                        .on_input(move |value| Message::NiriEnvNameChanged(index, value))
                        .into(),
                    text_input("value", var.value.clone())
                        .on_input(move |value| Message::NiriEnvValueChanged(index, value))
                        .into(),
                    button::text("Remove")
                        .on_press(Message::NiriEnvRemove(index))
                        .into(),
                ])
                .spacing(8),
            ));
        }
        environment = environment.add(settings::item(
            "Add variable",
            row(vec![
                text_input("NAME", self.new_env_name.clone())
                    .on_input(Message::NiriEnvNameInputChanged)
                    .into(),
                text_input("value", self.new_env_value.clone())
                    .on_input(Message::NiriEnvValueInputChanged)
                    .into(),
                button::text("Add").on_press(Message::NiriEnvAdd).into(),
            ])
            .spacing(8),
        ));
        items.push(environment.into());

        let mut outputs = settings::section().title("Outputs");
        if self.outputs.is_empty() {
            outputs = outputs.add(settings::item(
                "Result",
                text("No output section detected."),
            ));
        }
        for (index, output) in self.outputs.iter().enumerate() {
            outputs = outputs
                .add(settings::item(
                    format!("{} scale", output.name),
                    dropdown(
                        vec!["unchanged", "100%", "125%", "150%", "200%"],
                        ["", "100", "125", "150", "200"].iter().position(|value| {
                            output.scale.map(|scale| scale.to_string()).as_deref() == Some(*value)
                        }),
                        move |value| Message::NiriOutputScale(index, value),
                    ),
                ))
                .add(settings::item(
                    format!("{} transform", output.name),
                    dropdown(
                        vec![
                            "unchanged",
                            "normal",
                            "90",
                            "180",
                            "270",
                            "flipped",
                            "flipped-90",
                            "flipped-180",
                            "flipped-270",
                        ],
                        [
                            "",
                            "normal",
                            "90",
                            "180",
                            "270",
                            "flipped",
                            "flipped-90",
                            "flipped-180",
                            "flipped-270",
                        ]
                        .iter()
                        .position(|value| output.transform.as_deref() == Some(*value)),
                        move |value| Message::NiriOutputTransform(index, value),
                    ),
                ))
                .add(settings::item(
                    format!("{} variable refresh rate", output.name),
                    toggler(output.variable_refresh_rate.unwrap_or(false))
                        .on_toggle(move |value| Message::NiriOutputVrr(index, value)),
                ));
        }
        outputs = outputs.add(settings::item(
            "Add output section",
            row(vec![
                text_input("eDP-1", self.new_output.clone())
                    .on_input(Message::NiriOutputNameChanged)
                    .into(),
                button::text("Add").on_press(Message::NiriOutputAdd).into(),
            ])
            .spacing(8),
        ));
        items.push(outputs.into());

        items.push(
            settings::section()
                .title("Screenshot and overlay")
                .add(settings::item(
                    "Screenshot path (empty = unchanged)",
                    text_input(
                        "~/Pictures/Screenshots/%Y-%m-%d_%H-%M-%S.png",
                        self.screenshot_path.clone(),
                    )
                    .on_input(Message::NiriScreenshotPathChanged),
                ))
                .add(settings::item(
                    "Hide hotkey overlay at startup",
                    toggler(self.hotkey_overlay_skip).on_toggle(Message::NiriHotkeyOverlayToggled),
                ))
                .into(),
        );

        let mut binds = settings::section().title("Keybinds");
        if self.keybinds.is_empty() {
            binds = binds.add(settings::item(
                "Result",
                text("No keybinds in the top-level binds block."),
            ));
        }
        for (index, bind) in self.keybinds.iter().enumerate() {
            binds = binds.add(settings::item(
                format!("Keybind {}", index + 1),
                column(vec![
                    row(vec![
                        text_input("Mod+Return", bind.key.clone())
                            .on_input(move |value| Message::NiriBindKeyChanged(index, value))
                            .into(),
                        dropdown(
                            {
                                let mut options = BIND_ACTIONS.to_vec();
                                options.push("custom");
                                options
                            },
                            Some(
                                self.keybind_actions
                                    .get(index)
                                    .copied()
                                    .unwrap_or(BIND_ACTIONS.len())
                                    .min(BIND_ACTIONS.len()),
                            ),
                            move |value| Message::NiriBindActionChanged(index, value),
                        )
                        .into(),
                        button::text(if self.capture == Some(index) {
                            "Press a key…"
                        } else {
                            "Capture"
                        })
                        .on_press(Message::NiriStartCapture(index))
                        .into(),
                        button::text("Remove")
                            .on_press(Message::NiriBindRemove(index))
                            .into(),
                    ])
                    .spacing(8)
                    .into(),
                    text_input(
                        "arguments (shell syntax)",
                        self.keybind_args.get(index).cloned().unwrap_or_default(),
                    )
                    .on_input(move |value| Message::NiriBindArgsChanged(index, value))
                    .into(),
                ])
                .spacing(4),
            ));
            if self
                .keybind_actions
                .get(index)
                .is_some_and(|choice| *choice >= BIND_ACTIONS.len())
            {
                binds = binds.add(settings::item(
                    "Custom action",
                    text_input(
                        "action-name",
                        self.keybind_custom.get(index).cloned().unwrap_or_default(),
                    )
                    .on_input(move |value| Message::NiriBindCustomChanged(index, value)),
                ));
            }
        }
        binds = binds.add(settings::item(
            "Add keybind",
            column(vec![
                row(vec![
                    text_input("Mod+T", self.new_bind_key.clone())
                        .on_input(Message::NiriNewBindKeyChanged)
                        .into(),
                    dropdown(
                        {
                            let mut options = BIND_ACTIONS.to_vec();
                            options.push("custom");
                            options
                        },
                        Some(self.new_bind_action.min(BIND_ACTIONS.len())),
                        Message::NiriNewBindActionChanged,
                    )
                    .into(),
                ])
                .spacing(8)
                .into(),
                text_input("arguments", self.new_bind_args.clone())
                    .on_input(Message::NiriNewBindArgsChanged)
                    .into(),
                button::text("Add").on_press(Message::NiriBindAdd).into(),
            ])
            .spacing(4),
        ));
        items.push(binds.into());

        items.push(
            settings::section()
                .title("Gestures")
                .add(settings::item(
                    "Hot corners off",
                    toggler(self.hot_corners_off).on_toggle(Message::NiriHotCornersOffToggled),
                ))
                .add(settings::item(
                    "Top left",
                    toggler(self.hot_corners[0])
                        .on_toggle(move |value| Message::NiriHotCornerToggled(0, value)),
                ))
                .add(settings::item(
                    "Top right",
                    toggler(self.hot_corners[1])
                        .on_toggle(move |value| Message::NiriHotCornerToggled(1, value)),
                ))
                .add(settings::item(
                    "Bottom left",
                    toggler(self.hot_corners[2])
                        .on_toggle(move |value| Message::NiriHotCornerToggled(2, value)),
                ))
                .add(settings::item(
                    "Bottom right",
                    toggler(self.hot_corners[3])
                        .on_toggle(move |value| Message::NiriHotCornerToggled(3, value)),
                ))
                .add(settings::item(
                    "View-scroll trigger width/height/delay/max speed",
                    row(vec![
                        self.edge_input(GestureEdge::ViewScroll, EdgeField::TriggerWidth, "30"),
                        self.edge_input(GestureEdge::ViewScroll, EdgeField::TriggerHeight, ""),
                        self.edge_input(GestureEdge::ViewScroll, EdgeField::Delay, "100"),
                        self.edge_input(GestureEdge::ViewScroll, EdgeField::MaxSpeed, "1500"),
                    ])
                    .spacing(8),
                ))
                .add(settings::item(
                    "Workspace-switch trigger width/height/delay/max speed",
                    row(vec![
                        self.edge_input(GestureEdge::WorkspaceSwitch, EdgeField::TriggerWidth, ""),
                        self.edge_input(
                            GestureEdge::WorkspaceSwitch,
                            EdgeField::TriggerHeight,
                            "100",
                        ),
                        self.edge_input(GestureEdge::WorkspaceSwitch, EdgeField::Delay, ""),
                        self.edge_input(GestureEdge::WorkspaceSwitch, EdgeField::MaxSpeed, ""),
                    ])
                    .spacing(8),
                ))
                .into(),
        );

        items.push(
            settings::section()
                .title("Apply")
                .add(settings::item(
                    "The daemon validates the complete config before replacing it.",
                    button::suggested("Validate and save")
                        .on_press_maybe((!self.action_busy).then_some(Message::SaveNiriRequested)),
                ))
                .into(),
        );

        scrollable(column(items).spacing(16).padding(24)).into()
    }
}

fn shell_join(command: &[String]) -> String {
    command
        .iter()
        .map(|arg| {
            if !arg.is_empty()
                && arg.chars().all(|c| {
                    c.is_ascii_alphanumeric()
                        || matches!(c, '_' | '-' | '.' | '/' | ':' | '@' | '%' | '+' | '=')
                })
            {
                arg.clone()
            } else {
                let escaped = arg.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{escaped}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_split(text: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut started = false;
    for character in text.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            started = true;
            continue;
        }
        match (quote, character) {
            (Some('"') | None, '\\') => escaped = true,
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => {
                quote = Some(character);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (_, c) => {
                current.push(c);
                started = true;
            }
        }
    }
    if escaped || quote.is_some() {
        return Err("Unclosed quote or escape in startup command.".into());
    }
    if started {
        args.push(current);
    }
    if args.is_empty() {
        return Err("Startup command is empty.".into());
    }
    Ok(args)
}

fn number_text(value: Option<u32>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn has_layout_extras(state: &NiriState) -> bool {
    !state.border_width.trim().is_empty()
        || !state.focus_ring_width.trim().is_empty()
        || !state.strut_left.trim().is_empty()
        || !state.strut_right.trim().is_empty()
        || !state.strut_top.trim().is_empty()
        || !state.strut_bottom.trim().is_empty()
}

fn layout_value(state: &NiriState, gaps: u32) -> Result<serde_json::Value, String> {
    let mut value = serde_json::Map::new();
    if gaps > 0 {
        value.insert("gaps".into(), json!(gaps));
    }
    for (key, raw, label, max) in [
        ("border_width", &state.border_width, "border width", 64),
        (
            "focus_ring_width",
            &state.focus_ring_width,
            "focus ring width",
            64,
        ),
        ("strut_left", &state.strut_left, "left strut", 1024),
        ("strut_right", &state.strut_right, "right strut", 1024),
        ("strut_top", &state.strut_top, "top strut", 1024),
        ("strut_bottom", &state.strut_bottom, "bottom strut", 1024),
    ] {
        if !raw.trim().is_empty() {
            value.insert(key.into(), json!(parse_number(raw, label, max)?));
        }
    }
    Ok(serde_json::Value::Object(value))
}

fn parse_speed(raw: &str, label: &str) -> Result<i32, String> {
    let value: i32 = raw
        .trim()
        .parse()
        .map_err(|_| format!("{label} must be -100..100."))?;
    if !(-100..=100).contains(&value) {
        return Err(format!("{label} must be -100..100."));
    }
    Ok(value)
}

fn parse_number(raw: &str, label: &str, max: u32) -> Result<u32, String> {
    let value: u32 = raw
        .trim()
        .parse()
        .map_err(|_| format!("{label} must be a number 0..{max}."))?;
    if value > max {
        return Err(format!("{label} must be a number 0..{max}."));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ursine_api::NiriValues;

    #[test]
    fn builds_managed_sections_and_rejects_bad_numbers() {
        let mut state = NiriState {
            gaps: "24".into(),
            animations_off: true,
            tap: true,
            cursor_size: "32".into(),
            cursor_theme: "Adwaita".into(),
            ..NiriState::default()
        };
        let sections = state.sections().unwrap();
        assert!(sections.iter().any(|(name, _)| name == "layout"));
        assert!(sections.iter().any(|(name, _)| name == "hotkey-overlay"));
        state.gaps = "999".into();
        assert!(state.sections().is_err());
        state.cursor_size = "4".into();
        assert!(state.sections().is_err());
    }

    #[test]
    fn shell_commands_roundtrip() {
        let args = vec!["sh".to_string(), "-c".to_string(), "echo hi".to_string()];
        let text = shell_join(&args);
        assert_eq!(shell_split(&text).unwrap(), args);
    }

    #[test]
    fn applies_view_values() {
        let mut state = NiriState::default();
        state.apply(NiriConfigView {
            path: "/home/alice/.config/niri/config.kdl".into(),
            exists: true,
            content: String::new(),
            values: NiriValues {
                layout_gaps: Some(16),
                animations_off: Some(true),
                cursor_size: Some(24),
                ..NiriValues::default()
            },
        });
        assert_eq!(state.gaps, "16");
        assert!(state.animations_off);
        assert_eq!(state.cursor_size, "24");
    }
}
