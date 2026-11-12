//! Shared section/row builders for the three package managers.
//!
//! apk, flatpak and nix must look and behave the same (user decision):
//! the callers inject their data and messages, this module owns the
//! layout so the three views cannot drift apart.

use cosmic::iced::Length;
use cosmic::widget::{button, row, selectable_text, settings, toggler};
use cosmic::Element;

use crate::app::Message;

/// One repository/remote/registry entry in the apk style: the title is
/// the item label; the control row holds the optional detail text on
/// the left and the action buttons on the right.
pub fn repository_row(
    title: String,
    detail: Option<String>,
    actions: Vec<Element<'_, Message>>,
) -> Element<'_, Message> {
    let mut cells: Vec<Element<'_, Message>> = Vec::with_capacity(actions.len() + 1);
    if let Some(detail) = detail {
        cells.push(selectable_text(detail).width(Length::Fill).into());
    }
    cells.extend(actions);
    settings::item(title, row(cells).spacing(8)).into()
}

/// The three Upgrade-section control rows every manager shares:
/// persisted auto toggler, the prominent count, the manual refresh.
pub fn update_control_items(
    auto: bool,
    on_toggle: impl Fn(bool) -> Message + 'static,
    number: Element<'_, Message>,
    refresh_message: Message,
    refresh_armed: bool,
) -> Vec<Element<'_, Message>> {
    let refresh: Element<'_, Message> = button::text("Check for updates")
        .on_press_maybe(refresh_armed.then_some(refresh_message))
        .into();
    vec![
        settings::item(
            "Check updates automatically",
            toggler(auto).on_toggle(on_toggle),
        )
        .into(),
        settings::item("Packages to update", number).into(),
        settings::item("Refresh", refresh).into(),
    ]
}
