//! Disks screen: block devices and mount state.

use cosmic::iced::Length;
use cosmic::widget::{button, column, row, scrollable, settings, text};
use cosmic::Element;
use ursine_api::Disk;

use crate::app::Message;

#[derive(Default)]
pub struct DisksState {
    pub disks: Vec<Disk>,
    pub busy: bool,
    pub action_busy: bool,
    pub error: Option<String>,
    pub message: Option<String>,
}

impl DisksState {
    pub fn apply(&mut self, disks: Vec<Disk>) {
        self.disks = disks;
        self.error = None;
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
            items.push(text(error.clone()).into());
        }
        if let Some(message) = &self.message {
            items.push(text(message.clone()).into());
        }
        if self.busy {
            items.push(text("Reading block devices…").into());
        } else if self.action_busy {
            items.push(text("Mounting/unmounting…").into());
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
                            .on_press(Message::DiskAction(device, !mounted))
                            .into(),
                    ])
                    .spacing(4),
                ));
        }
        items.push(list.into());
        scrollable(column(items).spacing(16).padding(24)).into()
    }
}
