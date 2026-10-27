//! APK and Flatpak package panels.

use cosmic::iced::Length;
use cosmic::widget::{button, column, dropdown, row, scrollable, settings, text, text_input};
use cosmic::Element;
use ursine_api::{ApkPackage, ApkUpdate, FlatpakPackage};

use crate::app::Message;
use crate::package_row::package_row;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    Apk,
    Flatpak,
}

impl PackageKind {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Apk => "APK packages",
            Self::Flatpak => "Flatpak applications",
        }
    }

    const fn search_placeholder(self) -> &'static str {
        match self {
            Self::Apk => "e.g. niri",
            Self::Flatpak => "e.g. firefox",
        }
    }
}

pub struct PackagesState {
    pub kind: Option<PackageKind>,
    pub apk: Vec<ApkPackage>,
    pub apk_updates: Vec<ApkUpdate>,
    pub apk_repositories: Vec<String>,
    pub flatpak: Vec<FlatpakPackage>,
    pub system: bool,
    pub query: String,
    pub install: String,
    pub busy: bool,
    pub action_busy: bool,
    pub searching: bool,
    pub error: Option<String>,
    pub message: Option<String>,
    pub pending_remove: Option<(PackageKind, String, String)>,
    pub pending_update: Option<PackageKind>,
    pub pending_orphans: bool,
}

impl Default for PackagesState {
    fn default() -> Self {
        Self {
            kind: None,
            apk: Vec::new(),
            apk_updates: Vec::new(),
            apk_repositories: Vec::new(),
            flatpak: Vec::new(),
            system: true,
            query: String::new(),
            install: String::new(),
            busy: false,
            action_busy: false,
            searching: false,
            error: None,
            message: None,
            pending_remove: None,
            pending_update: None,
            pending_orphans: false,
        }
    }
}

impl PackagesState {
    pub fn open(&mut self, kind: PackageKind) {
        if self.kind != Some(kind) {
            self.kind = Some(kind);
            self.query.clear();
            self.install.clear();
            self.apache_clear();
        }
    }

    fn apache_clear(&mut self) {
        self.apk.clear();
        self.apk_updates.clear();
        self.apk_repositories.clear();
        self.flatpak.clear();
        self.searching = false;
        self.error = None;
        self.message = None;
        self.pending_remove = None;
        self.pending_update = None;
        self.pending_orphans = false;
    }

    pub fn apply_apk(&mut self, packages: Vec<ApkPackage>) {
        self.apk = packages;
        self.flatpak.clear();
        self.error = None;
    }

    pub fn apply_apk_updates(&mut self, updates: Vec<ApkUpdate>) {
        self.apk_updates = updates;
    }

    pub fn apply_apk_repositories(&mut self, repositories: Vec<String>) {
        self.apk_repositories = repositories;
    }

    pub fn apply_flatpak(&mut self, packages: Vec<FlatpakPackage>) {
        self.flatpak = packages;
        self.apk.clear();
        self.error = None;
    }

