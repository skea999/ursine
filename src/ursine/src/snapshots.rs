//! Snapshots screen: btrbk list/stats/create, config init, and restore preview.

use cosmic::iced::Length;
use cosmic::widget::{button, column, row, scrollable, settings, text, text_input};
use cosmic::Element;
use ursine_api::{BtrbkConfigView, Snapshot, SnapshotStats};

use crate::app::Message;
use crate::client::SnapshotPage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapAction {
    Create(Option<String>),
    Restore {
        snapshot: String,
        target: String,
        dry_run: bool,
    },
    InitConfig,
}

#[derive(Default)]
pub struct SnapshotsState {
    pub snapshots: Vec<Snapshot>,
    pub stats: Vec<SnapshotStats>,
    pub config: Option<BtrbkConfigView>,
    pub busy: bool,
    pub error: Option<String>,
    pub section: String,
    pub restore_snapshot: String,
    pub restore_target: String,
    pub pending_restore: Option<(String, String)>,
    pub action_busy: bool,
    pub action_message: Option<String>,
}

impl SnapshotsState {
    pub fn apply(&mut self, page: SnapshotPage) {
        self.snapshots = page.snapshots;
        self.stats = page.stats;
        self.config = Some(page.config);
        self.error = None;
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            button::text("Home").on_press(Message::BackHome).into(),
            text::title2("Snapshots (btrbk)").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe(
                    (!self.busy && !self.action_busy).then_some(Message::RefreshSnapshots),
                )
                .into(),
        ])
        .spacing(12)
        .width(Length::Fill)
        .into()];

        if let Some(error) = &self.error {
            items.push(text(error.clone()).into());
        }
        if self.busy {
            items.push(text("Reading btrbk state…").into());
        }
        if self.action_busy {
            items.push(text("Running btrbk action…").into());
        }
        if let Some(message) = &self.action_message {
            items.push(
                settings::section()
                    .title("Last action")
                    .add(settings::item("Result", text(message.clone())))
                    .into(),
            );
        }

        if let Some(config) = &self.config {
            let mut content = settings::section().title("btrbk configuration");
            content = content.add(settings::item("Path", text(config.path.clone())));
            if config.exists {
                content = content.add(settings::item(
                    "Content",
                    scrollable(text(config.content.clone())).height(Length::Fixed(160.0)),
                ));
            } else {
                content = content.add(settings::item(
                    "Missing",
                    button::text("Create default")
                        .on_press_maybe((!self.action_busy).then_some(Message::InitBtrbkConfig)),
                ));
            }
            items.push(content.into());
        }

        items.push(
            settings::section()
                .title("Create snapshot")
                .add(settings::item(
                    "Section (optional)",
                    row(vec![
                        text_input("all sections", self.section.clone())
                            .on_input(Message::SnapshotSectionChanged)
                            .into(),
                        button::text("Snapshot")
                            .on_press_maybe(
                                (!self.action_busy).then_some(Message::SnapshotCreateRequested),
                            )
                            .into(),
                    ])
                    .spacing(8),
                ))
                .into(),
        );

        if !self.stats.is_empty() {
            let lines = self
                .stats
                .iter()
                .map(|stat| {
                    text(format!(
                        "{}: {} snapshots, {} backups, {} orphaned",
                        stat.source_subvolume, stat.snapshots, stat.backups, stat.orphaned
                    ))
                    .into()
                })
                .collect::<Vec<Element<'_, Message>>>();
            items.push(
                settings::section()
                    .title("Stats")
                    .add(column(lines).spacing(2))
                    .into(),
            );
        }

        if self.snapshots.is_empty() {
            if !self.busy {
                items.push(
                    settings::section()
                        .title("Snapshots")
                        .add(settings::item("Result", text("No snapshots reported.")))
                        .into(),
                );
            }
        } else {
            let rows = self
                .snapshots
                .iter()
                .map(|snapshot| {
                    text(format!(
                        "{} → {} [{}]",
                        snapshot.source_subvolume,
                        snapshot.snapshot_subvolume,
                        snapshot.status.as_deref().unwrap_or("unknown")
                    ))
                    .into()
                })
                .collect::<Vec<Element<'_, Message>>>();
            items.push(
                settings::section()
                    .title(format!("Snapshots ({})", self.snapshots.len()))
                    .add(column(rows).spacing(2))
                    .into(),
            );
        }

        items.push(
            settings::section()
                .title("Restore")
                .add(settings::item(
                    "Snapshot path",
                    text_input(
                        "/.snapshots/home.20261022T1200",
                        self.restore_snapshot.clone(),
                    )
                    .on_input(Message::SnapshotRestoreSnapshotChanged),
                ))
                .add(settings::item(
                    "Target path",
                    text_input("/home/restored", self.restore_target.clone())
                        .on_input(Message::SnapshotRestoreTargetChanged),
                ))
                .add(settings::item(
                    "Preview",
                    button::text("Preview restore").on_press_maybe(
                        (!self.action_busy).then_some(Message::SnapshotRestorePreview),
                    ),
                ))
                .into(),
        );

        if let Some((snapshot, target)) = &self.pending_restore {
            items.push(
                settings::section()
                    .title("Confirm restore")
                    .add(settings::item(
                        "Consequences",
                        text(format!("Create a writable snapshot of {snapshot} at {target}? Existing target is never overwritten.")),
                    ))
                    .add(settings::item(
                        "Actions",
                        row(vec![
                            button::suggested("Confirm restore")
                                .on_press_maybe((!self.action_busy).then_some(Message::SnapshotRestoreConfirmed))
                                .into(),
                            button::text("Cancel")
                                .on_press(Message::SnapshotRestoreCancelled)
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
