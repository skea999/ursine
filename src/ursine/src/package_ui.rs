//! The single package-manager view.
//!
//! apk, flatpak and nix render through [`view`] — section order, row
//! shapes and every wrapper are written here exactly once (user
//! decision: one GUI code path; per-manager differences are injected
//! DATA). A section a manager lacks stays in [`SECTIONS`] and is
//! hidden by its visibility flag, so moving a line moves it in every
//! manager and a future manager only fills data.

use cosmic::iced::Length;
use cosmic::widget::{
    button, column, row, scrollable, selectable_text, settings, text, text_input, toggler,
};
use cosmic::Element;

use crate::app::Message;

// ---------------------------------------------------------------------------
// Data contract
// ---------------------------------------------------------------------------

/// One settings section: title + rows. Rows are data, built by the
/// per-manager adapters — this module owns only the wrapper and order.
pub struct SectionVm<'a> {
    pub title: String,
    pub items: Vec<Element<'a, Message>>,
}

/// Header refresh button (label and arming differ per manager/state).
pub struct RefreshVm {
    pub label: String,
    pub message: Message,
    pub armed: bool,
}

/// A pending confirmation. ALL confirms render at the top through
/// [`confirm_block`] — same shape everywhere.
pub struct ConfirmVm {
    pub title: String,
    /// Context rows shown above the buttons, e.g. ("Consequences", …).
    pub rows: Vec<(String, String)>,
    pub confirm: Message,
    pub cancel: Message,
    pub armed: bool,
    pub destructive: bool,
}

/// Upgrade section data: the "Upgrade all" row FIRST (apk order),
/// then the three shared control rows.
pub struct UpgradeVm<'a> {
    pub all: Option<UpgradeAllVm>,
    /// Manager key for `[updates]` persistence (`apk`/`flatpak`/`nix`).
    pub manager: String,
    pub auto: bool,
    pub number: Element<'a, Message>,
    pub refresh: Message,
    pub refresh_armed: bool,
}

pub struct UpgradeAllVm {
    pub label: String,
    pub message: Message,
    pub armed: bool,
}

/// Search section data: controls only. Status lines live in the top
/// busy/status slots and results render in the bottom list — one rule
/// for every manager.
pub struct SearchVm<'a> {
    /// Extra row above Query (nix's repository dropdown).
    pub repository: Option<Element<'a, Message>>,
    pub query_placeholder: String,
    pub query: String,
    pub on_query_input: fn(String) -> Message,
    /// Enter-key submit (nix); `None` when the manager has none.
    pub query_submit: Option<Message>,
    pub query_label: String,
    pub search: Message,
    pub search_armed: bool,
    pub install_placeholder: String,
    pub install: String,
    pub on_install_input: fn(String) -> Message,
    /// Widgets between the install input and the Install button
    /// (e.g. flatpak's remote dropdown).
    pub install_extras: Vec<Element<'a, Message>>,
    /// Widgets after the Install button (e.g. apk's Preview APK).
    pub install_trailing: Vec<Element<'a, Message>>,
    pub install_message: Message,
    pub install_armed: bool,
}

/// Bottom list section (Installed or search results — the title says).
pub struct ListVm<'a> {
    pub title: String,
    /// Transient context rows (e.g. nix "Loading profile…").
    pub status: Vec<(String, String)>,
    /// Shown when there are no rows (label, text) — data decides when.
    pub empty_note: Option<(String, String)>,
    pub rows: Vec<RowVm<'a>>,
    /// Trailing control rows (e.g. nix "Profile actions").
    pub footer: Vec<Element<'a, Message>>,
}

/// One installed/search row. The renderer always composes it the same
/// way: title on the left; under it description → repository → notes.
pub struct RowVm<'a> {
    pub title: String,
    pub description: Option<String>,
    pub repository: Option<String>,
    pub notes: Vec<String>,
    pub actions: Vec<Element<'a, Message>>,
}

/// Everything the single view needs. Adapters fill only what applies.
pub struct PackageVm<'a> {
    pub title: String,
    pub refresh: RefreshVm,
    /// One busy line (data: which activity, which wording).
    pub busy: Option<String>,
    pub error: Option<String>,
    pub message: Option<&'a str>,
    /// Pending confirmations, stacked at the top in adapter order.
    pub confirms: Vec<ConfirmVm>,
    pub scope: SectionVm<'a>,
    pub maintenance: Option<SectionVm<'a>>,
    pub upgrade: UpgradeVm<'a>,
    pub search: SearchVm<'a>,
    pub repositories: SectionVm<'a>,
    pub pattern: PatternSection,
    pub list: ListVm<'a>,
    pub generations: Option<SectionVm<'a>>,
}

