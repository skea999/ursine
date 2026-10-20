//! Nix store: the target user's installed profile packages and generations.

use cosmic::widget::{button, column, row, scrollable, settings, text};
use cosmic::Element;
use ursine_api::InstalledPackage;

use crate::app::Message;

#[derive(Default)]
pub struct StoreState {
    pub packages: Vec<InstalledPackage>,
    pub generations: Vec<String>,
    pub busy: bool,
    pub error: Option<String>,
}

impl StoreState {
    pub fn view(&self) -> Element<'_, Message> {
        let header = row(vec![
            button::text("Home").on_press(Message::BackHome).into(),
            text::title2("Nix store").into(),
            button::standard(if self.busy {
                "Refreshing…"
            } else {
                "Refresh"
            })
            .on_press_maybe((!self.busy).then_some(Message::RefreshStore))
            .into(),
        ])
        .spacing(12);

        let mut items: Vec<Element<'_, Message>> = vec![
            header.into(),
            text("Installed packages and profile generations for this user.").into(),
        ];

        if let Some(error) = &self.error {
            items.push(text(error.clone()).into());
        }
        if self.busy && self.packages.is_empty() {
            items.push(text("Loading profile…").into());
        }

        let mut installed =
            settings::section().title(format!("Installed ({})", self.packages.len()));
        if self.packages.is_empty() {
            installed = installed.add(settings::item(
                "No packages",
                text("This profile has no installed packages."),
            ));
        } else {
            for package in &self.packages {
                installed = installed.add(settings::item(
                    package.name.clone(),
                    package_detail(package),
                ));
            }
        }
        items.push(installed.into());

        let mut generations =
            settings::section().title(format!("Generations ({})", self.generations.len()));
        if self.generations.is_empty() {
            generations = generations.add(settings::item(
                "No history",
                text("No profile generations reported."),
            ));
        } else {
            for (index, line) in self.generations.iter().enumerate() {
                generations = generations.add(settings::item(
                    format!("Generation {}", index + 1),
                    text(line.clone()),
                ));
            }
        }
        items.push(generations.into());

        scrollable(column(items).spacing(16).padding(24)).into()
    }
}

fn package_detail(package: &InstalledPackage) -> Element<'_, Message> {
    let mut lines: Vec<Element<'_, Message>> = Vec::new();
    if let Some(attr) = &package.attr_path {
        lines.push(text(attr.clone()).into());
    }
    if let Some(url) = &package.url {
        lines.push(text(url.clone()).into());
    }
    if !package.active {
        lines.push(text("inactive").into());
    }
    if lines.is_empty() {
        lines.push(text("installed").into());
    }
    column(lines).spacing(2).into()
}
