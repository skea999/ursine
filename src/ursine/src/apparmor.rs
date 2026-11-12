//! `AppArmor` screen (SPEC-apparmor): status, profile modes, the source
//! editor (syntax-checked save) and the notifications button.

use cosmic::iced::Length;
use cosmic::widget::selectable_text;
use cosmic::widget::text::monotext;
use cosmic::widget::text_editor::{self, text_editor};
use cosmic::widget::{button, column, container, row, scrollable, text, text_input};
use cosmic::Element;
use ursine_api::{ApparmorCheck, ApparmorProfileFile, ApparmorView};

use crate::app::Message;

/// A confirmed mutation (drives the confirm blocks and the daemon call).
#[derive(Debug, Clone)]
pub enum ApparmorOp {
    SetMode { path: String, mode: String },
    Load { path: String },
    Remove { path: String },
    ReloadAll,
    RemoveUnknown,
    Save { path: String },
    NotificationsEnable,
    Cleanprof { name: String },
    Autodep { path: String },
}

impl ApparmorOp {
    pub fn confirm_text(&self) -> String {
        match self {
            Self::SetMode { path, mode } if *mode == "disable" => {
                format!("Disable {path}? The profile is unloaded and skipped by reloads.")
            }
            Self::SetMode { path, mode } => format!("Set {path} to {mode}?"),
            Self::Load { path } => format!("Apply {path}? Reloads it into the kernel."),
            Self::Remove { path } => format!("Unload the profiles of {path}?"),
            Self::ReloadAll => "Reload every profile under /etc/apparmor.d?".to_string(),
            Self::RemoveUnknown => {
                "Remove loaded profiles that have no backing file? This cannot be undone."
                    .to_string()
            }
            Self::Save { path } => format!(
                "Write {path}? The content was syntax-checked; the current file rotates into the backup slots first."
            ),
            Self::NotificationsEnable => {
                "Enable AppArmor DENIED notifications? Installs the aa-notify autostart \
                entry — active from the next login."
                    .to_string()
            }
            Self::Cleanprof { name } => format!(
                "Clean {name}? Superfluous rules and ALL comments are removed from the file \
                (backed up first) and the profile reloads."
            ),
            Self::Autodep { path } => format!("Add hardlink dependencies to {path}?"),
        }
    }
}

/// A read-only query (raw output box).
#[derive(Debug, Clone)]
pub enum ApparmorQuery {
    Unconfined,
    FeaturesAbi,
    Decode(String),
    /// Generate a profile to stdout (aa-easyprof, never writes).
    Easyprof {
        binary: String,
        template: Option<String>,
        policy_groups: Option<String>,
    },
    EasyprofList(String),
}

#[derive(Debug)]
pub struct ApparmorState {
    pub busy: bool,
    pub action_busy: bool,
    pub loaded: bool,
    pub view: Option<ApparmorView>,
    /// Selected profile file path (editor + action bar target).
    pub selected: Option<String>,
    pub editor: text_editor::Content<cosmic::Renderer>,
    pub editor_path: Option<String>,
    pub editor_body: String,
    /// Mutation waiting for confirmation.
    pub pending: Option<ApparmorOp>,
    /// (label, text) of the last check/report output.
    pub output: Option<(String, String)>,
    pub decode_input: String,
    pub easyprof_binary: String,
    pub easyprof_template: String,
    pub easyprof_groups: String,
    pub error: Option<String>,
    pub message: Option<String>,
}

impl Default for ApparmorState {
    fn default() -> Self {
        Self {
            busy: false,
            action_busy: false,
            loaded: false,
            view: None,
            selected: None,
            editor: text_editor::Content::new(),
            editor_path: None,
            editor_body: String::new(),
            pending: None,
            output: None,
            decode_input: String::new(),
            easyprof_binary: String::new(),
            easyprof_template: String::new(),
            easyprof_groups: String::new(),
            error: None,
            message: None,
        }
    }
}

impl ApparmorState {
    pub fn apply_view(&mut self, view: ApparmorView) {
        self.view = Some(view);
        self.loaded = true;
        self.pending = None;
        self.action_busy = false;
        self.busy = false;
    }

    pub fn apply_profile(&mut self, file: ApparmorProfileFile) {
        self.editor = text_editor::Content::with_text(&file.content);
        self.editor_body = file.content;
        self.editor_path = Some(file.path);
        self.action_busy = false;
    }

