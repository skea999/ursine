//! Module handlers. v0.1: gating + wiring for `system`, `capabilities`,
//! `config`; every other action returns `NotImplemented` (but still runs
//! `ensure_available` first, so gating is exercised end to end).

use serde_json::json;
use ursine_api::{Action, ApiError, Capability, ModuleId, Response, UrsineConfig};

pub mod bluetooth;
pub mod disks;
pub mod firewall;
pub mod network;
pub mod niri;
pub mod packages_apk;
pub mod packages_flatpak;
pub mod packages_nix;
pub mod printers;
pub mod scanners;
pub mod services;
pub mod snapshots;
pub mod users;

use crate::capabilities::{module_switch, Registry};

/// Everything a handler may touch (no daemon internals).
#[derive(Debug)]
pub struct ModuleCtx<'a> {
    pub caps: &'a Registry,
    /// Snapshot of the daemon's live configuration for this request.
    pub config: UrsineConfig,
    /// Resolved target user (defaults to the principal).
    pub as_user: &'a str,
}

/// Gating requirements per module.
fn required_caps(module: ModuleId) -> &'static [Capability] {
    match module {
        ModuleId::PackagesNix => &[Capability::Nix],
        ModuleId::PackagesApk => &[Capability::Apk],
        ModuleId::PackagesFlatpak => &[Capability::Flatpak],
        ModuleId::Printers => &[Capability::Cups],
        ModuleId::Scanners => &[Capability::Sane],
        ModuleId::Firewall => &[Capability::Firewalld],
        ModuleId::Disks => &[Capability::Udisks2],
        ModuleId::Bluetooth => &[Capability::Bluez],
        ModuleId::Network => &[Capability::NetworkManager],
        ModuleId::Snapshots => &[Capability::Btrbk, Capability::Btrfs],
        ModuleId::Users => &[Capability::Accountsservice],
        ModuleId::Niri => &[Capability::Niri],
        ModuleId::Services | ModuleId::System | ModuleId::Config | ModuleId::Capabilities => &[],
    }
}

/// Route a validated call. `NotImplemented` for everything beyond v0.1 wiring.
pub async fn handle(
    module: ModuleId,
    action: &Action,
    ctx: &ModuleCtx<'_>,
) -> Result<serde_json::Value, ApiError> {
    // Binding rule: capability gate before ANY module action.
    if module == ModuleId::Services {
        let switch = module_switch(&ctx.config.modules, Capability::OpenRc);
        ctx.caps.ensure_available(switch, &[])?;
        if !ctx.caps.check(Capability::OpenRc) && !ctx.caps.check(Capability::Dinit) {
            return Err(ApiError::unavailable(
                "no supported init service manager",
                "boot OpenRC or dinit",
            ));
        }
    } else {
        for &cap in required_caps(module) {
            let switch = module_switch(&ctx.config.modules, cap);
            ctx.caps.ensure_available(switch, &[cap])?;
        }
    }

    match (module, action) {
        (ModuleId::System, Action::Status) => Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "protocol_version": ursine_api::PROTOCOL_VERSION,
            "auth": if cfg!(feature = "pam") { "pam" } else { "stub" },
        })),
        (ModuleId::Capabilities, Action::List) => Ok(json!(ctx.caps.reports())),
        (ModuleId::Capabilities, Action::Refresh) => {
            ctx.caps.refresh();
            Ok(json!(ctx.caps.reports()))
        }
        (ModuleId::PackagesNix, action) => packages_nix::handle(action, ctx).await,

        (ModuleId::PackagesApk, action) => packages_apk::handle(action, ctx).await,

        (ModuleId::PackagesFlatpak, action) => packages_flatpak::handle(action, ctx).await,

        (ModuleId::Printers, action) => printers::handle(action, ctx).await,

        (ModuleId::Scanners, action) => scanners::handle(action, ctx).await,

        (ModuleId::Firewall, action) => firewall::handle(action, ctx).await,

        (ModuleId::Services, action) => services::handle(action, ctx).await,

        (ModuleId::Disks, action) => disks::handle(action, ctx).await,

        (ModuleId::Bluetooth, action) => bluetooth::handle(action, ctx).await,

        (ModuleId::Network, action) => network::handle(action, ctx).await,

        (ModuleId::Snapshots, action) => snapshots::handle(action, ctx).await,

        (ModuleId::Users, action) => users::handle(action, ctx).await,

        (ModuleId::Niri, action) => niri::handle(action, ctx).await,

        _ => Err(ApiError::invalid(format!(
            "action {} is not valid for module {}",
            action_name(action),
            module.as_str()
        ))),
    }
}

