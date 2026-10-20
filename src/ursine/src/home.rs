//! Home screen: the modules this principal may use, and what the machine offers.

use cosmic::widget::{button, column, row, settings, text};
use cosmic::Element;
use ursine_api::{Capability, CapabilityReport};

use crate::app::Message;

#[derive(Default)]
pub struct HomeState {
    pub user: String,
    pub caps: Vec<CapabilityReport>,
    pub busy: bool,
    pub error: Option<String>,
}

/// Module metadata mirrored from the daemon gate. The daemon re-checks the
/// capability before every action, so a stale mirror can only mislabel a row.
struct ModuleEntry {
    title: &'static str,
    requires: &'static [Capability],
    panel: Option<Panel>,
}

#[derive(Clone, Copy)]
enum Panel {
    Store,
}

const MODULES: &[ModuleEntry] = &[
    ModuleEntry {
        title: "System",
        requires: &[],
        panel: None,
    },
    ModuleEntry {
        title: "Configuration",
        requires: &[],
        panel: None,
    },
    ModuleEntry {
        title: "Packages (Nix)",
        requires: &[Capability::Nix],
        panel: Some(Panel::Store),
    },
    ModuleEntry {
        title: "Snapshots (btrbk)",
        requires: &[Capability::Btrbk, Capability::Btrfs],
        panel: None,
    },
    ModuleEntry {
        title: "Users",
        requires: &[Capability::Accountsservice],
        panel: None,
    },
    ModuleEntry {
        title: "Niri desktop",
        requires: &[Capability::Niri],
        panel: None,
    },
];

impl HomeState {
    pub fn view(&self, expires_in_secs: u64) -> Element<'_, Message> {
        let header = row(vec![
            text::title2("Ursine").into(),
            text(format!("signed in as {}", self.user)).into(),
            button::text("Sign out").on_press(Message::SignOut).into(),
        ])
        .spacing(12);

        let mut items: Vec<Element<'_, Message>> = vec![header.into()];
        items.push(text(format!("session lifetime: {} min", expires_in_secs / 60)).into());

        if let Some(error) = &self.error {
            items.push(text(error.clone()).into());
        }

        let mut modules = settings::section().title("Modules");
        for entry in MODULES {
            modules = modules.add(settings::item(entry.title, self.module_status(entry)));
        }
        items.push(modules.into());

        let mut capabilities = settings::section().title("Capabilities");
        for report in &self.caps {
            capabilities = capabilities.add(settings::item(
                report.capability.as_str(),
                self.capability_detail(report),
            ));
        }
        items.push(capabilities.into());

        let refresh = button::standard(if self.busy {
            "Refreshing…"
        } else {
            "Refresh"
        })
        .on_press_maybe((!self.busy).then_some(Message::RefreshCapabilities));
        items.push(refresh.into());

        column(items).spacing(16).padding(24).into()
    }

    fn module_status(&self, entry: &ModuleEntry) -> Element<'_, Message> {
        let mut lines: Vec<Element<'_, Message>> = Vec::with_capacity(entry.requires.len());
        if entry.requires.is_empty() {
            lines.push(text("no capability required").into());
        } else {
            for required in entry.requires {
                let line = match self.report(required) {
                    Some(report) if report.available => {
                        format!("{}: available", required.as_str())
                    }
                    Some(report) => format!(
                        "{}: {}",
                        required.as_str(),
                        report.reason.as_deref().unwrap_or("unavailable")
                    ),
                    None => format!("{}: unknown", required.as_str()),
                };
                lines.push(text(line).into());
            }
        }

        let status: Element<'_, Message> = column(lines).spacing(2).into();
        match entry.panel {
            Some(Panel::Store) => {
                let available = entry
                    .requires
                    .iter()
                    .all(|required| self.report(required).is_some_and(|report| report.available));
                let open =
                    button::text("Open").on_press_maybe(available.then_some(Message::OpenStore));
                row(vec![status, open.into()]).spacing(12).into()
            }
            None => status,
        }
    }

    fn capability_detail(&self, report: &CapabilityReport) -> Element<'_, Message> {
        if report.available {
            return text("available").into();
        }

        let mut lines: Vec<Element<'_, Message>> = vec![text(
            report
                .reason
                .clone()
                .unwrap_or_else(|| "unavailable".to_string()),
        )
        .into()];
        if let Some(fix) = &report.fix {
            lines.push(text(format!("fix: {fix}")).into());
        }
        column(lines).spacing(2).into()
    }

    fn report(&self, capability: &Capability) -> Option<&CapabilityReport> {
        self.caps
            .iter()
            .find(|report| report.capability == *capability)
    }
}