    pub fn apply_check(&mut self, check: ApparmorCheck) {
        let label = if check.ok {
            "Syntax OK".to_string()
        } else {
            "Syntax check FAILED".to_string()
        };
        self.output = Some((label, check.output));
        self.action_busy = false;
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2("AppArmor").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe((!self.busy && !self.action_busy).then_some(Message::OpenApparmor))
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
            items.push(text("Reading AppArmor state…").into());
            return scrollable(column(items).spacing(16).padding(24)).into();
        }
        let Some(view) = &self.view else {
            items.push(text("AppArmor state could not be loaded.").into());
            return scrollable(column(items).spacing(16).padding(24)).into();
        };

        // ---- status card ----
        items.push(text::title3("Status").into());
        items.push(
            text(if view.enabled {
                "Kernel module: loaded"
            } else {
                "Kernel module: NOT loaded — add apparmor=1 security=apparmor to the kernel cmdline"
            })
            .into(),
        );
        items.push(
            text(if view.mounted {
                "securityfs: mounted"
            } else {
                "securityfs: NOT mounted"
            })
            .into(),
        );
        let enforce = view
            .profiles
            .iter()
            .filter(|profile| profile.mode == "enforce")
            .count();
        items.push(
            text(format!(
                "{} profiles loaded ({enforce} enforce, {} complain)",
                view.profiles.len(),
                view.profiles.len() - enforce
            ))
            .into(),
        );
        items.push(
            text(format!(
                "Processes: {} defined, {} enforce, {} complain",
                view.processes_defined, view.processes_enforce, view.processes_complain
            ))
            .into(),
        );

        // ---- notifications ----
        let notif_ready = view.notifications_configured && view.notifications_autostart;
        items.push(
            text(format!(
                "Notifications: {} (autostart: {})",
                if view.notifications_configured {
                    "configured"
                } else {
                    "off"
                },
                if view.notifications_autostart {
                    "installed"
                } else {
                    "not installed"
                }
            ))
            .into(),
        );
        if matches!(&self.pending, Some(ApparmorOp::NotificationsEnable)) {
            items.push(text(self.pending_text()).into());
            items.push(confirm_row(!self.action_busy));
        } else if !notif_ready {
            // No on_press when the tool is missing: an unbound button
            // renders disabled.
            let button = if view.aa_notify_present {
                button::suggested("Enable notifications").on_press_maybe(
                    (!self.busy && !self.action_busy).then_some(Message::ApparmorOpRequested(
                        ApparmorOp::NotificationsEnable,
                    )),
                )
            } else {
                button::suggested("Enable notifications")
            };
            items.push(button.into());
            if !view.aa_notify_present {
                items.push(text("aa-notify not found — apk add apparmor-utils").into());
            }
        }

        // ---- file table ----
        items.push(text::title3("Profiles (/etc/apparmor.d)").into());
        if view.files.is_empty() {
            items.push(text("No profile files found.").into());
        }
        for file in &view.files {
            let mode = file.mode.as_deref().unwrap_or("not loaded");
            let flags = format!("{mode}{}", if file.disabled { ", disabled" } else { "" });
            let selected = self.selected.as_deref() == Some(file.path.as_str());
            let pick = if selected {
                button::suggested(file.name.clone())
            } else {
                button::text(file.name.clone())
            }
            .on_press_maybe(
                (!self.busy && !self.action_busy)
                    .then_some(Message::ApparmorFileSelected(file.path.clone())),
            );
            items.push(
                row(vec![pick.into(), text(flags).into()])
                    .spacing(8)
                    .width(Length::Fill)
                    .into(),
            );
        }

