//! Snapshots screen: btrbk config dropdowns, list/stats/create, restore and delete.

use cosmic::iced::Length;
use cosmic::widget::{
    button, column, dropdown, row, scrollable, settings, text, text_input, toggler,
};
use cosmic::Element;
use ursine_api::{BtrbkConfigView, BtrbkSettings, Snapshot, SnapshotStats};

use crate::app::Message;
use crate::client::SnapshotPage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapSetting {
    TimestampFormat,
    SnapshotCreate,
    Incremental,
    SnapshotPreserveMin,
    SnapshotPreserve,
    TargetPreserveMin,
    TargetPreserve,
    ArchivePreserveMin,
    ArchivePreserve,
    PreserveHourOfDay,
    PreserveDayOfWeek,
    StreamCompress,
    BtrfsCommitDelete,
}

const YES_NO: &[&str] = &["not set", "yes", "no"];
const PRESERVE_MIN: &[&str] = &["not set", "no", "6h", "12h", "1d", "2d", "3d", "1w", "2w"];
const PRESERVE: &[&str] = &[
    "not set",
    "no",
    "14d",
    "24h 14d",
    "3d 14d 8w 6m",
    "14d 8w 6m",
    "20d 10w *m",
];
const HOURS: &[&str] = &[
    "not set", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14",
    "15", "16", "17", "18", "19", "20", "21", "22", "23",
];
const DAYS: &[&str] = &[
    "not set",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

impl SnapSetting {
    pub const ALL: [Self; 13] = [
        Self::TimestampFormat,
        Self::SnapshotCreate,
        Self::Incremental,
        Self::SnapshotPreserveMin,
        Self::SnapshotPreserve,
        Self::TargetPreserveMin,
        Self::TargetPreserve,
        Self::ArchivePreserveMin,
        Self::ArchivePreserve,
        Self::PreserveHourOfDay,
        Self::PreserveDayOfWeek,
        Self::StreamCompress,
        Self::BtrfsCommitDelete,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::TimestampFormat => "timestamp_format",
            Self::SnapshotCreate => "snapshot_create",
            Self::Incremental => "incremental",
            Self::SnapshotPreserveMin => "snapshot_preserve_min",
            Self::SnapshotPreserve => "snapshot_preserve",
            Self::TargetPreserveMin => "target_preserve_min",
            Self::TargetPreserve => "target_preserve",
            Self::ArchivePreserveMin => "archive_preserve_min",
            Self::ArchivePreserve => "archive_preserve",
            Self::PreserveHourOfDay => "preserve_hour_of_day",
            Self::PreserveDayOfWeek => "preserve_day_of_week",
            Self::StreamCompress => "stream_compress",
            Self::BtrfsCommitDelete => "btrfs_commit_delete",
        }
    }

    pub fn options(self) -> &'static [&'static str] {
        match self {
            Self::TimestampFormat => &["not set", "long", "short"],
            Self::SnapshotCreate => &["not set", "always", "ondemand", "no"],
            Self::Incremental => &["not set", "yes", "no", "strict"],
            Self::SnapshotPreserveMin | Self::TargetPreserveMin | Self::ArchivePreserveMin => {
                PRESERVE_MIN
            }
            Self::SnapshotPreserve | Self::TargetPreserve | Self::ArchivePreserve => PRESERVE,
            Self::PreserveHourOfDay => HOURS,
            Self::PreserveDayOfWeek => DAYS,
            Self::StreamCompress | Self::BtrfsCommitDelete => YES_NO,
        }
    }

    fn value(self, settings: &BtrbkSettings) -> Option<&String> {
        match self {
            Self::TimestampFormat => settings.timestamp_format.as_ref(),
            Self::SnapshotCreate => settings.snapshot_create.as_ref(),
            Self::Incremental => settings.incremental.as_ref(),
            Self::SnapshotPreserveMin => settings.snapshot_preserve_min.as_ref(),
            Self::SnapshotPreserve => settings.snapshot_preserve.as_ref(),
            Self::TargetPreserveMin => settings.target_preserve_min.as_ref(),
            Self::TargetPreserve => settings.target_preserve.as_ref(),
            Self::ArchivePreserveMin => settings.archive_preserve_min.as_ref(),
            Self::ArchivePreserve => settings.archive_preserve.as_ref(),
            Self::PreserveHourOfDay => settings.preserve_hour_of_day.as_ref(),
            Self::PreserveDayOfWeek => settings.preserve_day_of_week.as_ref(),
            Self::StreamCompress => settings.stream_compress.as_ref(),
            Self::BtrfsCommitDelete => settings.btrfs_commit_delete.as_ref(),
        }
    }

    pub fn selected(self, settings: &BtrbkSettings) -> Option<usize> {
        let value = self.value(settings)?;
        self.options().iter().position(|option| *option == value)
    }

    pub fn apply(self, settings: &mut BtrbkSettings, index: usize) {
        let value = if index == 0 {
            None
        } else {
            self.options().get(index).map(|value| (*value).to_string())
        };
        let field = match self {
            Self::TimestampFormat => &mut settings.timestamp_format,
            Self::SnapshotCreate => &mut settings.snapshot_create,
            Self::Incremental => &mut settings.incremental,
            Self::SnapshotPreserveMin => &mut settings.snapshot_preserve_min,
            Self::SnapshotPreserve => &mut settings.snapshot_preserve,
            Self::TargetPreserveMin => &mut settings.target_preserve_min,
            Self::TargetPreserve => &mut settings.target_preserve,
            Self::ArchivePreserveMin => &mut settings.archive_preserve_min,
            Self::ArchivePreserve => &mut settings.archive_preserve,
            Self::PreserveHourOfDay => &mut settings.preserve_hour_of_day,
            Self::PreserveDayOfWeek => &mut settings.preserve_day_of_week,
            Self::StreamCompress => &mut settings.stream_compress,
            Self::BtrfsCommitDelete => &mut settings.btrfs_commit_delete,
        };
        *field = value;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapAction {
    Create(Option<String>),
    Restore {
        snapshot: String,
        target: String,
        dry_run: bool,
    },
    Delete(Vec<String>),
    InitConfig,
    SetConfig,
}

#[derive(Default)]
pub struct SnapshotsState {
    pub snapshots: Vec<Snapshot>,
    pub stats: Vec<SnapshotStats>,
    pub config: Option<BtrbkConfigView>,
    pub settings: BtrbkSettings,
    pub busy: bool,
    pub error: Option<String>,
    pub section: String,
    pub selected_snapshot: Option<usize>,
    pub restore_snapshot: String,
    pub restore_target: String,
    pub pending_restore: Option<(String, String)>,
    pub pending_delete: Option<Vec<String>>,
    pub remove_sources: Vec<String>,
    pub action_busy: bool,
    pub action_message: Option<String>,
}

impl SnapshotsState {
    pub fn apply(&mut self, page: SnapshotPage) {
        self.settings = page.config.settings.clone();
        self.snapshots = page.snapshots;
        self.stats = page.stats;
        self.config = Some(page.config);
        self.error = None;
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
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
                let mut rows: Vec<Element<'_, Message>> = Vec::new();
                for setting in SnapSetting::ALL {
                    let selected = setting.selected(&self.settings);
                    rows.push(
                        settings::item(
                            setting.label(),
                            dropdown(setting.options(), selected, move |index| {
                                Message::SnapshotSettingChanged(setting, index)
                            }),
                        )
                        .into(),
                    );
                }
                rows.push(
                    settings::item(
                        "Save managed options",
                        button::text("Save").on_press_maybe(
                            (!self.action_busy).then_some(Message::SaveBtrbkConfig),
                        ),
                    )
                    .into(),
                );
                content = content.add(column(rows).spacing(4));
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
            let mut rows: Vec<Element<'_, Message>> = Vec::new();
            for stat in &self.stats {
                let source = stat.source_subvolume.clone();
                rows.push(
                    settings::item(
                        format!("{} ({} snapshots)", stat.source_subvolume, stat.snapshots),
                        toggler(self.remove_sources.contains(&source)).on_toggle(move |_| {
                            Message::SnapshotRemoveSourceToggled(source.clone())
                        }),
                    )
                    .into(),
                );
            }
            rows.push(
                settings::item(
                    "Remove all for selected sources",
                    button::text("Remove all").on_press_maybe(
                        (!self.action_busy).then_some(Message::SnapshotRemoveAllRequested),
                    ),
                )
                .into(),
            );
            items.push(
                settings::section()
                    .title("Stats / bulk remove")
                    .add(column(rows).spacing(4))
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
            let options: Vec<String> = self
                .snapshots
                .iter()
                .map(|snapshot| snapshot.snapshot_subvolume.clone())
                .collect();
            items.push(
                settings::section()
                    .title(format!("Snapshots ({})", self.snapshots.len()))
                    .add(settings::item(
                        "Select",
                        dropdown(options, self.selected_snapshot, Message::SnapshotSelected),
                    ))
                    .add(settings::item(
                        "Remove selected",
                        button::text("Remove").on_press_maybe(
                            (!self.action_busy).then_some(Message::SnapshotRemoveSelected),
                        ),
                    ))
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
                    text_input("/home", self.restore_target.clone())
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
                        text(format!("Restore {snapshot} to {target}? An existing target is moved to a timestamped backup path first.")),
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

        if let Some(paths) = &self.pending_delete {
            items.push(
                settings::section()
                    .title("Confirm snapshot removal")
                    .add(settings::item(
                        "Consequences",
                        text(format!("Permanently delete {} snapshot(s)?", paths.len())),
                    ))
                    .add(settings::item("Paths", text(paths.join("\n"))))
                    .add(settings::item(
                        "Actions",
                        row(vec![
                            button::suggested("Confirm delete")
                                .on_press_maybe(
                                    (!self.action_busy).then_some(Message::SnapshotDeleteConfirmed),
                                )
                                .into(),
                            button::text("Cancel")
                                .on_press(Message::SnapshotDeleteCancelled)
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
    fn setting_dropdown_roundtrips_known_values() {
        let mut settings = BtrbkSettings::default();
        let setting = SnapSetting::SnapshotPreserve;
        let index = setting
            .options()
            .iter()
            .position(|option| *option == "24h 14d")
            .unwrap();
        setting.apply(&mut settings, index);
        assert_eq!(settings.snapshot_preserve.as_deref(), Some("24h 14d"));
        assert_eq!(setting.selected(&settings), Some(index));
        setting.apply(&mut settings, 0);
        assert_eq!(settings.snapshot_preserve, None);
        assert_eq!(setting.selected(&settings), None);
    }

    #[test]
    fn bulk_delete_uses_selected_sources() {
        let state = SnapshotsState {
            snapshots: vec![
                Snapshot {
                    source_subvolume: "/home".into(),
                    snapshot_subvolume: "/snap/home.1".into(),
                    snapshot_name: None,
                    status: Some("ok".into()),
                    source_url: None,
                },
                Snapshot {
                    source_subvolume: "/var".into(),
                    snapshot_subvolume: "/snap/var.1".into(),
                    snapshot_name: None,
                    status: Some("ok".into()),
                    source_url: None,
                },
            ],
            remove_sources: vec!["/home".into()],
            ..SnapshotsState::default()
        };
        let paths: Vec<String> = state
            .snapshots
            .iter()
            .filter(|snapshot| {
                state.remove_sources.is_empty()
                    || state.remove_sources.contains(&snapshot.source_subvolume)
            })
            .map(|snapshot| snapshot.snapshot_subvolume.clone())
            .collect();
        assert_eq!(paths, vec!["/snap/home.1".to_string()]);
    }
}
