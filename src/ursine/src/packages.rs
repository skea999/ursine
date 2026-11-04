//! APK and Flatpak package panels.

use cosmic::iced::Length;
use cosmic::widget::{button, column, dropdown, row, scrollable, settings, text, text_input};
use cosmic::Element;
use ursine_api::{
    ApkPackage, ApkUpdate, FlatpakInfo, FlatpakPackage, FlatpakRefKind, FlatpakRemote,
};

use crate::app::Message;
use crate::package_row::package_row;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    Apk,
    Flatpak,
}

/// A Flatpak install waiting for confirmation, with the remote lookup that
/// feeds the size rows (failed lookup still allows the install).
pub struct PendingInstall {
    pub app_id: String,
    pub info: Option<FlatpakInfo>,
    pub info_error: Option<String>,
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
    pub flatpak_updates: Vec<FlatpakPackage>,
    pub flatpak_remotes: Vec<FlatpakRemote>,
    pub system: bool,
    pub query: String,
    pub install: String,
    /// Selected Flatpak remote for installs (empty = daemon default).
    pub install_remote: String,
    /// Flatpak list filter: false = apps, true = runtimes.
    pub show_runtimes: bool,
    pub busy: bool,
    pub action_busy: bool,
    pub searching: bool,
    pub error: Option<String>,
    pub message: Option<String>,
    pub pending_remove: Option<(PackageKind, String, String)>,
    pub pending_update: Option<PackageKind>,
    pub pending_orphans: bool,
    pub pending_install: Option<PendingInstall>,
}

impl Default for PackagesState {
    fn default() -> Self {
        Self {
            kind: None,
            apk: Vec::new(),
            apk_updates: Vec::new(),
            apk_repositories: Vec::new(),
            flatpak: Vec::new(),
            flatpak_updates: Vec::new(),
            flatpak_remotes: Vec::new(),
            system: true,
            query: String::new(),
            install: String::new(),
            install_remote: String::new(),
            show_runtimes: false,
            busy: false,
            action_busy: false,
            searching: false,
            error: None,
            message: None,
            pending_remove: None,
            pending_update: None,
            pending_orphans: false,
            pending_install: None,
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
        self.flatpak_updates.clear();
        self.flatpak_remotes.clear();
        self.searching = false;
        self.error = None;
        self.message = None;
        self.pending_remove = None;
        self.pending_update = None;
        self.pending_orphans = false;
        self.pending_install = None;
    }

    pub fn apply_apk(&mut self, packages: Vec<ApkPackage>) {
        self.apk = packages;
        self.flatpak.clear();
        self.flatpak_updates.clear();
        self.error = None;
    }

    pub fn apply_apk_updates(&mut self, updates: Vec<ApkUpdate>) {
        self.apk_updates = updates;
    }

    pub fn apply_apk_repositories(&mut self, repositories: Vec<String>) {
        self.apk_repositories = repositories;
    }

    pub fn apply_flatpak_remotes(&mut self, remotes: Vec<FlatpakRemote>) {
        // Keep the selection when it still exists in this scope, otherwise
        // prefer flathub, else the first configured remote.
        let stale = !self.install_remote.is_empty()
            && !remotes.iter().any(|r| r.name == self.install_remote);
        if self.install_remote.is_empty() || stale {
            self.install_remote = remotes
                .iter()
                .find(|r| r.name == "flathub")
                .or_else(|| remotes.first())
                .map(|r| r.name.clone())
                .unwrap_or_default();
        }
        self.flatpak_remotes = remotes;
    }

    /// Remote names in display order (the daemon lists one row per remote).
    pub fn remote_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for remote in &self.flatpak_remotes {
            if !names.contains(&remote.name) {
                names.push(remote.name.clone());
            }
        }
        names
    }

    /// Whether a Flatpak row shows under the current Show filter. Search
    /// hits are apps (appstream) and are never hidden behind Runtimes.
    pub fn flatpak_row_visible(&self, row_kind: FlatpakRefKind) -> bool {
        self.searching || (row_kind == FlatpakRefKind::Runtime) == self.show_runtimes
    }

    pub fn selected_remote(&self) -> Option<String> {
        if self.install_remote.is_empty() {
            None
        } else {
            Some(self.install_remote.clone())
        }
    }

    pub fn apply_flatpak(&mut self, packages: Vec<FlatpakPackage>) {
        self.flatpak = packages;
        self.apk.clear();
        self.error = None;
    }

