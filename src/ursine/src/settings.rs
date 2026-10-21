//! Settings screen: live capability facts and manual module switches.

use cosmic::iced::Length;
use cosmic::widget::{button, column, row, settings, text};
use cosmic::Element;
use ursine_api::{CapabilityReport, ModuleId, TriState};

use crate::app::Message;
use crate::client::ConfigSnapshot;

#[derive(Default)]
pub struct SettingsState {
    pub config: Option<ConfigSnapshot>,
    pub config_busy: bool,
    pub error: Option<String>,
}

struct ModuleSwitch {
    module: ModuleId,
    title: &'static str,
}

const MODULE_SWITCHES: &[ModuleSwitch] = &[
    ModuleSwitch {
        module: ModuleId::PackagesNix,
        title: "Packages (Nix)",
    },
    ModuleSwitch {
        module: ModuleId::Snapshots,
        title: "Snapshots",
    },
    ModuleSwitch {
        module: ModuleId::Users,
        title: "Users",
    },
    ModuleSwitch {
        module: ModuleId::Niri,
        title: "Niri desktop",
    },
];

impl SettingsState {
    pub fn view<'a>(
        &'a self,
        caps: &'a [CapabilityReport],
        caps_busy: bool,
        caps_error: Option<&'a str>,
    ) -> Element<'a, Message> {
        let back = button::text("Home").on_press(Message::BackHome);
        let header = row(vec![
            back.into(),
            text::title2("Settings").into(),
            text("").width(Length::Fill).into(),
        ])
        .spacing(12)
        .width(Length::Fill);

        let mut items: Vec<Element<'_, Message>> = vec![header.into()];

        if let Some(error) = caps_error {
            items.push(text(error).into());
        }
        if let Some(error) = &self.error {
            items.push(text(error.clone()).into());
            if self.config.is_none() && !self.config_busy {
                items.push(button::text("Retry").on_press(Message::OpenSettings).into());
            }
        }

        let refresh = button::standard(if caps_busy {
            "Refreshing…"
        } else {
            "Refresh capabilities"
        })
        .on_press_maybe((!caps_busy).then_some(Message::RefreshCapabilities));
        items.push(refresh.into());

        let mut capabilities = settings::section().title("Detected capabilities");
        for report in caps {
            capabilities = capabilities.add(settings::item(
                report.capability.as_str(),
                capability_detail(report),
            ));
        }
        items.push(capabilities.into());
        if caps.is_empty() {
            items.push(
                text(if caps_busy {
                    "Detecting capabilities…"
                } else {
                    "No capabilities reported"
                })
                .into(),
            );
        }

        let mut modules = settings::section().title("Module mode");
        modules = modules.add(settings::item(
            "Mode",
            text("Auto detects. Manual on forces use; manual off disables. Missing system dependencies still make a module unavailable."),
        ));
        if let Some(config) = &self.config {
            modules = modules.add(settings::item(
                "Configuration file",
                text(config.path.clone()),
            ));
        }
        for switch in MODULE_SWITCHES {
            modules = modules.add(settings::item(
                switch.title,
                self.switch_control(switch.module),
            ));
        }
        items.push(modules.into());

        if self.config_busy {
            items.push(text("Saving module mode…").into());
        }

        column(items).spacing(16).padding(24).into()
    }

    fn switch_control(&self, module: ModuleId) -> Element<'_, Message> {
        let Some(config) = &self.config else {
            return text(if self.config_busy {
                "loading…"
            } else {
                "unavailable"
            })
            .into();
        };
        let current = module_switch(config, module);
        let enabled = !self.config_busy;

        let choice = |value: TriState, label: &'static str| {
            let mut b = button::text(label);
            if enabled && current != value {
                b = b.on_press(Message::SetModuleSwitch(module, value));
            }
            if current == value {
                b = b.class(cosmic::theme::Button::Standard);
            }
            b.into()
        };

        row(vec![
            choice(TriState::Auto, "Auto"),
            choice(TriState::On, "Manual on"),
            choice(TriState::Off, "Manual off"),
        ])
        .spacing(8)
        .into()
    }
}

fn module_switch(config: &ConfigSnapshot, module: ModuleId) -> TriState {
    match module {
        ModuleId::PackagesNix => config.modules.packages_nix,
        ModuleId::Snapshots => config.modules.snapshots,
        ModuleId::Users => config.modules.users,
        ModuleId::Niri => config.modules.niri,
        ModuleId::System | ModuleId::Config | ModuleId::Capabilities => TriState::Auto,
    }
}

fn capability_detail(report: &CapabilityReport) -> Element<'_, Message> {
    if report.available {
        return text("available").into();
    }

    let mut lines = vec![text(
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

#[cfg(test)]
mod tests {
    use super::*;
    use ursine_api::ModulesConfig;

    #[test]
    fn module_switch_reads_the_current_snapshot() {
        let config = ConfigSnapshot {
            modules: ModulesConfig {
                packages_nix: TriState::Off,
                ..Default::default()
            },
            path: "/etc/ursine/config.toml".into(),
        };
        assert_eq!(module_switch(&config, ModuleId::PackagesNix), TriState::Off);
        assert_eq!(module_switch(&config, ModuleId::Snapshots), TriState::Auto);
    }
}