// ---------------------------------------------------------------------------
// The fixed section order — THE order for every package manager
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionId {
    Scope,
    Maintenance,
    Upgrade,
    Search,
    Repositories,
    Pattern,
    Installed,
    Generations,
}

/// Canonical section order (base = the apk view; extra slots taken
/// from flatpak/nix). Never reorder for one manager only.
pub const SECTIONS: [SectionId; 8] = [
    SectionId::Scope,
    SectionId::Maintenance,
    SectionId::Upgrade,
    SectionId::Search,
    SectionId::Repositories,
    SectionId::Pattern,
    SectionId::Installed,
    SectionId::Generations,
];

impl PackageVm<'_> {
    /// The visible sections for this manager, in canonical order.
    /// This IS what [`view`] renders — the alignment test asserts it.
    pub fn section_plan(&self) -> Vec<SectionId> {
        SECTIONS
            .iter()
            .copied()
            .filter(|id| match id {
                SectionId::Scope
                | SectionId::Upgrade
                | SectionId::Search
                | SectionId::Repositories
                | SectionId::Pattern
                | SectionId::Installed => true,
                SectionId::Maintenance => self.maintenance.is_some(),
                SectionId::Generations => self.generations.is_some(),
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// The one view
// ---------------------------------------------------------------------------

/// Render any package manager. Order and wrappers live here only.
pub fn view<'a>(vm: PackageVm<'a>) -> Element<'a, Message> {
    let plan = vm.section_plan();
    let PackageVm {
        title,
        refresh,
        busy,
        error,
        message,
        confirms,
        scope,
        maintenance,
        upgrade,
        search,
        repositories,
        pattern,
        list,
        generations,
    } = vm;

    let header = row(vec![
        text::title2(title).into(),
        text("").width(Length::Fill).into(),
        button::text(refresh.label)
            .on_press_maybe(refresh.armed.then_some(refresh.message))
            .into(),
    ])
    .spacing(12)
    .width(Length::Fill);

    let mut items: Vec<Element<'a, Message>> = vec![header.into()];
    items.extend(status_items(error.as_deref(), message));
    if let Some(busy) = busy {
        items.push(text(busy).into());
    }
    for confirm in confirms {
        items.push(confirm_block(confirm));
    }

    // The plan (canonical order minus hidden sections) drives what
    // renders; the match keeps one source of truth with the test.
    let mut scope = Some(scope);
    let mut maintenance = maintenance;
    let mut upgrade = Some(upgrade);
    let mut search = Some(search);
    let mut repositories = Some(repositories);
    let mut pattern = Some(pattern);
    let mut list = Some(list);
    let mut generations = generations;

    for id in plan {
        let section = match id {
            SectionId::Scope => scope.take().map(section_block),
            SectionId::Maintenance => maintenance.take().map(section_block),
            SectionId::Upgrade => upgrade.take().map(upgrade_section),
            SectionId::Search => search.take().map(search_section),
            SectionId::Repositories => repositories.take().map(section_block),
            SectionId::Pattern => pattern.take().map(pattern_section),
            SectionId::Installed => list.take().map(list_section),
            SectionId::Generations => generations.take().map(section_block),
        };
        if let Some(section) = section {
            items.push(section);
        }
    }

    scrollable(column(items).spacing(16).padding(24)).into()
}

// ---------------------------------------------------------------------------
// Shared blocks
// ---------------------------------------------------------------------------

/// Top-of-view status shared by the managers: a copyable error, then
/// the last-action result.
fn status_items(error: Option<&str>, message: Option<&str>) -> Vec<Element<'static, Message>> {
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

/// One pending confirmation: context rows + Confirm/Cancel. Every
/// manager's confirm renders here — titles and rows are data.
fn confirm_block(vm: ConfirmVm) -> Element<'static, Message> {
    let mut section = settings::section().title(vm.title);
    for (label, value) in vm.rows {
        // Values stay copyable (errors and questions get pasted back).
        section = section.add(settings::item(label, selectable_text(value)));
    }
    let confirm_button: Element<'static, Message> = if vm.destructive {
        button::destructive("Confirm")
            .on_press_maybe(vm.armed.then_some(vm.confirm))
            .into()
    } else {
        button::suggested("Confirm")
            .on_press_maybe(vm.armed.then_some(vm.confirm))
            .into()
    };
    section
        .add(settings::item(
            "Actions",
            row(vec![
                confirm_button,
                button::text("Cancel").on_press(vm.cancel).into(),
            ])
            .spacing(8),
        ))
        .into()
}

