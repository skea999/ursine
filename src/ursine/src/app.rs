//! Application state, message loop and screen routing.

use cosmic::{Element, Task};
use ursine_api::{Action, ApiError, CapabilityReport, ErrorCode, ModuleId, SessionToken, TriState};

use crate::client::{self, AuthSession, ConfigSnapshot};
use crate::home::HomeState;
use crate::login::LoginState;
use crate::settings::SettingsState;
use crate::store::StoreState;

/// Live session. The token stays in RAM: never written to disk, never logged.
struct Session {
    token: SessionToken,
    expires_in_secs: u64,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Screen {
    #[default]
    Home,
    Settings,
    Store,
}

pub struct App {
    core: cosmic::Core,
    login: LoginState,
    home: HomeState,
    settings: SettingsState,
    store: StoreState,
    screen: Screen,
    session: Option<Session>,
}

#[derive(Debug, Clone)]
pub enum Message {
    UserChanged(String),
    PasswordChanged(String),
    SignIn,
    SignInResult(Result<AuthSession, ApiError>),
    CapabilitiesLoaded(Result<Vec<CapabilityReport>, ApiError>),
    RefreshCapabilities,
    OpenSettings,
    ConfigLoaded(Result<ConfigSnapshot, ApiError>),
    SetModuleSwitch(ModuleId, TriState),
    ModuleSwitchSaved(Result<ConfigSnapshot, ApiError>),
    OpenStore,
    BackHome,
    StoreLoaded(Result<client::StoreSnapshot, ApiError>),
    RefreshStore,
    SignOut,
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "org.feralos.Ursine";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let app = Self {
            core,
            login: LoginState::default(),
            home: HomeState::default(),
            settings: SettingsState::default(),
            store: StoreState::default(),
            screen: Screen::Home,
            session: None,
        };
        (app, Task::none())
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::UserChanged(user) => self.login.user = user,
            Message::PasswordChanged(password) => self.login.password = password,
            Message::SignIn => return self.sign_in(),
            Message::SignInResult(result) => return self.finish_sign_in(result),
            Message::CapabilitiesLoaded(result) => self.finish_capabilities(result),
            Message::RefreshCapabilities => return self.refresh_capabilities(),
            Message::OpenSettings => return self.open_settings(),
            Message::ConfigLoaded(result) => self.finish_config(result),
            Message::SetModuleSwitch(module, value) => {
                return self.set_module_switch(module, value)
            }
            Message::ModuleSwitchSaved(result) => self.finish_config(result),
            Message::OpenStore => {
                self.screen = Screen::Store;
                return self.load_store();
            }
            Message::BackHome => self.screen = Screen::Home,
            Message::StoreLoaded(result) => return self.finish_store(result),
            Message::RefreshStore => return self.load_store(),
            Message::SignOut => self.sign_out(),
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        match &self.session {
            Some(session) => match self.screen {
                Screen::Home => self.home.view(
                    session.expires_in_secs,
                    self.settings.config.as_ref().map(|config| &config.modules),
                ),
                Screen::Settings => {
                    self.settings
                        .view(&self.home.caps, self.home.busy, self.home.error.as_deref())
                }
                Screen::Store => self.store.view(),
            },
            None => self.login.view(),
        }
    }
}

impl App {
    fn sign_in(&mut self) -> Task<cosmic::Action<Message>> {
        if self.login.busy {
            return Task::none();
        }
        let user = self.login.user.trim().to_string();
        if user.is_empty() {
            self.login.error = Some("Enter a user name.".to_string());
            return Task::none();
        }

        self.login.busy = true;
        self.login.error = None;
        let password = self.login.password.clone();

        Task::future(async move { Message::SignInResult(client::auth(user, password).await) })
            .map(cosmic::Action::App)
    }

    fn finish_sign_in(
        &mut self,
        result: Result<AuthSession, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        self.login.busy = false;

        match result {
            Ok(auth) => {
                let user = self.login.user.trim().to_string();
                // The password is no longer needed once the daemon issued a token.
                self.login.password.clear();
                self.login.error = None;
                self.screen = Screen::Home;
                self.store = StoreState::default();
                self.home.user.clone_from(&user);
                self.home.busy = true;
                self.home.error = None;

                let token = auth.token.clone();
                self.session = Some(Session {
                    token: auth.token,
                    expires_in_secs: auth.expires_in_secs,
                });
                Task::batch(vec![
                    self.load_capabilities(token, Action::List),
                    self.load_modules_config(),
                ])
            }
            Err(error) => {
                // Do not retain a rejected password while the user reads error.
                self.login.password.clear();
                self.login.error = Some(friendly(&error));
                Task::none()
            }
        }
    }

