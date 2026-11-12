//! Login screen: credentials in, session token out.

use cosmic::iced::Length;
use cosmic::widget::selectable_text;
use cosmic::widget::{button, column, container, scrollable, text, text_input};
use cosmic::Element;

use crate::app::Message;

pub struct LoginState {
    pub user: String,
    pub password: String,
    pub busy: bool,
    pub error: Option<String>,
    /// Password field id: focused on entry so typing works immediately.
    pub password_id: cosmic::widget::Id,
}

impl Default for LoginState {
    fn default() -> Self {
        Self {
            user: default_user(),
            password: String::new(),
            busy: false,
            error: None,
            password_id: cosmic::widget::Id::unique(),
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
                .id(self.password_id.clone())
                .on_input(Message::PasswordChanged)
                .on_submit(|_| Message::SignIn)
                .into(),
        ];

        if let Some(error) = &self.error {
            items.push(selectable_text(error.clone()).into());
        }
        if self.busy {
            items.push(text("Signing in…").into());
        }

        let submit =
            button::suggested("Sign in").on_press_maybe((!self.busy).then_some(Message::SignIn));
        items.push(submit.into());

        // Scrollable so a long error line can never clip the form on a
        // short window (hides itself when everything fits).
        container(scrollable(column(items).spacing(12).max_width(420)).width(Length::Fill))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(24)
            .into()
    }
}