    pub fn apply_flatpak_updates(&mut self, updates: Vec<FlatpakPackage>) {
        self.flatpak_updates = updates;
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

        let mut scope_section = settings::section().title("Scope").add(settings::item(
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
        ));
        if kind == PackageKind::Flatpak {
            scope_section = scope_section.add(settings::item(
                "Show",
                dropdown(
                    vec!["Apps", "Runtimes"],
                    Some(usize::from(self.show_runtimes)),
                    Message::PackageFlatpakFilterChanged,
                ),
            ));
        }
        items.push(scope_section.into());

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
        if kind == PackageKind::Flatpak && !self.flatpak_updates.is_empty() {
            upgrade = upgrade.add(settings::item(
                format!("Available updates ({})", self.flatpak_updates.len()),
                column(
                    self.flatpak_updates
                        .iter()
                        .map(|update| {
                            let installed = self
                                .flatpak
                                .iter()
                                .find(|row| row.app_id == update.app_id)
                                .map(|row| row.version.clone());
                            let detail = match installed {
                                Some(old) if !old.is_empty() => {
                                    format!("{} {} -> {}", update.name, old, update.version)
                                }
                                _ => format!("{} {}", update.name, update.version),
                            };
                            let detail = if update.origin.is_empty() {
                                detail
                            } else {
                                format!("{detail} · {}", update.origin)
                            };
                            row(vec![
                                text(detail).width(Length::Fill).into(),
                                button::text("Update")
                                    .on_press(Message::FlatpakUpgradeRequested(
                                        update.app_id.clone(),
                                    ))
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

        if kind == PackageKind::Flatpak && !self.flatpak_remotes.is_empty() {
            items.push(
                settings::section()
                    .title("Remotes")
                    .add(settings::item(
                        "Configured Flatpak remotes",
                        column(
                            self.flatpak_remotes
                                .iter()
                                .map(|remote| {
                                    text(format!("{} — {}", remote.name, remote.url)).into()
                                })
                                .collect::<Vec<Element<'_, Message>>>(),
                        )
                        .spacing(2),
                    ))
                    .into(),
            );
        }

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
                .add({
                    let mut install_row: Vec<Element<'_, Message>> = vec![text_input(
                        match kind {
                            PackageKind::Apk => "niri",
                            PackageKind::Flatpak => "org.mozilla.firefox",
                        },
                        self.install.clone(),
                    )
                    .on_input(Message::PackageInstallChanged)
                    .into()];
                    if kind == PackageKind::Flatpak {
                        let names = self.remote_names();
                        if !names.is_empty() {
                            let selected =
                                names.iter().position(|name| *name == self.install_remote);
                            let choices = names.clone();
                            install_row.push(
                                dropdown(names, selected, move |index| {
                                    Message::PackageInstallRemoteChanged(
                                        choices.get(index).cloned().unwrap_or_default(),
                                    )
                                })
                                .into(),
                            );
                        }
                    }
                    install_row.push(
                        button::text("Install")
                            .on_press_maybe(
                                (!self.action_busy).then_some(Message::PackageInstallRequested),
                            )
                            .into(),
                    );
                    install_row.push(
                        button::text("Preview APK")
                            .on_press_maybe(
                                (kind == PackageKind::Apk
                                    && !self.install.trim().is_empty()
                                    && !self.action_busy)
                                    .then_some(Message::ApkPreviewRequested),
                            )
                            .into(),
                    );
                    settings::item("Direct install", row(install_row).spacing(8))
                })
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
                let mut shown = 0usize;
                for package in self
                    .flatpak
                    .iter()
                    .filter(|package| self.flatpak_row_visible(package.kind))
                {
                    shown += 1;
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
                if !self.searching && shown == 0 {
                    list = list.add(settings::item(
                        "Result",
                        text(if self.show_runtimes {
                            "No runtimes installed."
                        } else {
                            "No apps installed."
                        }),
                    ));
                }
            }
        }
        items.push(list.into());

        if let Some(pending) = &self.pending_install {
            let mut confirm = settings::section()
                .title("Confirm Flatpak install")
                .add(settings::item("Application", text(pending.app_id.clone())))
                .add(settings::item(
                    "Remote",
                    text(if self.install_remote.is_empty() {
                        "flathub".to_string()
                    } else {
                        self.install_remote.clone()
                    }),
                ));
            match (&pending.info, &pending.info_error) {
                (Some(info), _) => {
                    confirm = confirm
                        .add(settings::item(
                            "Version",
                            text(if info.version.is_empty() {
                                "unknown".to_string()
                            } else {
                                info.version.clone()
                            }),
                        ))
                        .add(settings::item(
                            "Download size",
                            text(if info.download_size.is_empty() {
                                "unknown".to_string()
                            } else {
                                info.download_size.clone()
                            }),
                        ))
                        .add(settings::item(
                            "Installed size",
                            text(if info.installed_size.is_empty() {
                                "unknown".to_string()
                            } else {
                                info.installed_size.clone()
                            }),
                        ));
                }
                (_, Some(error)) => {
                    confirm = confirm.add(settings::item(
                        "Sizes",
                        text(format!("unavailable ({error})")),
                    ));
                }
                _ => {}
            }
            items.push(
                confirm
                    .add(settings::item(
                        "Actions",
                        row(vec![
                            button::destructive("Install")
                                .on_press_maybe(
                                    (!self.action_busy).then_some(Message::PackageInstallConfirmed),
                                )
                                .into(),
                            button::text("Cancel")
                                .on_press(Message::PackageInstallCancelled)
                                .into(),
                        ])
                        .spacing(8),
                    ))
                    .into(),
            );
        }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_filter_toggles_apps_and_runtimes() {
        let state = PackagesState::default();
        // Default: apps visible, runtimes hidden.
        assert!(state.flatpak_row_visible(FlatpakRefKind::App));
        assert!(!state.flatpak_row_visible(FlatpakRefKind::Runtime));
        let state = PackagesState {
            show_runtimes: true,
            ..PackagesState::default()
        };
        assert!(!state.flatpak_row_visible(FlatpakRefKind::App));
        assert!(state.flatpak_row_visible(FlatpakRefKind::Runtime));
    }

    #[test]
    fn search_hits_are_never_filtered() {
        let state = PackagesState {
            searching: true,
            show_runtimes: true,
            ..PackagesState::default()
        };
        // Search rows are apps; the Runtimes filter must not hide them.
        assert!(state.flatpak_row_visible(FlatpakRefKind::App));
    }
}
