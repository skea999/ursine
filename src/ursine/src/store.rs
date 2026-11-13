//! Nix store: search, profile mutations, generations and maintenance.
//!
//! The daemon owns every decision. This screen only renders daemon state and
//! sends the matching `packages_nix` action after a confirmation where needed.

use std::time::Duration;

use cosmic::widget::{button, dropdown, row, settings, text, text_input};
use cosmic::Element;
use ursine_api::{Action, CommandOutcome, InstalledPackage, SearchResult};

use crate::app::Message;
use crate::client::{GC_TIMEOUT, MUTATE_TIMEOUT, PROFILE_TIMEOUT, PRUNE_TIMEOUT};

const MAX_OUTPUT_CHARS: usize = 2000;

/// Profile the visible store state belongs to. Pending confirmations carry it
/// too, so a target-user edit cannot retarget an already-open confirmation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum StoreScope {
    #[default]
    NotLoaded,
    Principal,
    User(String),
}

impl StoreScope {
    pub fn from_target(target: Option<String>) -> Self {
        target.map_or(Self::Principal, Self::User)
    }

    pub fn as_user(&self) -> Option<String> {
        match self {
            Self::User(user) => Some(user.clone()),
            Self::NotLoaded | Self::Principal => None,
        }
    }

    pub fn matches(&self, target: Option<&str>) -> bool {
        match self {
            Self::NotLoaded => false,
            Self::Principal => target.is_none(),
            Self::User(user) => target == Some(user.as_str()),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::NotLoaded => "no profile loaded".into(),
            Self::Principal => "your profile".into(),
            Self::User(user) => format!("{user}'s profile"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingStoreAction {
    pub action: StoreAction,
    pub scope: StoreScope,
}

/// One user-initiated store operation. Confirmation policy lives here so the
/// view and the app agree on what is destructive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreAction {
    Install(String),
    Remove(String),
    RemoveAll,
    RemoveRegex {
        pattern: String,
        dry_run: bool,
    },
    Upgrade(String),
    UpgradeAll,
    UpgradeRegex {
        pattern: String,
        dry_run: bool,
    },
    Rollback(Option<u32>),
    DiffClosures(Option<u32>),
    WipeHistory {
        older_than: Option<String>,
        dry_run: bool,
    },
    StoreGc,
    PruneGenerations {
        keep: u32,
        dry_run: bool,
        /// Generation the preview showed; apply refuses if the profile moved.
        expected_current: Option<u32>,
        /// Exact generations the preview would delete.
        expected_delete: Option<Vec<u32>>,
    },
}

/// Only refs that resolve through the trusted nixpkgs input are unprompted.
/// Uses the daemon's own normalization so the prompt matches what runs.
fn is_trusted_installable(package: &str) -> bool {
    // Invalid refs are rejected before this check; never call them trusted.
    ursine_api::normalize_installable(package).is_ok_and(|effective| {
        effective.starts_with("nixpkgs#") || effective.starts_with("flake:nixpkgs#")
    })
}

impl StoreAction {
    /// Store GC is system-wide; every other action targets one profile.
    pub fn uses_profile(&self) -> bool {
        !matches!(self, Self::StoreGc)
    }

    pub fn target_label(&self, scope: &StoreScope) -> String {
        if self.uses_profile() {
            scope.label()
        } else {
            "system-wide (all users)".into()
        }
    }

    pub fn api(&self) -> Action {
        match self {
            Self::Install(package) => Action::Install {
                package: package.clone(),
            },
            Self::Remove(package) => Action::Remove {
                package: package.clone(),
            },
            Self::RemoveAll => Action::RemoveAll,
            Self::RemoveRegex { pattern, dry_run } => Action::RemoveRegex {
                pattern: pattern.clone(),
                dry_run: *dry_run,
            },
            Self::Upgrade(package) => Action::Upgrade {
                package: Some(package.clone()),
                all: false,
            },
            Self::UpgradeAll => Action::Upgrade {
                package: None,
                all: true,
            },
            Self::UpgradeRegex { pattern, dry_run } => Action::UpgradeRegex {
                pattern: pattern.clone(),
                dry_run: *dry_run,
            },
            Self::Rollback(generation) => Action::Rollback {
                generation: *generation,
            },
            Self::DiffClosures(generation) => Action::DiffClosures {
                generation: *generation,
            },
            Self::WipeHistory {
                older_than,
                dry_run,
            } => Action::WipeHistory {
                older_than: older_than.clone(),
                dry_run: *dry_run,
            },
            Self::StoreGc => Action::StoreGc,
            Self::PruneGenerations {
                keep,
                dry_run,
                expected_current,
                expected_delete,
            } => Action::PruneGenerations {
                keep: *keep,
                dry_run: *dry_run,
                expected_current: *expected_current,
                expected_delete: expected_delete.clone(),
            },
        }
    }

    /// Socket timeout matching the daemon's own limit for this action.
    pub fn timeout(&self) -> Duration {
        match self {
            Self::DiffClosures(_) => PROFILE_TIMEOUT,
            Self::StoreGc => GC_TIMEOUT,
            // Prune reads history (60 s) then deletes (900 s) under one daemon
            // request; keep the socket open past the 960 s worst case.
            Self::PruneGenerations { .. } => PRUNE_TIMEOUT,
            _ => MUTATE_TIMEOUT,
        }
    }

    pub fn requires_confirmation(&self) -> bool {
        matches!(
            self,
            Self::Install(package) if !is_trusted_installable(package)
        ) || matches!(
            self,
            Self::Remove(_)
                | Self::RemoveAll
                | Self::RemoveRegex { dry_run: false, .. }
                | Self::Rollback(_)
                | Self::StoreGc
                // Upgrades re-fetch the source, so a third-party flake can
                // evaluate new code without a fresh install prompt.
                | Self::Upgrade(_)
                | Self::UpgradeAll
                | Self::UpgradeRegex { dry_run: false, .. }
        ) || matches!(
            self,
            Self::WipeHistory { dry_run: false, .. }
                | Self::PruneGenerations { dry_run: false, .. }
        )
    }

    /// Prompt when a trusted ref still resolves through someone else's flake
    /// registry: that user can point `nixpkgs` at a third-party flake.
    pub fn requires_confirmation_in(&self, scope: &StoreScope) -> bool {
        self.requires_confirmation()
            || matches!(
                (self, scope),
                (Self::Install(package), StoreScope::User(_)) if is_trusted_installable(package)
            )
    }

    pub fn confirmation_in(&self, scope: &StoreScope) -> Option<String> {
        if let (Self::Install(package), StoreScope::User(user)) = (self, scope) {
            if is_trusted_installable(package) {
                return Some(format!(
                    "Install \"{package}\" into {user}'s profile? That user's flake registry decides which nixpkgs is used, so their entry can fetch third-party code."
                ));
            }
        }
        self.confirmation(scope)
    }

    pub fn confirmation(&self, scope: &StoreScope) -> Option<String> {
        let profile = scope.label();
        match self {
            Self::Install(package) if !is_trusted_installable(package) => Some(format!(
                "Install \"{package}\" into {profile}? This flake is not nixpkgs: its build can run third-party code."
            )),
            Self::Upgrade(package) => Some(format!(
                "Upgrade \"{package}\" in {profile}? A non-nixpkgs flake can fetch and run new code."
            )),
            Self::UpgradeAll => Some(format!(
                "Upgrade every package in {profile}? Non-nixpkgs flakes can fetch and run new code."
            )),
            Self::UpgradeRegex { pattern, .. } => Some(format!(
                "Upgrade every package matching \"{pattern}\" in {profile}? Non-nixpkgs flakes can fetch and run new code."
            )),
            Self::Remove(package) => {
                Some(format!("Remove \"{package}\" from {profile}?"))
            }
            Self::RemoveAll => Some(format!(
                "Remove every package from {profile}? The profile will be empty."
            )),
            Self::RemoveRegex { pattern, .. } => Some(format!(
                "Remove every package matching \"{pattern}\" from {profile}?"
            )),
            Self::Rollback(Some(generation)) => Some(format!(
                "Roll back {profile} to generation {generation}?"
            )),
            Self::Rollback(None) => Some(format!("Roll back {profile} to the previous generation?")),
            Self::WipeHistory { older_than, .. } => Some(older_than.as_deref().map_or_else(
                || {
                    format!(
                        "Delete all old generations from {profile}? The current generation is kept."
                    )
                },
                |age| {
                    format!(
                        "Delete generations older than {age} from {profile}? The current generation is kept."
                    )
                },
            )),
            Self::StoreGc => Some(
                "Delete unreachable paths from the nix store for all users? Running programs may break until their paths are rebuilt."
                    .into(),
            ),
            Self::PruneGenerations { keep, .. } => Some(format!(
                "Delete all generations from {profile} except the last {keep}?"
            )),
            _ => None,
        }
    }

    /// Whether the daemon changes profile/store state, so the view reloads.
    pub fn mutates(&self) -> bool {
        !matches!(
            self,
            Self::DiffClosures(_)
                | Self::WipeHistory { dry_run: true, .. }
                | Self::PruneGenerations { dry_run: true, .. }
        )
    }

    /// Profile list needs a reload after this action. GC changes the store, not
    /// the profile list, so it must not trigger a profile read.
    pub fn reloads_profile(&self) -> bool {
        self.mutates() && self.uses_profile()
    }

    /// User-facing operation name for long-running feedback.
    pub fn progress_label(&self) -> String {
        let text = match self {
            Self::Install(package) => format!("Installing {package}"),
            Self::Remove(package) => format!("Removing {package}"),
            Self::RemoveAll => "Removing every package".into(),
            Self::RemoveRegex { pattern, .. } => format!("Removing packages matching {pattern}"),
            Self::Upgrade(package) => format!("Upgrading {package}"),
            Self::UpgradeAll => "Upgrading every package".into(),
            Self::UpgradeRegex { pattern, .. } => format!("Upgrading packages matching {pattern}"),
            Self::Rollback(Some(generation)) => format!("Rolling back to generation {generation}"),
            Self::Rollback(None) => "Rolling back one generation".into(),
            Self::DiffClosures(_) => "Computing the closure diff".into(),
            Self::WipeHistory { .. } => "Deleting old generations".into(),
            Self::StoreGc => "Collecting the nix store".into(),
            Self::PruneGenerations { .. } => "Pruning generations".into(),
        };
        format!(
            "{text} (up to {} min)",
            self.timeout().as_secs().div_ceil(60)
        )
    }
}

/// Exact prune preview an apply is allowed to execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrunePreview {
    pub keep: u32,
    pub current: Option<u32>,
    pub would_delete: Vec<u32>,
}

pub struct StoreState {
    pub packages: Vec<InstalledPackage>,
    pub generations: Vec<String>,
    pub busy: bool,
    pub error: Option<String>,
    pub as_user_input: String,
    pub search_query: String,
    /// Registry flake the search runs against (default `nixpkgs`).
    pub search_repo: String,
    /// Flake registry entries for the target user (+ global, read-only).
    pub repositories: Vec<ursine_api::NixRepository>,
    /// Add-repository form inputs.
    pub repo_name: String,
    pub repo_url: String,
    /// Repository waiting for the remove confirmation.
    /// (name, scope) of the registry row awaiting confirmation.
    pub pending_repo_remove: Option<(String, String)>,
    /// Bumped after every registry mutation: an in-flight list reply
    /// that predates it is stale and gets dropped.
    pub repo_seq: u64,
    pub search_results: Vec<SearchResult>,
    pub search_busy: bool,
    pub search_done: bool,
    pub search_truncated: bool,
    pub search_error: Option<String>,
    /// Last successful prune preview; apply must match it exactly.
    pub prune_preview: Option<PrunePreview>,
    /// Fence for search replies: only the newest request may update results.
    pub search_seq: u64,
    pub install_input: String,
    pub regex: String,
    pub generation_input: String,
    /// Last `list_nix_updates` result (None = never checked this session).
    pub nix_updates: Option<ursine_api::NixUpdates>,
    /// A nix upgrade check is running right now.
    pub updates_checking: bool,
    pub keep_input: String,
    pub history_age: String,
    pub loaded_scope: StoreScope,
    pub pending: Option<PendingStoreAction>,
    pub action_busy: bool,
    pub action_message: Option<String>,
    pub action_label: Option<String>,
    pub action_started: Option<std::time::Instant>,
}

impl Default for StoreState {
    fn default() -> Self {
        Self {
            packages: Vec::new(),
            generations: Vec::new(),
            busy: false,
            error: None,
            as_user_input: String::new(),
            search_query: String::new(),
            search_repo: "nixpkgs".to_string(),
            repositories: Vec::new(),
            repo_name: String::new(),
            repo_url: String::new(),
            pending_repo_remove: None,
            repo_seq: 0,
            search_results: Vec::new(),
            search_busy: false,
            search_done: false,
            search_truncated: false,
            search_error: None,
            prune_preview: None,
            search_seq: 0,
            install_input: String::new(),
            regex: String::new(),
            generation_input: String::new(),
            nix_updates: None,
            updates_checking: false,
            keep_input: "5".into(),
            history_age: "30d".into(),
            loaded_scope: StoreScope::NotLoaded,
            pending: None,
            action_busy: false,
            action_message: None,
            action_label: None,
            action_started: None,
        }
    }
}

impl StoreState {
    /// Names offered by the search Repository dropdown: `nixpkgs` first,
    /// then every registry entry (single source for view + handler).
    pub fn search_repo_names(&self) -> Vec<String> {
        let mut names = vec!["nixpkgs".to_string()];
        for repository in &self.repositories {
            if !names.contains(&repository.name) {
                names.push(repository.name.clone());
            }
        }
        names
    }

    /// Drop a `search_repo` that a removal deleted (fall back to `nixpkgs`).
    pub fn revalidate_search_repo(&mut self) {
        if self.search_repo != "nixpkgs"
            && !self.repositories.iter().any(|r| r.name == self.search_repo)
        {
            self.search_repo = "nixpkgs".to_string();
        }
    }

    /// `None` means the principal's own profile; the daemon resolves it.
    pub fn target_user(&self) -> Option<String> {
        let user = self.as_user_input.trim();
        (!user.is_empty()).then(|| user.to_string())
    }

    pub fn current_scope(&self) -> StoreScope {
        StoreScope::from_target(self.target_user())
    }

    /// Change the target and drop profile data that belonged to the old one.
    /// Acting again requires an explicit reload.
    pub fn set_target_user(&mut self, value: String) {
        if value.trim() == self.as_user_input.trim() {
            self.as_user_input = value;
            return;
        }
        self.as_user_input = value;
        self.packages.clear();
        self.generations.clear();
        // Search results belong to the previous target; never show them as if
        // they described the new one.
        self.search_results.clear();
        self.search_done = false;
        self.search_busy = false;
        self.search_truncated = false;
        self.search_error = None;
        self.search_seq = self.search_seq.wrapping_add(1);
        self.loaded_scope = StoreScope::NotLoaded;
        self.pending = None;
        self.prune_preview = None;
        self.action_message = None;
    }

    /// Start a search and return its fence. Stale replies must be ignored.
    pub fn begin_search(&mut self) -> u64 {
        self.search_seq = self.search_seq.wrapping_add(1);
        self.search_busy = true;
        self.search_done = false;
        self.search_truncated = false;
        self.search_results.clear();
        self.search_error = None;
        self.search_seq
    }

    pub fn search_is_current(&self, seq: u64) -> bool {
        seq == self.search_seq
    }

    /// A prune apply is valid only for the exact preview the user saw.
    pub fn prune_apply_matches_preview(&self, keep: u32) -> bool {
        self.prune_preview
            .as_ref()
            .is_some_and(|preview| preview.keep == keep)
    }

    pub fn prune_expected_delete(&self) -> Option<Vec<u32>> {
        self.prune_preview
            .as_ref()
            .map(|preview| preview.would_delete.clone())
    }

    /// Actions target exactly the scope whose data is on screen, and no load is
    /// racing that scope change.
    pub fn scope_ready(&self) -> bool {
        !self.busy && !self.action_busy && self.loaded_scope.matches(self.target_user().as_deref())
    }

    pub fn needs_load(&self) -> bool {
        self.loaded_scope == StoreScope::NotLoaded && !self.busy
    }

    /// GC needs no profile; profile actions need the scope that is on screen.
    pub fn can_run(&self, action: &StoreAction) -> bool {
        !self.busy && !self.action_busy && (!action.uses_profile() || self.scope_ready())
    }

    /// The GUI is ONE code path (`package_ui::view`): this adapter only
    /// injects nix data — order and layout never live here.
    pub fn view(&self, updates_auto: bool) -> Element<'_, Message> {
        crate::package_ui::view(self.vm(updates_auto))
    }

    /// Everything the shared view renders, as data for this manager.
    fn vm(&self, updates_auto: bool) -> crate::package_ui::PackageVm<'_> {
        use crate::package_ui::{
            ConfirmVm, ListVm, PackageVm, RefreshVm, RowVm, SearchVm, SectionVm, UpgradeAllVm,
            UpgradeVm,
        };

        let enabled = self.scope_ready();

        // --- pending confirmations (all stack at the top) ---
        let mut confirms: Vec<ConfirmVm> = Vec::new();
        if let Some(pending) = &self.pending {
            let label = pending
                .action
                .confirmation_in(&pending.scope)
                .unwrap_or_else(|| "Apply this action?".into());
            let armed = self.can_run(&pending.action)
                && (!pending.action.uses_profile()
                    || pending.scope.matches(self.target_user().as_deref()));
            confirms.push(ConfirmVm {
                title: "Confirm action".to_string(),
                rows: vec![
                    (
                        "Target".to_string(),
                        pending.action.target_label(&pending.scope),
                    ),
                    ("Action".to_string(), label),
                ],
                confirm: Message::StoreActionConfirmed,
                cancel: Message::StoreConfirmCancelled,
                armed,
                destructive: false,
            });
        }
        if let Some((name, scope)) = &self.pending_repo_remove {
            let question = if scope == "global" {
                format!("Remove the system repository {name}?")
            } else {
                format!("Remove the repository {name}?")
            };
            confirms.push(ConfirmVm {
                title: "Confirm repository removal".to_string(),
                rows: vec![("Consequences".to_string(), question)],
                confirm: Message::StoreRepoRemoveConfirmed,
                cancel: Message::StoreRepoRemoveCancelled,
                armed: enabled && !self.action_busy && !self.search_busy,
                destructive: true,
            });
        }

        // --- Scope ---
        let mut scope_items: Vec<Element<'_, Message>> = vec![settings::item(
            "Target user",
            text_input("Signed-in user", self.as_user_input.clone())
                .on_input(Message::StoreAsUserChanged),
        )
        .into()];
        if self.needs_load() {
            scope_items.push(
                settings::item(
                    "Profile not loaded",
                    text("Refresh to load this target before acting on it."),
                )
                .into(),
            );
        }
        let scope = SectionVm {
            title: "Scope".to_string(),
            items: scope_items,
        };

        // --- Maintenance: prune, wipe history, store GC ---
        let keep_choices = ["1", "3", "5", "10", "20"];
        let history_choices = ["7d", "30d", "90d", "1y"];
        let maintenance = Some(SectionVm {
            title: "Maintenance".to_string(),
            items: vec![
                settings::item(
                    "Keep last generations",
                    row(vec![
                        dropdown(
                            keep_choices.to_vec(),
                            keep_choices
                                .iter()
                                .position(|value| *value == self.keep_input),
                            Message::StoreKeepPreset,
                        )
                        .into(),
                        text_input("5", self.keep_input.clone())
                            .on_input(Message::StoreKeepChanged)
                            .into(),
                        button::text("Preview prune")
                            .on_press_maybe(enabled.then_some(Message::StorePrunePreview))
                            .into(),
                        button::text("Prune")
                            .on_press_maybe(enabled.then_some(Message::StorePruneApply))
                            .into(),
                    ])
                    .spacing(8),
                )
                .into(),
                settings::item(
                    "Wipe history older than",
                    row(vec![
                        dropdown(
                            history_choices.to_vec(),
                            history_choices
                                .iter()
                                .position(|value| *value == self.history_age),
                            Message::StoreHistoryAgePreset,
                        )
                        .into(),
                        text_input("30d", self.history_age.clone())
                            .on_input(Message::StoreHistoryAgeChanged)
                            .into(),
                        button::text("Preview wipe")
                            .on_press_maybe(enabled.then_some(Message::StoreWipePreview))
                            .into(),
                        button::text("Wipe")
                            .on_press_maybe(enabled.then_some(Message::StoreWipeApply))
                            .into(),
                    ])
                    .spacing(8),
                )
                .into(),
                settings::item(
                    "Store garbage collection (all users)",
                    button::text("Run GC").on_press_maybe(
                        self.can_run(&StoreAction::StoreGc)
                            .then(|| Message::StoreActionRequested(StoreAction::StoreGc)),
                    ),
                )
                .into(),
            ],
        });

        // --- Upgrade ("Upgrade all" first, then the controls) ---
        let number: Element<'_, Message> = if self.updates_checking {
            text("Checking…").into()
        } else {
            self.nix_updates.as_ref().map_or_else(
                || text("—").into(),
                |updates| text::title2(updates.count.to_string()).into(),
            )
        };
        let upgrade = UpgradeVm {
            all: Some(UpgradeAllVm {
                label: "Upgrade all".to_string(),
                message: Message::StoreActionRequested(StoreAction::UpgradeAll),
                armed: enabled,
            }),
            manager: "nix".to_string(),
            auto: updates_auto,
            number,
            refresh: Message::NixUpdatesRequested,
            refresh_armed: enabled && !self.updates_checking,
        };

        // --- Search: controls only; results render in the bottom list ---
        let repo_names = self.search_repo_names();
        let repo_selected = repo_names
            .iter()
            .position(|name| *name == self.search_repo)
            .unwrap_or(0);
        let can_search = !self.search_busy && !self.search_query.trim().is_empty();
        let search = SearchVm {
            repository: Some(
                settings::item(
                    "Repository",
                    dropdown(
                        repo_names,
                        Some(repo_selected),
                        Message::StoreSearchRepoChanged,
                    ),
                )
                .into(),
            ),
            query_placeholder: "Package name".to_string(),
            query: self.search_query.clone(),
            on_query_input: Message::StoreSearchChanged,
            query_submit: Some(Message::StoreSearchSubmitted),
            query_label: if self.search_busy {
                "Searching…"
            } else {
                "Search"
            }
            .to_string(),
            search: Message::StoreSearchSubmitted,
            search_armed: can_search,
            install_placeholder: "nixpkgs#hello".to_string(),
            install: self.install_input.clone(),
            on_install_input: Message::StoreInstallChanged,
            install_extras: Vec::new(),
            install_trailing: Vec::new(),
            install_message: Message::StoreActionRequested(StoreAction::Install(
                self.install_input.trim().to_string(),
            )),
            install_armed: enabled && !self.install_input.trim().is_empty(),
        };

        // --- Flake repositories ---
        let mut repo_items: Vec<Element<'_, Message>> = Vec::new();
        // Only after the load settled: an in-flight load must not claim
        // "only the default entry" while the rows are still coming.
        if self.repositories.is_empty() && self.error.is_none() && !self.busy {
            repo_items.push(
                settings::item(
                    "Entries",
                    text("Only the default nixpkgs repository (nothing extra)."),
                )
                .into(),
            );
        }
        for repository in &self.repositories {
            // Every row removes (apk/flatpak parity): user/default
            // through `nix registry remove`, global through the system
            // registry file.
            let action = button::destructive("Remove")
                .on_press_maybe(
                    (enabled && !self.action_busy && !self.search_busy).then_some(
                        Message::StoreRepoRemoveRequested(
                            repository.name.clone(),
                            repository.scope.clone(),
                        ),
                    ),
                )
                .into();
            let detail = format!("{} · {}", repository.name, repository.scope);
            repo_items.push(crate::package_ui::repository_row(
                repository.url.clone(),
                Some(detail),
                vec![action],
            ));
        }
        repo_items.push(
            settings::item(
                "Add repository",
                row(vec![
                    text_input("name", self.repo_name.clone())
                        .on_input(Message::StoreRepoNameChanged)
                        .into(),
                    text_input("github:owner/repo", self.repo_url.clone())
                        .on_input(Message::StoreRepoUrlChanged)
                        .into(),
                    button::text("Add")
                        .on_press_maybe(
                            (enabled
                                && !self.action_busy
                                && !self.search_busy
                                && !self.repo_name.trim().is_empty()
                                && !self.repo_url.trim().is_empty())
                            .then_some(Message::StoreRepoAdd),
                        )
                        .into(),
                ])
                .spacing(8),
            )
            .into(),
        );
        let repositories = SectionVm {
            title: "Repositories".to_string(),
            items: repo_items,
        };

        // --- Pattern (nix schema, shared builder) ---
        let pattern = crate::package_ui::PatternSection {
            hint: "Matches against installed package names, e.g. ^python".to_string(),
            regex: self.regex.clone(),
            on_input: Message::StoreRegexChanged,
            preview: Message::StoreActionRequested(StoreAction::RemoveRegex {
                pattern: self.regex.trim().to_string(),
                dry_run: true,
            }),
            upgrade: Message::StoreActionRequested(StoreAction::UpgradeRegex {
                pattern: self.regex.trim().to_string(),
                dry_run: false,
            }),
            remove: Message::StoreActionRequested(StoreAction::RemoveRegex {
                pattern: self.regex.trim().to_string(),
                dry_run: false,
            }),
            armed: enabled && !self.regex.trim().is_empty() && !self.action_busy,
        };

        // --- Bottom list: search results while a search is done,
        // otherwise the installed profile (apk pattern) ---
        let showing_results = self.search_done;
        let rows: Vec<RowVm<'_>> = if showing_results {
            self.search_results
                .iter()
                .map(|result| RowVm {
                    title: match (&result.pname, &result.version) {
                        (Some(name), Some(version)) => format!("{name} {version}"),
                        (Some(name), None) => name.clone(),
                        (None, _) => result.attr.clone(),
                    },
                    // Old search rows always carried a description
                    // line — keep the "no description" placeholder.
                    description: Some(
                        result
                            .description
                            .clone()
                            .filter(|value| !value.is_empty())
                            .unwrap_or_else(|| "no description".to_string()),
                    ),
                    repository: None,
                    notes: vec![format!("attr: {}", result.attr)],
                    actions: vec![button::text("Install")
                        .on_press_maybe(enabled.then(|| {
                            Message::StoreActionRequested(StoreAction::Install(
                                result
                                    .installable
                                    .clone()
                                    .unwrap_or_else(|| result.attr.clone()),
                            ))
                        }))
                        .into()],
                })
                .collect()
        } else {
            self.packages
                .iter()
                .map(|package| {
                    let (description, repository, notes) =
                        package_detail_parts(package, &self.repositories);
                    let mut actions: Vec<Element<'_, Message>> = Vec::with_capacity(2);
                    // Upgrade only when the last check named this element
                    // as outdated; without a check the button is gone.
                    let updatable = self.nix_updates.as_ref().is_some_and(|updates| {
                        updates.packages.iter().any(|name| name == &package.name)
                    });
                    if updatable {
                        actions.push(
                            button::text("Upgrade")
                                .on_press_maybe(enabled.then(|| {
                                    Message::StoreActionRequested(StoreAction::Upgrade(
                                        package.name.clone(),
                                    ))
                                }))
                                .into(),
                        );
                    }
                    actions.push(
                        button::text("Remove")
                            .on_press_maybe(enabled.then(|| {
                                Message::StoreActionRequested(StoreAction::Remove(
                                    package.name.clone(),
                                ))
                            }))
                            .into(),
                    );
                    RowVm {
                        title: package.name.clone(),
                        description,
                        repository,
                        notes,
                        actions,
                    }
                })
                .collect()
        };
        let empty_note = if rows.is_empty() {
            if showing_results {
                Some(("No matches".to_string(), "No packages matched.".to_string()))
            } else if !self.busy {
                Some((
                    "No packages".to_string(),
                    "This profile has no installed packages.".to_string(),
                ))
            } else {
                None
            }
        } else {
            None
        };
        let status = if !showing_results && self.busy && self.packages.is_empty() {
            vec![("Status".to_string(), "Loading profile…".to_string())]
        } else {
            Vec::new()
        };
        let footer: Vec<Element<'_, Message>> = if showing_results {
            Vec::new()
        } else {
            vec![settings::item(
                "Profile actions",
                button::text("Remove all").on_press_maybe(
                    enabled.then(|| Message::StoreActionRequested(StoreAction::RemoveAll)),
                ),
            )
            .into()]
        };
        let list = ListVm {
            title: if showing_results {
                if self.search_truncated {
                    format!(
                        "Search results (first {} of more — narrow the query)",
                        self.search_results.len()
                    )
                } else {
                    format!("Search results ({})", self.search_results.len())
                }
            } else {
                format!("Installed ({})", self.packages.len())
            },
            status,
            empty_note,
            rows,
            footer,
        };

        // --- Generations (nix-only slot) ---
        let mut generation_items: Vec<Element<'_, Message>> = vec![settings::item(
            "Rollback / diff",
            row(vec![
                text_input(
                    "Generation (blank = previous)",
                    self.generation_input.clone(),
                )
                .on_input(Message::StoreGenerationChanged)
                .into(),
                button::text("Rollback")
                    .on_press_maybe(enabled.then_some(Message::StoreRollbackRequested))
                    .into(),
                button::text("Diff closures")
                    .on_press_maybe(enabled.then_some(Message::StoreDiffRequested))
                    .into(),
            ])
            .spacing(8),
        )
        .into()];
        if self.generations.is_empty() {
            generation_items.push(
                settings::item("No history", text("No profile generations reported.")).into(),
            );
        } else {
            // One row per generation: the NUMBER is the item label.
            let mut entries: Vec<(String, String)> = Vec::new();
            for line in &self.generations {
                if let Some(rest) = line.strip_prefix("Version ") {
                    let (number, body) = match rest.split_once(' ') {
                        Some((number, body)) => (number.to_string(), body.to_string()),
                        None => (rest.to_string(), String::new()),
                    };
                    entries.push((number, body));
                } else if let Some(last) = entries.last_mut() {
                    last.1.push('\n');
                    last.1.push_str(line);
                } else {
                    entries.push((String::new(), line.clone()));
                }
            }
            for (number, body) in entries {
                let title = if number.is_empty() {
                    "History".to_string()
                } else {
                    number
                };
                let body = if body.is_empty() {
                    "(no details)".to_string()
                } else {
                    body
                };
                generation_items.push(settings::item(title, text(body)).into());
            }
        }
        let generations = Some(SectionVm {
            title: format!("Generations ({})", self.generations.len()),
            items: generation_items,
        });

        // --- one busy line (data): action > search > none ---
        let busy = if self.action_busy {
            let label = self
                .action_label
                .clone()
                .unwrap_or_else(|| "Running profile action".into());
            let elapsed = self
                .action_started
                .map_or(0, |started| started.elapsed().as_secs());
            Some(format!("{label} — {elapsed} s elapsed…"))
        } else if self.search_busy {
            Some(format!(
                "Searching {}. This can take several minutes.",
                self.search_repo
            ))
        } else {
            None
        };

        PackageVm {
            title: "Nix store".to_string(),
            refresh: RefreshVm {
                label: if self.busy {
                    "Refreshing…".to_string()
                } else {
                    "Refresh".to_string()
                },
                message: Message::RefreshStore,
                armed: !self.busy && !self.action_busy,
            },
            busy,
            // Both errors stay visible (top status shows them together).
            error: match (self.error.clone(), self.search_error.clone()) {
                (Some(error), Some(search)) => Some(format!("{error}\n{search}")),
                (error, search) => error.or(search),
            },
            message: self.action_message.as_deref(),
            confirms,
            scope,
            maintenance,
            upgrade,
            search,
            repositories,
            pattern,
            list,
            generations,
        }
    }
}

