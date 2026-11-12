//! Booster screen (SPEC-booster): surgical `/etc/booster.yaml` editing
//! plus the regenerate button that runs the distro's kernel hooks.

use cosmic::iced::Length;
use cosmic::widget::selectable_text;
use cosmic::widget::text::monotext;
use cosmic::widget::{button, column, row, scrollable, text, text_input};
use cosmic::Element;
use ursine_api::{BoosterConfig, BoosterConfigPatch};

use crate::app::Message;

/// One editable key: wire name (also the `clear` entry) + label.
const FIELDS: &[(&str, &str)] = &[
    ("compression", "compression"),
    ("universal", "universal"),
    ("strip", "strip"),
    ("vconsole", "vconsole"),
    ("enable_lvm", "enable_lvm"),
    ("enable_mdraid", "enable_mdraid"),
    ("append_all_modaliases", "append_all_modaliases"),
    ("mount_timeout", "mount_timeout"),
    ("token_timeout", "token_timeout"),
    ("pin_delay", "pin_delay"),
    ("extra_files", "extra_files"),
    ("modules", "modules"),
    ("modules_force_load", "modules_force_load"),
    ("network.dhcp", "network.dhcp"),
    ("network.ip", "network.ip"),
    ("network.gateway", "network.gateway"),
    ("network.dns_servers", "network.dns_servers"),
    ("network.interfaces", "network.interfaces"),
];

#[derive(Debug)]
pub struct BoosterState {
    pub busy: bool,
    pub action_busy: bool,
    pub loaded: bool,
    /// The file exists on disk (vs "Save will create it").
    pub exists: bool,
    pub raw: String,
    /// Chain step the regenerate button will run (computed by the daemon).
    pub regenerate_step: String,
    /// Last successful get (change detection for Save/clear).
    pub loaded_config: BoosterConfig,
    /// Editable raw scalar text per wire key (empty = absent).
    pub values: Vec<(String, String)>,
    pub save_confirm: bool,
    pub regen_confirm: bool,
    /// Last regenerate report: (step, output).
    pub output: Option<(String, String)>,
    pub error: Option<String>,
    pub message: Option<String>,
}

impl Default for BoosterState {
    fn default() -> Self {
        Self {
            busy: false,
            action_busy: false,
            loaded: false,
            exists: false,
            raw: String::new(),
            regenerate_step: String::new(),
            loaded_config: BoosterConfig::default(),
            values: FIELDS
                .iter()
                .map(|(key, _)| (key.to_string(), String::new()))
                .collect(),
            save_confirm: false,
            regen_confirm: false,
            output: None,
            error: None,
            message: None,
        }
    }
}

impl BoosterState {
    fn value(&self, key: &str) -> &str {
        self.values
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
            .unwrap_or_default()
    }

    pub fn set_value(&mut self, key: &str, value: String) {
        if let Some(slot) = self.values.iter_mut().find(|(name, _)| name == key) {
            slot.1 = value;
        }
    }

    fn loaded_for(&self, key: &str) -> Option<&str> {
        Self::loaded_for_key(&self.loaded_config, key)
    }

    /// Diff the inputs against the last loaded config: changed keys are
    /// set, emptied keys are cleared, untouched keys are not sent.
    pub fn build_patch(&self) -> BoosterConfigPatch {
        let mut patch = BoosterConfigPatch::default();
        for (key, _) in FIELDS {
            let input = self.value(key);
            let loaded = self.loaded_for(key);
            if input.is_empty() {
                if loaded.is_some() {
                    patch.clear.push(key.to_string());
                }
            } else if loaded != Some(input) {
                let slot = patch_field_mut(&mut patch, key);
                *slot = Some(input.to_string());
            }
        }
        patch
    }

    /// Load a fresh get result into the editable fields.
    pub fn apply_config(&mut self, config: BoosterConfig) {
        self.exists = config.exists;
        self.raw = config.raw.clone();
        self.regenerate_step = config.regenerate_step.clone();
        for (key, _) in FIELDS {
            let text = Self::loaded_for_key(&config, key)
                .unwrap_or_default()
                .to_string();
            self.set_value(key, text);
        }
        self.loaded_config = config;
        self.loaded = true;
        self.save_confirm = false;
    }

