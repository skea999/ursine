//! APK and Flatpak package panels.

use cosmic::widget::{button, dropdown, row, settings, text, text_input};
use cosmic::Element;
use ursine_api::{
    ApkPackage, ApkUpdate, FlatpakInfo, FlatpakPackage, FlatpakRefKind, FlatpakRemote,
};

use crate::app::Message;

/// Filter the source against precomputed keys (zero allocations per frame),
/// then materialize only the requested page. Returns the filtered total.
fn filtered_page<'a, T>(
    items: &'a [T],
    keys: &[String],
    query: &str,
    page: usize,
) -> (usize, Vec<&'a T>) {
    let matches: Vec<&T> = items
        .iter()
        .zip(keys.iter())
        .filter(|(_, key)| crate::package_ui::contains_ci(key, query))
        .map(|(item, _)| item)
        .collect();
    let len = matches.len();
    let (_, _, start) = crate::package_ui::page_bounds(len, page);
    let page = matches
        .into_iter()
        .skip(start)
        .take(crate::package_ui::PAGE_SIZE)
        .collect();
    (len, page)
}

/// Search dialog status rows: busy line while the daemon works.
fn search_status(busy: bool, error: Option<&String>) -> Vec<(String, String)> {
    if busy {
        vec![("Status".to_string(), "Searching…".to_string())]
    } else if let Some(error) = error {
        vec![("Error".to_string(), error.clone())]
    } else {
        Vec::new()
    }
}

/// One installed/search row: identity, description, repository, actions.
fn package_row<'a>(
    name: &str,
    version: &str,
    description: Option<&String>,
    repository: Option<String>,
    actions: Vec<Element<'a, Message>>,
    compact: bool,
) -> crate::package_ui::RowVm<'a> {
    crate::package_ui::RowVm {
        compact,
        title: format!("{name} {version}"),
        description: description
            .map(String::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        repository,
        notes: Vec::new(),
        actions,
    }
}

/// The Remove button every manager shows on an unprotected package.
fn remove_button<'a>(kind: PackageKind, id: &str, armed: bool) -> Element<'a, Message> {
    button::text("Remove")
        .on_press_maybe(armed.then_some(Message::PackageRemoveRequested(
            kind,
            id.to_string(),
            id.to_string(),
        )))
        .into()
}

/// Which regex action the Pattern section is confirming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternOp {
    Upgrade,
    Remove,
}