/// Human-readable summary of one daemon response.
pub fn summarize(action: &StoreAction, value: &serde_json::Value) -> String {
    match action {
        StoreAction::DiffClosures(_) => {
            let lines = value
                .get("lines")
                .and_then(|lines| lines.as_array())
                .map(|lines| {
                    lines
                        .iter()
                        .filter_map(|line| line.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            if lines.is_empty() {
                "No differences reported.".into()
            } else {
                truncate(&lines)
            }
        }
        StoreAction::PruneGenerations { dry_run, .. } => prune_summary(*dry_run, value),
        StoreAction::RemoveRegex { dry_run: true, .. }
        | StoreAction::UpgradeRegex { dry_run: true, .. } => regex_preview_summary(value),
        _ => serde_json::from_value::<CommandOutcome>(value.clone()).map_or_else(
            |_| {
                // Never render machine JSON to the user. Known line-list
                // payloads get their text; anything else is a plain notice.
                value
                    .get("lines")
                    .and_then(serde_json::Value::as_array)
                    .map(|lines| {
                        lines
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .filter(|text| !text.is_empty())
                    .map_or_else(|| "Completed.".into(), |text| truncate(&text))
            },
            |outcome| outcome_summary(&outcome),
        ),
    }
}

fn regex_preview_summary(value: &serde_json::Value) -> String {
    let names = value
        .get("would_change")
        .and_then(|names| names.as_array())
        .map(|names| {
            names
                .iter()
                .filter_map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    if names.is_empty() {
        "No installed package matches.".into()
    } else {
        format!("Matching packages: {names}.")
    }
}

fn prune_summary(dry_run: bool, value: &serde_json::Value) -> String {
    let current = value
        .get("current")
        .and_then(serde_json::Value::as_u64)
        .map_or_else(String::new, |current| {
            format!(" Current generation: {current}.")
        });
    let key = if dry_run { "would_delete" } else { "deleted" };
    let generations = generation_list(value, key);
    if generations.is_empty() {
        format!("Nothing to prune.{current}")
    } else if dry_run {
        format!("Would delete generations: {generations}.{current}")
    } else {
        format!("Deleted generations: {generations}.{current}")
    }
}

fn generation_list(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(serde_json::Value::as_u64)
                .map(|generation| generation.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn outcome_summary(outcome: &CommandOutcome) -> String {
    let text = if outcome.stdout.trim().is_empty() {
        outcome.stderr.trim()
    } else {
        outcome.stdout.trim()
    };
    if text.is_empty() {
        "Done.".into()
    } else {
        truncate(text)
    }
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_OUTPUT_CHARS {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(MAX_OUTPUT_CHARS).collect();
    clipped.push_str("\n…");
    clipped
}

/// Split a package into the shared row's detail lines: description →
/// repository (`attr original_url · https link`) → notes. The shared
/// view composes them identically for every manager.
fn package_detail_parts(
    package: &InstalledPackage,
    repositories: &[ursine_api::NixRepository],
) -> (Option<String>, Option<String>, Vec<String>) {
    let description = package
        .description
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let mut identity: Vec<&str> = Vec::new();
    if let Some(attr) = &package.attr_path {
        identity.push(attr);
    }
    if let Some(repository) = &package.original_url {
        identity.push(repository);
    }
    let mut line = identity.join(" ");
    if let Some(link) = package_https(package, repositories) {
        if !line.is_empty() {
            line.push_str(" · ");
        }
        line.push_str(&link);
    }
    let repository = (!line.is_empty()).then_some(line);
    let mut notes: Vec<String> = Vec::new();
    if !package.active {
        notes.push("state: inactive".to_string());
    } else if description.is_none() && repository.is_none() {
        notes.push("state: installed".to_string());
    }
    (description, repository, notes)
}

/// Best https link for the package's repository: the registry entry for
/// `original_url` first (a ref like `nixos-26.05` beats a pinned
/// revision), then the element's own locked url, then the reference.
fn package_https(
    package: &InstalledPackage,
    repositories: &[ursine_api::NixRepository],
) -> Option<String> {
    if let Some(original) = &package.original_url {
        // Registry rows drop the `flake:` prefix (`nix registry list`
        // prints `flake:nixpkgs`, the GUI name is `nixpkgs`): match
        // both forms so the registry entry wins when it exists.
        let key = original.strip_prefix("flake:").unwrap_or(original);
        if let Some(entry) = repositories
            .iter()
            .find(|repo| repo.name == key || repo.name == *original)
        {
            if let Some(link) = flake_to_https(&entry.url) {
                return Some(link);
            }
        }
    }
    package
        .url
        .as_deref()
        .and_then(flake_to_https)
        .or_else(|| package.original_url.as_deref().and_then(flake_to_https))
}

/// `github:owner/repo[/ref]` → `https://github.com/owner/repo[/tree/ref]`
/// (same for `gitlab:`); http(s) and `git+https` pass through after the
/// query string is stripped. Anything else → None.
fn flake_to_https(reference: &str) -> Option<String> {
    let trimmed = reference.split('?').next().unwrap_or(reference);
    let stripped = trimmed.strip_prefix("git+").unwrap_or(trimmed);
    if stripped.starts_with("https://") || stripped.starts_with("http://") {
        return Some(stripped.to_string());
    }
    for (prefix, host) in [("github:", "github.com"), ("gitlab:", "gitlab.com")] {
        if let Some(rest) = stripped.strip_prefix(prefix) {
            let mut parts = rest.splitn(3, '/');
            let owner = parts.next()?;
            let repo = parts.next()?;
            if owner.is_empty() || repo.is_empty() {
                return None;
            }
            let mut link = format!("https://{host}/{owner}/{repo}");
            if let Some(reference) = parts.next() {
                if !reference.is_empty() {
                    link.push_str("/tree/");
                    link.push_str(reference);
                }
            }
            return Some(link);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flake_refs_become_https_links() {
        assert_eq!(
            flake_to_https("github:NixOS/nixpkgs/nixos-26.05"),
            Some("https://github.com/NixOS/nixpkgs/tree/nixos-26.05".to_string())
        );
        assert_eq!(
            flake_to_https("github:a/b/13d0c311e3ae923a00f734b43fd1d35b47d8943a?narHash=sha256-x"),
            Some(
                "https://github.com/a/b/tree/13d0c311e3ae923a00f734b43fd1d35b47d8943a".to_string()
            )
        );
        assert_eq!(
            flake_to_https("https://example.com/x.git"),
            Some("https://example.com/x.git".to_string())
        );
        assert_eq!(flake_to_https("flake:nixpkgs"), None);
    }

    #[test]
    fn blank_target_user_means_the_principal() {
        let mut state = StoreState::default();
        assert_eq!(state.target_user(), None);
        state.as_user_input = "  ".into();
        assert_eq!(state.target_user(), None);
        state.as_user_input = " bob ".into();
        assert_eq!(state.target_user().as_deref(), Some("bob"));
    }

    #[test]
    fn trust_mirrors_daemon_installable_normalization() {
        // Bare attrs are prefixed with `nixpkgs#` by the daemon.
        assert!(is_trusted_installable("hello"));
        assert!(is_trusted_installable("python3Packages.requests"));
        // A bare ref without an attr cannot be fetched: the prefix makes nix
        // fail evaluation, so it must not claim a third-party build runs.
        assert!(is_trusted_installable("github:attacker/repo"));
        // Explicit non-nixpkgs flake refs are evaluated and must prompt.
        assert!(!is_trusted_installable("github:attacker/repo#pkg"));
        assert!(!is_trusted_installable(
            "https://example.com/flake.tar.gz#pkg"
        ));
        // Invalid refs are rejected before the prompt; never trust them.
        assert!(!is_trusted_installable("--all"));
        assert!(!is_trusted_installable("./flake#pkg"));
        // Both trusted spellings stay unprompted.
        assert!(is_trusted_installable("nixpkgs#hello"));
        assert!(is_trusted_installable("flake:nixpkgs#hello"));
    }

    #[test]
    fn install_confirmation_follows_effective_installable() {
        assert!(!StoreAction::Install("hello".into()).requires_confirmation());
        assert!(!StoreAction::Install("github:attacker/repo".into()).requires_confirmation());
        assert!(StoreAction::Install("github:attacker/repo#pkg".into()).requires_confirmation());
        assert!(StoreAction::Install("nixpkgs#hello".into())
            .confirmation(&StoreScope::Principal)
            .is_none());
    }

    #[test]
    fn destructive_actions_request_confirmation() {
        for action in [
            StoreAction::Remove("hello".into()),
            StoreAction::RemoveAll,
            StoreAction::RemoveRegex {
                pattern: "^python".into(),
                dry_run: false,
            },
            StoreAction::Rollback(Some(3)),
            StoreAction::Rollback(None),
            StoreAction::StoreGc,
            StoreAction::WipeHistory {
                older_than: Some("30d".into()),
                dry_run: false,
            },
            StoreAction::PruneGenerations {
                keep: 3,
                dry_run: false,
                expected_current: None,
                expected_delete: None,
            },
            StoreAction::Install("github:attacker/repo#pkg".into()),
            StoreAction::Upgrade("hello".into()),
            StoreAction::UpgradeAll,
            StoreAction::UpgradeRegex {
                pattern: "^python".into(),
                dry_run: false,
            },
        ] {
            assert!(action.requires_confirmation(), "{action:?} must ask first");
        }
        for action in [
            StoreAction::Install("hello".into()),
            StoreAction::Install("nixpkgs#hello".into()),
            StoreAction::DiffClosures(None),
            StoreAction::RemoveRegex {
                pattern: "^python".into(),
                dry_run: true,
            },
            StoreAction::UpgradeRegex {
                pattern: "^python".into(),
                dry_run: true,
            },
            StoreAction::WipeHistory {
                older_than: Some("30d".into()),
                dry_run: true,
            },
            StoreAction::PruneGenerations {
                keep: 3,
                dry_run: true,
                expected_current: None,
                expected_delete: None,
            },
        ] {
            assert!(
                !action.requires_confirmation(),
                "{action:?} must not ask first"
            );
        }
    }

    #[test]
    fn pending_confirmation_names_its_scope() {
        let scope = StoreScope::User("alice".into());
        let text = StoreAction::Remove("hello".into())
            .confirmation(&scope)
            .unwrap();
        assert!(text.contains("alice's profile"), "{text}");
        assert!(!text.contains("this profile"), "{text}");
    }

    #[test]
    fn scope_matches_only_the_loaded_target() {
        let principal = StoreScope::from_target(None);
        let alice = StoreScope::from_target(Some("alice".into()));
        assert!(principal.matches(None));
        assert!(!principal.matches(Some("alice")));
        assert!(alice.matches(Some("alice")));
        assert!(!alice.matches(None));
        assert!(!StoreScope::NotLoaded.matches(None));
        assert_eq!(alice.label(), "alice's profile");
        assert_eq!(principal.label(), "your profile");
    }

    #[test]
    fn actions_wait_for_a_matching_load() {
        let mut state = StoreState::default();
        assert!(!state.scope_ready());
        state.loaded_scope = StoreScope::Principal;
        assert!(state.scope_ready());
        state.as_user_input = "alice".into();
        assert!(!state.scope_ready());
        state.loaded_scope = StoreScope::User("alice".into());
        assert!(state.scope_ready());
    }

    #[test]
    fn changing_target_drops_profile_data() {
        let mut state = StoreState {
            loaded_scope: StoreScope::User("alice".into()),
            ..StoreState::default()
        };
        state.packages.push(InstalledPackage {
            name: "hello".into(),
            attr_path: None,
            original_url: None,
            url: None,
            store_paths: Vec::new(),
            active: true,

            description: None,
        });
        state.generations.push("Version 3".into());
        state.set_target_user("bob".into());
        assert!(state.packages.is_empty());
        assert!(state.generations.is_empty());
        assert_eq!(state.loaded_scope, StoreScope::NotLoaded);
        assert!(state.needs_load());
        assert!(!state.scope_ready());
    }

    #[test]
    fn gc_is_system_wide_and_needs_no_profile() {
        let state = StoreState::default();
        let gc = StoreAction::StoreGc;
        assert!(!gc.uses_profile());
        assert!(state.can_run(&gc), "no profile needed for store GC");
        assert_eq!(
            gc.target_label(&StoreScope::Principal),
            "system-wide (all users)"
        );
        assert!(StoreAction::Remove("hello".into()).uses_profile());
        assert!(!state.can_run(&StoreAction::Remove("hello".into())));
    }

    #[test]
    fn prune_apply_carries_the_previewed_generation() {
        let action = StoreAction::PruneGenerations {
            keep: 3,
            dry_run: false,
            expected_current: Some(7),
            expected_delete: Some(vec![3, 4]),
        };
        assert_eq!(
            action.api(),
            Action::PruneGenerations {
                keep: 3,
                dry_run: false,
                expected_current: Some(7),
                expected_delete: Some(vec![3, 4]),
            }
        );
    }

    #[test]
    fn changing_target_drops_the_previous_search_and_preview() {
        let mut state = StoreState {
            search_results: vec![SearchResult {
                attr: "legacyPackages.x86_64-linux.hello".into(),
                installable: Some("hello".into()),
                pname: Some("hello".into()),
                version: None,
                description: None,
            }],
            search_done: true,
            search_truncated: true,
            prune_preview: Some(PrunePreview {
                keep: 3,
                current: Some(7),
                would_delete: vec![3, 4],
            }),
            ..StoreState::default()
        };
        state.set_target_user("alice".into());
        assert!(state.search_results.is_empty());
        assert!(!state.search_done);
        assert!(!state.search_truncated);
        assert_eq!(state.prune_preview, None);
        assert!(!state.search_busy);
    }

    #[test]
    fn explicit_target_user_prompts_even_for_trusted_refs() {
        let action = StoreAction::Install("nixpkgs#hello".into());
        assert!(!action.requires_confirmation_in(&StoreScope::Principal));
        assert!(action.requires_confirmation_in(&StoreScope::User("alice".into())));
        let text = action
            .confirmation_in(&StoreScope::User("alice".into()))
            .unwrap();
        assert!(text.contains("alice's profile"), "{text}");
        assert!(text.contains("flake registry"), "{text}");
    }

    #[test]
    fn prune_apply_requires_the_matching_preview() {
        let mut state = StoreState::default();
        assert!(!state.prune_apply_matches_preview(3));
        state.prune_preview = Some(PrunePreview {
            keep: 3,
            current: Some(7),
            would_delete: vec![3, 4],
        });
        assert!(state.prune_apply_matches_preview(3));
        assert_eq!(state.prune_expected_delete(), Some(vec![3, 4]));
        assert!(
            !state.prune_apply_matches_preview(2),
            "a different keep count needs its own preview"
        );
    }

    #[test]
    fn stale_search_replies_are_fenced() {
        let mut state = StoreState::default();
        let first = state.begin_search();
        let second = state.begin_search();
        assert!(!state.search_is_current(first));
        assert!(state.search_is_current(second));
        state.set_target_user("alice".into());
        assert!(!state.search_is_current(second));
        assert!(!state.search_busy);
    }

    #[test]
    fn regex_preview_summarizes_matches() {
        let value =
            serde_json::json!({"dry_run": true, "would_change": ["python3-a", "python3-b"]});
        assert_eq!(
            summarize(
                &StoreAction::RemoveRegex {
                    pattern: "^python".into(),
                    dry_run: true,
                },
                &value
            ),
            "Matching packages: python3-a, python3-b."
        );
        let empty = serde_json::json!({"dry_run": true, "would_change": []});
        assert_eq!(
            summarize(
                &StoreAction::UpgradeRegex {
                    pattern: "^python".into(),
                    dry_run: true,
                },
                &empty
            ),
            "No installed package matches."
        );
    }

    #[test]
    fn unknown_payloads_summarize_without_raw_json() {
        let text = summarize(
            &StoreAction::Remove("hello".into()),
            &serde_json::json!({"unexpected": "machine", "payload": 7}),
        );
        assert_eq!(text, "Completed.");
    }

    #[test]
    fn prune_keeps_the_socket_open_past_the_daemon_worst_case() {
        let timeout = StoreAction::PruneGenerations {
            keep: 3,
            dry_run: false,
            expected_current: None,
            expected_delete: None,
        }
        .timeout();
        assert_eq!(
            timeout,
            Duration::from_secs(1000),
            "daemon 960 s worst case + 30 s write + 10 s margin"
        );
    }

    #[test]
    fn prune_preview_summarizes_the_plan() {
        let value = serde_json::json!({
            "dry_run": true,
            "current": 7,
            "would_delete": [3, 4],
        });
        assert_eq!(
            summarize(
                &StoreAction::PruneGenerations {
                    keep: 2,
                    dry_run: true,
                    expected_current: None,
                    expected_delete: None,
                },
                &value
            ),
            "Would delete generations: 3, 4. Current generation: 7."
        );
    }
}
