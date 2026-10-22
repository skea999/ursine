//! Application state, message loop and screen routing.

use cosmic::{Element, Task};
use ursine_api::{
    validate_installable, Action, ApiError, CapabilityReport, ErrorCode, ModuleId, SessionToken,
    TriState,
};

use crate::client::{self, AuthSession, ConfigSnapshot, SearchPage};
use crate::home::HomeState;
use crate::login::LoginState;
use crate::settings::SettingsState;
use crate::store::{PendingStoreAction, StoreAction, StoreScope, StoreState};

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
    /// Bumped on sign-in/sign-out so late async results cannot repopulate a
    /// different session's screens.
    epoch: u64,
}

#[derive(Debug, Clone)]
pub enum Message {
    UserChanged(String),
    PasswordChanged(String),
    SignIn,
    SignInResult(Result<AuthSession, ApiError>),
    CapabilitiesLoaded(u64, Result<Vec<CapabilityReport>, ApiError>),
    RefreshCapabilities,
    StatusLoaded(u64, Result<client::SystemStatus, ApiError>),
    OpenSettings,
    ConfigLoaded(u64, Result<ConfigSnapshot, ApiError>),
    InitConfig,
    ConfigInitialized(u64, Result<ConfigSnapshot, ApiError>),
    SetModuleSwitch(ModuleId, TriState),
    ModuleSwitchSaved(u64, Result<ConfigSnapshot, ApiError>),
    OpenStore,
    BackHome,
    StoreLoaded(u64, StoreScope, Result<client::StoreSnapshot, ApiError>),
    RefreshStore,
    StoreAsUserChanged(String),
    StoreSearchChanged(String),
    StoreSearchSubmitted,
    StoreSearchLoaded(u64, u64, StoreScope, Result<SearchPage, ApiError>),
    StoreInstallChanged(String),
    StoreRegexChanged(String),
    StoreGenerationChanged(String),
    StoreKeepChanged(String),
    StoreHistoryAgeChanged(String),
    StoreActionRequested(StoreAction),
    StoreActionConfirmed,
    StoreConfirmCancelled,
    StoreActionFinished(
        u64,
        StoreAction,
        StoreScope,
        Result<serde_json::Value, ApiError>,
    ),
    StoreRollbackRequested,
    StoreDiffRequested,
    StorePrunePreview,
    StorePruneApply,
    StoreWipePreview,
    StoreWipeApply,
    /// Redraw tick while a long daemon action is running.
    StoreTick,
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
            epoch: 0,
        };
        (app, Task::none())
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::UserChanged(user) => self.login.user = user,
            Message::PasswordChanged(password) => self.login.password = password,
            Message::SignIn => return self.sign_in(),
            Message::SignInResult(result) => return self.finish_sign_in(result),
            Message::CapabilitiesLoaded(epoch, result) => self.finish_capabilities(epoch, result),
            Message::RefreshCapabilities => return self.refresh_capabilities(),
            Message::StatusLoaded(epoch, result) => self.finish_status(epoch, result),
            Message::OpenSettings => return self.open_settings(),
            Message::ConfigLoaded(epoch, result) | Message::ConfigInitialized(epoch, result) => {
                self.finish_config(epoch, result);
            }
            Message::InitConfig => return self.init_config(),
            Message::SetModuleSwitch(module, value) => {
                return self.set_module_switch(module, value)
            }
            Message::ModuleSwitchSaved(epoch, result) => self.finish_config(epoch, result),
            Message::OpenStore => {
                self.screen = Screen::Store;
                return self.load_store();
            }
            Message::BackHome => self.screen = Screen::Home,
            Message::StoreLoaded(epoch, scope, result) => {
                return self.finish_store(epoch, scope, result)
            }
            Message::RefreshStore => return self.load_store(),
            Message::StoreAsUserChanged(value) => self.store.set_target_user(value),
            Message::StoreSearchChanged(query) => self.store.search_query = query,
            Message::StoreSearchSubmitted => return self.search_store(),
            Message::StoreSearchLoaded(epoch, seq, scope, result) => {
                self.finish_store_search(epoch, seq, &scope, result);
            }
            Message::StoreInstallChanged(value) => self.store.install_input = value,
            Message::StoreRegexChanged(value) => self.store.regex = value,
            Message::StoreGenerationChanged(value) => self.store.generation_input = value,
            Message::StoreKeepChanged(value) => {
                // A different keep count needs its own preview.
                self.store.keep_input = value;
                self.store.prune_preview = None;
            }
            Message::StoreHistoryAgeChanged(value) => self.store.history_age = value,
            Message::StoreActionRequested(action) => return self.request_store_action(action),
            Message::StoreActionConfirmed => {
                if let Some(PendingStoreAction { action, scope }) = self.store.pending.clone() {
                    // Keep the pending action on a failed precondition: the
                    // dialog must survive so the user can retry it.
                    if !self.store.can_run(&action) {
                        self.store.error =
                            Some("Wait for the profile load to finish, then confirm again.".into());
                        return Task::none();
                    }
                    if action.uses_profile() && !scope.matches(self.store.target_user().as_deref())
                    {
                        self.store.error =
                            Some("Target user changed. Refresh and confirm again.".into());
                        return Task::none();
                    }
                    self.store.pending = None;
                    return self.run_store_action(action, scope);
                }
            }
            Message::StoreConfirmCancelled => self.store.pending = None,
            Message::StoreActionFinished(epoch, action, scope, result) => {
                return self.finish_store_action(epoch, &action, &scope, result)
            }
            Message::StoreRollbackRequested => return self.request_rollback(),
            Message::StoreDiffRequested => return self.request_diff(),
            Message::StorePrunePreview => return self.request_prune(true),
            Message::StorePruneApply => return self.request_prune(false),
            Message::StoreWipePreview => return self.request_wipe(true),
            Message::StoreWipeApply => return self.request_wipe(false),
            Message::StoreTick => {}
            Message::SignOut => self.sign_out(),
        }
        Task::none()
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Message> {
        if self.store.action_busy {
            cosmic::iced::time::every(std::time::Duration::from_secs(1)).map(|_| Message::StoreTick)
        } else {
            cosmic::iced::Subscription::none()
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        self.session.as_ref().map_or_else(
            || self.login.view(),
            |session| match self.screen {
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
        )
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
                self.epoch = self.epoch.wrapping_add(1);
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
                    self.load_capabilities(token.clone(), Action::List),
                    self.load_status(token),
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
        let epoch = self.epoch;
        Task::future(async move {
            match client::call(token, ModuleId::Capabilities, action, None).await {
                Ok(value) => match serde_json::from_value::<Vec<CapabilityReport>>(value) {
                    Ok(reports) => Message::CapabilitiesLoaded(epoch, Ok(reports)),
                    Err(error) => Message::CapabilitiesLoaded(
                        epoch,
                        Err(ApiError::internal(format!(
                            "cannot read the capability report: {error}"
                        ))),
                    ),
                },
                Err(error) => Message::CapabilitiesLoaded(epoch, Err(error)),
            }
        })
        .map(cosmic::Action::App)
    }

    fn load_status(&self, token: SessionToken) -> Task<cosmic::Action<Message>> {
        let epoch = self.epoch;
        Task::future(
            async move { Message::StatusLoaded(epoch, client::system_status(token).await) },
        )
        .map(cosmic::Action::App)
    }

    fn finish_status(&mut self, epoch: u64, result: Result<client::SystemStatus, ApiError>) {
        if epoch != self.epoch {
            return;
        }
        match result {
            Ok(status) => self.home.status = Some(status),
            Err(error) => {
                if !self.handle_session_error(&error) {
                    self.home.error = Some(friendly(&error));
                }
            }
        }
    }

    fn init_config(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.settings.config_busy {
            return Task::none();
        }
        self.settings.config_busy = true;
        self.settings.error = None;
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(
            async move { Message::ConfigInitialized(epoch, client::init_config(token).await) },
        )
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

    fn finish_capabilities(&mut self, epoch: u64, result: Result<Vec<CapabilityReport>, ApiError>) {
        if epoch != self.epoch {
            return;
        }
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
        let epoch = self.epoch;
        Task::future(
            async move { Message::ConfigLoaded(epoch, client::modules_config(token).await) },
        )
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
        let epoch = self.epoch;
        Task::future(async move {
            Message::ModuleSwitchSaved(epoch, client::set_module_switch(token, module, value).await)
        })
        .map(cosmic::Action::App)
    }

    fn finish_config(&mut self, epoch: u64, result: Result<ConfigSnapshot, ApiError>) {
        if epoch != self.epoch {
            return;
        }
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
        if self.store.busy || self.store.action_busy {
            return Task::none();
        }
        let scope = self.store.current_scope();
        let epoch = self.epoch;
        self.store.busy = true;
        self.store.error = None;
        let token = session.token.clone();
        let as_user = scope.as_user();
        Task::future(async move {
            Message::StoreLoaded(epoch, scope, client::store_snapshot(token, as_user).await)
        })
        .map(cosmic::Action::App)
    }

    fn finish_store(
        &mut self,
        epoch: u64,
        scope: StoreScope,
        result: Result<client::StoreSnapshot, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.store.busy = false;
        if !scope.matches(self.store.target_user().as_deref()) {
            return Task::none();
        }
        match result {
            Ok(snapshot) => {
                self.store.packages = snapshot.packages;
                self.store.generations = snapshot.generations;
                self.store.loaded_scope = scope;
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

    fn search_store(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.store.search_busy {
            return Task::none();
        }
        let query = self.store.search_query.trim().to_string();
        if query.is_empty() {
            self.store.search_error = Some("Enter a package name.".to_string());
            return Task::none();
        }

        let seq = self.store.begin_search();
        let token = session.token.clone();
        let scope = self.store.current_scope();
        let epoch = self.epoch;
        let as_user = scope.as_user();
        Task::future(async move {
            Message::StoreSearchLoaded(
                epoch,
                seq,
                scope,
                client::search(token, query, as_user).await,
            )
        })
        .map(cosmic::Action::App)
    }

    fn finish_store_search(
        &mut self,
        epoch: u64,
        seq: u64,
        scope: &StoreScope,
        result: Result<SearchPage, ApiError>,
    ) {
        if epoch != self.epoch || !self.store.search_is_current(seq) {
            return;
        }
        self.store.search_busy = false;
        if !scope.matches(self.store.target_user().as_deref()) {
            return;
        }
        match result {
            Ok(page) => {
                self.store.search_results = page.results;
                self.store.search_truncated = page.truncated;
                self.store.search_done = true;
                self.store.search_error = None;
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return;
                }
                self.store.search_error = Some(friendly(&error));
            }
        }
    }

    fn request_store_action(&mut self, action: StoreAction) -> Task<cosmic::Action<Message>> {
        if self.store.pending.is_some() {
            self.store.error = Some(
                "Finish or cancel the open confirmation before starting another action.".into(),
            );
            return Task::none();
        }
        if !self.store.can_run(&action) {
            self.store.error = Some(if action.uses_profile() {
                "Refresh the target profile before acting on it.".into()
            } else {
                "Wait for the current operation to finish.".into()
            });
            return Task::none();
        }
        if let Some(package) = match &action {
            StoreAction::Install(package)
            | StoreAction::Upgrade(package)
            | StoreAction::Remove(package) => Some(package.as_str()),
            _ => None,
        } {
            if package.trim().is_empty() {
                self.store.error = Some("Enter a package name.".to_string());
                return Task::none();
            }
            if let Err(error) = validate_installable(package) {
                self.store.error = Some(friendly(&error));
                return Task::none();
            }
        }
        let scope = self.store.current_scope();
        if action.requires_confirmation_in(&scope) {
            self.store.pending = Some(PendingStoreAction { action, scope });
            return Task::none();
        }
        self.run_store_action(action, scope)
    }

    fn run_store_action(
        &mut self,
        action: StoreAction,
        scope: StoreScope,
    ) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if !self.store.can_run(&action) {
            self.store.error = Some("Wait for the current operation to finish.".into());
            return Task::none();
        }
        if action.uses_profile() && !scope.matches(self.store.target_user().as_deref()) {
            self.store.error = Some("Target user changed. Refresh and try again.".into());
            return Task::none();
        }

        self.store.action_busy = true;
        self.store.action_label = Some(action.progress_label());
        self.store.action_started = Some(std::time::Instant::now());
        self.store.action_message = None;
        self.store.error = None;
        self.store.pending = None;

        let token = session.token.clone();
        let epoch = self.epoch;
        // GC is system-wide: never attach a profile identity to it.
        let as_user = if action.uses_profile() {
            scope.as_user()
        } else {
            None
        };
        let api = action.api();
        let timeout = action.timeout();
        Task::future(async move {
            let result = client::package_call(token, api, as_user, timeout).await;
            Message::StoreActionFinished(epoch, action, scope, result)
        })
        .map(cosmic::Action::App)
    }

    fn finish_store_action(
        &mut self,
        epoch: u64,
        action: &StoreAction,
        scope: &StoreScope,
        result: Result<serde_json::Value, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.store.action_busy = false;
        self.store.action_label = None;
        self.store.action_started = None;
        let scope_changed =
            action.uses_profile() && !scope.matches(self.store.target_user().as_deref());
        let is_preview = matches!(action, StoreAction::PruneGenerations { dry_run: true, .. });
        if scope_changed {
            // Never write state derived from another profile. Drop the stale
            // preview and any message the user could mistake for current.
            self.store.prune_preview = None;
            self.store.action_message = None;
            self.store.error = Some("Target user changed; result discarded.".into());
            return Task::none();
        }
        match result {
            Ok(value) => {
                if let StoreAction::PruneGenerations {
                    keep,
                    dry_run: true,
                    ..
                } = &action
                {
                    // Bind apply to the exact preview the user saw.
                    self.store.prune_preview = Some(crate::store::PrunePreview {
                        keep: *keep,
                        current: value
                            .get("current")
                            .and_then(serde_json::Value::as_u64)
                            .and_then(|current| u32::try_from(current).ok()),
                        would_delete: value
                            .get("would_delete")
                            .and_then(serde_json::Value::as_array)
                            .map(|list| {
                                list.iter()
                                    .filter_map(serde_json::Value::as_u64)
                                    .filter_map(|value| u32::try_from(value).ok())
                                    .collect()
                            })
                            .unwrap_or_default(),
                    });
                } else {
                    // Any other completed action can move generations.
                    self.store.prune_preview = None;
                }
                self.store.action_message = Some(format!(
                    "{}: {}",
                    action.target_label(scope),
                    crate::store::summarize(action, &value)
                ));
                if action.reloads_profile() {
                    return self.load_store();
                }
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                if !is_preview {
                    self.store.prune_preview = None;
                }
                self.store.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn parsed_generation(&self) -> Result<Option<u32>, String> {
        let raw = self.store.generation_input.trim();
        if raw.is_empty() {
            return Ok(None);
        }
        raw.parse::<u32>()
            .map(Some)
            .map_err(|_| "Generation must be a whole number.".to_string())
    }

    fn request_rollback(&mut self) -> Task<cosmic::Action<Message>> {
        match self.parsed_generation() {
            Ok(generation) => self.request_store_action(StoreAction::Rollback(generation)),
            Err(error) => {
                self.store.error = Some(error);
                Task::none()
            }
        }
    }

    fn request_diff(&mut self) -> Task<cosmic::Action<Message>> {
        match self.parsed_generation() {
            Ok(generation) => self.request_store_action(StoreAction::DiffClosures(generation)),
            Err(error) => {
                self.store.error = Some(error);
                Task::none()
            }
        }
    }

    fn request_prune(&mut self, dry_run: bool) -> Task<cosmic::Action<Message>> {
        let keep = match self.store.keep_input.trim().parse::<u32>() {
            Ok(keep) if (1..=1000).contains(&keep) => keep,
            _ => {
                self.store.error = Some("Keep must be a number between 1 and 1000.".to_string());
                return Task::none();
            }
        };
        if dry_run {
            // A new preview replaces the generation its apply may rely on.
            self.store.prune_preview = None;
        } else if !self.store.prune_apply_matches_preview(keep) {
            self.store.error = Some("Preview this keep value before pruning.".into());
            return Task::none();
        }
        let expected_current = if dry_run {
            None
        } else {
            self.store
                .prune_preview
                .as_ref()
                .and_then(|preview| preview.current)
        };
        let expected_delete = if dry_run {
            None
        } else {
            self.store.prune_expected_delete()
        };
        self.request_store_action(StoreAction::PruneGenerations {
            keep,
            dry_run,
            expected_current,
            expected_delete,
        })
    }

    fn request_wipe(&mut self, dry_run: bool) -> Task<cosmic::Action<Message>> {
        let age = self.store.history_age.trim().to_string();
        if age.is_empty() {
            self.store.error = Some("Enter an age like 30d.".to_string());
            return Task::none();
        }
        self.request_store_action(StoreAction::WipeHistory {
            older_than: Some(age),
            dry_run,
        })
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
        self.epoch = self.epoch.wrapping_add(1);
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
        _ => error.fix.as_ref().map_or_else(
            || error.message.clone(),
            |fix| format!("{} ({fix})", error.message),
        ),
    }
}