        // ---- selected-file action bar ----
        if let Some(path) = &self.selected {
            items.push(text::title3("Selected file actions").into());
            items.push(
                row(vec![
                    mode_button("Enforce", path, "enforce", !self.action_busy && !self.busy),
                    mode_button(
                        "Complain",
                        path,
                        "complain",
                        !self.action_busy && !self.busy,
                    ),
                    mode_button("Disable", path, "disable", !self.action_busy && !self.busy),
                ])
                .spacing(8)
                .into(),
            );
            let disabled_row = self
                .view
                .as_ref()
                .and_then(|view| view.files.iter().find(|f| f.path == *path))
                .is_some_and(|file| file.disabled);
            let mut second: Vec<Element<'_, Message>> = vec![
                small_button(
                    "Check",
                    (!self.action_busy && !self.busy)
                        .then_some(Message::ApparmorQueryCheck(path.clone())),
                ),
                small_button(
                    "Apply (load)",
                    (!self.action_busy && !self.busy).then_some(Message::ApparmorOpRequested(
                        ApparmorOp::Load { path: path.clone() },
                    )),
                ),
                small_button(
                    "Unload",
                    (!self.action_busy && !self.busy).then_some(Message::ApparmorOpRequested(
                        ApparmorOp::Remove { path: path.clone() },
                    )),
                ),
            ];
            if disabled_row {
                second.push(small_button(
                    "Enable",
                    (!self.action_busy && !self.busy).then_some(Message::ApparmorOpRequested(
                        ApparmorOp::SetMode {
                            path: path.clone(),
                            mode: "enable".into(),
                        },
                    )),
                ));
            }
            items.push(row(second).spacing(8).into());
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            items.push(
                row(vec![
                    small_button(
                        "Cleanprof",
                        (!self.action_busy && !self.busy).then_some(Message::ApparmorOpRequested(
                            ApparmorOp::Cleanprof { name },
                        )),
                    ),
                    small_button(
                        "Autodep",
                        (!self.action_busy && !self.busy).then_some(Message::ApparmorOpRequested(
                            ApparmorOp::Autodep { path: path.clone() },
                        )),
                    ),
                ])
                .spacing(8)
                .into(),
            );
        }

        // ---- global actions ----
        items.push(text::title3("Global").into());
        items.push(
            row(vec![
                small_button(
                    "Reload all",
                    (!self.action_busy && !self.busy)
                        .then_some(Message::ApparmorOpRequested(ApparmorOp::ReloadAll)),
                ),
                small_button(
                    "Remove unknown",
                    (!self.action_busy && !self.busy)
                        .then_some(Message::ApparmorOpRequested(ApparmorOp::RemoveUnknown)),
                ),
                small_button(
                    "Unconfined",
                    (!self.action_busy && !self.busy)
                        .then_some(Message::ApparmorQueryRequested(ApparmorQuery::Unconfined)),
                ),
                small_button(
                    "Features ABI",
                    (!self.action_busy && !self.busy)
                        .then_some(Message::ApparmorQueryRequested(ApparmorQuery::FeaturesAbi)),
                ),
            ])
            .spacing(8)
            .into(),
        );
        items.push(
            row(vec![
                text("Decode hex:").into(),
                text_input("2F746D70", &self.decode_input)
                    .on_input(Message::ApparmorDecodeChanged)
                    .width(Length::Fill)
                    .into(),
                small_button(
                    "Decode",
                    (!self.action_busy && !self.busy && !self.decode_input.trim().is_empty())
                        .then_some(Message::ApparmorQueryRequested(ApparmorQuery::Decode(
                            self.decode_input.trim().to_string(),
                        ))),
                ),
            ])
            .spacing(8)
            .width(Length::Fill)
            .into(),
        );

        // ---- easyprof generation (stdout only — nothing is written) ----
        items.push(text::title3("Easyprof (generate to output)").into());
        items.push(
            row(vec![
                text("Binary:").into(),
                text_input("/usr/bin/app", &self.easyprof_binary)
                    .on_input(|value| Message::ApparmorEasyprofChanged("binary", value))
                    .width(Length::Fill)
                    .into(),
            ])
            .spacing(8)
            .width(Length::Fill)
            .into(),
        );
        items.push(
            row(vec![
                text("Template:").into(),
                text_input("user-application", &self.easyprof_template)
                    .on_input(|value| Message::ApparmorEasyprofChanged("template", value))
                    .width(Length::Fill)
                    .into(),
                text("Policy groups:").into(),
                text_input("networking", &self.easyprof_groups)
                    .on_input(|value| Message::ApparmorEasyprofChanged("groups", value))
                    .width(Length::Fill)
                    .into(),
            ])
            .spacing(8)
            .width(Length::Fill)
            .into(),
        );
        items.push(
            row(vec![
                small_button(
                    "Generate",
                    (!self.action_busy && !self.busy && !self.easyprof_binary.trim().is_empty())
                        .then_some(Message::ApparmorQueryRequested(ApparmorQuery::Easyprof {
                            binary: self.easyprof_binary.trim().to_string(),
                            template: non_empty(&self.easyprof_template),
                            policy_groups: non_empty(&self.easyprof_groups),
                        })),
                ),
                small_button(
                    "List templates",
                    (!self.action_busy && !self.busy).then_some(Message::ApparmorQueryRequested(
                        ApparmorQuery::EasyprofList("templates".into()),
                    )),
                ),
                small_button(
                    "List policy groups",
                    (!self.action_busy && !self.busy).then_some(Message::ApparmorQueryRequested(
                        ApparmorQuery::EasyprofList("policy_groups".into()),
                    )),
                ),
            ])
            .spacing(8)
            .into(),
        );

        // ---- confirm block for a pending mutation ----
        if let Some(pending) = &self.pending {
            items.push(text::title3("Confirm").into());
            items.push(text(pending.confirm_text()).into());
            items.push(confirm_row(!self.action_busy));
        }

        // ---- output box ----
        if let Some((label, output)) = &self.output {
            items.push(text::title3(label.clone()).into());
            let body = if output.trim().is_empty() {
                "(no output)".to_string()
            } else {
                output.clone()
            };
            items.push(
                scrollable(
                    container(monotext(body))
                        .padding(8)
                        .class(cosmic::theme::Container::Background),
                )
                .height(Length::Fixed(200.0))
                .into(),
            );
        }

        // ---- editor ----
        items.push(text::title3("Edit profile").into());
        match &self.editor_path {
            Some(path) => items.push(text(format!("File: {path}")).into()),
            None => items.push(text("Select a profile file above to edit it.").into()),
        }
        items.push(
            text_editor(&self.editor)
                .on_action(|_action| Message::ApparmorEdited)
                .height(Length::Fixed(320.0))
                .into(),
        );
        if let Some(ApparmorOp::Save { .. }) = &self.pending {
            // confirm block already rendered above
        } else {
            items.push(
                button::suggested("Save")
                    .on_press_maybe(
                        (self.editor_path.is_some() && !self.busy && !self.action_busy).then_some(
                            Message::ApparmorOpRequested(ApparmorOp::Save {
                                path: self.editor_path.clone().unwrap_or_default(),
                            }),
                        ),
                    )
                    .into(),
            );
            items.push(
                text("Saving never applies the change — use Apply (load) after a successful save.")
                    .into(),
            );
        }

        scrollable(column(items).spacing(16).padding(24)).into()
    }

    fn pending_text(&self) -> String {
        self.pending
            .as_ref()
            .map(ApparmorOp::confirm_text)
            .unwrap_or_default()
    }
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn confirm_row(armed: bool) -> Element<'static, Message> {
    row(vec![
        button::suggested("Confirm")
            .on_press_maybe(armed.then_some(Message::ApparmorOpConfirmed))
            .into(),
        button::text("Cancel")
            .on_press(Message::ApparmorOpCancelled)
            .into(),
    ])
    .spacing(8)
    .into()
}

