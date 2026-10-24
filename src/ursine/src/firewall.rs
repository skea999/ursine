//! Firewall screen: firewalld zones, services and ports.

use cosmic::iced::Length;
use cosmic::widget::{button, column, dropdown, row, scrollable, settings, text, text_input};
use cosmic::Element;
use ursine_api::FirewallStatus;

use crate::app::Message;

#[derive(Default)]
pub struct FirewallState {
    pub status: Option<FirewallStatus>,
    pub busy: bool,
    pub action_busy: bool,
    pub error: Option<String>,
    pub message: Option<String>,
    pub zone: String,
    pub service: String,
    pub port: String,
}

impl FirewallState {
    pub fn apply(&mut self, status: FirewallStatus) {
        if self.zone.is_empty() {
            self.zone.clone_from(&status.default_zone);
        }
        self.status = Some(status);
        self.error = None;
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2("Firewall").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe(
                    (!self.busy && !self.action_busy).then_some(Message::RefreshFirewall),
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
            items.push(text("Reading firewalld…").into());
        } else if self.action_busy {
            items.push(text("Applying firewall change…").into());
        }

        let Some(status) = &self.status else {
            return scrollable(column(items).spacing(16).padding(24)).into();
        };
        if !status.running {
            items.push(
                settings::section()
                    .title("Result")
                    .add(settings::item("State", text("firewalld is not running.")))
                    .into(),
            );
            return scrollable(column(items).spacing(16).padding(24)).into();
        }

        let zones = status
            .zones
            .iter()
            .map(|zone| zone.name.clone())
            .collect::<Vec<_>>();
        let selected = zones.iter().position(|zone| *zone == self.zone);
        let zones_for_zone = zones.clone();
        items.push(
            settings::section()
                .title("Zone setup")
                .add(settings::item(
                    "Default zone",
                    dropdown(
                        zones.clone(),
                        zones.iter().position(|zone| *zone == status.default_zone),
                        Message::FirewallDefaultZoneSelected,
                    ),
                ))
                .add(settings::item(
                    "Selected zone",
                    dropdown(zones, selected, move |index| {
                        Message::FirewallZoneChanged(
                            zones_for_zone.get(index).cloned().unwrap_or_default(),
                        )
                    }),
                ))
                .add(settings::item(
                    "Add service",
                    row(vec![
                        text_input("https", self.service.clone())
                            .on_input(Message::FirewallServiceChanged)
                            .into(),
                        button::text("Open")
                            .on_press_maybe(
                                (!self.action_busy).then_some(Message::FirewallAddService),
                            )
                            .into(),
                    ])
                    .spacing(8),
                ))
                .add(settings::item(
                    "Add port",
                    row(vec![
                        text_input("8080/tcp", self.port.clone())
                            .on_input(Message::FirewallPortChanged)
                            .into(),
                        button::text("Open")
                            .on_press_maybe((!self.action_busy).then_some(Message::FirewallAddPort))
                            .into(),
                    ])
                    .spacing(8),
                ))
                .add(settings::item(
                    "Reload",
                    button::text("Reload firewalld")
                        .on_press_maybe((!self.action_busy).then_some(Message::FirewallReload)),
                ))
                .into(),
        );

        for zone in &status.zones {
            let mut section = settings::section().title(format!(
                "{} zone{}",
                zone.name,
                if zone.default { " · default" } else { "" }
            ));
            if zone.services.is_empty() && zone.ports.is_empty() {
                section = section.add(settings::item("Rules", text("none")));
            }
            for service in &zone.services {
                let zone_name = zone.name.clone();
                let service_name = service.clone();
                section = section.add(settings::item(
                    format!("service {service}"),
                    button::text("Close")
                        .on_press(Message::FirewallRemoveService(zone_name, service_name)),
                ));
            }
            for port in &zone.ports {
                let zone_name = zone.name.clone();
                let port_name = port.clone();
                section = section.add(settings::item(
                    format!("port {port}"),
                    button::text("Close")
                        .on_press(Message::FirewallRemovePort(zone_name, port_name)),
                ));
            }
            items.push(section.into());
        }

        scrollable(column(items).spacing(16).padding(24)).into()
    }
}