impl std::fmt::Display for PatternOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Upgrade => f.write_str("Upgrade"),
            Self::Remove => f.write_str("Remove"),
        }
    }
}

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
    /// Search results for each manager, shown in their own dialog so the
    /// installed list stays on screen.
    pub search_apk: Vec<ApkPackage>,
    pub search_flatpak: Vec<FlatpakPackage>,
    pub search_open: bool,
    pub search_page: usize,
    /// An update check has completed for the current scope/kind.
    pub updates_checked: bool,
    /// An update check is running right now.
    pub updates_checking: bool,
    /// Generation counter: bumped on every reload so a check started
    /// before the reload (wrong kind/scope) can never land after it.
    pub updates_seq: u64,
    /// A flatpak/apk search is in flight (drives the Search label
    /// exactly like `nix`'s `search_busy`).
    pub search_busy: bool,
    /// Pattern-actions input (regex over installed names).
    pub pattern: String,
    /// Inline preview result of the last pattern dry-run.
    pub pattern_result: Option<String>,
    /// Client-side filter over the installed/search list.
    pub installed_filter: String,
    /// The installed list is shown in the half-page browser dialog.
    pub list_browser: bool,
    /// Current page of the installed list (browser dialog).
    pub installed_page: usize,
    /// Lowercased row titles for the CURRENT source: the filter runs
    /// against these without allocating per frame.
    pub filter_keys: Vec<String>,
    /// (operation, pattern) waiting for confirmation.
    pub pending_pattern: Option<(PatternOp, String)>,
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
            search_apk: Vec::new(),
            search_flatpak: Vec::new(),
            search_open: false,
            search_page: 0,
            updates_checked: false,
            updates_checking: false,
            updates_seq: 0,
            pattern: String::new(),
            pattern_result: None,
            installed_filter: String::new(),
            list_browser: false,
            installed_page: 0,
            filter_keys: Vec::new(),
            pending_pattern: None,
            search_busy: false,
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
        self.search_apk.clear();
        self.search_flatpak.clear();
        self.search_open = false;
        self.search_page = 0;
        self.search_busy = false;
        self.pattern_result = None;
        self.installed_filter.clear();
        self.list_browser = false;
        self.installed_page = 0;
        self.filter_keys.clear();
        self.updates_checked = false;
        self.updates_checking = false;
        self.updates_seq = self.updates_seq.wrapping_add(1);
        self.pending_pattern = None;
        self.error = None;
        self.message = None;
        self.pending_remove = None;
        self.pending_update = None;
        self.pending_orphans = false;
        self.pending_install = None;
        self.pending_apk_repo_remove = None;
        self.pending_flatpak_remote_remove = None;
    }

    pub fn apply_apk(&mut self, packages: Vec<ApkPackage>) {
        self.apk = packages;
        self.flatpak.clear();
        self.flatpak_updates.clear();
        self.error = None;
        self.rebuild_filter_keys();
    }

    /// Precompute the lowercased row titles whenever the source list
    /// changes — filtering then costs zero allocations per frame.
    pub fn rebuild_filter_keys(&mut self) {
        self.filter_keys = match self.kind {
            Some(PackageKind::Apk) => self
                .apk
                .iter()
                .map(|package| format!("{} {}", package.name, package.version))
                .collect(),
            Some(PackageKind::Flatpak) => self
                .flatpak
                .iter()
                .map(|package| format!("{} {}", package.name, package.version))
                .collect(),
            None => Vec::new(),
        };
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
        self.rebuild_filter_keys();
    }

    pub fn apply_flatpak_updates(&mut self, updates: Vec<FlatpakPackage>) {
        self.flatpak_updates = updates;
    }

    pub fn view(
        &self,
        updates_auto: ursine_api::UpdatesAuto,
        compact: bool,
    ) -> Element<'_, Message> {
        let Some(kind) = self.kind else {
            return text("Select a package source.").into();
        };
        crate::package_ui::view(self.vm(kind, updates_auto, compact))
    }

    /// Everything the shared view renders, as data for this manager.
    pub(crate) fn vm(
        &self,
        kind: PackageKind,
        updates_auto: ursine_api::UpdatesAuto,
        compact: bool,
    ) -> crate::package_ui::PackageVm<'_> {
        use crate::package_ui::{
            page_bounds, text_row, ConfirmVm, FilterVm, ListVm, PackageVm, RefreshVm, RowVm,
            SearchVm, SectionVm, UpgradeAllVm, UpgradeVm,
        };

        let manager = match kind {
            PackageKind::Apk => "apk",
            PackageKind::Flatpak => "flatpak",
        };

        // --- pending confirmations (stacked in the confirm modal) ---
        let mut confirms: Vec<ConfirmVm<'_>> = Vec::new();
        if let Some((op, pattern)) = &self.pending_pattern {
            confirms.push(ConfirmVm {
                title: "Confirm action".to_string(),
                rows: vec![(
                    kind.title().to_string(),
                    text_row(format!(
                        "{op} every installed package matching \"{pattern}\"?"
                    )),
                )],
                confirm: Message::PackagePatternConfirmed,
                cancel: Message::PackagePatternCancelled,
                armed: !self.action_busy && !self.busy,
                destructive: true,
            });
        }
        if let Some(url) = &self.pending_apk_repo_remove {
            confirms.push(ConfirmVm {
                title: "Confirm repository removal".to_string(),
                rows: vec![(
                    "Consequences".to_string(),
                    text_row(format!(
                        "Remove {url} from /etc/apk/repositories? Indexes refresh after the change."
                    )),
                )],
                confirm: Message::PackageApkRepoRemoveConfirmed,
                cancel: Message::PackageApkRepoRemoveCancelled,
                armed: !self.action_busy,
                destructive: true,
            });
        }
        if let Some((name, _system)) = &self.pending_flatpak_remote_remove {
            confirms.push(ConfirmVm {
                title: "Confirm repository removal".to_string(),
                rows: vec![(
                    "Consequences".to_string(),
                    text_row(format!(
                        "Remove the repository {name}? Packages from it stop receiving updates."
                    )),
                )],
                confirm: Message::FlatpakRemoteRemoveConfirmed,
                cancel: Message::FlatpakRemoteRemoveCancelled,
                armed: !self.action_busy,
                destructive: true,
            });
        }
        if let Some(pending) = &self.pending_install {
            let mut rows = vec![
                ("Application".to_string(), text_row(pending.app_id.clone())),
                (
                    "Repository".to_string(),
                    text_row(if self.install_remote.is_empty() {
                        "flathub".to_string()
                    } else {
                        self.install_remote.clone()
                    }),
                ),
            ];
            match (&pending.info, &pending.info_error) {
                (Some(info), _) => {
                    rows.push((
                        "Version".to_string(),
                        text_row(if info.version.is_empty() {
                            "unknown".to_string()
                        } else {
                            info.version.clone()
                        }),
                    ));
                    rows.push((
                        "Download size".to_string(),
                        text_row(if info.download_size.is_empty() {
                            "unknown".to_string()
                        } else {
                            info.download_size.clone()
                        }),
                    ));
                    rows.push((
                        "Installed size".to_string(),
                        text_row(if info.installed_size.is_empty() {
                            "unknown".to_string()
                        } else {
                            info.installed_size.clone()
                        }),
                    ));
                }
                (_, Some(error)) => {
                    rows.push((
                        "Sizes".to_string(),
                        text_row(format!("unavailable ({error})")),
                    ));
                }
                _ => {}
            }
            confirms.push(ConfirmVm {
                title: "Confirm Flatpak install".to_string(),
                rows,
                confirm: Message::PackageInstallConfirmed,
                cancel: Message::PackageInstallCancelled,
                armed: !self.action_busy,
                destructive: true,
            });
        }
        if let Some((kind, name, _id)) = &self.pending_remove {
            confirms.push(ConfirmVm {
                title: "Confirm removal".to_string(),
                rows: vec![(
                    "Consequences".to_string(),
                    text_row(format!("Remove {name} from {}?", kind.title())),
                )],
                confirm: Message::PackageRemoveConfirmed,
                cancel: Message::PackageRemoveCancelled,
                armed: !self.action_busy,
                destructive: true,
            });
        }
        if self.pending_orphans {
            confirms.push(ConfirmVm {
                title: "Confirm orphan removal".to_string(),
                rows: vec![(
                    "Consequences".to_string(),
                    text_row("Remove unused Flatpak runtimes in this installation?"),
                )],
                confirm: Message::PackageOrphansConfirmed,
                cancel: Message::PackageOrphansCancelled,
                armed: !self.action_busy,
                destructive: true,
            });
        }
        if let Some(kind) = &self.pending_update {
            confirms.push(ConfirmVm {
                title: "Confirm update".to_string(),
                rows: vec![(
                    "Consequences".to_string(),
                    text_row(format!("Update every {} package?", kind.title())),
                )],
                confirm: Message::PackageUpdateConfirmed,
                cancel: Message::PackageUpdateCancelled,
                armed: !self.action_busy,
                destructive: true,
            });
        }

        // --- Scope ---
        let installation: Element<'_, Message> = if kind == PackageKind::Flatpak {
            dropdown(
                vec!["System", "User"],
                Some(usize::from(!self.system)),
                Message::PackageScopeChanged,
            )
            .into()
        } else {
            text("system-wide").into()
        };
        let scope = SectionVm {
            title: "Scope".to_string(),
            items: vec![settings::item("Installation", installation).into()],
        };

        // --- Maintenance (flatpak only; apk's check refreshes indexes) ---
        let maintenance = (kind == PackageKind::Flatpak).then(|| {
            SectionVm {
                title: "Maintenance".to_string(),
                items: vec![settings::item(
                    "Remove unused runtimes",
                    // The daemon no longer counts unused runtimes: counting
                    // them needed a `flatpak uninstall` probe a read-only
                    // listing must not run, so the button states no count.
                    button::text("Remove orphans…").on_press_maybe(
                        (!self.action_busy).then_some(Message::PackageOrphansRequested),
                    ),
                )
                .into()],
            }
        });

        // --- Upgrade ("Upgrade all" first, then the controls) ---
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
        let upgrade = UpgradeVm {
            all: Some(UpgradeAllVm {
                label: "Upgrade all".to_string(),
                message: Message::PackageUpdateRequested,
                armed: !self.busy && !self.action_busy,
            }),
            manager: manager.to_string(),
            auto: match kind {
                PackageKind::Apk => updates_auto.apk,
                PackageKind::Flatpak => updates_auto.flatpak,
            },
            number,
            refresh: Message::PackageUpdatesRefreshRequested,
            refresh_armed: !self.busy && !self.action_busy && !self.updates_checking,
        };

        // --- Search (controls only; results render in the list) ---
        let install_extras: Vec<Element<'_, Message>> = if kind == PackageKind::Flatpak {
            let names = self.remote_names();
            if names.is_empty() {
                Vec::new()
            } else {
                let selected = names.iter().position(|name| *name == self.install_remote);
                let choices = names.clone();
                vec![dropdown(names, selected, move |index| {
                    Message::PackageInstallRemoteChanged(
                        choices.get(index).cloned().unwrap_or_default(),
                    )
                })
                .into()]
            }
        } else {
            Vec::new()
        };
        let install_trailing: Vec<Element<'_, Message>> = match kind {
            PackageKind::Apk => vec![button::text("Preview APK")
                .on_press_maybe(
                    (!self.install.trim().is_empty() && !self.action_busy)
                        .then_some(Message::ApkPreviewRequested),
                )
                .into()],
            PackageKind::Flatpak => vec![button::text("Preview")
                .on_press_maybe(
                    (!self.install.trim().is_empty() && !self.action_busy)
                        .then_some(Message::FlatpakPreviewRequested),
                )
                .into()],
        };
        let search = SearchVm {
            repository: None,
            query_placeholder: kind.search_placeholder().to_string(),
            query: self.query.clone(),
            on_query_input: Message::PackageQueryChanged,
            query_submit: Some(Message::PackageSearchRequested),
            query_label: if self.search_busy {
                "Searching…"
            } else {
                "Search"
            }
            .to_string(),
            search: Message::PackageSearchRequested,
            search_armed: !self.busy && !self.action_busy && !self.query.trim().is_empty(),
            install_placeholder: match kind {
                PackageKind::Apk => "niri",
                PackageKind::Flatpak => "org.mozilla.firefox",
            }
            .to_string(),
            install: self.install.clone(),
            on_install_input: Message::PackageInstallChanged,
            install_extras,
            install_trailing,
            install_message: Message::PackageInstallRequested,
            install_armed: !self.busy && !self.action_busy && !self.install.trim().is_empty(),
        };

        // --- Repositories / Remotes ---
        let mut repo_items: Vec<Element<'_, Message>> = Vec::new();
        match kind {
            PackageKind::Apk => {
                for repository in &self.apk_repositories {
                    let url = repository.url.clone();
                    // Uniform row shape: identity as the title, the
                    // full URL as the detail line (daemon-built label).
                    let label = if repository.label.is_empty() {
                        repository.url.clone()
                    } else {
                        repository.label.clone()
                    };
                    let mut actions: Vec<Element<'_, Message>> =
                        vec![button::destructive("Remove")
                            .on_press_maybe(
                                (!self.busy && !self.action_busy)
                                    .then_some(Message::PackageApkRepoRemoveRequested(url.clone())),
                            )
                            .into()];
                    if repository.tag.is_some() {
                        actions.push(
                            button::text("Unmask")
                                .on_press_maybe(
                                    (!self.action_busy)
                                        .then_some(Message::PackageApkRepoUnmask(url.clone())),
                                )
                                .into(),
                        );
                    }
                    repo_items.push(crate::package_ui::repository_row(
                        label,
                        Some(repository.url.clone()),
                        actions,
                        compact,
                    ));
                }
                repo_items.push(
                    settings::item(
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
                                    (!self.busy
                                        && !self.action_busy
                                        && !self.apk_repo_url.trim().is_empty())
                                    .then_some(Message::PackageApkRepoAdd),
                                )
                                .into(),
                        ])
                        .spacing(8),
                    )
                    .into(),
                );
            }
            PackageKind::Flatpak => {
                if self.flatpak_remotes.is_empty() {
                    repo_items.push(
                        settings::item("Entries", text("No repositories configured.")).into(),
                    );
                } else {
                    for remote in &self.flatpak_remotes {
                        let name = remote.name.clone();
                        repo_items.push(crate::package_ui::repository_row(
                            name.clone(),
                            Some(remote.url.clone()),
                            vec![button::destructive("Remove")
                                .on_press_maybe(
                                    (!self.busy && !self.action_busy)
                                        .then_some(Message::FlatpakRemoteRemoveRequested(name)),
                                )
                                .into()],
                            compact,
                        ));
                    }
                }
                repo_items.push(
                    settings::item(
                        "Add repository",
                        row(vec![
                            text_input("name", self.flatpak_remote_name.clone())
                                .on_input(Message::FlatpakRemoteNameChanged)
                                .into(),
                            text_input("https://…", self.flatpak_remote_url.clone())
                                .on_input(Message::FlatpakRemoteUrlChanged)
                                .into(),
                            button::text("Add")
                                .on_press_maybe(
                                    (!self.busy
                                        && !self.action_busy
                                        && !self.flatpak_remote_name.trim().is_empty()
                                        && !self.flatpak_remote_url.trim().is_empty())
                                    .then_some(Message::FlatpakRemoteAdd),
                                )
                                .into(),
                        ])
                        .spacing(8),
                    )
                    .into(),
                );
            }
        }
        let repositories = SectionVm {
            title: "Repositories".to_string(),
            items: repo_items,
        };

        // --- Pattern (nix schema, shared builder) ---
        let pattern = crate::package_ui::PatternSection {
            hint: match kind {
                PackageKind::Apk => "Matches against installed package names, e.g. ^python",
                PackageKind::Flatpak => "Matches against installed app ids and names, e.g. ^org",
            }
            .to_string(),
            regex: self.pattern.clone(),
            on_input: Message::PackagePatternChanged,
            preview: Message::PackagePatternPreviewRequested,
            upgrade: Message::PackagePatternApplyRequested(PatternOp::Upgrade),
            remove: Message::PackagePatternApplyRequested(PatternOp::Remove),
            // Clicks fire whenever a pattern exists; blocked guards
            // answer with a copyable error instead of silence.
            armed: !self.pattern.trim().is_empty(),
            result: self.pattern_result.clone(),
        };

        // --- Bottom list (Installed / search results) ---
        // Filter the SOURCE against precomputed keys (zero allocations
        // per frame), then build ONLY the current page's widgets.
        let query = self.installed_filter.trim();
        let filtering = !query.is_empty();
        let (matched, rows): (usize, Vec<RowVm<'_>>) = match kind {
            PackageKind::Apk => {
                let (len, page) =
                    filtered_page(&self.apk, &self.filter_keys, query, self.installed_page);
                let rows = page
                    .into_iter()
                    .map(|package| {
                        let actions: Vec<Element<'_, Message>> = if package.protected {
                            vec![text("protected").into()]
                        } else {
                            let mut buttons: Vec<Element<'_, Message>> = Vec::with_capacity(2);
                            if self.apk_updates.iter().any(|u| u.name == package.name) {
                                buttons.push(
                                    button::text("Upgrade")
                                        .on_press_maybe(
                                            (!self.busy && !self.action_busy).then_some(
                                                Message::ApkUpgradeRequested(package.name.clone()),
                                            ),
                                        )
                                        .into(),
                                );
                            }
                            buttons.push(remove_button(
                                kind,
                                &package.name,
                                !self.busy && !self.action_busy,
                            ));
                            buttons
                        };
                        package_row(
                            &package.name,
                            &package.version,
                            package.description.as_ref(),
                            package.origin.as_ref().map(|origin| {
                                package.origin_label.as_ref().map_or_else(
                                    || origin.clone(),
                                    |label| format!("{label} · {origin}"),
                                )
                            }),
                            actions,
                            compact,
                        )
                    })
                    .collect();
                (len, rows)
            }
            PackageKind::Flatpak => {
                let (len, page) =
                    filtered_page(&self.flatpak, &self.filter_keys, query, self.installed_page);
                let rows = page
                    .into_iter()
                    .map(|package| {
                        let mut identity: Vec<&str> = vec![package.app_id.as_str()];
                        if !package.branch.is_empty() {
                            identity.push(&package.branch);
                        }
                        if !package.origin.is_empty() {
                            identity.push(&package.origin);
                        }
                        let mut line = identity.join(" · ");
                        if !package.origin.is_empty() {
                            if let Some(remote) = self
                                .flatpak_remotes
                                .iter()
                                .find(|remote| remote.name == package.origin)
                            {
                                line.push_str(" · ");
                                line.push_str(&remote.url);
                            }
                        }
                        let actions: Vec<Element<'_, Message>> =
                            if package.kind == FlatpakRefKind::Runtime {
                                vec![text("protected").into()]
                            } else {
                                let mut buttons: Vec<Element<'_, Message>> = Vec::with_capacity(3);
                                if self
                                    .flatpak_updates
                                    .iter()
                                    .any(|u| u.app_id == package.app_id)
                                {
                                    buttons.push(
                                        button::text("Upgrade")
                                            .on_press_maybe(
                                                (!self.busy && !self.action_busy).then_some(
                                                    Message::FlatpakUpgradeRequested(
                                                        package.app_id.clone(),
                                                    ),
                                                ),
                                            )
                                            .into(),
                                    );
                                }
                                buttons.push(remove_button(
                                    kind,
                                    &package.app_id,
                                    !self.busy && !self.action_busy,
                                ));
                                buttons
                            };
                        package_row(
                            &package.name,
                            &package.version,
                            package.description.as_ref(),
                            Some(line),
                            actions,
                            compact,
                        )
                    })
                    .collect();
                (len, rows)
            }
        };
        let (page_count, page, _) = page_bounds(matched, self.installed_page);
        let listed = matched;
        let empty_note = if rows.is_empty() {
            if filtering {
                Some(("No matches".to_string(), "No packages matched.".to_string()))
            } else if !self.busy {
                Some(("Result".to_string(), "No packages installed.".to_string()))
            } else {
                None
            }
        } else {
            None
        };
        let list = ListVm {
            browse: Message::PackageListBrowserOpen,
            page,
            page_count,
            page_previous: Message::PackageListPagePrevious,
            page_next: Message::PackageListPageNext,
            title: format!("Installed ({listed})"),
            filter: Some(FilterVm {
                value: self.installed_filter.clone(),
                on_input: Message::PackageFilterChanged,
            }),
            status: Vec::new(),
            empty_note,
            rows,
            footer: Vec::new(),
        };

        // --- Search dialog (own window, installed list untouched) ---
        let search_list = self.search_open.then(|| match kind {
            PackageKind::Apk => {
                let len = self.search_apk.len();
                let (page_count, page, start) = page_bounds(len, self.search_page);
                let rows =
                    self.search_apk
                        .iter()
                        .skip(start)
                        .take(crate::package_ui::PAGE_SIZE)
                        .map(|package| {
                            package_row(
                                &package.name,
                                &package.version,
                                package.description.as_ref(),
                                package.origin.clone(),
                                vec![button::text("Install")
                                    .on_press_maybe((!self.busy && !self.action_busy).then_some(
                                        Message::PackageInstallDirect(package.name.clone()),
                                    ))
                                    .into()],
                                compact,
                            )
                        })
                        .collect();
                ListVm {
                    title: format!("Search results ({len})"),
                    browse: Message::PackageSearchClose,
                    page,
                    page_count,
                    page_previous: Message::PackageSearchPagePrevious,
                    page_next: Message::PackageSearchPageNext,
                    filter: None,
                    status: search_status(self.search_busy, self.error.as_ref()),
                    empty_note: (!self.search_busy && len == 0)
                        .then(|| ("No matches".to_string(), "No packages matched.".to_string())),
                    rows,
                    footer: Vec::new(),
                }
            }
            PackageKind::Flatpak => {
                let len = self.search_flatpak.len();
                let (page_count, page, start) = page_bounds(len, self.search_page);
                let rows = self
                    .search_flatpak
                    .iter()
                    .skip(start)
                    .take(crate::package_ui::PAGE_SIZE)
                    .map(|package| {
                        let mut identity: Vec<&str> = vec![package.app_id.as_str()];
                        if !package.branch.is_empty() {
                            identity.push(&package.branch);
                        }
                        if !package.origin.is_empty() {
                            identity.push(&package.origin);
                        }
                        package_row(
                            &package.name,
                            &package.version,
                            package.description.as_ref(),
                            Some(identity.join(" · ")),
                            vec![button::text("Install")
                                .on_press_maybe((!self.busy && !self.action_busy).then_some(
                                    Message::PackageInstallDirect(package.app_id.clone()),
                                ))
                                .into()],
                            compact,
                        )
                    })
                    .collect();
                ListVm {
                    title: format!("Search results ({len})"),
                    browse: Message::PackageSearchClose,
                    page,
                    page_count,
                    page_previous: Message::PackageSearchPagePrevious,
                    page_next: Message::PackageSearchPageNext,
                    filter: None,
                    status: search_status(self.search_busy, self.error.as_ref()),
                    empty_note: (!self.search_busy && len == 0)
                        .then(|| ("No matches".to_string(), "No packages matched.".to_string())),
                    rows,
                    footer: Vec::new(),
                }
            }
        });

        PackageVm {
            title: kind.title().to_string(),
            refresh: RefreshVm {
                label: "Refresh".to_string(),
                message: Message::PackageRefresh,
                armed: !self.busy && !self.action_busy,
            },
            busy: if self.busy {
                Some("Reading package state…".to_string())
            } else if self.action_busy {
                Some("Running package action…".to_string())
            } else {
                None
            },
            error: self.error.clone(),
            message: self.message.as_deref(),
            confirms,
            scope,
            maintenance,
            upgrade,
            search,
            repositories,
            pattern,
            list,
            search_list,
            generations: None,
            generations_browse: Message::PackageListBrowserClose,
        }
    }
}