    pub fn view(&self) -> Element<'_, Message> {
        let Some(kind) = self.kind else {
            return text("Select a package source.").into();
        };
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2(kind.title()).into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe(
                    (!self.busy && !self.action_busy).then_some(Message::PackageRefresh),
                )
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
            items.push(text("Reading package state…").into());
        } else if self.action_busy {
            items.push(text("Running package action…").into());
        }

        items.push(
            settings::section()
                .title("Scope")
                .add(settings::item(
                    "Installation",
                    if kind == PackageKind::Flatpak {
                        Element::from(dropdown(
                            vec!["System", "User"],
                            Some(usize::from(!self.system)),
                            Message::PackageScopeChanged,
                        ))
                    } else {
                        Element::from(text("system-wide"))
                    },
                ))
                .into(),
        );

        items.push(
            settings::section()
                .title("Maintenance")
                .add(settings::item(
                    match kind {
                        PackageKind::Apk => "Refresh repository indexes",
                        PackageKind::Flatpak => "Remove unused runtimes",
                    },
                    button::text(match kind {
                        PackageKind::Apk => "Refresh indexes",
                        PackageKind::Flatpak => "Remove orphans…",
                    })
                    .on_press_maybe((!self.action_busy).then_some(
                        match kind {
                            PackageKind::Apk => Message::PackageRefreshIndexes,
                            PackageKind::Flatpak => Message::PackageOrphansRequested,
                        },
                    )),
                ))
                .into(),
        );

        let mut upgrade = settings::section().title("Upgrade").add(settings::item(
            "Upgrade every installed item",
            button::suggested("Upgrade all")
                .on_press_maybe((!self.action_busy).then_some(Message::PackageUpdateRequested)),
        ));
        if kind == PackageKind::Apk && !self.apk_repositories.is_empty() {
            upgrade = upgrade.add(settings::item(
                "Repositories",
                column(
                    self.apk_repositories
                        .iter()
                        .map(|repository| text(repository.clone()).into())
                        .collect::<Vec<Element<'_, Message>>>(),
                )
                .spacing(2),
            ));
        }
        if kind == PackageKind::Apk && !self.apk_updates.is_empty() {
            upgrade = upgrade.add(settings::item(
                format!("Available updates ({})", self.apk_updates.len()),
                column(
                    self.apk_updates
                        .iter()
                        .map(|update| {
                            row(vec![
                                text(format!(
                                    "{} {} -> {}",
                                    update.name, update.installed, update.available
                                ))
                                .width(Length::Fill)
                                .into(),
                                button::text("Upgrade")
                                    .on_press(Message::ApkUpgradeRequested(update.name.clone()))
                                    .into(),
                            ])
                            .spacing(8)
                            .into()
                        })
                        .collect::<Vec<Element<'_, Message>>>(),
                )
                .spacing(4),
            ));
        }
        items.push(upgrade.into());

        items.push(
            settings::section()
                .title("Search")
                .add(settings::item(
                    "Query",
                    row(vec![
                        text_input(kind.search_placeholder(), self.query.clone())
                            .on_input(Message::PackageQueryChanged)
                            .into(),
                        button::text("Search")
                            .on_press_maybe(
                                (!self.busy && !self.action_busy)
                                    .then_some(Message::PackageSearchRequested),
                            )
                            .into(),
                    ])
                    .spacing(8),
                ))
                .add(settings::item(
                    "Direct install",
                    row(vec![
                        text_input(
                            match kind {
                                PackageKind::Apk => "niri",
                                PackageKind::Flatpak => "org.mozilla.firefox",
                            },
                            self.install.clone(),
                        )
                        .on_input(Message::PackageInstallChanged)
                        .into(),
                        button::text("Install")
                            .on_press_maybe(
                                (!self.action_busy).then_some(Message::PackageInstallRequested),
                            )
                            .into(),
                    ])
                    .spacing(8),
                ))
                .into(),
        );

        let mut list = settings::section().title(if self.searching {
            "Search results"
        } else {
            "Installed"
        });
        match kind {
            PackageKind::Apk => {
                for package in &self.apk {
                    let name = package.name.clone();
                    let action: Element<'_, Message> = if self.searching {
                        button::text("Install")
                            .on_press(Message::PackageInstallDirect(package.name.clone()))
                            .into()
                    } else if package.protected {
                        text("protected").into()
                    } else {
                        row(vec![
                            button::text("Upgrade")
                                .on_press(Message::ApkUpgradeRequested(name.clone()))
                                .into(),
                            button::text("Remove")
                                .on_press(Message::PackageRemoveRequested(kind, name.clone(), name))
                                .into(),
                        ])
                        .spacing(8)
                        .into()
                    };
                    let description = package.description.clone().map_or_else(
                        || text("no description").into(),
                        |description| text(description).into(),
                    );
                    list = list.add(package_row(
                        format!("{} {}", package.name, package.version),
                        description,
                        action,
                    ));
                }
            }
            PackageKind::Flatpak => {
                for package in &self.flatpak {
                    let app_id = package.app_id.clone();
                    let action: Element<'_, Message> = if self.searching {
                        button::text("Install")
                            .on_press(Message::PackageInstallDirect(app_id.clone()))
                            .into()
                    } else {
                        row(vec![
                            button::text("Upgrade")
                                .on_press(Message::FlatpakUpgradeRequested(app_id.clone()))
                                .into(),
                            button::text("Remove")
                                .on_press(Message::PackageRemoveRequested(
                                    kind,
                                    app_id.clone(),
                                    app_id,
                                ))
                                .into(),
                        ])
                        .spacing(8)
                        .into()
                    };
                    let description = if package.origin.is_empty() {
                        format!("{} · {}", package.app_id, package.branch)
                    } else {
                        format!(
                            "{} · {} · {}",
                            package.app_id, package.branch, package.origin
                        )
                    };
                    list = list.add(package_row(
                        format!("{} {}", package.name, package.version),
                        text(description).into(),
                        action,
                    ));
                }
            }
        }
        items.push(list.into());

        if let Some((kind, name, _id)) = &self.pending_remove {
            items.push(
                settings::section()
                    .title("Confirm removal")
                    .add(settings::item(
                        "Consequences",
                        text(format!("Remove {name} from {}?", kind.title())),
                    ))
                    .add(settings::item(
                        "Actions",
                        row(vec![
                            button::destructive("Remove")
                                .on_press_maybe(
                                    (!self.action_busy).then_some(Message::PackageRemoveConfirmed),
                                )
                                .into(),
                            button::text("Cancel")
                                .on_press(Message::PackageRemoveCancelled)
                                .into(),
                        ])
                        .spacing(8),
                    ))
                    .into(),
            );
        }

        if self.pending_orphans {
            items.push(
                settings::section()
                    .title("Confirm orphan removal")
                    .add(settings::item(
                        "Consequences",
                        text("Remove unused Flatpak runtimes in this installation?"),
                    ))
                    .add(settings::item(
                        "Actions",
                        row(vec![
                            button::destructive("Remove orphans")
                                .on_press_maybe(
                                    (!self.action_busy).then_some(Message::PackageOrphansConfirmed),
                                )
                                .into(),
                            button::text("Cancel")
                                .on_press(Message::PackageOrphansCancelled)
                                .into(),
                        ])
                        .spacing(8),
                    ))
                    .into(),
            );
        }

        if let Some(kind) = &self.pending_update {
            items.push(
                settings::section()
                    .title("Confirm update")
                    .add(settings::item(
                        "Consequences",
                        text(format!("Update every {} package?", kind.title())),
                    ))
                    .add(settings::item(
                        "Actions",
                        row(vec![
                            button::destructive("Update all")
                                .on_press_maybe(
                                    (!self.action_busy).then_some(Message::PackageUpdateConfirmed),
                                )
                                .into(),
                            button::text("Cancel")
                                .on_press(Message::PackageUpdateCancelled)
                                .into(),
                        ])
                        .spacing(8),
                    ))
                    .into(),
            );
        }

        scrollable(column(items).spacing(16).padding(24)).into()
    }
}