    fn loaded_for_key<'a>(config: &'a BoosterConfig, key: &str) -> Option<&'a str> {
        let value = match key {
            "compression" => config.compression.as_deref(),
            "universal" => config.universal.as_deref(),
            "strip" => config.strip.as_deref(),
            "vconsole" => config.vconsole.as_deref(),
            "enable_lvm" => config.enable_lvm.as_deref(),
            "enable_mdraid" => config.enable_mdraid.as_deref(),
            "append_all_modaliases" => config.append_all_modaliases.as_deref(),
            "mount_timeout" => config.mount_timeout.as_deref(),
            "token_timeout" => config.token_timeout.as_deref(),
            "pin_delay" => config.pin_delay.as_deref(),
            "extra_files" => config.extra_files.as_deref(),
            "modules" => config.modules.as_deref(),
            "modules_force_load" => config.modules_force_load.as_deref(),
            "network.dhcp" => config.network_dhcp.as_deref(),
            "network.ip" => config.network_ip.as_deref(),
            "network.gateway" => config.network_gateway.as_deref(),
            "network.dns_servers" => config.network_dns_servers.as_deref(),
            "network.interfaces" => config.network_interfaces.as_deref(),
            _ => None,
        };
        value
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2("Booster (initramfs)").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe((!self.busy && !self.action_busy).then_some(Message::OpenBooster))
                .into(),
        ])
        .spacing(12)
        .width(Length::Fill)
        .into()];

        if let Some(error) = &self.error {
            items.push(selectable_text(error.clone()).into());
        }
        if let Some(message) = &self.message {
            items.push(text(message.clone()).into());
        }
        if self.busy {
            items.push(text("Reading /etc/booster.yaml…").into());
            return scrollable(column(items).spacing(16).padding(24)).into();
        }
        if !self.loaded {
            items.push(text("/etc/booster.yaml could not be loaded.").into());
            return scrollable(column(items).spacing(16).padding(24)).into();
        }
        if !self.exists {
            items.push(text("No /etc/booster.yaml yet — Save creates it.").into());
        }

        items.push(text::title3("Known keys").into());
        for (key, _) in FIELDS {
            items.push(field_row(key, self.value(key).to_string()));
        }

        if self.save_confirm {
            items.push(text(
                "Write /etc/booster.yaml? The current file rotates into the backup slots first.",
            )
            .into());
            items.push(
                row(vec![
                    button::suggested("Confirm save")
                        .on_press_maybe(
                            (!self.action_busy && !self.busy)
                                .then_some(Message::BoosterSaveConfirmed),
                        )
                        .into(),
                    button::text("Cancel")
                        .on_press(Message::BoosterSaveCancelled)
                        .into(),
                ])
                .spacing(8)
                .into(),
            );
        } else {
            items.push(
                button::suggested("Save config")
                    .on_press_maybe(
                        (!self.busy && !self.action_busy).then_some(Message::BoosterSaveRequested),
                    )
                    .into(),
            );
        }

        items.push(text::title3("Regenerate initramfs").into());
        if self.regen_confirm {
            items.push(
                text(format!(
                    "Rebuild the initramfs now? Step: {}.",
                    if self.regenerate_step.is_empty() {
                        "booster build"
                    } else {
                        &self.regenerate_step
                    }
                ))
                .into(),
            );
            items.push(
                row(vec![
                    button::suggested("Confirm regenerate")
                        .on_press_maybe(
                            (!self.action_busy && !self.busy)
                                .then_some(Message::BoosterRegenConfirmed),
                        )
                        .into(),
                    button::text("Cancel")
                        .on_press(Message::BoosterRegenCancelled)
                        .into(),
                ])
                .spacing(8)
                .into(),
            );
        } else {
            items.push(
                button::suggested("Regenerate")
                    .on_press_maybe(
                        (!self.busy && !self.action_busy).then_some(Message::BoosterRegenRequested),
                    )
                    .into(),
            );
        }
        if let Some((step, output)) = &self.output {
            items.push(text(format!("Step: {step}")).into());
            items.push(
                scrollable(container_mono(output))
                    .height(Length::Fixed(220.0))
                    .into(),
            );
        }

        items.push(text::title3("Current file (/etc/booster.yaml)").into());
        if self.raw.trim().is_empty() {
            items.push(monotext("(empty)").into());
        } else {
            items.push(
                scrollable(container_mono(&self.raw))
                    .height(Length::Fixed(200.0))
                    .into(),
            );
        }

        scrollable(column(items).spacing(16).padding(24)).into()
    }
}

