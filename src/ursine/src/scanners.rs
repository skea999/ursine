//! Scanners screen: SANE device list and one-page PNG scans.

use cosmic::iced::Length;
use cosmic::widget::{button, column, dropdown, row, scrollable, settings, text, text_input};
use cosmic::Element;
use ursine_api::Scanner;

use crate::app::Message;

const RESOLUTIONS: [u32; 6] = [75, 100, 150, 200, 300, 600];
const MODES: [&str; 3] = ["Color", "Gray", "Lineart"];

#[derive(Default)]
pub struct ScannersState {
    pub scanners: Vec<Scanner>,
    pub selected: Option<usize>,
    pub resolution: usize,
    pub mode: usize,
    pub source: String,
    pub busy: bool,
    pub action_busy: bool,
    pub error: Option<String>,
    pub message: Option<String>,
}

impl ScannersState {
    pub fn apply(&mut self, scanners: Vec<Scanner>) {
        self.scanners = scanners;
        if self.selected.is_none() && !self.scanners.is_empty() {
            self.selected = Some(0);
        }
        self.error = None;
    }

    pub fn selected_device(&self) -> Option<&Scanner> {
        self.scanners.get(self.selected?)
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
            items.push(text(message.clone()).into());
        }
        if self.busy {
            items.push(text("Discovering scanners…").into());
        } else if self.action_busy {
            items.push(text("Scanning…").into());
        }

        let options = self
            .scanners
            .iter()
            .map(|scanner| format!("{} — {}", scanner.id, scanner.description))
            .collect::<Vec<_>>();
        items.push(
            settings::section()
                .title("Device")
                .add(settings::item(
                    "Scanner",
                    dropdown(options, self.selected, Message::ScannerSelected),
                ))
                .add(settings::item(
                    "Resolution",
                    dropdown(
                        RESOLUTIONS.iter().map(u32::to_string).collect::<Vec<_>>(),
                        Some(self.resolution),
                        Message::ScannerResolutionChanged,
                    ),
                ))
                .add(settings::item(
                    "Mode",
                    dropdown(
                        MODES
                            .iter()
                            .map(|mode| (*mode).to_string())
                            .collect::<Vec<_>>(),
                        Some(self.mode),
                        Message::ScannerModeChanged,
                    ),
                ))
                .add(settings::item(
                    "Source (e.g. Flatbed, ADF)",
                    text_input("Flatbed", self.source.clone())
                        .on_input(Message::ScannerSourceChanged),
                ))
                .add(settings::item(
                    "Output",
                    text("~/Pictures/ursine-scan-<timestamp>.png"),
                ))
                .add(settings::item(
                    "Scan",
                    button::suggested("Scan one page").on_press_maybe(
                        (!self.action_busy && self.selected.is_some())
                            .then_some(Message::ScannerScanRequested),
                    ),
                ))
                .into(),
        );

        if self.scanners.is_empty() && !self.busy {
            items.push(
                settings::section()
                    .title("Result")
                    .add(settings::item(
                        "No scanners",
                        text("No SANE devices detected."),
                    ))
                    .into(),
            );
        }

        scrollable(column(items).spacing(16).padding(24)).into()
    }
}