/// Inputs of the shared Pattern-actions section (nix schema).
pub struct PatternSection {
    /// Hint shown in the "Package pattern" row.
    pub hint: String,
    pub regex: String,
    pub on_input: fn(String) -> Message,
    pub preview: Message,
    pub upgrade: Message,
    pub remove: Message,
    /// Buttons fire only while armed (pattern present, nothing busy).
    pub armed: bool,
}

/// The Pattern-actions block every manager shares: hint, regex input,
/// then Preview/Upgrade/Remove in one row.
fn pattern_section(spec: PatternSection) -> Element<'static, Message> {
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

/// The three Upgrade-section control rows every manager shares:
/// persisted auto toggler, the prominent count, the manual refresh.
fn update_control_items(
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

// ---------------------------------------------------------------------------
// Internal rendering helpers
// ---------------------------------------------------------------------------

fn section_block(section: SectionVm<'_>) -> Element<'_, Message> {
    let mut built = settings::section().title(section.title);
    for item in section.items {
        built = built.add(item);
    }
    built.into()
}

/// One row of any list: identical detail composition everywhere
/// (description → repository → notes), actions on the right.
/// Title + description stacked at one left edge, action on the right.
fn package_row<'a>(
    title: String,
    description: Element<'a, Message>,
    action: Element<'a, Message>,
) -> Element<'a, Message> {
    row(vec![
        column(vec![selectable_text(title).into(), description])
            .spacing(2)
            .width(Length::Fill)
            .into(),
        action,
    ])
    .spacing(12)
    .into()
}

