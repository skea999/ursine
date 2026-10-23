//! Users screen: accountsservice listing and root group/password actions.

use cosmic::iced::Length;
use cosmic::widget::{button, column, row, scrollable, settings, text, text_input, toggler};
use cosmic::Element;
use ursine_api::UserSummary;

use crate::app::Message;

#[derive(Default)]
pub struct UsersState {
    pub users: Vec<UserSummary>,
    pub groups: Vec<String>,
    pub busy: bool,
    pub action_busy: bool,
    pub error: Option<String>,
    pub message: Option<String>,
    pub new_name: String,
    pub new_fullname: String,
    pub new_admin: bool,
    pub selected: Option<u32>,
    pub password: String,
    pub password_confirm: String,
    pub groups_input: String,
    pub pending_delete: Option<UserSummary>,
}

impl UsersState {
    pub fn selected_user(&self) -> Option<&UserSummary> {
        let uid = self.selected?;
        self.users.iter().find(|user| user.uid == uid)
    }

    pub fn select(&mut self, uid: u32) {
        self.selected = Some(uid);
        self.password.clear();
        self.password_confirm.clear();
        self.groups_input = self
            .selected_user()
            .map(|user| user.groups.join(", "))
            .unwrap_or_default();
    }

    pub fn apply(&mut self, users: Vec<UserSummary>, groups: Vec<String>) {
        self.users = users;
        self.groups = groups;
        if let Some(uid) = self.selected {
            if self.users.iter().any(|user| user.uid == uid) {
                self.select(uid);
            } else {
                self.selected = None;
                self.groups_input.clear();
            }
        }
        self.error = None;
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2("Users").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe((!self.busy && !self.action_busy).then_some(Message::RefreshUsers))
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
            items.push(text("Reading accounts…").into());
        } else if self.action_busy {
            items.push(text("Running user action…").into());
        }

        items.push(
            settings::section()
                .title("Create user")
                .add(settings::item(
                    "Name",
                    text_input("alice", self.new_name.clone())
                        .on_input(Message::NewUserNameChanged),
                ))
                .add(settings::item(
                    "Full name",
                    text_input("Alice", self.new_fullname.clone())
                        .on_input(Message::NewUserFullnameChanged),
                ))
                .add(settings::item(
                    "Administrator",
                    toggler(self.new_admin).on_toggle(Message::NewUserAdminToggled),
                ))
                .add(settings::item(
                    "Account is created locked; set a password below.",
                    button::text("Create").on_press_maybe(
                        (!self.action_busy).then_some(Message::CreateUserRequested),
                    ),
                ))
                .into(),
        );

        let mut list = settings::section().title(format!("Accounts ({})", self.users.len()));
        if self.users.is_empty() {
            list = list.add(settings::item("Result", text("No human users reported.")));
        } else {
            for user in &self.users {
                let uid = user.uid;
                let selected = self.selected == Some(uid);
                list = list.add(settings::item(
                    format!(
                        "{} ({}){}",
                        user.name,
                        uid,
                        if user.fullname.is_empty() {
                            String::new()
                        } else {
                            format!(" — {}", user.fullname)
                        }
                    ),
                    button::text(if selected { "Selected" } else { "Select" })
                        .on_press_maybe((!selected).then_some(Message::UserSelected(uid))),
                ));
            }
        }
        items.push(list.into());

        if let Some(user) = self.selected_user() {
            let uid = user.uid;
            let user_name = user.name.clone();
            items.push(
                settings::section()
                    .title(format!("Edit {user_name}"))
                    .add(settings::item(
                        "Shell / home",
                        text(format!("{} · {}", user.shell, user.home)),
                    ))
                    .add(settings::item(
                        "New password",
                        text_input("new password", self.password.clone())
                            .password()
                            .on_input(Message::UserPasswordChanged),
                    ))
                    .add(settings::item(
                        "Confirm password",
                        text_input("repeat password", self.password_confirm.clone())
                            .password()
                            .on_input(Message::UserPasswordConfirmChanged),
                    ))
                    .add(settings::item(
                        "Set password",
                        button::text("Set password").on_press_maybe(
                            (!self.action_busy).then_some(Message::SetPasswordRequested(uid)),
                        ),
                    ))
                    .add(settings::item(
                        "Supplementary groups",
                        text_input("wheel, network", self.groups_input.clone())
                            .on_input(Message::UserGroupsChanged),
                    ))
                    .add(settings::item(
                        format!("Known groups: {}", self.groups.join(", ")),
                        button::text("Save groups").on_press_maybe(
                            (!self.action_busy).then_some(Message::SetGroupsRequested(uid)),
                        ),
                    ))
                    .add(settings::item(
                        "Delete account",
                        button::destructive("Delete…").on_press_maybe(
                            (!self.action_busy).then_some(Message::DeleteUserRequested(uid)),
                        ),
                    ))
                    .into(),
            );
        }

        if let Some(user) = &self.pending_delete {
            items.push(
                settings::section()
                    .title("Confirm account deletion")
                    .add(settings::item(
                        "Consequences",
                        text(format!(
                            "Delete {} ({}) and the account's home directory? This cannot be undone.",
                            user.name, user.uid
                        )),
                    ))
                    .add(settings::item(
                        "Actions",
                        row(vec![
                            button::destructive("Delete account")
                                .on_press_maybe(
                                    (!self.action_busy).then_some(Message::DeleteUserConfirmed),
                                )
                                .into(),
                            button::text("Cancel")
                                .on_press(Message::DeleteUserCancelled)
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