    fn load_capabilities(
        &self,
        token: SessionToken,
        action: Action,
    ) -> Task<cosmic::Action<Message>> {
        Task::future(async move {
            match client::call(token, ModuleId::Capabilities, action, None).await {
                Ok(value) => match serde_json::from_value::<Vec<CapabilityReport>>(value) {
                    Ok(reports) => Message::CapabilitiesLoaded(Ok(reports)),
                    Err(error) => Message::CapabilitiesLoaded(Err(ApiError::internal(format!(
                        "cannot read the capability report: {error}"
                    )))),
                },
                Err(error) => Message::CapabilitiesLoaded(Err(error)),
            }
        })
        .map(cosmic::Action::App)
    }

    fn refresh_capabilities(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.home.busy {
            return Task::none();
        }
        self.home.busy = true;
        self.home.error = None;
        self.load_capabilities(session.token.clone(), Action::Refresh)
    }

    fn finish_capabilities(&mut self, result: Result<Vec<CapabilityReport>, ApiError>) {
        self.home.busy = false;
        match result {
            Ok(reports) => {
                self.home.caps = reports;
                self.home.error = None;
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return;
                }
                self.home.error = Some(friendly(&error));
            }
        }
    }

    fn open_settings(&mut self) -> Task<cosmic::Action<Message>> {
        self.screen = Screen::Settings;
        let mut tasks = vec![self.load_modules_config()];
        if self.home.caps.is_empty() && !self.home.busy {
            self.home.busy = true;
            if let Some(session) = &self.session {
                tasks.push(self.load_capabilities(session.token.clone(), Action::List));
            }
        }
        Task::batch(tasks)
    }

    fn load_modules_config(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.settings.config_busy {
            return Task::none();
        }
        self.settings.config_busy = true;
        self.settings.error = None;
        let token = session.token.clone();
        Task::future(async move { Message::ConfigLoaded(client::modules_config(token).await) })
            .map(cosmic::Action::App)
    }

    fn set_module_switch(
        &mut self,
        module: ModuleId,
        value: TriState,
    ) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.settings.config_busy {
            return Task::none();
        }
        self.settings.config_busy = true;
        self.settings.error = None;
        let token = session.token.clone();
        Task::future(async move {
            Message::ModuleSwitchSaved(client::set_module_switch(token, module, value).await)
        })
        .map(cosmic::Action::App)
    }

    fn finish_config(&mut self, result: Result<ConfigSnapshot, ApiError>) {
        self.settings.config_busy = false;
        match result {
            Ok(snapshot) => {
                self.settings.config = Some(snapshot);
                self.settings.error = None;
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return;
                }
                self.settings.error = Some(friendly(&error));
            }
        }
    }

    fn load_store(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.store.busy {
            return Task::none();
        }
        self.store.busy = true;
        self.store.error = None;
        let token = session.token.clone();
        Task::future(async move { Message::StoreLoaded(client::store_snapshot(token).await) })
            .map(cosmic::Action::App)
    }

    fn finish_store(
        &mut self,
        result: Result<client::StoreSnapshot, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        self.store.busy = false;
        match result {
            Ok(snapshot) => {
                self.store.packages = snapshot.packages;
                self.store.generations = snapshot.generations;
                self.store.error = None;
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.store.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    /// Drop a dead session and return to login with an actionable message.
    fn handle_session_error(&mut self, error: &ApiError) -> bool {
        if error.code != ErrorCode::Unauthorized {
            return false;
        }
        self.sign_out();
        self.login.error = Some("Session expired. Sign in again.".to_string());
        true
    }

    fn sign_out(&mut self) {
        self.session = None;
        self.home = HomeState::default();
        self.settings = SettingsState::default();
        self.store = StoreState::default();
        self.screen = Screen::Home;
        self.login.password.clear();
        self.login.error = None;
    }
}

/// Turn a daemon error into something a human can act on.
fn friendly(error: &ApiError) -> String {
    match error.code {
        ErrorCode::Unauthorized => "Invalid user name or password.".to_string(),
        ErrorCode::Forbidden => {
            "This account may not administer the system (wheel group required).".to_string()
        }
        ErrorCode::RateLimited => "Too many attempts. Wait a minute, then try again.".to_string(),
        _ => match &error.fix {
            Some(fix) => format!("{} ({fix})", error.message),
            None => error.message.clone(),
        },
    }
}