/// Stable action name for audit lines.
pub fn action_name(action: &Action) -> &'static str {
    match action {
        Action::Status => "status",
        Action::Init => "init",
        Action::GetConfig => "get_config",
        Action::SetModuleSwitch { .. } => "set_module_switch",
        Action::List => "list",
        Action::Refresh => "refresh",
        Action::ListInstalled => "list_installed",
        Action::ListApk => "list_apk",
        Action::SearchApk { .. } => "search_apk",
        Action::RefreshApk => "refresh_apk",
        Action::ListApkRepositories => "list_apk_repositories",
        Action::InstallApk { .. } => "install_apk",
        Action::RemoveApk { .. } => "remove_apk",
        Action::UpgradeApk => "upgrade_apk",
        Action::ListApkUpdates => "list_apk_updates",
        Action::UpgradeApkPackage { .. } => "upgrade_apk_package",
        Action::ListFlatpak { .. } => "list_flatpak",
        Action::ListFlatpakRemotes => "list_flatpak_remotes",
        Action::ListPrinters => "list_printers",
        Action::ListScanners => "list_scanners",
        Action::GetScannerConfig => "get_scanner_config",
        Action::SetScannerDevice { .. } => "set_scanner_device",
        Action::RemoveScannerDevice { .. } => "remove_scanner_device",
        Action::SetScannerDiscovery { .. } => "set_scanner_discovery",
        Action::PrinterJobs { .. } => "printer_jobs",
        Action::AddPrinter { .. } => "add_printer",
        Action::AddPrinterManual { .. } => "add_printer_manual",
        Action::RemovePrinter { .. } => "remove_printer",
        Action::SetDefaultPrinter { .. } => "set_default_printer",
        Action::SetPrinterOptions { .. } => "set_printer_options",
        Action::PausePrinter { .. } => "pause_printer",
        Action::CancelPrinterJobs { .. } => "cancel_printer_jobs",
        Action::SearchFlatpak { .. } => "search_flatpak",
        Action::InstallFlatpak { .. } => "install_flatpak",
        Action::RemoveFlatpak { .. } => "remove_flatpak",
        Action::UpdateFlatpak { .. } => "update_flatpak",
        Action::UpdateFlatpakApp { .. } => "update_flatpak_app",
        Action::RemoveFlatpakOrphans { .. } => "remove_flatpak_orphans",
        Action::Search { .. } => "search",
        Action::Install { .. } => "install",
        Action::Remove { .. } => "remove",
        Action::RemoveAll => "remove_all",
        Action::RemoveRegex { .. } => "remove_regex",
        Action::Upgrade { .. } => "upgrade",
        Action::UpgradeRegex { .. } => "upgrade_regex",
        Action::Generations => "generations",
        Action::Rollback { .. } => "rollback",
        Action::WipeHistory { .. } => "wipe_history",
        Action::DiffClosures { .. } => "diff_closures",
        Action::StoreGc => "store_gc",
        Action::PruneGenerations { .. } => "prune_generations",
        Action::ListSnapshots => "list_snapshots",
        Action::Stats => "stats",
        Action::CreateSnapshot { .. } => "create_snapshot",
        Action::RestoreSnapshot { .. } => "restore_snapshot",
        Action::GetBtrbkConfig => "get_btrbk_config",
        Action::InitBtrbkConfig => "init_btrbk_config",
        Action::SetBtrbkConfig { .. } => "set_btrbk_config",
        Action::DeleteSnapshot { .. } => "delete_snapshot",
        Action::DeleteSnapshots { .. } => "delete_snapshots",
        Action::ListUsers => "list_users",
        Action::CreateUser { .. } => "create_user",
        Action::DeleteUser { .. } => "delete_user",
        Action::SetPassword { .. } => "set_password",
        Action::ListGroups => "list_groups",
        Action::SetGroups { .. } => "set_groups",
        Action::GetNiriConfig => "get_niri_config",
        Action::ValidateNiriConfig { .. } => "validate_niri_config",
        Action::SetNiriSection { .. } => "set_niri_section",
        Action::GetFirewall => "get_firewall",
        Action::SetDefaultFirewallZone { .. } => "set_default_firewall_zone",
        Action::AddFirewallService { .. } => "add_firewall_service",
        Action::RemoveFirewallService { .. } => "remove_firewall_service",
        Action::AddFirewallPort { .. } => "add_firewall_port",
        Action::RemoveFirewallPort { .. } => "remove_firewall_port",
        Action::ReloadFirewall => "reload_firewall",
        Action::ListServices => "list_services",
        Action::ServiceAction { .. } => "service_action",
        Action::ListDisks => "list_disks",
        Action::MountDisk { .. } => "mount_disk",
        Action::UnmountDisk { .. } => "unmount_disk",
        Action::GetBluetooth => "get_bluetooth",
        Action::SetBluetoothPower { .. } => "set_bluetooth_power",
        Action::ScanBluetooth => "scan_bluetooth",
        Action::BluetoothAction { .. } => "bluetooth_action",
        Action::GetNetwork => "get_network",
        Action::SetWifiEnabled { .. } => "set_wifi_enabled",
        Action::ConnectWifi { .. } => "connect_wifi",
        Action::DisconnectNetwork { .. } => "disconnect_network",
    }
}

