//! Shared package row layout for all package backends.

use cosmic::iced::Length;
use cosmic::widget::{column, row, text};
use cosmic::Element;

use crate::app::Message;

/// Title + description stacked at one left edge, action on the right.
pub fn package_row<'a>(
    title: String,
    description: Element<'a, Message>,
    action: Element<'a, Message>,
) -> Element<'a, Message> {
    row(vec![
        column(vec![text(title).into(), description])
            .spacing(2)
            .width(Length::Fill)
            .into(),
        action,
    ])
    .spacing(12)
    .into()
}
