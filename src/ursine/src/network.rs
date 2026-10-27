//! Network screen: `NetworkManager` devices and Wi-Fi.

use cosmic::iced::Length;
use cosmic::widget::{button, column, row, scrollable, settings, text, text_input, toggler};
use cosmic::Element;
use ursine_api::NetworkView;

use crate::app::Message;

#[derive(Default)]
pub struct NetworkState {
    pub view: Option<NetworkView>,
    pub busy: bool,
    pub action_busy: bool,
    pub error: Option<String>,
    pub message: Option<String>,
    pub ssid: String,
    pub password: String,
}

impl NetworkState {
    pub fn apply(&mut self, view: NetworkView) {
        self.view = Some(view);
        self.error = None;
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2("Network").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe(
                    (!self.busy && !self.action_busy).then_some(Message::RefreshNetwork),
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
            items.push(text(message.clone()).into());
        }
        if self.busy {
            items.push(text("Reading NetworkManager…").into());
        } else if self.action_busy {
            items.push(text("Applying network change…").into());
        }

        if let Some(view) = &self.view {
            items.push(
                settings::section()
                    .title("Wi-Fi")
                    .add(settings::item(
                        "Radio",
                        toggler(view.wifi_enabled).on_toggle(Message::WifiEnabled),
                    ))
                    .add(settings::item(
                        "Connect",
                        column(vec![
                            text_input("SSID", self.ssid.clone())
                                .on_input(Message::WifiSsidChanged)
                                .into(),
                            text_input("password", self.password.clone())
                                .password()
                                .on_input(Message::WifiPasswordChanged)
                                .into(),
                            button::text("Connect")
                                .on_press_maybe(
                                    (!self.action_busy && !self.ssid.trim().is_empty())
                                        .then_some(Message::WifiConnect),
                                )
                                .into(),
                        ])
                        .spacing(6),
                    ))
                    .into(),
            );

            let mut wifi = settings::section().title("Visible networks");
            if view.wifi.is_empty() {
                wifi = wifi.add(settings::item("Result", text("No Wi-Fi networks.")));
            }
            for network in &view.wifi {
                let ssid = network.ssid.clone();
                wifi = wifi.add(settings::item(
                    format!(
                        "{} · {}% · {}{}",
                        network.ssid,
                        network.signal,
                        network.security,
                        if network.active { " · active" } else { "" }
                    ),
                    button::text("Use").on_press(Message::WifiSsidChanged(ssid)),
                ));
            }
            items.push(wifi.into());

            let mut devices = settings::section().title("Devices");
            for device in &view.devices {
                let name = device.device.clone();
                devices = devices.add(settings::item(
                    format!(
                        "{} · {}{}{}{}",
                        device.device,
                        device.state,
                        device
                            .connection
                            .as_ref()
                            .map(|connection| format!(" · {connection}"))
                            .unwrap_or_default(),
                        device
                            .ip4
                            .first()
                            .map(|address| format!(" · {address}"))
                            .unwrap_or_default(),
                        device
                            .gateway
                            .as_ref()
                            .map(|gateway| format!(" · gw {gateway}"))
                            .unwrap_or_default()
                    ),
                    column(vec![
                        text(format!(
                            "IP: {}{}",
                            if device.ip4.is_empty() {
                                "none".to_string()
                            } else {
                                device.ip4.join(", ")
                            },
                            if device.dns.is_empty() {
                                String::new()
                            } else {
                                format!(" · DNS {}", device.dns.join(", "))
                            }
                        ))
                        .into(),
                        button::text(if device.state.contains("connected") {
                            "Disconnect"
                        } else {
                            "Connect"
                        })
                        .on_press(if device.state.contains("connected") {
                            Message::DisconnectNetwork(name)
                        } else {
                            Message::ConnectNetwork(name)
                        })
                        .into(),
                    ])
                    .spacing(4),
                ));
            }
            items.push(devices.into());
        }

        scrollable(column(items).spacing(16).padding(24)).into()
    }
}