fn container_mono(body: &str) -> Element<'_, Message> {
    use cosmic::widget::container;
    container(monotext(body.to_string()))
        .class(cosmic::theme::Container::Background)
        .padding(8)
        .into()
}

fn field_row(key: &'static str, value: String) -> Element<'static, Message> {
    row(vec![
        text(key.to_string()).width(Length::Fixed(180.0)).into(),
        text_input("value", value)
            .on_input(move |next| Message::BoosterField(key, next))
            .width(Length::Fill)
            .into(),
    ])
    .spacing(8)
    .width(Length::Fill)
    .into()
}

/// Map a wire key to its slot in a patch (shared with the GUI diff).
fn patch_field_mut<'a>(patch: &'a mut BoosterConfigPatch, key: &str) -> &'a mut Option<String> {
    match key {
        "compression" => &mut patch.compression,
        "universal" => &mut patch.universal,
        "strip" => &mut patch.strip,
        "vconsole" => &mut patch.vconsole,
        "enable_lvm" => &mut patch.enable_lvm,
        "enable_mdraid" => &mut patch.enable_mdraid,
        "append_all_modaliases" => &mut patch.append_all_modaliases,
        "mount_timeout" => &mut patch.mount_timeout,
        "token_timeout" => &mut patch.token_timeout,
        "pin_delay" => &mut patch.pin_delay,
        "extra_files" => &mut patch.extra_files,
        "modules" => &mut patch.modules,
        "modules_force_load" => &mut patch.modules_force_load,
        "network.dhcp" => &mut patch.network_dhcp,
        "network.ip" => &mut patch.network_ip,
        "network.gateway" => &mut patch.network_gateway,
        "network.dns_servers" => &mut patch.network_dns_servers,
        "network.interfaces" => &mut patch.network_interfaces,
        // FIELDS only contains the keys above.
        _ => unreachable!("unknown booster field key"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_empty_and_unloaded() {
        let state = BoosterState::default();
        assert!(!state.loaded);
        assert_eq!(state.values.len(), FIELDS.len());
        let _ = state.view();
    }

    #[test]
    fn view_renders_confirm_and_output_states() {
        let mut state = BoosterState::default();
        state.apply_config(BoosterConfig {
            exists: true,
            raw: "compression: zstd\n".into(),
            compression: Some("zstd".into()),
            regenerate_step: "apk fix kernel-hooks".into(),
            ..BoosterConfig::default()
        });
        let _ = state.view();
        state.save_confirm = true;
        state.regen_confirm = true;
        state.output = Some(("apk fix kernel-hooks".into(), "hook ran".into()));
        let _ = state.view();
        state.error = Some("boom".into());
        state.message = Some("saved".into());
        let _ = state.view();
    }

    #[test]
    fn patch_diff_sets_changes_and_clears_emptied_keys() {
        let mut state = BoosterState::default();
        state.apply_config(BoosterConfig {
            exists: true,
            compression: Some("zstd".into()),
            network_dhcp: Some("off".into()),
            ..BoosterConfig::default()
        });
        // Unchanged key: not sent. Changed: set. Emptied: cleared.
        state.set_value("compression", "xz".into());
        state.set_value("network.dhcp", String::new());
        let patch = state.build_patch();
        assert_eq!(patch.compression.as_deref(), Some("xz"));
        assert_eq!(patch.network_dhcp, None);
        assert!(patch.clear.iter().any(|key| key == "network.dhcp"));
        assert_eq!(patch.universal, None);
        assert!(!patch.clear.iter().any(|key| key == "universal"));
    }
}
