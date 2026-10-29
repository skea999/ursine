//! Logs screen: dynamic `/var/log` viewer (SPEC-logs) — tail with an
//! editable line count, previous chunks, and a follow (`tail -f`) mode.

use cosmic::iced::Length;
use cosmic::widget::{button, column, dropdown, row, scrollable, text, text_input, toggler};
use cosmic::Element;
use ursine_api::{default_log_lines, LogFile};

use crate::app::Message;

/// Which read produced a chunk (keeps stale responses from misfiling).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogPhase {
    Tail,
    Previous,
    Follow,
}

#[derive(Debug)]
pub struct LogsState {
    pub files: Vec<LogFile>,
    pub selected: Option<String>,
    /// Plain-text body shown in the box.
    pub content: String,
    /// Byte offsets for previous-chunk reads and follow polling.
    pub start: u64,
    pub end: u64,
    pub size: u64,
    pub has_before: bool,
    /// A tail/previous read is in flight (blocks switches).
    pub busy: bool,
    /// Follow (`tail -f`) mode.
    pub follow: bool,
    pub lines_input: String,
    pub count: u32,
    pub error: Option<String>,
}

impl Default for LogsState {
    fn default() -> Self {
        let count = default_log_lines();
        Self {
            files: Vec::new(),
            selected: None,
            content: String::new(),
            start: 0,
            end: 0,
            size: 0,
            has_before: false,
            busy: false,
            follow: false,
            lines_input: count.to_string(),
            count,
            error: None,
        }
    }
}

/// Human byte size for the file list (display only — precision loss ok).
#[allow(clippy::cast_precision_loss)]
pub fn human_size(size: u64) -> String {
    if size >= 1024 * 1024 {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    } else if size >= 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{size} B")
    }
}

/// Scroll id for the log body (snap-to-bottom in follow mode).
pub const LOGS_SCROLL_ID: &str = "ursine-logs-body";

impl LogsState {
    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2("Logs").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe((!self.busy).then_some(Message::OpenLogs))
                .into(),
        ])
        .spacing(12)
        .width(Length::Fill)
        .into()];

        if let Some(error) = &self.error {
            items.push(text(error.clone()).into());
        }
        if self.busy {
            items.push(text("Reading…").into());
        }

        // Controls: line count, previous chunk, follow.
        items.push(
            row(vec![
                text("Lines:").into(),
                text_input("200", &self.lines_input)
                    .on_input(Message::LogLinesChanged)
                    .width(Length::Fixed(96.0))
                    .into(),
                button::text("Apply")
                    .on_press_maybe((!self.busy).then_some(Message::LogLinesApply))
                    .into(),
                button::text("Load previous")
                    .on_press_maybe(
                        (self.has_before && !self.busy && self.start > 0)
                            .then_some(Message::LogLoadPrevious),
                    )
                    .into(),
                toggler(self.follow)
                    .on_toggle(Message::LogFollowToggled)
                    .into(),
                text("Follow (tail -f)").into(),
            ])
            .spacing(8)
            .align_y(cosmic::iced::Alignment::Center)
            .into(),
        );

        if self.files.is_empty() {
            if !self.busy {
                items.push(text("No log files found under /var/log.").into());
            }
            return scrollable(column(items).spacing(16).padding(24)).into();
        }

        // File picker: every file found in /var/log (dynamic discovery —
        // Refresh rescans and new files appear).
        let file_items: Vec<String> = self
            .files
            .iter()
            .map(|file| format!("{}  ({})", file.path, human_size(file.size)))
            .collect();
        let selected_index = self
            .selected
            .as_ref()
            .and_then(|sel| self.files.iter().position(|f| &f.path == sel));
        items.push(dropdown(file_items, selected_index, Message::LogsFileSelected).into());

        if let Some(selected) = &self.selected {
            let status = format!(
                "{} — {} lines shown, file {} ({})",
                selected,
                self.content.lines().count(),
                human_size(self.size),
                if self.follow { "following" } else { "paused" },
            );
            items.push(text(status).into());
            let body = if self.content.is_empty() {
                text("waiting for output…")
            } else {
                text(self.content.clone())
            };
            items.push(
                scrollable(
                    cosmic::widget::container(body)
                        .class(cosmic::theme::Container::Background)
                        .padding(4)
                        .height(Length::Fixed(420.0)),
                )
                .id(cosmic::iced::core::widget::Id::new(LOGS_SCROLL_ID))
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
    fn defaults_to_200_lines_and_formats_sizes() {
        let state = LogsState::default();
        assert_eq!(state.count, 200);
        assert_eq!(state.lines_input, "200");
        assert!(!state.follow);
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(40 * 1024), "40.0 KB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn view_renders_empty_and_loaded_states() {
        let mut state = LogsState::default();
        let _ = state.view();
        state.files.push(LogFile {
            path: "dmesg".into(),
            size: 2048,
            modified: 0,
        });
        state.selected = Some("dmesg".into());
        state.content = "line one\nline two\n".into();
        state.size = 2048;
        state.has_before = true;
        state.follow = true;
        let _ = state.view();
        state.error = Some("read failed".into());
        let _ = state.view();
    }
}
