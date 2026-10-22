//! Nix store: search, profile mutations, generations and maintenance.
//!
//! The daemon owns every decision. This screen only renders daemon state and
//! sends the matching `packages_nix` action after a confirmation where needed.

use std::time::Duration;

use cosmic::widget::{button, column, row, scrollable, settings, text, text_input};
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

    pub fn view(&self) -> Element<'_, Message> {
        let header = row(vec![
            button::text("Home").on_press(Message::BackHome).into(),
            text::title2("Nix store").into(),
            text("").width(cosmic::iced::Length::Fill).into(),
            button::standard(if self.busy {
                "Refreshing…"
            } else {
                "Refresh"
            })
            .on_press_maybe((!self.busy && !self.action_busy).then_some(Message::RefreshStore))
            .into(),
        ])
        .spacing(12);

        let mut items: Vec<Element<'_, Message>> = vec![
            header.into(),
            text("Search, install and maintain the target user's nix profile.").into(),
        ];

        if let Some(error) = &self.error {
            items.push(text(error.clone()).into());
        }
        if let Some(message) = &self.action_message {
            items.push(
                settings::section()
                    .title("Last action")
                    .add(settings::item("Result", text(message.clone())))
                    .into(),
            );
        }
        if self.action_busy {
            let label = self
                .action_label
                .clone()
                .unwrap_or_else(|| "Running profile action".into());
            let elapsed = self
                .action_started
                .map_or(0, |started| started.elapsed().as_secs());
            items.push(text(format!("{label} — {elapsed} s elapsed…")).into());
        }
        if let Some(pending) = &self.pending {
            let label = pending
                .action
                .confirmation_in(&pending.scope)
                .unwrap_or_else(|| "Apply this action?".into());
            let confirm = self.can_run(&pending.action)
                && (!pending.action.uses_profile()
                    || pending.scope.matches(self.target_user().as_deref()));
            items.push(
                settings::section()
                    .title("Confirm action")
                    .add(settings::item(
                        "Target",
                        text(pending.action.target_label(&pending.scope)),
                    ))
                    .add(settings::item(
                        label,
                        row(vec![
                            button::suggested("Confirm")
                                .on_press_maybe(confirm.then_some(Message::StoreActionConfirmed))
                                .into(),
                            button::text("Cancel")
                                .on_press(Message::StoreConfirmCancelled)
                                .into(),
                        ])
                        .spacing(8),
                    ))
                    .into(),
            );
        }

        let mut scope_section = settings::section().title("Scope").add(settings::item(
            "Target user",
            text_input("Signed-in user", self.as_user_input.clone())
                .on_input(Message::StoreAsUserChanged),
        ));
        if self.needs_load() {
            scope_section = scope_section.add(settings::item(
                "Profile not loaded",
                text("Refresh to load this target before acting on it."),
            ));
        }
        items.push(scope_section.into());
        items.push(self.search_view());
        items.push(self.installed_view());
        items.push(self.advanced_view());
        items.push(self.generations_view());
        items.push(self.maintenance_view());

        scrollable(column(items).spacing(16).padding(24)).into()
    }

    fn search_view(&self) -> Element<'_, Message> {
        let can_search = !self.search_busy && !self.search_query.trim().is_empty();
        let can_act = self.scope_ready();
        let mut section = settings::section()
            .title("Search nixpkgs")
            .add(settings::item(
                "Query",
                row(vec![
                    text_input("Package name", self.search_query.clone())
                        .on_input(Message::StoreSearchChanged)
                        .on_submit(|_| Message::StoreSearchSubmitted)
                        .into(),
                    button::text(if self.search_busy {
                        "Searching…"
                    } else {
                        "Search"
                    })
                    .on_press_maybe(can_search.then_some(Message::StoreSearchSubmitted))
                    .into(),
                ])
                .spacing(8),
            ));

        section = section.add(settings::item(
            "Direct install",
            row(vec![
                text_input("nixpkgs#hello", self.install_input.clone())
                    .on_input(Message::StoreInstallChanged)
                    .into(),
                button::text("Install")
                    .on_press_maybe((can_act && !self.install_input.trim().is_empty()).then(|| {
                        Message::StoreActionRequested(StoreAction::Install(
                            self.install_input.trim().to_string(),
                        ))
                    }))
                    .into(),
            ])
            .spacing(8),
        ));

        if self.search_busy {
            section = section.add(settings::item(
                "Status",
                text("Searching nixpkgs. This can take several minutes."),
            ));
        }
        if let Some(error) = &self.search_error {
            section = section.add(settings::item("Search failed", text(error.clone())));
        }

        if self.search_results.is_empty() && self.search_done {
            section = section.add(settings::item("No matches", text("No packages matched.")));
        }
        if !self.search_results.is_empty() {
            let title = if self.search_truncated {
                format!(
                    "Search results (first {} of more — narrow the query)",
                    self.search_results.len()
                )
            } else {
                format!("Search results ({})", self.search_results.len())
            };
            section = section.title(title);
            for result in &self.search_results {
                let title = match (&result.pname, &result.version) {
                    (Some(name), Some(version)) => format!("{name} {version}"),
                    (Some(name), None) => name.clone(),
                    (None, _) => result.attr.clone(),
                };
                let installable = result
                    .installable
                    .clone()
                    .unwrap_or_else(|| result.attr.clone());
                let details = row(vec![
                    text(result.attr.clone()).into(),
                    button::text("Install")
                        .on_press_maybe(can_act.then(|| {
                            Message::StoreActionRequested(StoreAction::Install(installable))
                        }))
                        .into(),
                ])
                .spacing(8);
                section = section.add(settings::item(
                    title,
                    column(vec![
                        result.description.clone().map_or_else(
                            || text("no description").into(),
                            |description| text(description).into(),
                        ),
                        details.into(),
                    ])
                    .spacing(4),
                ));
            }
        }

        section.into()
    }

    fn installed_view(&self) -> Element<'_, Message> {
        let enabled = self.scope_ready();
        let mut section = settings::section().title(format!("Installed ({})", self.packages.len()));

        if self.busy && self.packages.is_empty() {
            section = section.add(settings::item("Status", text("Loading profile…")));
        }
        if self.packages.is_empty() && !self.busy {
            section = section.add(settings::item(
                "No packages",
                text("This profile has no installed packages."),
            ));
        }

        for package in &self.packages {
            let actions = row(vec![
                button::text("Upgrade")
                    .on_press_maybe(enabled.then(|| {
                        Message::StoreActionRequested(StoreAction::Upgrade(package.name.clone()))
                    }))
                    .into(),
                button::text("Remove")
                    .on_press_maybe(enabled.then(|| {
                        Message::StoreActionRequested(StoreAction::Remove(package.name.clone()))
                    }))
                    .into(),
            ])
            .spacing(8);
            section = section.add(settings::item(
                package.name.clone(),
                column(vec![package_detail(package), actions.into()]).spacing(4),
            ));
        }

        section = section.add(settings::item(
            "Profile actions",
            row(vec![
                button::text("Upgrade all")
                    .on_press_maybe(
                        enabled.then(|| Message::StoreActionRequested(StoreAction::UpgradeAll)),
                    )
                    .into(),
                button::text("Remove all")
                    .on_press_maybe(
                        enabled.then(|| Message::StoreActionRequested(StoreAction::RemoveAll)),
                    )
                    .into(),
            ])
            .spacing(8),
        ));

        section.into()
    }

    fn advanced_view(&self) -> Element<'_, Message> {
        let enabled = self.scope_ready();
        let ready = enabled && !self.regex.trim().is_empty();
        settings::section()
            .title("Pattern actions")
            .add(settings::item(
                "Package pattern",
                row(vec![
                    text_input("Regex, e.g. ^python", self.regex.clone())
                        .on_input(Message::StoreRegexChanged)
                        .into(),
                    button::text("Preview matches")
                        .on_press_maybe(ready.then(|| {
                            Message::StoreActionRequested(StoreAction::RemoveRegex {
                                pattern: self.regex.trim().to_string(),
                                dry_run: true,
                            })
                        }))
                        .into(),
                    button::text("Upgrade matching")
                        .on_press_maybe(ready.then(|| {
                            Message::StoreActionRequested(StoreAction::UpgradeRegex {
                                pattern: self.regex.trim().to_string(),
                                dry_run: false,
                            })
                        }))
                        .into(),
                    button::text("Remove matching")
                        .on_press_maybe(ready.then(|| {
                            Message::StoreActionRequested(StoreAction::RemoveRegex {
                                pattern: self.regex.trim().to_string(),
                                dry_run: false,
                            })
                        }))
                        .into(),
                ])
                .spacing(8),
            ))
            .into()
    }

    fn generations_view(&self) -> Element<'_, Message> {
        let enabled = self.scope_ready();
        let mut section =
            settings::section().title(format!("Generations ({})", self.generations.len()));

        section = section.add(settings::item(
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
        ));

        if self.generations.is_empty() {
            section = section.add(settings::item(
                "No history",
                text("No profile generations reported."),
            ));
        } else {
            for line in &self.generations {
                // The daemon returns real `Version N (...) …` lines. Do not
                // invent a second numbering that disagrees with rollback input.
                section = section.add(settings::item("Profile history", text(line.clone())));
            }
        }

        section.into()
    }

    fn maintenance_view(&self) -> Element<'_, Message> {
        let enabled = self.scope_ready();
        let global = self.can_run(&StoreAction::StoreGc);
        settings::section()
            .title("Maintenance")
            .add(settings::item(
                "Keep last generations",
                row(vec![
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
            ))
            .add(settings::item(
                "Wipe history older than",
                row(vec![
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
            ))
            .add(settings::item(
                "Store garbage collection (all users)",
                button::text("Run GC").on_press_maybe(
                    global.then(|| Message::StoreActionRequested(StoreAction::StoreGc)),
                ),
            ))
            .into()
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

#[cfg(test)]
mod tests {
    use super::*;

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
