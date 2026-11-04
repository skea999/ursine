//! Network screen: `NetworkManager` devices and Wi-Fi.

use cosmic::iced::Length;
use cosmic::widget::{
    button, column, dropdown, row, scrollable, settings, text, text_input, toggler,
};
use cosmic::Element;
use ursine_api::NetworkView;

use crate::app::Message;

/// Indices of [`Message::NetworkIpv4Field`].
pub const IPV4_FIELD_ADDRESSES: u8 = 0;
pub const IPV4_FIELD_GATEWAY: u8 = 1;
pub const IPV4_FIELD_DNS: u8 = 2;

/// Open IPv4 editor for one connection profile.
pub struct Ipv4Form {
    pub connection: String,
    /// false = DHCP, true = static.
    pub static_ip: bool,
    pub addresses: String,
    pub gateway: String,
    pub dns: String,
    /// Apply confirmation (an active connection reconnects).
    pub confirm: bool,
}

#[derive(Default)]
pub struct NetworkState {
    pub view: Option<NetworkView>,
    pub busy: bool,
    pub action_busy: bool,
    pub error: Option<String>,
    pub message: Option<String>,
    pub ssid: String,
    pub password: String,
    pub ipv4: Option<Ipv4Form>,
}

impl NetworkState {
    pub fn apply(&mut self, view: NetworkView) {
        self.view = Some(view);
        self.error = None;
    }

    /// Open the editor prefilled from the device's connection settings.
    /// Returns `false` when the settings could not be read — the editor
    /// never guesses a method that could wipe a manual profile.
    pub fn open_ipv4(&mut self, connection: String) -> bool {
        let Some(settings) = self.view.as_ref().and_then(|view| {
            view.devices
                .iter()
                .find(|device| device.connection.as_deref() == Some(connection.as_str()))
                .and_then(|device| device.ipv4_settings.clone())
        }) else {
            self.error = Some(format!("Could not read IPv4 settings for {connection}."));
            return false;
        };
        self.ipv4 = Some(Ipv4Form {
            connection,
            static_ip: settings.method == "manual",
            addresses: settings.addresses.join(", "),
            gateway: settings.gateway.unwrap_or_default(),
            dns: settings.dns.join(" "),
            confirm: false,
        });
        true
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

            if let Some(form) = &self.ipv4 {
                let mut section = settings::section()
                    .title(format!("IPv4 — {}", form.connection))
                    .add(settings::item(
                        "Method",
                        dropdown(
                            vec!["DHCP (automatic)", "Static"],
                            Some(usize::from(form.static_ip)),
                            Message::NetworkIpv4MethodChanged,
                        ),
                    ));
                if form.static_ip {
                    section = section
                        .add(settings::item(
                            "Addresses (CIDR, comma-separated)",
                            text_input("192.168.1.5/24", form.addresses.clone()).on_input(
                                |value| Message::NetworkIpv4Field(IPV4_FIELD_ADDRESSES, value),
                            ),
                        ))
                        .add(settings::item(
                            "Gateway",
                            text_input("192.168.1.1", form.gateway.clone()).on_input(|value| {
                                Message::NetworkIpv4Field(IPV4_FIELD_GATEWAY, value)
                            }),
                        ))
                        .add(settings::item(
                            "DNS (space-separated)",
                            text_input("192.168.1.1 1.1.1.1", form.dns.clone())
                                .on_input(|value| Message::NetworkIpv4Field(IPV4_FIELD_DNS, value)),
                        ));
                }
                let actions = if form.confirm {
                    row(vec![
                        text("Apply now? An active connection reconnects.").into(),
                        button::destructive("Confirm")
                            .on_press_maybe(
                                (!self.action_busy).then_some(Message::NetworkIpv4Apply),
                            )
                            .into(),
                        button::text("Back")
                            .on_press(Message::NetworkIpv4Cancelled)
                            .into(),
                    ])
                    .spacing(8)
                } else {
                    row(vec![
                        button::text("Apply…")
                            .on_press_maybe(
                                (!self.action_busy).then_some(Message::NetworkIpv4Apply),
                            )
                            .into(),
                        button::text("Cancel")
                            .on_press_maybe(
                                (!self.action_busy).then_some(Message::NetworkIpv4Cancelled),
                            )
                            .into(),
                    ])
                    .spacing(8)
                };
                items.push(section.add(settings::item("Actions", actions)).into());
            }

            let mut devices = settings::section().title("Devices");
            for device in &view.devices {
                let name = device.device.clone();
                let mut actions = column(vec![text(format!(
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
                .into()])
                .spacing(4);
                let mut buttons = row(vec![button::text(if device.state.contains("connected") {
                    "Disconnect"
                } else {
                    "Connect"
                })
                .on_press(if device.state.contains("connected") {
                    Message::DisconnectNetwork(name.clone())
                } else {
                    Message::ConnectNetwork(name.clone())
                })
                .into()])
                .spacing(8);
                if let Some(connection) = device.connection.clone() {
                    let ipv4_button: Element<'_, Message> = button::text("IPv4…")
                        .on_press_maybe(
                            (!self.action_busy).then_some(Message::NetworkIpv4Edit(connection)),
                        )
                        .into();
                    buttons = buttons.push(ipv4_button);
                }
                actions = actions.push(buttons);
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
                    actions,
                ));
            }
            items.push(devices.into());
        }

        scrollable(column(items).spacing(16).padding(24)).into()
    }
}
