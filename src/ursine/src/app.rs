//! Application state, message loop and screen routing.

use cosmic::iced::Length;
use cosmic::{Element, Task};
use ursine_api::{
    validate_installable, Action, ApiError, ApkPackage, Capability, CapabilityReport, ErrorCode,
    FlatpakPackage, ModuleId, SessionToken, TriState,
};

use crate::client::{self, AuthSession, ConfigSnapshot, SearchPage};
use crate::home::HomeState;
use crate::login::LoginState;
use crate::niri::NiriState;
use crate::packages::{PackageKind, PackagesState};
use crate::printers::PrintersState;
use crate::scanners::ScannersState;
use crate::settings::SettingsState;
use crate::snapshots::{SnapAction, SnapSetting, SnapshotsState};
use crate::store::{PendingStoreAction, StoreAction, StoreScope, StoreState};
use crate::users::UsersState;

/// Live session. The token stays in RAM: never written to disk, never logged.
struct Session {
    token: SessionToken,
    expires_in_secs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureEdge {
    ViewScroll,
    WorkspaceSwitch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeField {
    TriggerWidth,
    TriggerHeight,
    Delay,
    MaxSpeed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrutEdge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Screen {
    #[default]
    Home,
    Settings,
    Store,
    Snapshots,
    Users,
    Niri,
    Apk,
    Flatpak,
    Printers,
    Scanners,
}

pub struct App {
    core: cosmic::Core,
    login: LoginState,
    home: HomeState,
    settings: SettingsState,
    store: StoreState,
    snapshots: SnapshotsState,
    users: UsersState,
    niri: NiriState,
    packages: PackagesState,
    printers: PrintersState,
    scanners: ScannersState,
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
    OpenSnapshots,
    OpenUsers,
    OpenNiri,
    OpenPackages(PackageKind),
    OpenPrinters,
    OpenScanners,
    RefreshScanners,
    ScannersLoaded(u64, Result<Vec<ursine_api::Scanner>, ApiError>),
    ScannerSelected(usize),
    ScannerResolutionChanged(usize),
    ScannerModeChanged(usize),
    ScannerSourceChanged(String),
    ScannerScanRequested,
    ScanFinished(u64, Result<ursine_api::ScanResult, ApiError>),
    RefreshPrinters,
    PrintersLoaded(
        u64,
        Result<(Vec<ursine_api::Printer>, Vec<ursine_api::PrintJob>), ApiError>,
    ),
    PrinterNameChanged(String),
    PrinterUriChanged(String),
    PrinterDescriptionChanged(String),
    PrinterLocationChanged(String),
    PrinterAddRequested,
    PrinterSetDefault(String),
    PrinterPause(String, bool),
    PrinterRemoveRequested(String),
    PrinterRemoveConfirmed,
    PrinterRemoveCancelled,
    PrinterCancelAllRequested,
    PrinterCancelConfirmed,
    PrinterCancelCancelled,
    PrinterActionFinished(u64, String, Result<serde_json::Value, ApiError>),
    PackageQueryChanged(String),
    PackageInstallChanged(String),
    PackageScopeChanged(usize),
    PackageSearchRequested,
    PackageInstallRequested,
    PackageInstallDirect(String),
    PackageRefresh,
    PackageRefreshIndexes,
    PackageUpdateRequested,
    PackageUpdateConfirmed,
    PackageUpdateCancelled,
    PackageRemoveRequested(PackageKind, String, String),
    PackageRemoveConfirmed,
    PackageRemoveCancelled,
    PackageApkLoaded(u64, Result<Vec<ApkPackage>, ApiError>),
    PackageFlatpakLoaded(u64, Result<Vec<FlatpakPackage>, ApiError>),
    PackageApkSearchLoaded(u64, Result<Vec<ApkPackage>, ApiError>),
    PackageFlatpakSearchLoaded(u64, Result<Vec<FlatpakPackage>, ApiError>),
    PackageActionFinished(u64, String, Result<serde_json::Value, ApiError>),
    BackHome,
    SnapshotsLoaded(u64, Result<Box<client::SnapshotPage>, ApiError>),
    RefreshSnapshots,
    SnapshotSectionChanged(String),
    SnapshotRestoreSnapshotChanged(String),
    SnapshotRestoreTargetChanged(String),
    SnapshotCreateRequested,
    SnapshotRestorePreview,
    SnapshotRestorePreviewed(u64, String, String, Result<serde_json::Value, ApiError>),
    SnapshotRestoreConfirmed,
    SnapshotRestoreCancelled,
    SnapshotSettingChanged(SnapSetting, usize),
    SaveBtrbkConfig,
    SnapshotSelected(usize),
    SnapshotRemoveSelected,
    SnapshotRemoveAllRequested,
    SnapshotRemoveSourceToggled(String),
    SnapshotDeleteConfirmed,
    SnapshotDeleteCancelled,
    InitBtrbkConfig,
    SnapshotActionFinished(u64, SnapAction, Result<serde_json::Value, ApiError>),
    UsersLoaded(u64, Result<client::UsersPage, ApiError>),
    RefreshUsers,
    NewUserNameChanged(String),
    NewUserFullnameChanged(String),
    NewUserAdminToggled(bool),
    CreateUserRequested,
    UserSelected(u32),
    UserPasswordChanged(String),
    UserPasswordConfirmChanged(String),
    SetPasswordRequested(u32),
    UserGroupsChanged(String),
    SetGroupsRequested(u32),
    DeleteUserRequested(u32),
    DeleteUserConfirmed,
    DeleteUserCancelled,
    UsersActionFinished(u64, String, Result<serde_json::Value, ApiError>),
    NiriLoaded(u64, Result<ursine_api::NiriConfigView, ApiError>),
    RefreshNiri,
    NiriGapsChanged(String),
    NiriGapsPreset(usize),
    NiriCursorSizePreset(usize),
    NiriKeyboardLayoutChanged(String),
    NiriRepeatDelayChanged(String),
    NiriRepeatRateChanged(String),
    NiriTrackLayoutChanged(usize),
    NiriNumlockToggled(bool),
    NiriTouchpadSpeedPreset(usize),
    NiriTouchpadProfileChanged(usize),
    NiriMouseSpeedPreset(usize),
    NiriMouseProfileChanged(usize),
    NiriMouseNaturalScrollToggled(bool),
    NiriBorderWidthChanged(String),
    NiriFocusRingWidthChanged(String),
    NiriStrutChanged(StrutEdge, String),
    NiriScreenshotPathChanged(String),
    NiriHotkeyOverlayToggled(bool),
    NiriOutputScale(usize, usize),
    NiriOutputTransform(usize, usize),
    NiriOutputVrr(usize, bool),
    NiriOutputNameChanged(String),
    NiriOutputAdd,
    NiriEnvNameChanged(usize, String),
    NiriEnvValueChanged(usize, String),
    NiriEnvAdd,
    NiriEnvRemove(usize),
    NiriEnvNameInputChanged(String),
    NiriEnvValueInputChanged(String),
    NiriSpawnChanged(usize, String),
    NiriSpawnAdd,
    NiriSpawnRemove(usize),
    NiriSpawnInputChanged(String),
    NiriBindKeyChanged(usize, String),
    NiriBindActionChanged(usize, usize),
    NiriBindCustomChanged(usize, String),
    NiriBindArgsChanged(usize, String),
    NiriBindRemove(usize),
    NiriNewBindKeyChanged(String),
    NiriNewBindActionChanged(usize),
    NiriNewBindArgsChanged(String),
    NiriBindAdd,
    NiriStartCapture(usize),
    NiriKeyCaptured(String),
    NiriHotCornerToggled(usize, bool),
    NiriHotCornersOffToggled(bool),
    NiriEdgeChanged(GestureEdge, EdgeField, String),
    NiriAnimationsToggled(bool),
    NiriTapToggled(bool),
    NiriNaturalScrollToggled(bool),
    NiriDwtToggled(bool),
    NiriCursorSizeChanged(String),
    NiriCursorThemeChanged(String),
    SaveNiriRequested,
    ValidateNiriRequested,
    NiriSaveFinished(u64, Result<ursine_api::NiriConfigView, ApiError>),
    NiriValidateFinished(u64, Result<serde_json::Value, ApiError>),
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
    StoreKeepPreset(usize),
    StoreHistoryAgePreset(usize),
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
            snapshots: SnapshotsState::default(),
            users: UsersState::default(),
            niri: NiriState::default(),
            packages: PackagesState::default(),
            printers: PrintersState::default(),
            scanners: ScannersState::default(),
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
            Message::OpenSnapshots => {
                self.screen = Screen::Snapshots;
                return self.load_snapshots();
            }
            Message::OpenUsers => {
                self.screen = Screen::Users;
                return self.load_users();
            }
            Message::OpenNiri => {
                self.screen = Screen::Niri;
                return self.load_niri();
            }
            Message::OpenPackages(kind) => return self.open_packages(kind),
            Message::OpenPrinters => {
                self.screen = Screen::Printers;
                return self.load_printers();
            }
            Message::OpenScanners => {
                self.screen = Screen::Scanners;
                return self.load_scanners();
            }
            Message::RefreshScanners => return self.load_scanners(),
            Message::ScannersLoaded(epoch, result) => return self.finish_scanners(epoch, result),
            Message::ScannerSelected(index) => self.scanners.selected = Some(index),
            Message::ScannerResolutionChanged(index) => self.scanners.resolution = index,
            Message::ScannerModeChanged(index) => self.scanners.mode = index,
            Message::ScannerSourceChanged(value) => self.scanners.source = value,
            Message::ScannerScanRequested => return self.scan_document(),
            Message::ScanFinished(epoch, result) => return self.finish_scan(epoch, result),
            Message::RefreshPrinters => return self.load_printers(),
            Message::PrintersLoaded(epoch, result) => return self.finish_printers(epoch, result),
            Message::PrinterNameChanged(value) => self.printers.name = value,
            Message::PrinterUriChanged(value) => self.printers.uri = value,
            Message::PrinterDescriptionChanged(value) => self.printers.description = value,
            Message::PrinterLocationChanged(value) => self.printers.location = value,
            Message::PrinterAddRequested => return self.add_printer(),
            Message::PrinterSetDefault(name) => {
                return self
                    .printer_action("default", client::set_default_printer(self.token(), name))
            }
            Message::PrinterPause(name, pause) => {
                return self
                    .printer_action("pause", client::pause_printer(self.token(), name, pause))
            }
            Message::PrinterRemoveRequested(name) => self.printers.pending_remove = Some(name),
            Message::PrinterRemoveConfirmed => {
                if let Some(name) = self.printers.pending_remove.take() {
                    return self
                        .printer_action("remove", client::remove_printer(self.token(), name));
                }
            }
            Message::PrinterRemoveCancelled => self.printers.pending_remove = None,
            Message::PrinterCancelAllRequested => self.printers.pending_cancel = true,
            Message::PrinterCancelConfirmed => {
                self.printers.pending_cancel = false;
                return self
                    .printer_action("cancel", client::cancel_printer_jobs(self.token(), None));
            }
            Message::PrinterCancelCancelled => self.printers.pending_cancel = false,
            Message::PrinterActionFinished(epoch, label, result) => {
                return self.finish_printer_action(epoch, &label, result)
            }
            Message::PackageQueryChanged(value) => self.packages.query = value,
            Message::PackageInstallChanged(value) => self.packages.install = value,
            Message::PackageScopeChanged(index) => {
                self.packages.system = index == 0;
                return self.load_packages(self.packages.kind.unwrap_or(PackageKind::Apk));
            }
            Message::PackageSearchRequested => return self.search_packages(),
            Message::PackageInstallRequested => return self.install_package_input(),
            Message::PackageInstallDirect(id) => {
                return self.install_package(id);
            }
            Message::PackageRefresh => {
                return self.load_packages(self.packages.kind.unwrap_or(PackageKind::Apk))
            }
            Message::PackageRefreshIndexes => return self.refresh_apk_indexes(),
            Message::PackageUpdateRequested => {
                self.packages.pending_update = self.packages.kind;
            }
            Message::PackageUpdateConfirmed => return self.confirm_package_update(),
            Message::PackageUpdateCancelled => self.packages.pending_update = None,
            Message::PackageRemoveRequested(kind, name, id) => {
                self.packages.pending_remove = Some((kind, name, id));
            }
            Message::PackageRemoveConfirmed => return self.confirm_package_remove(),
            Message::PackageRemoveCancelled => self.packages.pending_remove = None,
            Message::PackageApkLoaded(epoch, result) => return self.finish_apk(epoch, result),
            Message::PackageFlatpakLoaded(epoch, result) => {
                return self.finish_flatpak(epoch, result)
            }
            Message::PackageApkSearchLoaded(epoch, result) => {
                return self.finish_apk_search(epoch, result)
            }
            Message::PackageFlatpakSearchLoaded(epoch, result) => {
                return self.finish_flatpak_search(epoch, result)
            }
            Message::PackageActionFinished(epoch, label, result) => {
                return self.finish_package_action(epoch, &label, result)
            }
            Message::SnapshotsLoaded(epoch, result) => return self.finish_snapshots(epoch, result),
            Message::RefreshSnapshots => return self.load_snapshots(),
            Message::SnapshotSectionChanged(value) => self.snapshots.section = value,
            Message::SnapshotRestoreSnapshotChanged(value) => {
                self.snapshots.restore_snapshot = value;
            }
            Message::SnapshotRestoreTargetChanged(value) => self.snapshots.restore_target = value,
            Message::SnapshotCreateRequested => return self.create_snapshot(),
            Message::SnapshotRestorePreview => return self.preview_restore(),
            Message::SnapshotRestorePreviewed(epoch, snapshot, target, result) => {
                return self.finish_restore_preview(epoch, snapshot, target, result)
            }
            Message::SnapshotRestoreConfirmed => return self.confirm_restore(),
            Message::SnapshotRestoreCancelled => self.snapshots.pending_restore = None,
            Message::SnapshotSettingChanged(setting, index) => {
                setting.apply(&mut self.snapshots.settings, index);
            }
            Message::SaveBtrbkConfig => return self.save_btrbk_config(),
            Message::SnapshotSelected(index) => self.select_snapshot(index),
            Message::SnapshotRemoveSelected => self.request_delete_selected(),
            Message::SnapshotRemoveAllRequested => self.request_delete_all(),
            Message::SnapshotRemoveSourceToggled(source) => self.toggle_remove_source(&source),
            Message::SnapshotDeleteConfirmed => return self.confirm_delete(),
            Message::SnapshotDeleteCancelled => self.snapshots.pending_delete = None,
            Message::InitBtrbkConfig => return self.init_btrbk_config(),
            Message::SnapshotActionFinished(epoch, action, result) => {
                return self.finish_snapshot_action(epoch, &action, result)
            }
            Message::UsersLoaded(epoch, result) => return self.finish_users(epoch, result),
            Message::RefreshUsers => return self.load_users(),
            Message::NewUserNameChanged(value) => self.users.new_name = value,
            Message::NewUserFullnameChanged(value) => self.users.new_fullname = value,
            Message::NewUserAdminToggled(value) => self.users.new_admin = value,
            Message::CreateUserRequested => return self.create_user(),
            Message::UserSelected(uid) => self.users.select(uid),
            Message::UserPasswordChanged(value) => self.users.password = value,
            Message::UserPasswordConfirmChanged(value) => self.users.password_confirm = value,
            Message::SetPasswordRequested(uid) => return self.set_user_password(uid),
            Message::UserGroupsChanged(value) => self.users.groups_input = value,
            Message::SetGroupsRequested(uid) => return self.set_user_groups(uid),
            Message::DeleteUserRequested(uid) => {
                self.users.pending_delete = self
                    .users
                    .users
                    .iter()
                    .find(|user| user.uid == uid)
                    .cloned();
            }
            Message::DeleteUserConfirmed => return self.confirm_delete_user(),
            Message::DeleteUserCancelled => self.users.pending_delete = None,
            Message::UsersActionFinished(epoch, label, result) => {
                return self.finish_users_action(epoch, &label, result)
            }
            Message::NiriLoaded(epoch, result) => return self.finish_niri(epoch, result),
            Message::RefreshNiri => return self.load_niri(),
            Message::NiriGapsChanged(value) => self.niri.gaps = value,
            Message::NiriGapsPreset(index) => {
                self.niri.gaps = ["", "0", "4", "8", "12", "16", "24", "32"]
                    .get(index)
                    .copied()
                    .unwrap_or_default()
                    .to_string();
            }
            Message::NiriCursorSizePreset(index) => {
                self.niri.cursor_size = ["", "16", "24", "32", "48", "64"]
                    .get(index)
                    .copied()
                    .unwrap_or_default()
                    .to_string();
            }
            Message::NiriKeyboardLayoutChanged(value) => self.niri.keyboard_layout = value,
            Message::NiriRepeatDelayChanged(value) => self.niri.repeat_delay = value,
            Message::NiriRepeatRateChanged(value) => self.niri.repeat_rate = value,
            Message::NiriTrackLayoutChanged(index) => {
                self.niri.track_layout = ["", "global", "window"]
                    .get(index)
                    .copied()
                    .unwrap_or_default()
                    .to_string();
            }
            Message::NiriNumlockToggled(value) => self.niri.numlock = value,
            Message::NiriTouchpadSpeedPreset(index) => {
                self.niri.touchpad_accel_speed = ["", "-50", "-25", "0", "25", "50"]
                    .get(index)
                    .copied()
                    .unwrap_or_default()
                    .to_string();
            }
            Message::NiriTouchpadProfileChanged(index) => {
                self.niri.touchpad_accel_profile = ["", "adaptive", "flat"]
                    .get(index)
                    .copied()
                    .unwrap_or_default()
                    .to_string();
            }
            Message::NiriMouseSpeedPreset(index) => {
                self.niri.mouse_accel_speed = ["", "-50", "-25", "0", "25", "50"]
                    .get(index)
                    .copied()
                    .unwrap_or_default()
                    .to_string();
            }
            Message::NiriMouseProfileChanged(index) => {
                self.niri.mouse_accel_profile = ["", "adaptive", "flat"]
                    .get(index)
                    .copied()
                    .unwrap_or_default()
                    .to_string();
            }
            Message::NiriMouseNaturalScrollToggled(value) => self.niri.mouse_natural_scroll = value,
            Message::NiriBorderWidthChanged(value) => self.niri.border_width = value,
            Message::NiriFocusRingWidthChanged(value) => self.niri.focus_ring_width = value,
            Message::NiriStrutChanged(edge, value) => match edge {
                StrutEdge::Left => self.niri.strut_left = value,
                StrutEdge::Right => self.niri.strut_right = value,
                StrutEdge::Top => self.niri.strut_top = value,
                StrutEdge::Bottom => self.niri.strut_bottom = value,
            },
            Message::NiriScreenshotPathChanged(value) => self.niri.screenshot_path = value,
            Message::NiriHotkeyOverlayToggled(value) => self.niri.hotkey_overlay_skip = value,
            Message::NiriOutputScale(index, value) => {
                if let Some(output) = self.niri.outputs.get_mut(index) {
                    output.scale = ["", "100", "125", "150", "200"]
                        .get(value)
                        .and_then(|value| value.parse().ok());
                }
            }
            Message::NiriOutputTransform(index, value) => {
                if let Some(output) = self.niri.outputs.get_mut(index) {
                    output.transform = [
                        "",
                        "normal",
                        "90",
                        "180",
                        "270",
                        "flipped",
                        "flipped-90",
                        "flipped-180",
                        "flipped-270",
                    ]
                    .get(value)
                    .filter(|value| !value.is_empty())
                    .map(|value| (*value).to_string());
                }
            }
            Message::NiriOutputVrr(index, value) => {
                if let Some(output) = self.niri.outputs.get_mut(index) {
                    output.variable_refresh_rate = Some(value);
                }
            }
            Message::NiriOutputNameChanged(value) => self.niri.new_output = value,
            Message::NiriEnvNameChanged(index, value) => {
                if let Some(var) = self.niri.environment.get_mut(index) {
                    var.name = value;
                }
            }
            Message::NiriEnvValueChanged(index, value) => {
                if let Some(var) = self.niri.environment.get_mut(index) {
                    var.value = value;
                }
            }
            Message::NiriEnvAdd => {
                let name = self.niri.new_env_name.trim();
                if !name.is_empty() && !self.niri.environment.iter().any(|var| var.name == name) {
                    self.niri.environment.push(ursine_api::NiriEnvVar {
                        name: name.to_string(),
                        value: self.niri.new_env_value.trim().to_string(),
                    });
                    self.niri.new_env_name.clear();
                    self.niri.new_env_value.clear();
                }
            }
            Message::NiriEnvRemove(index) => {
                if index < self.niri.environment.len() {
                    self.niri.environment.remove(index);
                }
            }
            Message::NiriEnvNameInputChanged(value) => self.niri.new_env_name = value,
            Message::NiriEnvValueInputChanged(value) => self.niri.new_env_value = value,
            Message::NiriSpawnChanged(index, value) => {
                if let Some(spawn) = self.niri.spawn_text.get_mut(index) {
                    *spawn = value;
                    self.niri.spawn_dirty = true;
                }
            }
            Message::NiriSpawnAdd => {
                let value = self.niri.new_spawn.trim();
                if !value.is_empty() {
                    self.niri.spawn_text.push(value.to_string());
                    self.niri.new_spawn.clear();
                    self.niri.spawn_dirty = true;
                }
            }
            Message::NiriSpawnRemove(index) => {
                if index < self.niri.spawn_text.len() {
                    self.niri.spawn_text.remove(index);
                    self.niri.spawn_dirty = true;
                }
            }
            Message::NiriSpawnInputChanged(value) => self.niri.new_spawn = value,
            Message::NiriBindKeyChanged(index, value) => {
                if let Some(keybind) = self.niri.keybinds.get_mut(index) {
                    keybind.key = value;
                    self.niri.keybind_dirty = true;
                }
            }
            Message::NiriBindActionChanged(index, action) => {
                if let Some(choice) = self.niri.keybind_actions.get_mut(index) {
                    *choice = action;
                    if action < crate::niri::BIND_ACTIONS.len() {
                        if let Some(custom) = self.niri.keybind_custom.get_mut(index) {
                            custom.clear();
                        }
                    }
                    self.niri.keybind_dirty = true;
                }
            }
            Message::NiriBindCustomChanged(index, value) => {
                if let Some(custom) = self.niri.keybind_custom.get_mut(index) {
                    *custom = value;
                    self.niri.keybind_dirty = true;
                }
            }
            Message::NiriBindArgsChanged(index, value) => {
                if let Some(text) = self.niri.keybind_args.get_mut(index) {
                    *text = value;
                    self.niri.keybind_dirty = true;
                }
            }
            Message::NiriBindRemove(index) => {
                if index < self.niri.keybinds.len() {
                    self.niri.keybinds.remove(index);
                    self.niri.keybind_actions.remove(index);
                    self.niri.keybind_custom.remove(index);
                    self.niri.keybind_args.remove(index);
                    self.niri.keybind_dirty = true;
                }
            }
            Message::NiriNewBindKeyChanged(value) => self.niri.new_bind_key = value,
            Message::NiriNewBindActionChanged(value) => self.niri.new_bind_action = value,
            Message::NiriNewBindArgsChanged(value) => self.niri.new_bind_args = value,
            Message::NiriStartCapture(index) => {
                self.niri.capture = Some(index);
            }
            Message::NiriKeyCaptured(key) => {
                if let Some(index) = self.niri.capture.take() {
                    if let Some(bind) = self.niri.keybinds.get_mut(index) {
                        bind.key = key;
                        self.niri.keybind_dirty = true;
                    }
                }
            }
            Message::NiriBindAdd => {
                let key = self.niri.new_bind_key.trim();
                if !key.is_empty() {
                    self.niri.keybinds.push(ursine_api::NiriKeybind {
                        key: key.to_string(),
                        actions: vec![ursine_api::NiriBindAction {
                            name: crate::niri::BIND_ACTIONS
                                .get(self.niri.new_bind_action)
                                .copied()
                                .unwrap_or("spawn")
                                .to_string(),
                            args: Vec::new(),
                        }],
                    });
                    self.niri.keybind_actions.push(self.niri.new_bind_action);
                    self.niri.keybind_custom.push(String::new());
                    self.niri.keybind_args.push(self.niri.new_bind_args.clone());
                    self.niri.new_bind_key.clear();
                    self.niri.new_bind_args.clear();
                    self.niri.keybind_dirty = true;
                }
            }
            Message::NiriHotCornerToggled(index, value) => {
                if let Some(corner) = self.niri.hot_corners.get_mut(index) {
                    *corner = value;
                    self.niri.gesture_dirty = true;
                }
            }
            Message::NiriHotCornersOffToggled(value) => {
                self.niri.hot_corners_off = value;
                self.niri.gesture_dirty = true;
            }
            Message::NiriEdgeChanged(edge, field, value) => {
                self.niri.set_edge(edge, field, value);
                self.niri.gesture_dirty = true;
            }
            Message::NiriOutputAdd => {
                let name = self.niri.new_output.trim();
                if !name.is_empty() && !self.niri.outputs.iter().any(|output| output.name == name) {
                    self.niri.outputs.push(ursine_api::NiriOutput {
                        name: name.to_string(),
                        scale: None,
                        transform: None,
                        variable_refresh_rate: None,
                    });
                    self.niri.new_output.clear();
                }
            }
            Message::NiriAnimationsToggled(value) => self.niri.animations_off = value,
            Message::NiriTapToggled(value) => self.niri.tap = value,
            Message::NiriNaturalScrollToggled(value) => self.niri.natural_scroll = value,
            Message::NiriDwtToggled(value) => self.niri.dwt = value,
            Message::NiriCursorSizeChanged(value) => self.niri.cursor_size = value,
            Message::NiriCursorThemeChanged(value) => self.niri.cursor_theme = value,
            Message::SaveNiriRequested => return self.save_niri(),
            Message::ValidateNiriRequested => return self.validate_niri(),
            Message::NiriSaveFinished(epoch, result) => {
                return self.finish_niri_save(epoch, result)
            }
            Message::NiriValidateFinished(epoch, result) => {
                return self.finish_niri_validate(epoch, result)
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
            Message::StoreKeepPreset(index) => {
                self.store.keep_input = ["1", "3", "5", "10", "20"]
                    .get(index)
                    .copied()
                    .unwrap_or("5")
                    .to_string();
                self.store.prune_preview = None;
            }
            Message::StoreHistoryAgePreset(index) => {
                self.store.history_age = ["7d", "30d", "90d", "1y"]
                    .get(index)
                    .copied()
                    .unwrap_or("30d")
                    .to_string();
            }
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
        let mut subscriptions = Vec::new();
        if self.store.action_busy {
            subscriptions.push(
                cosmic::iced::time::every(std::time::Duration::from_secs(1))
                    .map(|_| Message::StoreTick),
            );
        }
        if self.screen == Screen::Niri && self.niri.capture.is_some() {
            subscriptions.push(cosmic::iced::event::listen_with(niri_key_message));
        }
        cosmic::iced::Subscription::batch(subscriptions)
    }

    fn view(&self) -> Element<'_, Self::Message> {
        self.session.as_ref().map_or_else(
            || self.login.view(),
            |session| {
                let content = match self.screen {
                    Screen::Home => self.home.view(
                        session.expires_in_secs,
                        self.settings.config.as_ref().map(|config| &config.modules),
                    ),
                    Screen::Settings => self.settings.view(
                        &self.home.caps,
                        self.home.busy,
                        self.home.error.as_deref(),
                    ),
                    Screen::Store => self.store.view(),
                    Screen::Snapshots => self.snapshots.view(),
                    Screen::Users => self.users.view(),
                    Screen::Niri => self.niri.view(),
                    Screen::Apk | Screen::Flatpak => self.packages.view(),
                    Screen::Printers => self.printers.view(),
                    Screen::Scanners => self.scanners.view(),
                };
                cosmic::iced::widget::row(vec![self.sidebar(), content])
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            },
        )
    }
}

impl App {
    fn load_snapshots(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.snapshots.busy {
            return Task::none();
        }
        self.snapshots.busy = true;
        self.snapshots.error = None;
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(async move {
            Message::SnapshotsLoaded(epoch, client::snapshot_page(token).await.map(Box::new))
        })
        .map(cosmic::Action::App)
    }

    fn finish_snapshots(
        &mut self,
        epoch: u64,
        result: Result<Box<client::SnapshotPage>, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.snapshots.busy = false;
        match result {
            Ok(page) => self.snapshots.apply(*page),
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.snapshots.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn create_snapshot(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.snapshots.action_busy {
            return Task::none();
        }
        let section = self.snapshots.section.trim();
        let section = (!section.is_empty()).then(|| section.to_string());
        self.snapshots.action_busy = true;
        self.snapshots.error = None;
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(async move {
            let result = client::create_snapshot(token, section.clone()).await;
            Message::SnapshotActionFinished(epoch, SnapAction::Create(section), result)
        })
        .map(cosmic::Action::App)
    }

    fn preview_restore(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.snapshots.action_busy {
            return Task::none();
        }
        let snapshot = self.snapshots.restore_snapshot.trim().to_string();
        let target = self.snapshots.restore_target.trim().to_string();
        if snapshot.is_empty() || target.is_empty() {
            self.snapshots.error = Some("Enter snapshot and target paths.".into());
            return Task::none();
        }
        self.snapshots.action_busy = true;
        self.snapshots.error = None;
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(async move {
            let result =
                client::restore_snapshot(token, snapshot.clone(), target.clone(), true).await;
            Message::SnapshotRestorePreviewed(epoch, snapshot, target, result)
        })
        .map(cosmic::Action::App)
    }

    fn finish_restore_preview(
        &mut self,
        epoch: u64,
        snapshot: String,
        target: String,
        result: Result<serde_json::Value, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.snapshots.action_busy = false;
        match result {
            Ok(value) => {
                self.snapshots.pending_restore = Some((snapshot, target));
                self.snapshots.action_message = Some(format!(
                    "Preview: {}",
                    value
                        .get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("restore ready")
                ));
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.snapshots.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn confirm_restore(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let Some((snapshot, target)) = self.snapshots.pending_restore.take() else {
            return Task::none();
        };
        self.snapshots.action_busy = true;
        let token = session.token.clone();
        let epoch = self.epoch;
        let action = SnapAction::Restore {
            snapshot: snapshot.clone(),
            target: target.clone(),
            dry_run: false,
        };
        Task::future(async move {
            let result = client::restore_snapshot(token, snapshot, target, false).await;
            Message::SnapshotActionFinished(epoch, action, result)
        })
        .map(cosmic::Action::App)
    }

    fn init_btrbk_config(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.snapshots.action_busy {
            return Task::none();
        }
        self.snapshots.action_busy = true;
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(async move {
            let result = client::init_btrbk_config(token)
                .await
                .map(|config| serde_json::json!({"path": config.path, "exists": config.exists}));
            Message::SnapshotActionFinished(epoch, SnapAction::InitConfig, result)
        })
        .map(cosmic::Action::App)
    }

    fn save_btrbk_config(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.snapshots.action_busy {
            return Task::none();
        }
        self.snapshots.action_busy = true;
        let token = session.token.clone();
        let epoch = self.epoch;
        let settings = self.snapshots.settings.clone();
        Task::future(async move {
            let result = client::set_btrbk_config(token, settings)
                .await
                .map(|config| serde_json::json!({"path": config.path, "exists": config.exists}));
            Message::SnapshotActionFinished(epoch, SnapAction::SetConfig, result)
        })
        .map(cosmic::Action::App)
    }

    fn select_snapshot(&mut self, index: usize) {
        let Some(snapshot) = self.snapshots.snapshots.get(index) else {
            return;
        };
        self.snapshots.selected_snapshot = Some(index);
        self.snapshots.restore_snapshot = snapshot.snapshot_subvolume.clone();
        self.snapshots.restore_target = snapshot.source_subvolume.clone();
    }

    fn request_delete_selected(&mut self) {
        let Some(index) = self.snapshots.selected_snapshot else {
            self.snapshots.error = Some("Select a snapshot first.".into());
            return;
        };
        let Some(snapshot) = self.snapshots.snapshots.get(index) else {
            self.snapshots.error = Some("Selected snapshot is no longer available.".into());
            return;
        };
        if self.snapshots.action_busy {
            return;
        }
        self.snapshots.pending_delete = Some(vec![snapshot.snapshot_subvolume.clone()]);
    }

    fn request_delete_all(&mut self) {
        if self.snapshots.action_busy {
            return;
        }
        let paths: Vec<String> = self
            .snapshots
            .snapshots
            .iter()
            .filter(|snapshot| {
                self.snapshots.remove_sources.is_empty()
                    || self
                        .snapshots
                        .remove_sources
                        .contains(&snapshot.source_subvolume)
            })
            .map(|snapshot| snapshot.snapshot_subvolume.clone())
            .collect();
        if paths.is_empty() {
            self.snapshots.error = Some("No snapshots match the selected sources.".into());
            return;
        }
        self.snapshots.pending_delete = Some(paths);
    }

    fn toggle_remove_source(&mut self, source: &str) {
        if let Some(index) = self
            .snapshots
            .remove_sources
            .iter()
            .position(|value| value == source)
        {
            self.snapshots.remove_sources.remove(index);
        } else {
            self.snapshots.remove_sources.push(source.to_string());
        }
    }

    fn confirm_delete(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let Some(paths) = self.snapshots.pending_delete.take() else {
            return Task::none();
        };
        self.snapshots.action_busy = true;
        let token = session.token.clone();
        let epoch = self.epoch;
        let action_paths = paths.clone();
        Task::future(async move {
            let mut deleted = Vec::new();
            for path in paths {
                match client::delete_snapshot(token.clone(), path.clone()).await {
                    Ok(_) => deleted.push(path),
                    Err(error) => {
                        return Message::SnapshotActionFinished(
                            epoch,
                            SnapAction::Delete(action_paths),
                            Err(error),
                        );
                    }
                }
            }
            Message::SnapshotActionFinished(
                epoch,
                SnapAction::Delete(action_paths),
                Ok(serde_json::json!({"deleted": deleted})),
            )
        })
        .map(cosmic::Action::App)
    }

    fn finish_snapshot_action(
        &mut self,
        epoch: u64,
        action: &SnapAction,
        result: Result<serde_json::Value, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.snapshots.action_busy = false;
        match result {
            Ok(_) => {
                self.snapshots.action_message = Some(match action {
                    SnapAction::Create(_) => "Snapshot created.".into(),
                    SnapAction::Restore { .. } => "Restore completed.".into(),
                    SnapAction::Delete(paths) => format!("Deleted {} snapshot(s).", paths.len()),
                    SnapAction::InitConfig => "btrbk configuration created.".into(),
                    SnapAction::SetConfig => "btrbk configuration saved.".into(),
                });
                self.snapshots.error = None;
                return self.load_snapshots();
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.snapshots.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn load_users(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.users.busy || self.users.action_busy {
            return Task::none();
        }
        self.users.busy = true;
        self.users.error = None;
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(async move { Message::UsersLoaded(epoch, client::users_page(token).await) })
            .map(cosmic::Action::App)
    }

    fn finish_users(
        &mut self,
        epoch: u64,
        result: Result<client::UsersPage, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.users.busy = false;
        match result {
            Ok(page) => self.users.apply(page.users, page.groups),
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.users.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn create_user(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.users.action_busy {
            return Task::none();
        }
        let name = self.users.new_name.trim().to_string();
        let fullname = self.users.new_fullname.trim().to_string();
        if name.is_empty() {
            self.users.error = Some("Enter a user name.".into());
            return Task::none();
        }
        self.users.action_busy = true;
        self.users.error = None;
        let admin = self.users.new_admin;
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(async move {
            let result = client::create_user(token, name, fullname, admin).await;
            Message::UsersActionFinished(epoch, "created".into(), result)
        })
        .map(cosmic::Action::App)
    }

    fn set_user_password(&mut self, uid: u32) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.users.action_busy {
            return Task::none();
        }
        if self.users.password.is_empty() {
            self.users.error = Some("Enter a new password.".into());
            return Task::none();
        }
        if self.users.password != self.users.password_confirm {
            self.users.error = Some("Passwords do not match.".into());
            return Task::none();
        }
        self.users.action_busy = true;
        self.users.error = None;
        let password = self.users.password.clone();
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(async move {
            let result = client::set_password(token, uid, password).await;
            Message::UsersActionFinished(epoch, "password_set".into(), result)
        })
        .map(cosmic::Action::App)
    }

    fn set_user_groups(&mut self, uid: u32) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.users.action_busy {
            return Task::none();
        }
        let mut groups = Vec::new();
        for group in self.users.groups_input.split(',') {
            let group = group.trim();
            if !group.is_empty() && !groups.iter().any(|known: &String| known == group) {
                groups.push(group.to_string());
            }
        }
        self.users.action_busy = true;
        self.users.error = None;
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(async move {
            let result = client::set_groups(token, uid, groups).await;
            Message::UsersActionFinished(epoch, "groups_set".into(), result)
        })
        .map(cosmic::Action::App)
    }

    fn confirm_delete_user(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let Some(user) = self.users.pending_delete.take() else {
            return Task::none();
        };
        self.users.action_busy = true;
        self.users.error = None;
        let uid = user.uid;
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(async move {
            let result = client::delete_user(token, uid, true).await;
            Message::UsersActionFinished(epoch, "deleted".into(), result)
        })
        .map(cosmic::Action::App)
    }

    fn finish_users_action(
        &mut self,
        epoch: u64,
        label: &str,
        result: Result<serde_json::Value, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.users.action_busy = false;
        match result {
            Ok(_) => {
                self.users.error = None;
                self.users.message = Some(label.to_string());
                match label {
                    "created" => {
                        self.users.new_name.clear();
                        self.users.new_fullname.clear();
                        self.users.new_admin = false;
                    }
                    "password_set" => {
                        self.users.password.clear();
                        self.users.password_confirm.clear();
                    }
                    "deleted" => {
                        self.users.selected = None;
                        self.users.groups_input.clear();
                    }
                    _ => {}
                }
                return self.load_users();
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.users.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn load_niri(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.niri.busy || self.niri.action_busy {
            return Task::none();
        }
        self.niri.busy = true;
        self.niri.error = None;
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(
            async move { Message::NiriLoaded(epoch, client::get_niri_config(token).await) },
        )
        .map(cosmic::Action::App)
    }

    fn finish_niri(
        &mut self,
        epoch: u64,
        result: Result<ursine_api::NiriConfigView, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.niri.busy = false;
        match result {
            Ok(view) => self.niri.apply(view),
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.niri.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn save_niri(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.niri.action_busy {
            return Task::none();
        }
        let sections = match self.niri.sections() {
            Ok(sections) => sections,
            Err(error) => {
                self.niri.error = Some(error);
                return Task::none();
            }
        };
        self.niri.action_busy = true;
        self.niri.error = None;
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(async move {
            let mut last = None;
            for (section, value) in sections {
                match client::set_niri_section(token.clone(), section, value).await {
                    Ok(view) => last = Some(view),
                    Err(error) => return Message::NiriSaveFinished(epoch, Err(error)),
                }
            }
            let result = last.ok_or_else(|| ApiError::internal("no niri sections applied"));
            Message::NiriSaveFinished(epoch, result)
        })
        .map(cosmic::Action::App)
    }

    fn validate_niri(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.niri.action_busy {
            return Task::none();
        }
        self.niri.action_busy = true;
        self.niri.error = None;
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(async move {
            Message::NiriValidateFinished(epoch, client::validate_niri_config(token, None).await)
        })
        .map(cosmic::Action::App)
    }

    fn finish_niri_save(
        &mut self,
        epoch: u64,
        result: Result<ursine_api::NiriConfigView, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.niri.action_busy = false;
        match result {
            Ok(view) => {
                self.niri.apply(view);
                self.niri.message = Some("niri config validated and saved.".into());
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.niri.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn finish_niri_validate(
        &mut self,
        epoch: u64,
        result: Result<serde_json::Value, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.niri.action_busy = false;
        match result {
            Ok(value) => {
                if value.get("valid").and_then(serde_json::Value::as_bool) == Some(true) {
                    self.niri.message = Some("niri accepts the current configuration.".into());
                    self.niri.error = None;
                } else {
                    let detail = value
                        .get("stderr")
                        .and_then(serde_json::Value::as_str)
                        .filter(|text| !text.trim().is_empty())
                        .or_else(|| value.get("stdout").and_then(serde_json::Value::as_str))
                        .unwrap_or("niri rejected the configuration");
                    self.niri.error = Some(detail.trim().to_string());
                }
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.niri.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn open_packages(&mut self, kind: PackageKind) -> Task<cosmic::Action<Message>> {
        self.screen = match kind {
            PackageKind::Apk => Screen::Apk,
            PackageKind::Flatpak => Screen::Flatpak,
        };
        self.packages.open(kind);
        self.load_packages(kind)
    }

    fn load_packages(&mut self, kind: PackageKind) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.packages.busy || self.packages.action_busy {
            return Task::none();
        }
        self.packages.busy = true;
        self.packages.searching = false;
        self.packages.error = None;
        let token = session.token.clone();
        let epoch = self.epoch;
        let system = self.packages.system;
        Task::future(async move {
            match kind {
                PackageKind::Apk => Message::PackageApkLoaded(epoch, client::apk_list(token).await),
                PackageKind::Flatpak => {
                    Message::PackageFlatpakLoaded(epoch, client::flatpak_list(token, system).await)
                }
            }
        })
        .map(cosmic::Action::App)
    }

    fn finish_apk(
        &mut self,
        epoch: u64,
        result: Result<Vec<ApkPackage>, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.packages.busy = false;
        match result {
            Ok(packages) => {
                self.packages.apply_apk(packages);
                self.packages.searching = false;
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.packages.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn finish_flatpak(
        &mut self,
        epoch: u64,
        result: Result<Vec<FlatpakPackage>, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.packages.busy = false;
        match result {
            Ok(packages) => {
                self.packages.apply_flatpak(packages);
                self.packages.searching = false;
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.packages.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn finish_apk_search(
        &mut self,
        epoch: u64,
        result: Result<Vec<ApkPackage>, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.packages.busy = false;
        match result {
            Ok(packages) => {
                self.packages.apply_apk(packages);
                self.packages.searching = true;
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.packages.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn finish_flatpak_search(
        &mut self,
        epoch: u64,
        result: Result<Vec<FlatpakPackage>, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.packages.busy = false;
        match result {
            Ok(packages) => {
                self.packages.apply_flatpak(packages);
                self.packages.searching = true;
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.packages.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn search_packages(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.packages.busy || self.packages.action_busy {
            return Task::none();
        }
        let query = self.packages.query.trim().to_string();
        if query.is_empty() {
            self.packages.error = Some("Enter a search term.".into());
            return Task::none();
        }
        let kind = self.packages.kind.unwrap_or(PackageKind::Apk);
        let system = self.packages.system;
        let token = session.token.clone();
        let epoch = self.epoch;
        self.packages.busy = true;
        self.packages.error = None;
        Task::future(async move {
            match kind {
                PackageKind::Apk => {
                    Message::PackageApkSearchLoaded(epoch, client::apk_search(token, query).await)
                }
                PackageKind::Flatpak => Message::PackageFlatpakSearchLoaded(
                    epoch,
                    client::flatpak_search(token, query, system).await,
                ),
            }
        })
        .map(cosmic::Action::App)
    }

    fn install_package_input(&mut self) -> Task<cosmic::Action<Message>> {
        let input = self.packages.install.trim().to_string();
        if input.is_empty() {
            self.packages.error = Some("Enter a package name.".into());
            return Task::none();
        }
        self.install_package(input)
    }

    fn install_package(&mut self, package: String) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.packages.action_busy {
            return Task::none();
        }
        let kind = self.packages.kind.unwrap_or(PackageKind::Apk);
        let system = self.packages.system;
        let token = session.token.clone();
        let epoch = self.epoch;
        self.packages.action_busy = true;
        self.packages.error = None;
        self.packages.pending_remove = None;
        Task::future(async move {
            let result = match kind {
                PackageKind::Apk => client::apk_install(token, package).await,
                PackageKind::Flatpak => client::flatpak_install(token, package, system).await,
            };
            Message::PackageActionFinished(epoch, "install".into(), result)
        })
        .map(cosmic::Action::App)
    }

    fn refresh_apk_indexes(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.packages.action_busy {
            return Task::none();
        }
        let token = session.token.clone();
        let epoch = self.epoch;
        self.packages.action_busy = true;
        self.packages.error = None;
        Task::future(async move {
            Message::PackageActionFinished(
                epoch,
                "refresh_indexes".into(),
                client::apk_refresh(token).await,
            )
        })
        .map(cosmic::Action::App)
    }

    fn confirm_package_update(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let Some(kind) = self.packages.pending_update.take() else {
            return Task::none();
        };
        if self.packages.action_busy {
            return Task::none();
        }
        let system = self.packages.system;
        let token = session.token.clone();
        let epoch = self.epoch;
        self.packages.action_busy = true;
        self.packages.error = None;
        Task::future(async move {
            let result = match kind {
                PackageKind::Apk => client::apk_upgrade(token).await,
                PackageKind::Flatpak => client::flatpak_update(token, system).await,
            };
            Message::PackageActionFinished(epoch, "update".into(), result)
        })
        .map(cosmic::Action::App)
    }

    fn confirm_package_remove(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let Some((kind, _name, id)) = self.packages.pending_remove.take() else {
            return Task::none();
        };
        if self.packages.action_busy {
            return Task::none();
        }
        let system = self.packages.system;
        let token = session.token.clone();
        let epoch = self.epoch;
        self.packages.action_busy = true;
        self.packages.error = None;
        Task::future(async move {
            let result = match kind {
                PackageKind::Apk => client::apk_remove(token, id).await,
                PackageKind::Flatpak => client::flatpak_remove(token, id, system).await,
            };
            Message::PackageActionFinished(epoch, "remove".into(), result)
        })
        .map(cosmic::Action::App)
    }

    fn finish_package_action(
        &mut self,
        epoch: u64,
        label: &str,
        result: Result<serde_json::Value, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.packages.action_busy = false;
        match result {
            Ok(value) => {
                self.packages.error = None;
                let detail = value
                    .get("output")
                    .and_then(|value| value.as_str())
                    .unwrap_or(label);
                self.packages.message = Some(detail.lines().next().unwrap_or(label).to_string());
                self.packages.searching = false;
                return self.load_packages(self.packages.kind.unwrap_or(PackageKind::Apk));
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.packages.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        use cosmic::widget::{button, column, container, scrollable, text};

        let entries: [(Screen, &str); 10] = [
            (Screen::Home, "Overview"),
            (Screen::Store, "Nix packages"),
            (Screen::Apk, "APK packages"),
            (Screen::Flatpak, "Flatpak applications"),
            (Screen::Printers, "Printers"),
            (Screen::Scanners, "Scanners"),
            (Screen::Snapshots, "Snapshots"),
            (Screen::Users, "Users"),
            (Screen::Niri, "Niri desktop"),
            (Screen::Settings, "Settings"),
        ];
        let mut items: Vec<Element<'_, Message>> = vec![
            text::title3("Ursine").into(),
            text(format!("signed in as {}", self.home.user)).into(),
            text("").height(Length::Fixed(12.0)).into(),
        ];
        for (screen, title) in entries {
            let selected = self.screen == screen;
            let available = self.screen_available(screen);
            let mut entry = if selected {
                button::suggested(title)
            } else {
                button::text(title)
            }
            .width(Length::Fill);
            if !selected && available {
                entry = entry.on_press(Self::navigate(screen));
            }
            items.push(entry.into());
        }
        items.push(text("").height(Length::Fill).into());
        items.push(button::text("Sign out").on_press(Message::SignOut).into());
        let panel = column(items)
            .spacing(4)
            .padding(16)
            .width(Length::Fixed(240.0));
        container(scrollable(panel))
            .class(cosmic::theme::Container::Background)
            .height(Length::Fill)
            .into()
    }

    fn navigate(screen: Screen) -> Message {
        match screen {
            Screen::Home => Message::BackHome,
            Screen::Settings => Message::OpenSettings,
            Screen::Store => Message::OpenStore,
            Screen::Snapshots => Message::OpenSnapshots,
            Screen::Users => Message::OpenUsers,
            Screen::Niri => Message::OpenNiri,
            Screen::Apk => Message::OpenPackages(PackageKind::Apk),
            Screen::Flatpak => Message::OpenPackages(PackageKind::Flatpak),
            Screen::Printers => Message::OpenPrinters,
            Screen::Scanners => Message::OpenScanners,
        }
    }

    fn cap_available(&self, capability: Capability) -> bool {
        self.home
            .caps
            .iter()
            .find(|report| report.capability == capability)
            .is_some_and(|report| report.available)
    }

    fn screen_available(&self, screen: Screen) -> bool {
        match screen {
            Screen::Home | Screen::Settings => true,
            Screen::Store => self.cap_available(Capability::Nix),
            Screen::Snapshots => {
                self.cap_available(Capability::Btrbk) && self.cap_available(Capability::Btrfs)
            }
            Screen::Users => self.cap_available(Capability::Accountsservice),
            Screen::Niri => self.cap_available(Capability::Niri),
            Screen::Apk => self.cap_available(Capability::Apk),
            Screen::Flatpak => self.cap_available(Capability::Flatpak),
            Screen::Printers => self.cap_available(Capability::Cups),
            Screen::Scanners => self.cap_available(Capability::Sane),
        }
    }

    fn token(&self) -> SessionToken {
        self.session
            .as_ref()
            .map_or_else(|| SessionToken::new(""), |session| session.token.clone())
    }

    fn load_printers(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.printers.busy || self.printers.action_busy {
            return Task::none();
        }
        self.printers.busy = true;
        self.printers.error = None;
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(
            async move { Message::PrintersLoaded(epoch, client::printer_page(token).await) },
        )
        .map(cosmic::Action::App)
    }

    fn finish_printers(
        &mut self,
        epoch: u64,
        result: Result<(Vec<ursine_api::Printer>, Vec<ursine_api::PrintJob>), ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.printers.busy = false;
        match result {
            Ok((printers, jobs)) => self.printers.apply(printers, jobs),
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.printers.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn add_printer(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.printers.action_busy {
            return Task::none();
        }
        let name = self.printers.name.trim().to_string();
        let uri = self.printers.uri.trim().to_string();
        if name.is_empty() || uri.is_empty() {
            self.printers.error = Some("Name and URI are required.".into());
            return Task::none();
        }
        let description = self.printers.description.trim().to_string();
        let location = self.printers.location.trim().to_string();
        let token = session.token.clone();
        let epoch = self.epoch;
        self.printers.action_busy = true;
        self.printers.error = None;
        Task::future(async move {
            let result = client::add_printer(token, name, uri, description, location).await;
            Message::PrinterActionFinished(epoch, "add".into(), result)
        })
        .map(cosmic::Action::App)
    }

    fn printer_action(
        &mut self,
        label: &str,
        future: impl std::future::Future<Output = Result<serde_json::Value, ApiError>> + Send + 'static,
    ) -> Task<cosmic::Action<Message>> {
        let Some(_session) = &self.session else {
            return Task::none();
        };
        if self.printers.action_busy {
            return Task::none();
        }
        let epoch = self.epoch;
        let label = label.to_string();
        self.printers.action_busy = true;
        self.printers.error = None;
        Task::future(async move { Message::PrinterActionFinished(epoch, label, future.await) })
            .map(cosmic::Action::App)
    }

    fn finish_printer_action(
        &mut self,
        epoch: u64,
        label: &str,
        result: Result<serde_json::Value, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.printers.action_busy = false;
        match result {
            Ok(_) => {
                self.printers.message = Some(label.to_string());
                self.printers.error = None;
                return self.load_printers();
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.printers.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn load_scanners(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.scanners.busy || self.scanners.action_busy {
            return Task::none();
        }
        self.scanners.busy = true;
        self.scanners.error = None;
        let token = session.token.clone();
        let epoch = self.epoch;
        Task::future(
            async move { Message::ScannersLoaded(epoch, client::scanner_list(token).await) },
        )
        .map(cosmic::Action::App)
    }

    fn finish_scanners(
        &mut self,
        epoch: u64,
        result: Result<Vec<ursine_api::Scanner>, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.scanners.busy = false;
        match result {
            Ok(scanners) => {
                self.scanners.apply(scanners);
                self.scanners.error = None;
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.scanners.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

    fn scan_document(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        if self.scanners.action_busy {
            return Task::none();
        }
        let Some(device) = self
            .scanners
            .selected_device()
            .map(|scanner| scanner.id.clone())
        else {
            self.scanners.error = Some("Select a scanner.".into());
            return Task::none();
        };
        let resolution = [75_u32, 100, 150, 200, 300, 600]
            .get(self.scanners.resolution)
            .copied()
            .unwrap_or(300);
        let mode = ["Color", "Gray", "Lineart"]
            .get(self.scanners.mode)
            .copied()
            .unwrap_or("Color")
            .to_string();
        let source = if self.scanners.source.trim().is_empty() {
            "Flatbed".to_string()
        } else {
            self.scanners.source.trim().to_string()
        };
        let token = session.token.clone();
        let epoch = self.epoch;
        self.scanners.action_busy = true;
        self.scanners.error = None;
        Task::future(async move {
            Message::ScanFinished(
                epoch,
                client::scan_document(token, device, resolution, mode, source).await,
            )
        })
        .map(cosmic::Action::App)
    }

    fn finish_scan(
        &mut self,
        epoch: u64,
        result: Result<ursine_api::ScanResult, ApiError>,
    ) -> Task<cosmic::Action<Message>> {
        if epoch != self.epoch {
            return Task::none();
        }
        self.scanners.action_busy = false;
        match result {
            Ok(result) => {
                self.scanners.message = Some(format!("Saved {}", result.path));
                self.scanners.error = None;
            }
            Err(error) => {
                if self.handle_session_error(&error) {
                    return Task::none();
                }
                self.scanners.error = Some(friendly(&error));
            }
        }
        Task::none()
    }

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
                self.users = UsersState::default();
                self.niri = NiriState::default();
                self.packages = PackagesState::default();
                self.printers = PrintersState::default();
                self.scanners = ScannersState::default();
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
        self.users = UsersState::default();
        self.niri = NiriState::default();
        self.packages = PackagesState::default();
        self.printers = PrintersState::default();
        self.scanners = ScannersState::default();
        self.screen = Screen::Home;
        self.login.password.clear();
        self.login.error = None;
    }
}

fn niri_key_message(
    event: cosmic::iced::Event,
    _status: cosmic::iced::event::Status,
    _window: cosmic::iced::window::Id,
) -> Option<Message> {
    let cosmic::iced::Event::Keyboard(cosmic::iced::keyboard::Event::KeyPressed {
        key,
        modifiers,
        ..
    }) = event
    else {
        return None;
    };
    let key = key.as_ref();
    niri_combo(modifiers, &key).map(Message::NiriKeyCaptured)
}

fn niri_combo(
    modifiers: cosmic::iced::keyboard::Modifiers,
    key: &cosmic::iced::keyboard::Key<&str>,
) -> Option<String> {
    use cosmic::iced::keyboard::key::Named;
    let key = match key {
        cosmic::iced::keyboard::Key::Character(character) => character.to_uppercase(),
        cosmic::iced::keyboard::Key::Named(named) => match named {
            Named::Control | Named::Shift | Named::Alt | Named::Super | Named::Meta => return None,
            other => format!("{other:?}"),
        },
        cosmic::iced::keyboard::Key::Unidentified => return None,
    };
    let mut combo = Vec::new();
    if modifiers.logo() {
        combo.push("Mod".to_string());
    }
    if modifiers.control() {
        combo.push("Ctrl".to_string());
    }
    if modifiers.alt() {
        combo.push("Alt".to_string());
    }
    if modifiers.shift() {
        combo.push("Shift".to_string());
    }
    combo.push(key);
    Some(combo.join("+"))
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
