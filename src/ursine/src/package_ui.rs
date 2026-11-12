//! Shared section/row builders for the three package managers.
//!
//! apk, flatpak and nix must look and behave the same (user decision):
//! the callers inject their data and messages, this module owns the
//! layout so the three views cannot drift apart.

use cosmic::iced::Length;
use cosmic::widget::{button, row, selectable_text, settings, text, text_input, toggler};
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

/// Top-of-view status shared by the three managers: a copyable error,
/// then the last-action result (nix schema).
pub fn status_items(error: Option<&str>, message: Option<&str>) -> Vec<Element<'static, Message>> {
    let mut items = Vec::new();
    if let Some(error) = error {
        items.push(selectable_text(error.to_string()).into());
    }
    if let Some(message) = message {
        items.push(
            settings::section()
                .title("Last action")
                .add(settings::item(
                    "Result",
                    selectable_text(message.to_string()),
                ))
                .into(),
        );
    }
    items
}

/// Inputs of the shared Pattern-actions section (nix schema).
pub struct PatternSection {
    /// Hint shown in the "Package pattern" row.
    pub hint: String,
    pub regex: String,
    /// Payload variant used as the input callback (`Fn(String)`).
    pub on_input: fn(String) -> Message,
    pub preview: Message,
    pub upgrade: Message,
    pub remove: Message,
    /// Buttons fire only while armed (pattern present, nothing busy).
    pub armed: bool,
}

/// The Pattern-actions block every manager shares: hint, regex input,
/// then Preview/Upgrade/Remove in one row.
pub fn pattern_section(spec: PatternSection) -> Element<'static, Message> {
    settings::section()
        .title("Pattern actions")
        .add(settings::item("Package pattern", text(spec.hint)))
        .add(settings::item(
            "Regex",
            text_input("Regex, e.g. ^python", spec.regex)
                .on_input(spec.on_input)
                .width(Length::Fill),
        ))
        .add(settings::item(
            "Actions",
            row(vec![
                button::text("Preview matches")
                    .on_press_maybe(spec.armed.then_some(spec.preview))
                    .into(),
                button::text("Upgrade matching")
                    .on_press_maybe(spec.armed.then_some(spec.upgrade))
                    .into(),
                button::text("Remove matching")
                    .on_press_maybe(spec.armed.then_some(spec.remove))
                    .into(),
            ])
            .spacing(8),
        ))
        .into()
}

/// The destructive confirmation block every manager shares (nix
/// schema): Target + label + Confirm/Cancel, rendered at the top of
/// the view while an action is pending.
pub fn confirm_section(
    target: String,
    label: String,
    confirm: Message,
    cancel: Message,
    armed: bool,
    // Red button for destructive confirms; nix keeps its original
    // suggested styling (its pending block also covers installs).
    destructive: bool,
) -> Element<'static, Message> {
    let confirm_button: Element<'static, Message> = if destructive {
        button::destructive("Confirm")
            .on_press_maybe(armed.then_some(confirm))
            .into()
    } else {
        button::suggested("Confirm")
            .on_press_maybe(armed.then_some(confirm))
            .into()
    };
    settings::section()
        .title("Confirm action")
        .add(settings::item("Target", text(target)))
        .add(settings::item(
            label,
            row(vec![
                confirm_button,
                button::text("Cancel").on_press(cancel).into(),
            ])
            .spacing(8),
        ))
        .into()
}
