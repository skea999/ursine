//! Login screen: credentials in, session token out.

use cosmic::iced::Length;
use cosmic::widget::{button, column, container, text, text_input};
use cosmic::Element;

use crate::app::Message;

pub struct LoginState {
    pub user: String,
    pub password: String,
    pub busy: bool,
    pub error: Option<String>,
}

impl Default for LoginState {
    fn default() -> Self {
        Self {
            user: default_user(),
            password: String::new(),
            busy: false,
            error: None,
        }
    }
}

/// Best guess at the interactive user; the daemon verifies it via PAM anyway.
fn default_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_default()
}

impl LoginState {
    pub fn view(&self) -> Element<'_, Message> {
        let mut items = vec![
            text::title1("Ursine").into(),
            text("Sign in to administer this machine.").into(),
            text_input("User name", self.user.clone())
                .on_input(Message::UserChanged)
                .on_submit(|_| Message::SignIn)
                .into(),
            text_input("Password", self.password.clone())
                .password()
                .on_input(Message::PasswordChanged)
                .on_submit(|_| Message::SignIn)
                .into(),
        ];

        if let Some(error) = &self.error {
            items.push(text(error.clone()).into());
        }
        if self.busy {
            items.push(text("Signing in…").into());
        }

        let submit =
            button::suggested("Sign in").on_press_maybe((!self.busy).then_some(Message::SignIn));
        items.push(submit.into());

        container(column(items).spacing(12).max_width(420))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(24)
            .into()
    }
}
