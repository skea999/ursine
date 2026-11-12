//! APK and Flatpak package panels.

use cosmic::iced::Length;
use cosmic::widget::{
    button, column, dropdown, row, scrollable, selectable_text, settings, text, text_input, toggler,
};
use cosmic::Element;
use ursine_api::{
    ApkPackage, ApkUpdate, FlatpakEnvVar, FlatpakInfo, FlatpakPackage, FlatpakPermissions,
    FlatpakRefKind, FlatpakRemote,
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

/// Which permissions-editor input or list an event targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionsField {
    Socket,
    Device,
    Filesystem,
    EnvName,
    EnvValue,
    Env,
}

/// Open Flatpak permissions editor for one installed app.
pub struct PermissionsEditor {
    pub app_id: String,
    pub sockets: Vec<String>,
    pub devices: Vec<String>,
    pub filesystems: Vec<String>,
    pub env: Vec<FlatpakEnvVar>,
    pub effective: String,
    pub add_socket: String,
    pub add_device: String,
    pub add_filesystem: String,
    pub add_env_name: String,
    pub add_env_value: String,
    /// Apply confirmation step (Cockpit-style: review, then write).
    pub confirm: bool,
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
    pub apk_repositories: Vec<ursine_api::ApkRepository>,
    /// Add-repository form inputs.
    pub apk_repo_url: String,
    pub apk_repo_tag: String,
    pub flatpak: Vec<FlatpakPackage>,
    pub flatpak_updates: Vec<FlatpakPackage>,
    pub flatpak_remotes: Vec<FlatpakRemote>,
    pub system: bool,
    pub query: String,
    pub install: String,
    /// Selected Flatpak remote for installs (empty = daemon default).
    pub install_remote: String,
    pub busy: bool,
    pub action_busy: bool,
    pub searching: bool,
    /// An update check has completed for the current scope/kind.
    pub updates_checked: bool,
    /// An update check is running right now.
    pub updates_checking: bool,
    /// Generation counter: bumped on every reload so a check started
    /// before the reload (wrong kind/scope) can never land after it.
    pub updates_seq: u64,
    pub error: Option<String>,
    pub message: Option<String>,
    pub pending_remove: Option<(PackageKind, String, String)>,
    pub pending_update: Option<PackageKind>,
    pub pending_orphans: bool,
    pub pending_install: Option<PendingInstall>,
    /// APK repository waiting for the remove confirmation.
    pub pending_apk_repo_remove: Option<String>,
    /// Add-remote form inputs (Flatpak).
    pub flatpak_remote_name: String,
    pub flatpak_remote_url: String,
    /// Flatpak remote waiting for the remove confirmation; the bool is
    /// the SCOPE captured at request time (a dropdown flip while the
    /// confirm is open must not delete in the other installation).
    pub pending_flatpak_remote_remove: Option<(String, bool)>,
    /// Open permissions editor (Flatpak installed apps only).
    pub permissions: Option<PermissionsEditor>,
}

