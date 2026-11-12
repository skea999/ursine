//! Disks screen: block devices and mount state.

use cosmic::iced::Length;
use cosmic::widget::selectable_text;
use cosmic::widget::{button, column, dropdown, row, scrollable, settings, text, text_input};
use cosmic::Element;
use ursine_api::{Disk, DiskView, DISK_FSTYPES};

use crate::app::Message;

#[derive(Default)]
pub struct DisksState {
    pub disks: Vec<Disk>,
    pub busy: bool,
    pub action_busy: bool,
    pub error: Option<String>,
    pub message: Option<String>,
    /// `disks_format = true` in the daemon config.
    pub format_enabled: bool,
    /// Active destructive flow (at most one at a time).
    pub format_target: Option<String>,
    pub create_target: Option<String>,
    pub delete_target: Option<(String, u32)>,
    pub fstype_index: usize,
    pub confirm_device: String,
    pub create_start: String,
    pub create_end: String,
}

impl DisksState {
    pub fn apply(&mut self, view: DiskView) {
        self.disks = view.disks;
        self.format_enabled = view.format_enabled;
        self.error = None;
    }

    /// Close any open destructive flow.
    pub fn cancel_flow(&mut self) {
        self.format_target = None;
        self.create_target = None;
        self.delete_target = None;
        self.confirm_device.clear();
        self.create_start.clear();
        self.create_end.clear();
    }

