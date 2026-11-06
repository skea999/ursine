//! Firewall screen: firewalld zones, services and ports.

use cosmic::iced::Length;
use cosmic::widget::{
    button, column, dropdown, row, scrollable, settings, text, text_input, toggler,
};
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
    /// Disable confirmation armed (security-sensitive).
    pub pending_disable: bool,
    /// Custom-service delete waiting for confirmation.
    pub pending_delete: Option<String>,
    /// Name input of the "New custom service" form (separate from the
    /// zone-scoped `service` field).
    pub new_service: String,
    /// Custom-service definition port list (`8080/tcp 443/tcp`).
    pub new_service_ports: String,
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
        if self.pending_disable {
            items.push(
                settings::section()
                    .title("Confirm disable")
                    .add(settings::item(
                        "Consequences",
                        text("Stop filtering on the default zone (target ACCEPT)? Traffic the firewall blocked will pass."),
                    ))
                    .add(settings::item(
                        "Actions",
                        row(vec![
                            button::destructive("Disable filtering")
                                .on_press_maybe(
                                    (!self.action_busy)
                                        .then_some(Message::FirewallEnabledConfirmed),
                                )
                                .into(),
                            button::text("Cancel")
                                .on_press_maybe(
                                    (!self.action_busy)
                                        .then_some(Message::FirewallEnabledCancelled),
                                )
                                .into(),
                        ])
                        .spacing(8),
                    ))
                    .into(),
            );
        }

        items.push(
            settings::section()
                .title("Zone setup")
                .add(settings::item(
                    "Filtering",
                    toggler(status.enabled).on_toggle(Message::FirewallEnabledToggled),
                ))
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

        if let Some(name) = &self.pending_delete {
            items.push(
                settings::section()
                    .title("Confirm delete")
                    .add(settings::item(
                        "Consequences",
                        text(format!(
                            "Delete the custom service definition {name}? Zones referencing it lose the service."
                        )),
                    ))
                    .add(settings::item(
                        "Actions",
                        row(vec![
                            button::destructive("Delete")
                                .on_press_maybe(
                                    (!self.action_busy)
                                        .then_some(Message::FirewallDeleteConfirmed),
                                )
                                .into(),
                            button::text("Cancel")
                                .on_press_maybe(
                                    (!self.action_busy)
                                        .then_some(Message::FirewallDeleteCancelled),
                                )
                                .into(),
                        ])
                        .spacing(8),
                    ))
                    .into(),
            );
        }

        let mut custom = settings::section().title("Custom services");
        if status.custom_services.is_empty() {
            custom = custom.add(settings::item(
                "Definitions",
                text("none (built-in services only)"),
            ));
        }
        for service in &status.custom_services {
            let name = service.clone();
            custom = custom.add(settings::item(
                service.clone(),
                button::text("Delete…").on_press_maybe(
                    (!self.action_busy).then_some(Message::FirewallDeleteService(name)),
                ),
            ));
        }
        custom = custom.add(settings::item(
            "New custom service",
            row(vec![
                text_input("myapp", self.new_service.clone())
                    .on_input(Message::FirewallNewServiceChanged)
                    .into(),
                text_input("ports: 8080/tcp 8443/tcp", self.new_service_ports.clone())
                    .on_input(Message::FirewallPortsChanged)
                    .into(),
                button::text("Create")
                    .on_press_maybe((!self.action_busy).then_some(Message::FirewallCreateService))
                    .into(),
            ])
            .spacing(8),
        ));
        items.push(custom.into());

        for zone in &status.zones {
            let mut section = settings::section().title(format!(
                "{} zone{}",
                zone.name,
                if zone.default { " · default" } else { "" }
            ));
            section = section.add(settings::item(
                "Target",
                text(if zone.target.is_empty() {
                    "unknown".to_string()
                } else {
                    zone.target.clone()
                }),
            ));
            if !zone.interfaces.is_empty() {
                section = section.add(settings::item(
                    "Interfaces",
                    text(zone.interfaces.join(", ")),
                ));
            }
            if zone.services.is_empty() && zone.ports.is_empty() && zone.rich_rules.is_empty() {
                section = section.add(settings::item("Rules", text("none")));
            }
            for rule in &zone.rich_rules {
                section = section.add(settings::item("rich rule", text(rule.clone())));
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
