//! Printers screen: CUPS printers and queued jobs.

use cosmic::iced::Length;
use cosmic::widget::{button, column, row, scrollable, settings, text, text_input};
use cosmic::Element;
use ursine_api::{PrintJob, Printer};

use crate::app::Message;

#[derive(Default)]
pub struct PrintersState {
    pub printers: Vec<Printer>,
    pub jobs: Vec<PrintJob>,
    pub busy: bool,
    pub action_busy: bool,
    pub error: Option<String>,
    pub message: Option<String>,
    pub name: String,
    pub uri: String,
    pub description: String,
    pub location: String,
    pub manual_driver: String,
    pub manual_ppd: String,
    pub manual_options: String,
    pub options_name: String,
    pub options_text: String,
    pub pending_remove: Option<String>,
    pub pending_cancel: bool,
}

impl PrintersState {
    pub fn apply(&mut self, printers: Vec<Printer>, jobs: Vec<PrintJob>) {
        self.printers = printers;
        self.jobs = jobs;
        self.error = None;
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2("Printers").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe(
                    (!self.busy && !self.action_busy).then_some(Message::RefreshPrinters),
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
            items.push(text("Reading CUPS state…").into());
        } else if self.action_busy {
            items.push(text("Running printer action…").into());
        }

        let mut printers = settings::section().title(format!("Printers ({})", self.printers.len()));
        if self.printers.is_empty() {
            printers = printers.add(settings::item("Result", text("No printers configured.")));
        }
        for printer in &self.printers {
            let name = printer.name.clone();
            printers = printers.add(settings::item(
                format!(
                    "{} — {}{}",
                    printer.name,
                    printer.state,
                    if printer.is_default {
                        " · default"
                    } else {
                        ""
                    }
                ),
                column(vec![
                    row(vec![
                        button::text("Set default")
                            .on_press(Message::PrinterSetDefault(name.clone()))
                            .into(),
                        button::text(if printer.accepting { "Pause" } else { "Resume" })
                            .on_press(Message::PrinterPause(name.clone(), printer.accepting))
                            .into(),
                        button::destructive("Remove…")
                            .on_press(Message::PrinterRemoveRequested(name.clone()))
                            .into(),
                    ])
                    .spacing(8)
                    .into(),
                    text(format!(
                        "{}{}{}",
                        printer.description.as_deref().unwrap_or("no description"),
                        printer
                            .location
                            .as_ref()
                            .map(|location| format!(" · {location}"))
                            .unwrap_or_default(),
                        printer
                            .model
                            .as_ref()
                            .map(|model| format!(" · {model}"))
                            .unwrap_or_default()
                    ))
                    .into(),
                ])
                .spacing(4),
            ));
        }
        items.push(printers.into());

        items.push(
            settings::section()
                .title("Printer options")
                .add(settings::item(
                    "Printer",
                    text_input("Office", self.options_name.clone())
                        .on_input(Message::PrinterOptionsNameChanged),
                ))
                .add(settings::item(
                    "Options, one per line key=value",
                    text_input("media=A4", self.options_text.clone())
                        .on_input(Message::PrinterOptionsChanged),
                ))
                .add(settings::item(
                    "Paper, duplex, color: media=A4, Duplex=DuplexNoTumble, ColorModel=Gray",
                    button::text("Apply options").on_press_maybe(
                        (!self.action_busy).then_some(Message::PrinterOptionsRequested),
                    ),
                ))
                .into(),
        );

        items.push(
            settings::section()
                .title("Manual configuration")
                .add(settings::item(
                    "Driver or model",
                    text_input("everywhere or drv:///...", self.manual_driver.clone())
                        .on_input(Message::PrinterManualDriverChanged),
                ))
                .add(settings::item(
                    "PPD path (overrides driver)",
                    text_input("/usr/share/ppd/...", self.manual_ppd.clone())
                        .on_input(Message::PrinterManualPpdChanged),
                ))
                .add(settings::item(
                    "Options, one per line key=value",
                    text_input("Option=Value", self.manual_options.clone())
                        .on_input(Message::PrinterManualOptionsChanged),
                ))
                .add(settings::item(
                    "Use the name/URI/description/location above",
                    button::suggested("Add manual printer").on_press_maybe(
                        (!self.action_busy).then_some(Message::PrinterAddManualRequested),
                    ),
                ))
                .into(),
        );

        items.push(
            settings::section()
                .title("Add driverless printer")
                .add(settings::item(
                    "Name",
                    text_input("Office", self.name.clone()).on_input(Message::PrinterNameChanged),
                ))
                .add(settings::item(
                    "URI",
                    text_input("ipps://printer.local/ipp/print", self.uri.clone())
                        .on_input(Message::PrinterUriChanged),
                ))
                .add(settings::item(
                    "Description",
                    text_input("Office printer", self.description.clone())
                        .on_input(Message::PrinterDescriptionChanged),
                ))
                .add(settings::item(
                    "Location",
                    text_input("Desk", self.location.clone())
                        .on_input(Message::PrinterLocationChanged),
                ))
                .add(settings::item(
                    "Add",
                    button::text("Add printer").on_press_maybe(
                        (!self.action_busy).then_some(Message::PrinterAddRequested),
                    ),
                ))
                .into(),
        );

        let mut jobs = settings::section().title(format!("Queued jobs ({})", self.jobs.len()));
        if self.jobs.is_empty() {
            jobs = jobs.add(settings::item("Result", text("No queued jobs.")));
        }
        for job in &self.jobs {
            jobs = jobs.add(settings::item(
                format!("{} · {} · {}", job.id, job.printer, job.user),
                text(job.domain()),
            ));
        }
        jobs = jobs.add(settings::item(
            "Cancel all jobs",
            button::destructive("Cancel all…")
                .on_press_maybe((!self.action_busy).then_some(Message::PrinterCancelAllRequested)),
        ));
        items.push(jobs.into());

        if let Some(name) = &self.pending_remove {
            items.push(
                settings::section()
                    .title("Confirm printer removal")
                    .add(settings::item(
                        "Consequences",
                        text(format!(
                            "Remove printer {name}? Queued jobs are not cancelled."
                        )),
                    ))
                    .add(settings::item(
                        "Actions",
                        row(vec![
                            button::destructive("Remove")
                                .on_press_maybe(
                                    (!self.action_busy).then_some(Message::PrinterRemoveConfirmed),
                                )
                                .into(),
                            button::text("Cancel")
                                .on_press(Message::PrinterRemoveCancelled)
                                .into(),
                        ])
                        .spacing(8),
                    ))
                    .into(),
            );
        }

        if self.pending_cancel {
            items.push(
                settings::section()
                    .title("Confirm cancel all jobs")
                    .add(settings::item(
                        "Consequences",
                        text("Cancel every queued print job on this system?"),
                    ))
                    .add(settings::item(
                        "Actions",
                        row(vec![
                            button::destructive("Cancel jobs")
                                .on_press_maybe(
                                    (!self.action_busy).then_some(Message::PrinterCancelConfirmed),
                                )
                                .into(),
                            button::text("Back")
                                .on_press(Message::PrinterCancelCancelled)
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

trait JobDetail {
    fn domain(&self) -> String;
}

impl JobDetail for PrintJob {
    fn domain(&self) -> String {
        format!(
            "{}{}{}",
            self.size.as_deref().unwrap_or("unknown size"),
            self.submitted
                .as_ref()
                .map(|submitted| format!(" · {submitted}"))
                .unwrap_or_default(),
            ""
        )
    }
}