    /// Trailing partition number from an lsblk name (`sda1` → 1).
    pub fn partition_number(name: &str) -> Option<u32> {
        let digits: String = name
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect();
        let digits: String = digits.chars().rev().collect();
        if digits.is_empty() || digits.len() > 3 {
            return None;
        }
        digits.parse().ok()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2("Disks").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe((!self.busy && !self.action_busy).then_some(Message::RefreshDisks))
                .into(),
        ])
        .spacing(12)
        .width(Length::Fill)
        .into()];

        if let Some(error) = &self.error {
            items.push(selectable_text(error.clone()).into());
        }
        if let Some(message) = &self.message {
            items.push(text(message.clone()).into());
        }
        if self.busy {
            items.push(text("Reading block devices…").into());
        } else if self.action_busy {
            items.push(text("Disk operation running…").into());
        }
        if !self.format_enabled && !self.disks.is_empty() {
            items.push(
                text(
                    "Destructive disk operations are disabled. Set disks_format = true in /etc/ursine/config.toml and restart ursined to enable format/partition.",
                )
                .into(),
            );
        }

        let mut list = settings::section().title(format!("Block devices ({})", self.disks.len()));
        if self.disks.is_empty() {
            list = list.add(settings::item("Result", text("No block devices reported.")));
        }
        for disk in &self.disks {
            let device = disk.path.clone();
            let mounted = disk.mountpoint.is_some();
            list =
                list.add(settings::item(
                    format!(
                        "{} ({}) {}{}{}",
                        disk.name,
                        disk.kind,
                        disk.size,
                        disk.filesystem
                            .as_ref()
                            .map(|fs| format!(" · {fs}"))
                            .unwrap_or_default(),
                        disk.mountpoint
                            .as_ref()
                            .map(|mount| format!(" · {mount}"))
                            .unwrap_or_default()
                            + &disk
                                .use_percent
                                .as_ref()
                                .map(|usage| format!(" · {usage} used"))
                                .unwrap_or_default()
                    ),
                    column(vec![
                        text(disk.uuid.as_ref().map_or_else(
                            || "UUID: none".to_string(),
                            |uuid| format!("UUID: {uuid}"),
                        ))
                        .into(),
                        text(disk.smart.as_ref().map_or_else(
                            || "SMART: unknown".to_string(),
                            |smart| format!("SMART: {smart}"),
                        ))
                        .into(),
                        button::text(if mounted { "Unmount" } else { "Mount" })
                            .on_press_maybe(
                                (self.format_target.is_none()
                                    && self.create_target.is_none()
                                    && self.delete_target.is_none()
                                    && !self.action_busy
                                    && !self.busy)
                                    .then_some(Message::DiskAction(device.clone(), !mounted)),
                            )
                            .into(),
                        if self.format_enabled && disk.kind == "part" {
                            button::text("Format…")
                                .on_press_maybe(
                                    (!self.action_busy)
                                        .then_some(Message::DiskFormatRequested(device.clone())),
                                )
                                .into()
                        } else {
                            text("").into()
                        },
                        if self.format_enabled && disk.kind == "disk" {
                            button::text("Partition…")
                                .on_press_maybe(
                                    (!self.action_busy)
                                        .then_some(Message::DiskCreateRequested(device.clone())),
                                )
                                .into()
                        } else {
                            text("").into()
                        },
                        if self.format_enabled && disk.kind == "part" {
                            Self::partition_number(&disk.name).map_or_else(
                                || text("").into(),
                                |number| {
                                    button::text("Delete…")
                                        .on_press_maybe((!self.action_busy).then_some(
                                            Message::DiskDeleteRequested(device.clone(), number),
                                        ))
                                        .into()
                                },
                            )
                        } else {
                            text("").into()
                        },
                    ])
                    .spacing(4),
                ));
        }
        items.push(list.into());

        // --- destructive flows (one at a time) ---
        if let Some(target) = &self.format_target {
            let section = settings::section()
                .title(format!("Format {target}"))
                .add(settings::item(
                    "Filesystem",
                    dropdown(
                        DISK_FSTYPES.to_vec(),
                        Some(self.fstype_index),
                        Message::DiskFormatFstypeChanged,
                    ),
                ))
                .add(settings::item(
                    "Type the device to confirm",
                    text_input(target, self.confirm_device.clone())
                        .on_input(Message::DiskConfirmChanged),
                ))
                .add(settings::item(
                    "Actions",
                    row(vec![
                        button::destructive("Format (wipes all data)")
                            .on_press_maybe(
                                (!self.action_busy).then_some(Message::DiskFormatConfirmed),
                            )
                            .into(),
                        button::text("Cancel")
                            .on_press_maybe(
                                (!self.action_busy).then_some(Message::DiskFlowCancelled),
                            )
                            .into(),
                    ])
                    .spacing(8),
                ));
            items.push(section.into());
        }
        if let Some(target) = &self.create_target {
            let section = settings::section()
                .title(format!("New partition on {target}"))
                .add(settings::item(
                    "Start (MiB)",
                    text_input("1024", self.create_start.clone())
                        .on_input(Message::DiskCreateStartChanged),
                ))
                .add(settings::item(
                    "End (MiB)",
                    text_input("2048", self.create_end.clone())
                        .on_input(Message::DiskCreateEndChanged),
                ))
                .add(settings::item(
                    "Type the device to confirm",
                    text_input(target, self.confirm_device.clone())
                        .on_input(Message::DiskConfirmChanged),
                ))
                .add(settings::item(
                    "Actions",
                    row(vec![
                        button::destructive("Create partition")
                            .on_press_maybe(
                                (!self.action_busy).then_some(Message::DiskCreateConfirmed),
                            )
                            .into(),
                        button::text("Cancel")
                            .on_press_maybe(
                                (!self.action_busy).then_some(Message::DiskFlowCancelled),
                            )
                            .into(),
                    ])
                    .spacing(8),
                ));
            items.push(section.into());
        }
        if let Some((target, number)) = &self.delete_target {
            let section = settings::section()
                .title(format!("Delete partition {number} on {target}"))
                .add(settings::item(
                    "Consequences",
                    text("The partition and its data are removed from the table."),
                ))
                .add(settings::item(
                    "Type the device to confirm",
                    text_input(target, self.confirm_device.clone())
                        .on_input(Message::DiskConfirmChanged),
                ))
                .add(settings::item(
                    "Actions",
                    row(vec![
                        button::destructive("Delete partition")
                            .on_press_maybe(
                                (!self.action_busy).then_some(Message::DiskDeleteConfirmed),
                            )
                            .into(),
                        button::text("Cancel")
                            .on_press_maybe(
                                (!self.action_busy).then_some(Message::DiskFlowCancelled),
                            )
                            .into(),
                    ])
                    .spacing(8),
                ));
            items.push(section.into());
        }
        scrollable(column(items).spacing(16).padding(24)).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_numbers_parse_from_lsblk_names() {
        assert_eq!(DisksState::partition_number("sda1"), Some(1));
        assert_eq!(DisksState::partition_number("nvme0n1p2"), Some(2));
        assert_eq!(DisksState::partition_number("sda"), None);
        assert_eq!(DisksState::partition_number(""), None);
    }

    #[test]
    fn flows_cancel_clears_every_field() {
        let mut state = DisksState {
            format_target: Some("/dev/sda1".into()),
            confirm_device: "/dev/sda1".into(),
            create_start: "1".into(),
            ..DisksState::default()
        };
        state.cancel_flow();
        assert!(state.format_target.is_none());
        assert!(state.confirm_device.is_empty());
        assert!(state.create_start.is_empty());
    }
}