fn row_block(vm: RowVm<'_>) -> Element<'_, Message> {
    let mut lines: Vec<Element<'_, Message>> = Vec::new();
    for line in [vm.description, vm.repository].into_iter().flatten() {
        if !line.is_empty() {
            lines.push(selectable_text(line).into());
        }
    }
    for note in vm.notes {
        if !note.is_empty() {
            lines.push(selectable_text(note).into());
        }
    }
    if lines.is_empty() {
        lines.push(text("").into());
    }
    package_row(
        vm.title,
        column(lines).spacing(2).into(),
        row(vm.actions).spacing(8).into(),
    )
}

fn list_section(list: ListVm<'_>) -> Element<'_, Message> {
    let mut section = settings::section().title(list.title);
    for (label, value) in list.status {
        section = section.add(settings::item(label, text(value)));
    }
    if let Some((label, value)) = list.empty_note {
        section = section.add(settings::item(label, text(value)));
    }
    for row_vm in list.rows {
        section = section.add(row_block(row_vm));
    }
    for footer in list.footer {
        section = section.add(footer);
    }
    section.into()
}

/// Upgrade section: "Upgrade all" row first, then the controls.
fn upgrade_section(vm: UpgradeVm<'_>) -> Element<'_, Message> {
    let UpgradeVm {
        all,
        manager,
        auto,
        number,
        refresh,
        refresh_armed,
    } = vm;
    let mut section = settings::section().title("Upgrade");
    if let Some(all) = all {
        section = section.add(settings::item(
            "Upgrade every installed item",
            button::suggested(all.label).on_press_maybe(all.armed.then_some(all.message)),
        ));
    }
    for item in update_control_items(
        auto,
        move |value| Message::UpdateAutoChanged(manager.clone(), value),
        number,
        refresh,
        refresh_armed,
    ) {
        section = section.add(item);
    }
    section.into()
}

fn search_section(vm: SearchVm<'_>) -> Element<'_, Message> {
    let SearchVm {
        repository,
        query_placeholder,
        query,
        on_query_input,
        query_submit,
        query_label,
        search,
        search_armed,
        install_placeholder,
        install,
        on_install_input,
        install_extras,
        install_trailing,
        install_message,
        install_armed,
    } = vm;

    let mut section = settings::section().title("Search");
    if let Some(repository) = repository {
        section = section.add(repository);
    }

    let mut input = text_input(query_placeholder, query).on_input(on_query_input);
    if let Some(submit) = query_submit {
        input = input.on_submit(move |_| submit.clone());
    }
    section = section.add(settings::item(
        "Query",
        row(vec![
            input.into(),
            button::text(query_label)
                .on_press_maybe(search_armed.then_some(search))
                .into(),
        ])
        .spacing(8),
    ));

    let mut install_row: Vec<Element<'_, Message>> = vec![text_input(install_placeholder, install)
        .on_input(on_install_input)
        .into()];
    install_row.extend(install_extras);
    install_row.push(
        button::text("Install")
            .on_press_maybe(install_armed.then_some(install_message))
            .into(),
    );
    install_row.extend(install_trailing);
    section = section.add(settings::item(
        "Direct install",
        row(install_row).spacing(8),
    ));
    section.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packages::PatternOp;

    /// Minimal data for one manager; visibility flags are the only
    /// per-kind difference.
    fn minimal_vm(kind: &'static str) -> PackageVm<'static> {
        PackageVm {
            title: "t".to_string(),
            refresh: RefreshVm {
                label: "Refresh".to_string(),
                message: Message::RefreshStore,
                armed: true,
            },
            busy: None,
            error: None,
            message: None,
            confirms: Vec::new(),
            scope: SectionVm {
                title: "Scope".to_string(),
                items: Vec::new(),
            },
            maintenance: (kind != "apk").then(|| SectionVm {
                title: "Maintenance".to_string(),
                items: Vec::new(),
            }),
            upgrade: UpgradeVm {
                all: None,
                manager: kind.to_string(),
                auto: false,
                number: text("—").into(),
                refresh: Message::NixUpdatesRequested,
                refresh_armed: true,
            },
            search: SearchVm {
                repository: None,
                query_placeholder: String::new(),
                query: String::new(),
                on_query_input: Message::PackageQueryChanged,
                query_submit: None,
                query_label: "Search".to_string(),
                search: Message::PackageSearchRequested,
                search_armed: true,
                install_placeholder: String::new(),
                install: String::new(),
                on_install_input: Message::PackageInstallChanged,
                install_extras: Vec::new(),
                install_trailing: Vec::new(),
                install_message: Message::PackageInstallRequested,
                install_armed: true,
            },
            repositories: SectionVm {
                title: "Repositories".to_string(),
                items: Vec::new(),
            },
            pattern: PatternSection {
                hint: String::new(),
                regex: String::new(),
                on_input: Message::PackagePatternChanged,
                preview: Message::PackagePatternPreviewRequested,
                upgrade: Message::PackagePatternApplyRequested(PatternOp::Upgrade),
                remove: Message::PackagePatternApplyRequested(PatternOp::Remove),
                armed: false,
            },
            list: ListVm {
                title: "Installed (0)".to_string(),
                status: Vec::new(),
                empty_note: None,
                rows: Vec::new(),
                footer: Vec::new(),
            },
            generations: (kind == "nix").then(|| SectionVm {
                title: "Generations".to_string(),
                items: Vec::new(),
            }),
        }
    }

    /// THE alignment guarantee: every manager renders the canonical
    /// order; a manager may only HIDE sections, never reorder them.
    /// Moving a line for one manager without the others fails here.
    #[test]
    fn every_manager_shares_the_same_section_order() {
        let apk = minimal_vm("apk");
        let flatpak = minimal_vm("flatpak");
        let nix = minimal_vm("nix");

        assert_eq!(
            apk.section_plan(),
            vec![
                SectionId::Scope,
                SectionId::Upgrade,
                SectionId::Search,
                SectionId::Repositories,
                SectionId::Pattern,
                SectionId::Installed,
            ]
        );
        assert_eq!(
            flatpak.section_plan(),
            vec![
                SectionId::Scope,
                SectionId::Maintenance,
                SectionId::Upgrade,
                SectionId::Search,
                SectionId::Repositories,
                SectionId::Pattern,
                SectionId::Installed,
            ]
        );
        assert_eq!(
            nix.section_plan(),
            vec![
                SectionId::Scope,
                SectionId::Maintenance,
                SectionId::Upgrade,
                SectionId::Search,
                SectionId::Repositories,
                SectionId::Pattern,
                SectionId::Installed,
                SectionId::Generations,
            ]
        );

        // Every plan is a subsequence of the canonical order (visibility
        // may remove sections, never reorder or inject them).
        for plan in [
            apk.section_plan(),
            flatpak.section_plan(),
            nix.section_plan(),
        ] {
            let mut canonical = SECTIONS.iter();
            for id in &plan {
                assert!(
                    canonical.any(|entry| entry == id),
                    "section {id:?} broke the canonical order"
                );
            }
        }
    }
}
