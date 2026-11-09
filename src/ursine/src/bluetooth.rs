//! Bluetooth screen: adapter power, scan and device actions.

use cosmic::iced::Length;
use cosmic::widget::{button, column, row, scrollable, settings, text, toggler};
use cosmic::Element;
use ursine_api::{BluetoothOp, BluetoothView};

use crate::app::Message;

#[derive(Default)]
pub struct BluetoothState {
    pub view: Option<BluetoothView>,
    pub busy: bool,
    pub action_busy: bool,
    pub error: Option<String>,
    pub message: Option<String>,
}

impl BluetoothState {
    pub fn apply(&mut self, view: BluetoothView) {
        self.view = Some(view);
        self.error = None;
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2("Bluetooth").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe(
                    (!self.busy && !self.action_busy).then_some(Message::RefreshBluetooth),
                )
                .into(),
            button::text("Scan")
                .on_press_maybe((!self.busy && !self.action_busy).then_some(Message::BluetoothScan))
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
            items.push(text("Reading BlueZ…").into());
        } else if self.action_busy {
            items.push(text("Running Bluetooth action…").into());
        }

        if let Some(view) = &self.view {
            items.push(
                settings::section()
                    .title("Adapter")
                    .add(settings::item(
                        "Address",
                        text(if view.adapter.address.is_empty() {
                            "no adapter".to_string()
                        } else {
                            view.adapter.address.clone()
                        }),
                    ))
                    .add(settings::item("Name", text(view.adapter.name.clone())))
                    .add(settings::item(
                        "Powered",
                        toggler(view.adapter.powered).on_toggle(Message::BluetoothPower),
                    ))
                    .into(),
            );

            let mut devices = settings::section().title("Known devices");
            if view.devices.is_empty() {
                devices = devices.add(settings::item("Result", text("No devices.")));
            }
            for device in &view.devices {
                let address = device.address.clone();
                devices = devices.add(settings::item(
                    format!(
                        "{} ({}){}{}{}",
                        device.name,
                        device.address,
                        if device.connected {
                            " · connected"
                        } else {
                            ""
                        },
                        if device.paired { " · paired" } else { "" },
                        device
                            .battery
                            .map(|battery| format!(" · {battery}%"))
                            .unwrap_or_default()
                    ),
                    column(vec![row(vec![
                        button::text("Pair")
                            .on_press(Message::BluetoothAction(address.clone(), BluetoothOp::Pair))
                            .into(),
                        button::text(if device.connected {
                            "Disconnect"
                        } else {
                            "Connect"
                        })
                        .on_press(Message::BluetoothAction(
                            address.clone(),
                            if device.connected {
                                BluetoothOp::Disconnect
                            } else {
                                BluetoothOp::Connect
                            },
                        ))
                        .into(),
                        button::text(if device.trusted { "Untrust" } else { "Trust" })
                            .on_press(Message::BluetoothAction(
                                address.clone(),
                                if device.trusted {
                                    BluetoothOp::Untrust
                                } else {
                                    BluetoothOp::Trust
                                },
                            ))
                            .into(),
                        button::destructive("Forget")
                            .on_press(Message::BluetoothAction(
                                address.clone(),
                                BluetoothOp::Forget,
                            ))
                            .into(),
                    ])
                    .spacing(8)
                    .into()])
                    .spacing(4),
                ));
            }
            items.push(devices.into());
        }

        scrollable(column(items).spacing(16).padding(24)).into()
    }
}
