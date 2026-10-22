//! Home screen: the modules this principal may use, and what the machine offers.

use cosmic::iced::Length;
use cosmic::widget::{button, column, icon, row, settings, text};
use cosmic::Element;
use ursine_api::{Capability, CapabilityReport, ModuleId, ModulesConfig, TriState};

use crate::app::Message;
use crate::client::SystemStatus;

#[derive(Default)]
pub struct HomeState {
    pub user: String,
    pub caps: Vec<CapabilityReport>,
    pub status: Option<SystemStatus>,
    pub busy: bool,
    pub error: Option<String>,
}

/// Module metadata mirrored from the daemon gate. The daemon re-checks the
/// capability before every action, so a stale mirror can only mislabel a row.
struct ModuleEntry {
    title: &'static str,
    module: Option<ModuleId>,
    requires: &'static [Capability],
    panel: Option<Panel>,
}

#[derive(Clone, Copy)]
enum Panel {
    Store,
    Snapshots,
}

const MODULES: &[ModuleEntry] = &[
    ModuleEntry {
        title: "System",
        module: None,
        requires: &[],
        panel: None,
    },
    ModuleEntry {
        title: "Configuration",
        module: None,
        requires: &[],
        panel: None,
    },
    ModuleEntry {
        title: "Packages (Nix)",
        module: Some(ModuleId::PackagesNix),
        requires: &[Capability::Nix],
        panel: Some(Panel::Store),
    },
    ModuleEntry {
        title: "Snapshots (btrbk)",
        module: Some(ModuleId::Snapshots),
        requires: &[Capability::Btrbk, Capability::Btrfs],
        panel: Some(Panel::Snapshots),
    },
    ModuleEntry {
        title: "Users",
        module: Some(ModuleId::Users),
        requires: &[Capability::Accountsservice],
        panel: None,
    },
    ModuleEntry {
        title: "Niri desktop",
        module: Some(ModuleId::Niri),
        requires: &[Capability::Niri],
        panel: None,
    },
];

impl HomeState {
    pub fn view(
        &self,
        expires_in_secs: u64,
        module_config: Option<&ModulesConfig>,
    ) -> Element<'_, Message> {
        let settings_button = button::icon(icon::from_name("preferences-system-symbolic"))
            .on_press(Message::OpenSettings);
        let header = row(vec![
            text::title2("Ursine").into(),
            text(format!("signed in as {}", self.user)).into(),
            text("").width(Length::Fill).into(),
            settings_button.into(),
            button::text("Sign out").on_press(Message::SignOut).into(),
        ])
        .spacing(12)
        .width(Length::Fill);

        let mut items: Vec<Element<'_, Message>> = vec![header.into()];
        items.push(text(format!("session lifetime: {} min", expires_in_secs / 60)).into());
        if let Some(status) = &self.status {
            items.push(
                text(format!(
                    "daemon {} · protocol {} · auth {}",
                    status.version, status.protocol_version, status.auth
                ))
                .into(),
            );
        }

        if let Some(error) = &self.error {
            items.push(text(error.clone()).into());
        }

        let mut modules = settings::section().title("Modules");
        for entry in MODULES {
            modules = modules.add(settings::item(
                entry.title,
                self.module_status(entry, module_config),
            ));
        }
        items.push(modules.into());

        column(items).spacing(16).padding(24).into()
    }

    fn module_status(
        &self,
        entry: &ModuleEntry,
        modules: Option<&ModulesConfig>,
    ) -> Element<'_, Message> {
        if Self::is_manually_off(entry, modules) {
            return text("disabled in Settings").into();
        }

        let mut lines: Vec<Element<'_, Message>> = Vec::with_capacity(entry.requires.len());
        if entry.requires.is_empty() {
            lines.push(text("no capability required").into());
        } else {
            for required in entry.requires {
                let line = match self.report(*required) {
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
            Some(Panel::Store | Panel::Snapshots) => {
                let available = !Self::is_manually_off(entry, modules)
                    && entry.requires.iter().all(|required| {
                        self.report(*required)
                            .is_some_and(|report| report.available)
                    });
                let open_message = match entry.panel {
                    Some(Panel::Store) => Message::OpenStore,
                    Some(Panel::Snapshots) => Message::OpenSnapshots,
                    None => unreachable!(),
                };
                let open = button::text("Open").on_press_maybe(available.then_some(open_message));
                row(vec![status, open.into()]).spacing(12).into()
            }
            None => status,
        }
    }

    fn is_manually_off(entry: &ModuleEntry, modules: Option<&ModulesConfig>) -> bool {
        let (Some(module), Some(modules)) = (entry.module, modules) else {
            return false;
        };
        module_mode(modules, module) == TriState::Off
    }

    fn report(&self, capability: Capability) -> Option<&CapabilityReport> {
        self.caps
            .iter()
            .find(|report| report.capability == capability)
    }
}

fn module_mode(modules: &ModulesConfig, module: ModuleId) -> TriState {
    match module {
        ModuleId::PackagesNix => modules.packages_nix,
        ModuleId::Snapshots => modules.snapshots,
        ModuleId::Users => modules.users,
        ModuleId::Niri => modules.niri,
        ModuleId::System | ModuleId::Config | ModuleId::Capabilities => TriState::Auto,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_off_disables_the_module_row() {
        let entry = MODULES
            .iter()
            .find(|entry| entry.module == Some(ModuleId::PackagesNix))
            .unwrap();
        let modules = ModulesConfig {
            packages_nix: TriState::Off,
            ..Default::default()
        };
        assert!(HomeState::is_manually_off(entry, Some(&modules)));
        let modules = ModulesConfig::default();
        assert!(!HomeState::is_manually_off(entry, Some(&modules)));
    }
}
