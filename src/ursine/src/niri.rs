//! Niri screen: schema-driven form for managed `config.kdl` sections.

use cosmic::iced::Length;
use cosmic::widget::{button, column, row, scrollable, settings, text, text_input, toggler};
use cosmic::Element;
use serde_json::json;
use ursine_api::NiriConfigView;

use crate::app::Message;

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
            sections.push(("layout".to_string(), json!({"gaps": gaps})));
        }
        sections.push((
            "animations".to_string(),
            json!({"off": self.animations_off}),
        ));
        sections.push((
            "input.touchpad".to_string(),
            json!({
                "tap": self.tap,
                "natural_scroll": self.natural_scroll,
                "dwt": self.dwt,
            }),
        ));
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

    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            button::text("Home").on_press(Message::BackHome).into(),
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
                    "Gaps (empty = leave unchanged)",
                    text_input("16", self.gaps.clone()).on_input(Message::NiriGapsChanged),
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
                .into(),
        );

        items.push(
            settings::section()
                .title("Cursor")
                .add(settings::item(
                    "Size (8..128, empty = leave unchanged)",
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
        assert_eq!(sections.len(), 4);
        state.gaps = "999".into();
        assert!(state.sections().is_err());
        state.cursor_size = "4".into();
        assert!(state.sections().is_err());
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