/// Response helper (kept here so dispatch stays lean).
pub fn ok(id: Option<u64>, value: serde_json::Value) -> Response {
    Response::ok(id, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::checks::Sysinfo;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    struct AllMissing;
    impl Sysinfo for AllMissing {
        fn path_exists(&self, _p: &Path) -> bool {
            false
        }
        fn read_trimmed(&self, _p: &Path) -> Option<String> {
            None
        }
        fn cmd_in_path(&self, _c: &str) -> bool {
            false
        }
    }

    fn ctx_with<S: Sysinfo + 'static>(sys: S) -> (Registry, UrsineConfig, PathBuf) {
        (
            Registry::new(Arc::new(sys)),
            UrsineConfig::default(),
            PathBuf::from("/tmp/none.toml"),
        )
    }

    #[tokio::test]
    async fn system_status_reports_stub_auth_in_dev() {
        let (caps, cfg, _path) = ctx_with(AllMissing);
        let ctx = ModuleCtx {
            caps: &caps,
            config: cfg.clone(),
            as_user: "u",
        };
        let v = handle(ModuleId::System, &Action::Status, &ctx)
            .await
            .unwrap();
        assert_eq!(
            v["auth"],
            if cfg!(feature = "pam") { "pam" } else { "stub" }
        );
        assert_eq!(v["protocol_version"], ursine_api::PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn capabilities_list_returns_all_fifteen() {
        let (caps, cfg, _path) = ctx_with(AllMissing);
        let ctx = ModuleCtx {
            caps: &caps,
            config: cfg.clone(),
            as_user: "u",
        };
        let v = handle(ModuleId::Capabilities, &Action::List, &ctx)
            .await
            .unwrap();
        assert_eq!(v.as_array().unwrap().len(), 15);
    }

    #[tokio::test]
    async fn package_actions_gated_before_notimplemented() {
        let (caps, cfg, _path) = ctx_with(AllMissing);
        let ctx = ModuleCtx {
            caps: &caps,
            config: cfg.clone(),
            as_user: "u",
        };
        let err = handle(ModuleId::PackagesNix, &Action::ListInstalled, &ctx)
            .await
            .unwrap_err();
        // nix missing on fake machine -> gating error wins over NotImplemented
        assert_eq!(err.code, ursine_api::ErrorCode::Unavailable);
        assert!(err.fix.is_some());
    }

    #[tokio::test]
    async fn disabled_module_is_unavailable_even_if_forced_off() {
        let (caps, mut cfg, _path) = ctx_with(AllMissing);
        cfg.modules.packages_nix = ursine_api::TriState::Off;
        let ctx = ModuleCtx {
            caps: &caps,
            config: cfg.clone(),
            as_user: "u",
        };
        let err = handle(
            ModuleId::PackagesNix,
            &Action::Search {
                query: "hello".into(),
            },
            &ctx,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ursine_api::ErrorCode::Unavailable);
    }

    #[tokio::test]
    async fn invalid_module_action_combo_is_rejected() {
        let (caps, cfg, _path) = ctx_with(AllMissing);
        let ctx = ModuleCtx {
            caps: &caps,
            config: cfg.clone(),
            as_user: "u",
        };
        let err = handle(
            ModuleId::System,
            &Action::Install {
                package: "x".into(),
            },
            &ctx,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ursine_api::ErrorCode::Invalid);
    }
}