fn mode_button(
    label: &'static str,
    path: &str,
    mode: &'static str,
    armed: bool,
) -> Element<'static, Message> {
    button::text(label)
        .on_press_maybe(
            armed.then_some(Message::ApparmorOpRequested(ApparmorOp::SetMode {
                path: path.to_string(),
                mode: mode.to_string(),
            })),
        )
        .into()
}

fn small_button(label: &'static str, message: Option<Message>) -> Element<'static, Message> {
    button::text(label).on_press_maybe(message).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_empty() {
        let state = ApparmorState::default();
        assert!(!state.loaded);
        assert!(state.view.is_none());
        let _ = state.view();
    }

    #[test]
    fn view_renders_status_editor_and_confirms() {
        let mut state = ApparmorState::default();
        state.apply_view(ApparmorView {
            enabled: true,
            mounted: true,
            aa_notify_present: true,
            ..ApparmorView::default()
        });
        let _ = state.view();
        state.selected = Some("/etc/apparmor.d/usr.bin.foo".into());
        state.pending = Some(ApparmorOp::Save {
            path: "/etc/apparmor.d/usr.bin.foo".into(),
        });
        let _ = state.view();
        state.apply_check(ApparmorCheck {
            ok: false,
            output: "syntax error".into(),
        });
        state.output = Some(("Syntax check FAILED".into(), "syntax error".into()));
        state.error = Some("boom".into());
        state.message = Some("saved".into());
        let _ = state.view();
    }

    #[test]
    fn confirm_texts_name_the_risk() {
        assert!(ApparmorOp::RemoveUnknown
            .confirm_text()
            .contains("cannot be undone"));
        assert!(ApparmorOp::NotificationsEnable
            .confirm_text()
            .contains("next login"));
        let save = ApparmorOp::Save {
            path: "/etc/apparmor.d/x".into(),
        };
        assert!(save.confirm_text().contains("syntax-checked"));
    }
}
