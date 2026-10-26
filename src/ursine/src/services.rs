//! Services screen: init service status and lifecycle (`OpenRC` and dinit).

use cosmic::iced::Length;
use cosmic::widget::{button, column, row, scrollable, settings, text};
use cosmic::Element;
use ursine_api::{Service, ServiceOp};

use crate::app::Message;

#[derive(Default)]
pub struct ServicesState {
    pub services: Vec<Service>,
    pub busy: bool,
    pub action_busy: bool,
    pub error: Option<String>,
    pub message: Option<String>,
    pub pending: Option<(String, ServiceOp)>,
}

impl ServicesState {
    pub fn apply(&mut self, services: Vec<Service>) {
        self.services = services;
        self.error = None;
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2("Services").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe(
                    (!self.busy && !self.action_busy).then_some(Message::RefreshServices),
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
            items.push(text("Reading services…").into());
        } else if self.action_busy {
            items.push(text("Running service action…").into());
        }

        let init = if self.services.iter().any(|service| service.init == "dinit") {
            "dinit"
        } else if self.services.iter().any(|service| service.init == "openrc") {
            "OpenRC"
        } else {
            "Services"
        };
        let mut list =
            settings::section().title(format!("{init} services ({})", self.services.len()));
        if self.services.is_empty() {
            list = list.add(settings::item("Result", text("No services reported.")));
        }
        for service in &self.services {
            let name = service.name.clone();
            let started = service.state == "started";
            list = list.add(settings::item(
                format!(
                    "{} — {}{}",
                    service.name,
                    service.state,
                    if service.enabled { " · enabled" } else { "" }
                ),
                column(vec![row(vec![
                    button::text(if started { "Stop" } else { "Start" })
                        .on_press(Message::ServiceRequested(
                            name.clone(),
                            if started {
                                ServiceOp::Stop
                            } else {
                                ServiceOp::Start
                            },
                        ))
                        .into(),
                    button::text("Restart")
                        .on_press(Message::ServiceRequested(name.clone(), ServiceOp::Restart))
                        .into(),
                    button::text(if service.enabled { "Disable" } else { "Enable" })
                        .on_press(Message::ServiceRequested(
                            name.clone(),
                            if service.enabled {
                                ServiceOp::Disable
                            } else {
                                ServiceOp::Enable
                            },
                        ))
                        .into(),
                ])
                .spacing(8)
                .into()])
                .spacing(4),
            ));
        }
        items.push(list.into());

        if let Some((name, op)) = &self.pending {
            items.push(
                settings::section()
                    .title(format!("Confirm {op:?}"))
                    .add(settings::item(
                        "Consequences",
                        text(format!("Apply {op:?} to service {name}?")),
                    ))
                    .add(settings::item(
                        "Actions",
                        row(vec![
                            button::destructive("Confirm")
                                .on_press_maybe(
                                    (!self.action_busy).then_some(Message::ServiceConfirmed),
                                )
                                .into(),
                            button::text("Cancel")
                                .on_press(Message::ServiceCancelled)
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
