//! Crontab screen (SPEC-crontab): view and replace the session user's
//! crontab; works with busybox, cronie and vixie/Debian via `crontab`.

use cosmic::iced::Length;
use cosmic::widget::text_editor::{self, text_editor};
use cosmic::widget::{button, column, row, scrollable, text};
use cosmic::Element;

use crate::app::Message;

#[derive(Debug)]
pub struct CrontabState {
    pub busy: bool,
    pub action_busy: bool,
    /// The get request finished at least once.
    pub loaded: bool,
    /// A crontab existed when loaded (vs "none yet").
    pub had_crontab: bool,
    /// Editor buffer (source of truth; synced on every edit action).
    pub editor: text_editor::Content<cosmic::Renderer>,
    /// Plain text of the editor (what Save sends).
    pub body: String,
    /// Save confirmation step (writes happen behind confirm).
    pub confirm: bool,
    pub error: Option<String>,
    pub message: Option<String>,
}

impl Default for CrontabState {
    fn default() -> Self {
        Self {
            busy: false,
            action_busy: false,
            loaded: false,
            had_crontab: false,
            editor: text_editor::Content::new(),
            body: String::new(),
            confirm: false,
            error: None,
            message: None,
        }
    }
}

impl CrontabState {
    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2("Crontab").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe((!self.busy && !self.action_busy).then_some(Message::OpenCrontab))
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
            items.push(text("Reading crontab…").into());
        } else if self.action_busy {
            items.push(text("Saving crontab…").into());
        }

        if !self.loaded {
            if !self.busy {
                items.push(text("Crontab could not be loaded.").into());
            }
            return scrollable(column(items).spacing(16).padding(24)).into();
        }
        if !self.had_crontab && self.body.trim().is_empty() {
            items.push(text("No crontab yet for this user — write one below.").into());
        }

        items.push(
            text_editor(&self.editor)
                .on_action(|_action| Message::CrontabEdited)
                .height(Length::Fixed(360.0))
                .into(),
        );

        if self.confirm {
            items.push(
                text("Replace the crontab with the editor content? An empty body clears it.")
                    .into(),
            );
            items.push(
                row(vec![
                    button::suggested("Confirm save")
                        .on_press_maybe(
                            (!self.action_busy && !self.busy)
                                .then_some(Message::CrontabSaveConfirmed),
                        )
                        .into(),
                    button::text("Cancel")
                        .on_press(Message::CrontabSaveCancelled)
                        .into(),
                ])
                .spacing(8)
                .into(),
            );
        } else {
            items.push(
                button::suggested("Save crontab")
                    .on_press_maybe(
                        (!self.busy && !self.action_busy).then_some(Message::CrontabSaveRequested),
                    )
                    .into(),
            );
            items.push(
                text("Applied through the system `crontab` tool (busybox, cronie and Debian all work).")
                    .into(),
            );
        }

        scrollable(column(items).spacing(16).padding(24)).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_defaults_to_empty_editor() {
        let state = CrontabState::default();
        assert!(!state.loaded);
        assert!(!state.confirm);
        assert!(state.body.is_empty());
        let _ = state.view();
    }

    #[test]
    fn view_renders_loaded_and_confirm_states() {
        let mut state = CrontabState {
            loaded: true,
            had_crontab: true,
            body: "* * * * * true\n".into(),
            editor: text_editor::Content::new(),
            ..CrontabState::default()
        };
        let _ = state.view();
        state.confirm = true;
        let _ = state.view();
        state.error = Some("boom".into());
        state.message = Some("saved".into());
        let _ = state.view();
    }
}