impl Default for PackagesState {
    fn default() -> Self {
        Self {
            kind: None,
            apk: Vec::new(),
            apk_updates: Vec::new(),
            apk_repositories: Vec::new(),
            apk_repo_url: String::new(),
            apk_repo_tag: String::new(),
            flatpak: Vec::new(),
            flatpak_updates: Vec::new(),
            flatpak_remotes: Vec::new(),
            system: true,
            query: String::new(),
            install: String::new(),
            install_remote: String::new(),
            busy: false,
            action_busy: false,
            searching: false,
            updates_checked: false,
            updates_checking: false,
            updates_seq: 0,
            error: None,
            message: None,
            pending_remove: None,
            pending_update: None,
            pending_orphans: false,
            pending_install: None,
            pending_apk_repo_remove: None,
            flatpak_remote_name: String::new(),
            flatpak_remote_url: String::new(),
            pending_flatpak_remote_remove: None,
            permissions: None,
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
        self.updates_checked = false;
        self.updates_checking = false;
        self.updates_seq = self.updates_seq.wrapping_add(1);
        self.error = None;
        self.message = None;
        self.pending_remove = None;
        self.pending_update = None;
        self.pending_orphans = false;
        self.pending_install = None;
        self.permissions = None;
        self.pending_apk_repo_remove = None;
        self.pending_flatpak_remote_remove = None;
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

    pub fn apply_apk_repositories(&mut self, repositories: Vec<ursine_api::ApkRepository>) {
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

    /// Open (or reopen) the editor from a freshly read daemon state.
    pub fn open_permissions(&mut self, state: FlatpakPermissions) {
        self.permissions = Some(PermissionsEditor {
            app_id: state.app_id,
            sockets: state.sockets,
            devices: state.devices,
            filesystems: state.filesystems,
            env: state.env,
            effective: state.effective,
            add_socket: String::new(),
            add_device: String::new(),
            add_filesystem: String::new(),
            add_env_name: String::new(),
            add_env_value: String::new(),
            confirm: false,
        });
    }

    /// Push one non-empty trimmed input into its list and clear the input.
    pub fn permissions_add(&mut self, field: PermissionsField) {
        let Some(editor) = self.permissions.as_mut() else {
            return;
        };
        match field {
            PermissionsField::Socket => {
                let value = editor.add_socket.trim().to_string();
                if !value.is_empty() && !editor.sockets.contains(&value) {
                    editor.sockets.push(value);
                }
                editor.add_socket.clear();
            }
            PermissionsField::Device => {
                let value = editor.add_device.trim().to_string();
                if !value.is_empty() && !editor.devices.contains(&value) {
                    editor.devices.push(value);
                }
                editor.add_device.clear();
            }
            PermissionsField::Filesystem => {
                let value = editor.add_filesystem.trim().to_string();
                if !value.is_empty() && !editor.filesystems.contains(&value) {
                    editor.filesystems.push(value);
                }
                editor.add_filesystem.clear();
            }
            PermissionsField::EnvName | PermissionsField::EnvValue => {
                let name = editor.add_env_name.trim().to_string();
                let value = editor.add_env_value.trim().to_string();
                if !name.is_empty() && !editor.env.iter().any(|var| var.name == name) {
                    editor.env.push(FlatpakEnvVar { name, value });
                }
                editor.add_env_name.clear();
                editor.add_env_value.clear();
            }
            PermissionsField::Env => {}
        }
    }

    /// Drop one entry by index (field selects the list).
    pub fn permissions_remove(&mut self, field: PermissionsField, index: usize) {
        let Some(editor) = self.permissions.as_mut() else {
            return;
        };
        match field {
            PermissionsField::Socket => {
                if index < editor.sockets.len() {
                    editor.sockets.remove(index);
                }
            }
            PermissionsField::Device => {
                if index < editor.devices.len() {
                    editor.devices.remove(index);
                }
            }
            PermissionsField::Filesystem => {
                if index < editor.filesystems.len() {
                    editor.filesystems.remove(index);
                }
            }
            PermissionsField::Env => {
                if index < editor.env.len() {
                    editor.env.remove(index);
                }
            }
            PermissionsField::EnvName | PermissionsField::EnvValue => {}
        }
    }

    pub fn view(&self, updates_auto: ursine_api::UpdatesAuto) -> Element<'_, Message> {
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

        let scope_section = settings::section().title("Scope").add(settings::item(
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
        items.push(scope_section.into());

        // No Maintenance row for apk: "Check for updates" refreshes the
        // indexes itself (one control instead of two doing the same job).
        if kind == PackageKind::Flatpak {
            items.push(
                settings::section()
                    .title("Maintenance")
                    .add(settings::item(
                        "Remove unused runtimes",
                        button::text("Remove orphans…").on_press_maybe(
                            (!self.action_busy).then_some(Message::PackageOrphansRequested),
                        ),
                    ))
                    .into(),
            );
        }

        let mut upgrade = settings::section().title("Upgrade").add(settings::item(
            "Upgrade every installed item",
            button::suggested("Upgrade all")
                .on_press_maybe((!self.action_busy).then_some(Message::PackageUpdateRequested)),
        ));
        if let Some(url) = &self.pending_apk_repo_remove {
            items.push(
                settings::section()
                    .title("Confirm repository removal")
                    .add(settings::item(
                        "Consequences",
                        text(format!(
                            "Remove {url} from /etc/apk/repositories? Indexes refresh after the change."
                        )),
                    ))
                    .add(settings::item(
                        "Actions",
                        row(vec![
                            button::destructive("Remove")
                                .on_press_maybe(
                                    (!self.action_busy)
                                        .then_some(Message::PackageApkRepoRemoveConfirmed),
                                )
                                .into(),
                            button::text("Cancel")
                                .on_press_maybe(
                                    (!self.action_busy)
                                        .then_some(Message::PackageApkRepoRemoveCancelled),
                                )
                                .into(),
                        ])
                        .spacing(8),
                    ))
                    .into(),
            );
        }

        if kind == PackageKind::Apk {
            let mut repos = settings::section().title("Repositories");
            for repository in &self.apk_repositories {
                let url = repository.url.clone();
                let label = repository.tag.as_ref().map_or_else(
                    || repository.url.clone(),
                    |tag| format!("@{tag} {}", repository.url),
                );
                let mut actions = row(vec![button::destructive("Remove")
                    .on_press_maybe(
                        (!self.action_busy)
                            .then_some(Message::PackageApkRepoRemoveRequested(url.clone())),
                    )
                    .into()])
                .spacing(8);
                if repository.tag.is_some() {
                    let unmask: Element<'_, Message> = button::text("Unmask")
                        .on_press_maybe(
                            (!self.action_busy)
                                .then_some(Message::PackageApkRepoUnmask(url.clone())),
                        )
                        .into();
                    actions = actions.push(unmask);
                }
                repos = repos.add(settings::item(label, actions));
            }
            repos = repos.add(settings::item(
                "Add repository",
                row(vec![
                    text_input("https://… or /media/…", self.apk_repo_url.clone())
                        .on_input(Message::PackageApkRepoUrlChanged)
                        .into(),
                    text_input("mask tag (e.g. edge)", self.apk_repo_tag.clone())
                        .on_input(Message::PackageApkRepoTagChanged)
                        .into(),
                    button::text("Add")
                        .on_press_maybe(
                            (!self.action_busy && !self.apk_repo_url.trim().is_empty())
                                .then_some(Message::PackageApkRepoAdd),
                        )
                        .into(),
                ])
                .spacing(8),
            ));
            items.push(repos.into());
        }
        // Auto-check preference (persisted in [updates]) + the count +
        // a manual check — same three rows for every manager.
        let (manager, auto) = match kind {
            PackageKind::Apk => ("apk", updates_auto.apk),
            PackageKind::Flatpak => ("flatpak", updates_auto.flatpak),
        };
        upgrade = upgrade.add(settings::item(
            "Check updates automatically",
            toggler(auto)
                .on_toggle(move |value| Message::UpdateAutoChanged(manager.to_string(), value)),
        ));
        let number: Element<'_, Message> = if self.updates_checking {
            text("Checking…").into()
        } else if self.updates_checked {
            let count = match kind {
                PackageKind::Apk => self.apk_updates.len(),
                PackageKind::Flatpak => self.flatpak_updates.len(),
            };
            text::title2(count.to_string()).into()
        } else {
            text("—").into()
        };
        upgrade = upgrade.add(settings::item("Packages to update", number));
        let check_button: Element<'_, Message> = button::text("Check for updates")
            .on_press_maybe(
                (!self.busy && !self.action_busy && !self.updates_checking)
                    .then_some(Message::PackageUpdatesRefreshRequested),
            )
            .into();
        upgrade = upgrade.add(settings::item("Refresh", check_button));
        // Number only for BOTH managers (user decision): no per-package
        // update list — "Upgrade all" covers everything the count does,
        // and the per-package Upgrade buttons stay in the Installed list.
        items.push(upgrade.into());

        if kind == PackageKind::Flatpak && !self.flatpak_remotes.is_empty() {
            items.push(
                settings::section()
                    .title("Remotes")
                    .add(self.pending_flatpak_remote_remove.as_ref().map_or_else(
                        || {
                            settings::item(
                                "Add remote",
                                row(vec![
                                    text_input("name", self.flatpak_remote_name.clone())
                                        .on_input(Message::FlatpakRemoteNameChanged)
                                        .into(),
                                    text_input("https://…", self.flatpak_remote_url.clone())
                                        .on_input(Message::FlatpakRemoteUrlChanged)
                                        .into(),
                                    button::text("Add")
                                        .on_press_maybe(
                                            (!self.action_busy
                                                && !self.flatpak_remote_name.trim().is_empty()
                                                && !self.flatpak_remote_url.trim().is_empty())
                                                .then_some(Message::FlatpakRemoteAdd),
                                        )
                                        .into(),
                                ])
                                .spacing(8),
                            )
                        },
                        |(name, _system)| {
                            settings::item(
                                "Confirm removal",
                                row(vec![
                                    text(format!(
                                        "Remove the remote {name}? Packages from it stop receiving updates."
                                    ))
                                    .into(),
                                    button::destructive("Remove")
                                        .on_press_maybe(
                                            (!self.action_busy)
                                                .then_some(Message::FlatpakRemoteRemoveConfirmed),
                                        )
                                        .into(),
                                    button::text("Cancel")
                                        .on_press_maybe(
                                            (!self.action_busy)
                                                .then_some(Message::FlatpakRemoteRemoveCancelled),
                                        )
                                        .into(),
                                ])
                                .spacing(8),
                            )
                        },
                    ))
                    .add({
                        if self.flatpak_remotes.is_empty() {
                            settings::item("Configured", text("No remotes."))
                        } else {
                            settings::item(
                                "Configured",
                                column(
                                    self.flatpak_remotes
                                        .iter()
                                        .map(|remote| {
                                            let name = remote.name.clone();
                                            row(vec![
                                                text(format!("{} — {}", remote.name, remote.url))
                                                    .width(Length::Fill)
                                                    .into(),
                                                button::text("Remove…").on_press_maybe(
                                                    (!self.action_busy)
                                                        .then_some(
                                                            Message::FlatpakRemoteRemoveRequested(
                                                                name,
                                                            ),
                                                        ),
                                                ).into(),
                                            ])
                                            .spacing(8)
                                            .into()
                                        })
                                        .collect::<Vec<Element<'_, Message>>>(),
                                )
                                .spacing(2),
                            )
                        }
                    })
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
                        // Upgrade only when the updates check says this
                        // package has one; up-to-date (or not checked)
                        // shows no button.
                        let mut buttons: Vec<Element<'_, Message>> = Vec::with_capacity(2);
                        if self.apk_updates.iter().any(|update| update.name == name) {
                            buttons.push(
                                button::text("Upgrade")
                                    .on_press(Message::ApkUpgradeRequested(name.clone()))
                                    .into(),
                            );
                        }
                        buttons.push(
                            button::text("Remove")
                                .on_press(Message::PackageRemoveRequested(kind, name.clone(), name))
                                .into(),
                        );
                        row(buttons).spacing(8).into()
                    };
                    let description = package.description.clone().map_or_else(
                        || text("").into(),
                        |description| selectable_text(description).into(),
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
                // Apps AND runtimes in one list (user decision).
                for package in &self.flatpak {
                    shown += 1;
                    let app_id = package.app_id.clone();
                    let action: Element<'_, Message> = if self.searching {
                        button::text("Install")
                            .on_press(Message::PackageInstallDirect(app_id.clone()))
                            .into()
                    } else if package.kind == FlatpakRefKind::Runtime {
                        // Runtimes are protected (apk-style): listed, no
                        // action buttons at all.
                        text("protected").into()
                    } else {
                        // Upgrade only when the updates check says this
                        // app has one; up-to-date (or not checked) shows
                        // no button.
                        let mut buttons: Vec<Element<'_, Message>> = Vec::with_capacity(3);
                        if self
                            .flatpak_updates
                            .iter()
                            .any(|update| update.app_id == app_id)
                        {
                            buttons.push(
                                button::text("Upgrade")
                                    .on_press(Message::FlatpakUpgradeRequested(app_id.clone()))
                                    .into(),
                            );
                        }
                        buttons.push(
                            button::text("Permissions…")
                                .on_press(Message::PackagePermissionsRequested(app_id.clone()))
                                .into(),
                        );
                        buttons.push(
                            button::text("Remove")
                                .on_press(Message::PackageRemoveRequested(
                                    kind,
                                    app_id.clone(),
                                    app_id,
                                ))
                                .into(),
                        );
                        row(buttons).spacing(8).into()
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
                        selectable_text(description).into(),
                        action,
                    ));
                }
                if !self.searching && shown == 0 {
                    list = list.add(settings::item("Result", text("No packages installed.")));
                }
            }
        }
        items.push(list.into());

        if let Some(editor) = &self.permissions {
            items.push(permissions_section(editor, self.action_busy));
        }

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

/// Permissions editor section: four editable lists with add inputs, the
/// effective sandbox view (read-only) and Apply/Cancel.
fn permissions_section(editor: &PermissionsEditor, busy: bool) -> Element<'_, Message> {
    fn list_block<'a>(
        title: &'static str,
        items: &'a [String],
        field: PermissionsField,
        placeholder: &'static str,
        input_value: &'a str,
        busy: bool,
    ) -> Element<'a, Message> {
        let rows: Vec<Element<'a, Message>> = if items.is_empty() {
            vec![text("(none)").into()]
        } else {
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    row(vec![
                        text(item.clone()).width(Length::Fill).into(),
                        button::text("Remove")
                            .on_press_maybe(
                                (!busy).then_some(Message::PackagePermissionsRemove(field, index)),
                            )
                            .into(),
                    ])
                    .spacing(8)
                    .into()
                })
                .collect()
        };
        let add = row(vec![
            text_input(placeholder, input_value)
                .on_input(move |value| Message::PackagePermissionsInput(field, value))
                .into(),
            button::text("Add")
                .on_press_maybe((!busy).then_some(Message::PackagePermissionsAdd(field)))
                .into(),
        ])
        .spacing(8);
        settings::item(
            title,
            column(vec![column(rows).spacing(4).into(), add.into()]).spacing(8),
        )
        .into()
    }

    let mut section = settings::section().title(format!("Flatpak permissions — {}", editor.app_id));
    section = section.add(list_block(
        "Sockets",
        &editor.sockets,
        PermissionsField::Socket,
        "wayland",
        &editor.add_socket,
        busy,
    ));
    section = section.add(list_block(
        "Devices",
        &editor.devices,
        PermissionsField::Device,
        "dri",
        &editor.add_device,
        busy,
    ));
    section = section.add(list_block(
        "Filesystems",
        &editor.filesystems,
        PermissionsField::Filesystem,
        "xdg-download",
        &editor.add_filesystem,
        busy,
    ));

    let env_rows: Vec<Element<'_, Message>> = if editor.env.is_empty() {
        vec![text("(none)").into()]
    } else {
        editor
            .env
            .iter()
            .enumerate()
            .map(|(index, var)| {
                let label = if var.value.is_empty() {
                    format!("{} (unset)", var.name)
                } else {
                    format!("{}={}", var.name, var.value)
                };
                row(vec![
                    text(label).width(Length::Fill).into(),
                    button::text("Remove")
                        .on_press_maybe((!busy).then_some(Message::PackagePermissionsRemove(
                            PermissionsField::Env,
                            index,
                        )))
                        .into(),
                ])
                .spacing(8)
                .into()
            })
            .collect()
    };
    let env_add = row(vec![
        text_input("NAME", editor.add_env_name.clone())
            .on_input(|value| Message::PackagePermissionsInput(PermissionsField::EnvName, value))
            .into(),
        text_input("value", editor.add_env_value.clone())
            .on_input(|value| Message::PackagePermissionsInput(PermissionsField::EnvValue, value))
            .into(),
        button::text("Add")
            .on_press_maybe(
                (!busy).then_some(Message::PackagePermissionsAdd(PermissionsField::EnvName)),
            )
            .into(),
    ])
    .spacing(8);
    section = section.add(settings::item(
        "Environment",
        column(vec![column(env_rows).spacing(4).into(), env_add.into()]).spacing(8),
    ));

    if !editor.effective.trim().is_empty() {
        section = section.add(settings::item(
            "Effective sandbox (read-only)",
            text(editor.effective.clone()),
        ));
    }

    let actions = if editor.confirm {
        row(vec![
            text("Write these overrides to the app sandbox?").into(),
            button::destructive("Confirm write")
                .on_press_maybe((!busy).then_some(Message::PackagePermissionsApplyRequested))
                .into(),
            button::text("Back")
                .on_press(Message::PackagePermissionsCancelled)
                .into(),
        ])
        .spacing(8)
    } else {
        row(vec![
            button::destructive("Apply overrides")
                .on_press_maybe((!busy).then_some(Message::PackagePermissionsApplyRequested))
                .into(),
            button::text("Close")
                .on_press_maybe((!busy).then_some(Message::PackagePermissionsCancelled))
                .into(),
        ])
        .spacing(8)
    };
    section = section.add(settings::item("Actions", actions));
    section.into()
}
