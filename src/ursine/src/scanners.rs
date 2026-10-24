//! Scanners screen: SANE discovery and manual airscan setup.

use cosmic::iced::Length;
use cosmic::widget::{
    button, column, dropdown, row, scrollable, settings, text, text_input, toggler,
};
use cosmic::Element;
use ursine_api::ScannerConfigView;

use crate::app::Message;

#[derive(Default)]
pub struct ScannersState {
    pub view: Option<ScannerConfigView>,
    pub busy: bool,
    pub action_busy: bool,
    pub error: Option<String>,
    pub message: Option<String>,
    pub name: String,
    pub address: String,
    pub protocol: usize,
    pub pending_remove: Option<String>,
}

impl ScannersState {
    pub fn apply(&mut self, view: ScannerConfigView) {
        self.view = Some(view);
        self.error = None;
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2("Scanners").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe(
                    (!self.busy && !self.action_busy).then_some(Message::RefreshScanners),
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
            items.push(text("Saved.").into());
            let _ = message;
        }
        if self.busy {
            items.push(text("Discovering scanners…").into());
        } else if self.action_busy {
            items.push(text("Writing airscan config…").into());
        }

        if let Some(view) = &self.view {
            items.push(
                settings::section()
                    .title("airscan configuration")
                    .add(settings::item("Path", text(view.path.clone())))
                    .add(settings::item(
                        "Discovery",
                        toggler(!view.discovery_disabled)
                            .on_toggle(Message::ScannerDiscoveryToggled),
                    ))
                    .add(settings::item(
                        "Detection",
                        if view.detected.is_empty() {
                            Element::from(text("No scanners detected by scanimage -L."))
                        } else {
                            Element::from(text(format!("{} detected", view.detected.len())))
                        },
                    ))
                    .into(),
            );

            let mut detected = settings::section().title("Detected devices");
            if view.detected.is_empty() {
                detected = detected.add(settings::item("Result", text("None.")));
            }
            for scanner in &view.detected {
                detected = detected.add(settings::item(
                    scanner.id.clone(),
                    text(scanner.description.clone()),
                ));
            }
            items.push(detected.into());

            let mut manual = settings::section().title("Manual airscan devices");
            if view.devices.is_empty() {
                manual = manual.add(settings::item("Result", text("None configured.")));
            }
            for device in &view.devices {
                let name = device.name.clone();
                manual = manual.add(settings::item(
                    format!("{} — {} ({})", device.name, device.address, device.protocol),
                    button::destructive("Remove…").on_press(Message::ScannerRemoveRequested(name)),
                ));
            }
            items.push(manual.into());

            items.push(
                settings::section()
                    .title("Add or update manual device")
                    .add(settings::item(
                        "Name",
                        text_input("Brother MFC-1910W", self.name.clone())
                            .on_input(Message::ScannerNameChanged),
                    ))
                    .add(settings::item(
                        "Address",
                        text_input(
                            "http://192.168.1.2:80/WebServices/ScannerService",
                            self.address.clone(),
                        )
                        .on_input(Message::ScannerAddressChanged),
                    ))
                    .add(settings::item(
                        "Protocol",
                        dropdown(
                            vec!["WSD", "eSCL"],
                            Some(self.protocol.min(1)),
                            Message::ScannerProtocolChanged,
                        ),
                    ))
                    .add(settings::item(
                        "Save",
                        button::suggested("Save device").on_press_maybe(
                            (!self.action_busy).then_some(Message::ScannerSaveRequested),
                        ),
                    ))
                    .into(),
            );
        }

        if let Some(name) = &self.pending_remove {
            items.push(
                settings::section()
                    .title("Confirm scanner removal")
                    .add(settings::item(
                        "Consequences",
                        text(format!("Remove manual airscan device {name}?")),
                    ))
                    .add(settings::item(
                        "Actions",
                        row(vec![
                            button::destructive("Remove")
                                .on_press_maybe(
                                    (!self.action_busy).then_some(Message::ScannerRemoveConfirmed),
                                )
                                .into(),
                            button::text("Cancel")
                                .on_press(Message::ScannerRemoveCancelled)
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
