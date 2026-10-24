//! Home screen: the modules this principal may use, and what the machine offers.

use cosmic::iced::Length;
use cosmic::widget::{button, column, row, settings, text};
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
    Users,
    Niri,
    Apk,
    Flatpak,
    Printers,
    Scanners,
    Firewall,
    Services,
    Disks,
    Bluetooth,
    Network,
}

const MODULES: &[ModuleEntry] = &[
    ModuleEntry {
        title: "Packages (Nix)",
        module: Some(ModuleId::PackagesNix),
        requires: &[Capability::Nix],
        panel: Some(Panel::Store),
    },
    ModuleEntry {
        title: "APK packages",
        module: Some(ModuleId::PackagesApk),
        requires: &[Capability::Apk],
        panel: Some(Panel::Apk),
    },
    ModuleEntry {
        title: "Flatpak applications",
        module: Some(ModuleId::PackagesFlatpak),
        requires: &[Capability::Flatpak],
        panel: Some(Panel::Flatpak),
    },
    ModuleEntry {
        title: "Printers",
        module: Some(ModuleId::Printers),
        requires: &[Capability::Cups],
        panel: Some(Panel::Printers),
    },
    ModuleEntry {
        title: "Scanners",
        module: Some(ModuleId::Scanners),
        requires: &[Capability::Sane],
        panel: Some(Panel::Scanners),
    },
    ModuleEntry {
        title: "Firewall",
        module: Some(ModuleId::Firewall),
        requires: &[Capability::Firewalld],
        panel: Some(Panel::Firewall),
    },
    ModuleEntry {
        title: "Services",
        module: Some(ModuleId::Services),
        requires: &[Capability::OpenRc],
        panel: Some(Panel::Services),
    },
    ModuleEntry {
        title: "Disks",
        module: Some(ModuleId::Disks),
        requires: &[Capability::Udisks2],
        panel: Some(Panel::Disks),
    },
    ModuleEntry {
        title: "Bluetooth",
        module: Some(ModuleId::Bluetooth),
        requires: &[Capability::Bluez],
        panel: Some(Panel::Bluetooth),
    },
    ModuleEntry {
        title: "Network",
        module: Some(ModuleId::Network),
        requires: &[Capability::NetworkManager],
        panel: Some(Panel::Network),
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
        panel: Some(Panel::Users),
    },
    ModuleEntry {
        title: "Niri desktop",
        module: Some(ModuleId::Niri),
        requires: &[Capability::Niri],
        panel: Some(Panel::Niri),
    },
];

impl HomeState {
    pub fn view(
        &self,
        expires_in_secs: u64,
        module_config: Option<&ModulesConfig>,
    ) -> Element<'_, Message> {
        let header = row(vec![
            text::title2("Overview").into(),
            text("").width(Length::Fill).into(),
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
            Some(
                Panel::Store
                | Panel::Snapshots
                | Panel::Users
                | Panel::Niri
                | Panel::Apk
                | Panel::Flatpak
                | Panel::Printers
                | Panel::Scanners
                | Panel::Firewall
                | Panel::Services
                | Panel::Disks
                | Panel::Bluetooth
                | Panel::Network,
            ) => {
                let available = !Self::is_manually_off(entry, modules)
                    && entry.requires.iter().all(|required| {
                        self.report(*required)
                            .is_some_and(|report| report.available)
                    });
                let open_message = match entry.panel {
                    Some(Panel::Store) => Message::OpenStore,
                    Some(Panel::Snapshots) => Message::OpenSnapshots,
                    Some(Panel::Users) => Message::OpenUsers,
                    Some(Panel::Niri) => Message::OpenNiri,
                    Some(Panel::Apk) => Message::OpenPackages(crate::packages::PackageKind::Apk),
                    Some(Panel::Flatpak) => {
                        Message::OpenPackages(crate::packages::PackageKind::Flatpak)
                    }
                    Some(Panel::Printers) => Message::OpenPrinters,
                    Some(Panel::Scanners) => Message::OpenScanners,
                    Some(Panel::Firewall) => Message::OpenFirewall,
                    Some(Panel::Services) => Message::OpenServices,
                    Some(Panel::Disks) => Message::OpenDisks,
                    Some(Panel::Bluetooth) => Message::OpenBluetooth,
                    Some(Panel::Network) => Message::OpenNetwork,
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
        ModuleId::PackagesApk => modules.packages_apk,
        ModuleId::PackagesFlatpak => modules.packages_flatpak,
        ModuleId::Printers => modules.printers,
        ModuleId::Scanners => modules.scanners,
        ModuleId::Firewall => modules.firewall,
        ModuleId::Services => modules.services,
        ModuleId::Disks => modules.disks,
        ModuleId::Bluetooth => modules.bluetooth,
        ModuleId::Network => modules.network,
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
