//! Per-module action payloads.
//!
//! One flat enum keeps the wire format simple; `dispatch` validates that the
//! `(ModuleId, Action)` pair is legal. Variants cover the v1 spec surface even
//! where the handler still returns `NotImplemented`.

use serde::{Deserialize, Serialize};

use crate::ModuleId;

/// Serialized with an internal `action` tag: `{"action":"list","query":null}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    // -- system --
    Status,

    // -- config --
    /// Write a commented `/etc/ursine/config.toml` from a capability scan.
    Init,
    /// Return the current daemon configuration module switches.
    GetConfig,
    /// Persist one module switch (`"auto"`, true or false) and apply it live.
    SetModuleSwitch {
        module: ModuleId,
        value: crate::TriState,
    },

    // -- capabilities --
    /// List every capability with availability, reason and fix.
    List,
    /// Re-run detection now.
    Refresh,

    // -- jobs --
    /// Poll a background job log from byte `offset` (package progress).
    JobOutput {
        job_id: String,
        #[serde(default)]
        offset: u64,
    },

    // -- packages.nix --
    /// List installed packages in the target user's profile.
    ListInstalled,
    /// Search nixpkgs (slow: UI shows progress).
    Search {
        query: String,
    },
    /// Install a package into the target user's profile.
    Install {
        package: String,
    },
    /// Remove a package from the target user's profile.
    Remove {
        package: String,
    },
    /// Remove every package from the profile.
    RemoveAll,
    /// Remove all packages whose name matches a regex. `dry_run` lists matches.
    RemoveRegex {
        pattern: String,
        #[serde(default)]
        dry_run: bool,
    },
    /// Upgrade one package (or all when `all`).
    Upgrade {
        package: Option<String>,
        all: bool,
    },
    /// Upgrade all packages whose name matches a regex. `dry_run` lists matches.
    UpgradeRegex {
        pattern: String,
        #[serde(default)]
        dry_run: bool,
    },
    /// Profile history (generations).
    Generations,
    /// Roll the profile back to a previous generation (default: previous).
    Rollback {
        generation: Option<u32>,
    },
    /// Delete old profile generations (destructive). Dry-run supported.
    WipeHistory {
        older_than: Option<String>,
        dry_run: bool,
    },
    /// Show what changed between generations (human-readable closure diff).
    DiffClosures {
        generation: Option<u32>,
    },
    /// Garbage-collect the whole nix store (unreachable paths only).
    StoreGc,
    /// Prune profile generations, keeping the last `keep` (`dry_run` = no changes).
    PruneGenerations {
        keep: u32,
        dry_run: bool,
        /// Current generation seen at preview time. Apply refuses when the
        /// profile changed in between, so the preview stays exact.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_current: Option<u32>,
        /// Exact generation list the preview would delete.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_delete: Option<Vec<u32>>,
    },

    // -- packages.apk --
    /// Installed APK packages.
    ListApk,
    /// Search configured APK repositories.
    SearchApk {
        query: String,
    },
    /// Refresh APK repository indexes.
    RefreshApk,
    /// Configured APK repositories.
    ListApkRepositories,
    /// Install an APK package.
    InstallApk {
        package: String,
    },
    /// Simulate an APK install and return the dependency/size plan.
    PreviewApkInstall {
        package: String,
    },
    /// Remove an APK package.
    RemoveApk {
        package: String,
    },
    /// Upgrade every installed APK package.
    UpgradeApk,
    /// Installed APK packages with a newer version in the repositories.
    ListApkUpdates,
    /// Upgrade one installed APK package.
    UpgradeApkPackage {
        package: String,
    },

    // -- packages.flatpak --
    /// Installed Flatpak applications for system or user installation.
    ListFlatpak {
        system: bool,
    },
    /// Configured Flatpak remotes.
    ListFlatpakRemotes,
    /// Search the configured Flatpak remotes.
    SearchFlatpak {
        query: String,
        system: bool,
    },
    /// Install a Flatpak application from `flathub`.
    InstallFlatpak {
        app_id: String,
        system: bool,
    },
    /// Remove a Flatpak application.
    RemoveFlatpak {
        app_id: String,
        system: bool,
    },
    /// Update every Flatpak application in the selected installation.
    UpdateFlatpak {
        system: bool,
    },
    /// Update one Flatpak application in the selected installation.
    UpdateFlatpakApp {
        app_id: String,
        system: bool,
    },
    /// Remove unused Flatpak runtimes.
    RemoveFlatpakOrphans {
        system: bool,
    },

    // -- printers (CUPS) --
    /// List CUPS printers and the system default.
    ListPrinters,
    /// List queued jobs, optionally for one printer.
    PrinterJobs {
        printer: Option<String>,
    },
    /// Add a driverless IPP/USB printer.
    AddPrinter {
        name: String,
        uri: String,
        description: String,
        location: String,
    },
    /// Add a printer with an explicit driver or PPD and options.
    AddPrinterManual {
        name: String,
        uri: String,
        driver: String,
        ppd: String,
        description: String,
        location: String,
        options: Vec<String>,
    },
    /// Remove a printer.
    RemovePrinter {
        name: String,
    },
    /// Set the system default printer.
    SetDefaultPrinter {
        name: String,
    },
    /// Set CUPS printer options (`key=value`, e.g. media, Duplex, `ColorModel`).
    SetPrinterOptions {
        name: String,
        options: Vec<String>,
    },
    /// Pause or resume a printer.
    PausePrinter {
        name: String,
        pause: bool,
    },
    /// Cancel queued jobs, optionally for one printer.
    CancelPrinterJobs {
        printer: Option<String>,
    },

    // -- scanners (SANE) --
    /// List SANE scanner devices.
    ListScanners,
    /// Read `/etc/sane.d/airscan.conf` plus detected devices.
    GetScannerConfig,
    /// Add or update a manual airscan device.
    SetScannerDevice {
        name: String,
        address: String,
        protocol: String,
    },
    /// Remove a manual airscan device.
    RemoveScannerDevice {
        name: String,
    },
    /// Enable or disable airscan discovery.
    SetScannerDiscovery {
        enabled: bool,
    },

    // -- network (NetworkManager) --
    /// Devices, active connections and visible Wi-Fi networks.
    GetNetwork,
    /// Turn the Wi-Fi radio on or off.
    SetWifiEnabled {
        enabled: bool,
    },
    /// Connect to a Wi-Fi SSID with an optional password.
    ConnectWifi {
        ssid: String,
        password: String,
    },
    /// Connect one network device.
    ConnectNetwork {
        device: String,
    },
    /// Disconnect one network device.
    DisconnectNetwork {
        device: String,
    },

    // -- bluetooth (BlueZ) --
    /// Adapter state and known devices.
    GetBluetooth,
    /// Turn the adapter on or off.
    SetBluetoothPower {
        on: bool,
    },
    /// Scan for nearby devices, then return the updated list.
    ScanBluetooth,
    /// Pair/connect/disconnect/trust/forget one device.
    BluetoothAction {
        address: String,
        op: crate::BluetoothOp,
    },

    // -- disks (UDisks2) --
    /// List block devices and partitions.
    ListDisks,
    /// Mount a block device.
    MountDisk {
        device: String,
    },
    /// Unmount a block device.
    UnmountDisk {
        device: String,
    },

    // -- services (OpenRC/dinit) --
    /// List init services and their enabled state.
    ListServices,
    /// Start/stop/restart/enable/disable one service.
    ServiceAction {
        name: String,
        op: crate::ServiceOp,
    },

    // -- firewall (firewalld) --
    /// Read zones, default zone and services/ports.
    GetFirewall,
    /// Set the default zone.
    SetDefaultFirewallZone {
        zone: String,
    },
    /// Open a service in a zone permanently and reload.
    AddFirewallService {
        zone: String,
        service: String,
    },
    /// Close a service in a zone permanently and reload.
    RemoveFirewallService {
        zone: String,
        service: String,
    },
    /// Open a port in a zone permanently and reload.
    AddFirewallPort {
        zone: String,
        port: String,
    },
    /// Close a port in a zone permanently and reload.
    RemoveFirewallPort {
        zone: String,
        port: String,
    },
    /// Reload firewalld.
    ReloadFirewall,

    // -- snapshots (btrbk) --
    /// List btrbk-managed snapshots.
    ListSnapshots,
    /// btrbk stats output.
    Stats,
    /// Run a snapshot-only btrbk pass for a config section.
    CreateSnapshot {
        section: Option<String>,
    },
    /// Restore a snapshot to a target. `dry_run` returns the preview.
    RestoreSnapshot {
        snapshot: String,
        target: String,
        dry_run: bool,
    },
    /// Parsed view of `/etc/btrbk/btrbk.conf`.
    GetBtrbkConfig,
    /// Write a commented default btrbk.conf from detected subvolumes.
    InitBtrbkConfig,
    /// Update managed global btrbk options.
    SetBtrbkConfig {
        settings: Box<crate::BtrbkSettings>,
    },
    /// Delete one btrbk snapshot (destructive).
    DeleteSnapshot {
        snapshot: String,
    },
    /// Delete all snapshots for the selected source subvolumes (empty = all).
    DeleteSnapshots {
        sources: Vec<String>,
    },

    // -- users --
    /// List human users (uid >= 1000).
    ListUsers,
    /// Create a user; `admin` adds wheel membership.
    CreateUser {
        name: String,
        fullname: String,
        admin: bool,
    },
    /// Delete a user (confirmation enforced by the frontend).
    DeleteUser {
        uid: u32,
        remove_files: bool,
    },
    /// Set a user's password (value never logged or audited).
    SetPassword {
        uid: u32,
        new_password: String,
    },
    /// List system groups.
    ListGroups,
    /// Replace a user's supplementary groups.
    SetGroups {
        uid: u32,
        groups: Vec<String>,
    },

    // -- niri --
    /// Schema-typed view of the user's config.kdl.
    GetNiriConfig,
    /// Run `niri validate` against a proposed config (no write).
    ValidateNiriConfig {
        proposed: Option<String>,
    },
    /// Apply one section of form values (schema-driven).
    SetNiriSection {
        section: String,
        value: serde_json::Value,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_tag_is_present_and_snake_case() {
        let json = serde_json::to_value(&Action::ListInstalled).unwrap();
        assert_eq!(json["action"], "list_installed");
    }

    #[test]
    fn apk_and_flatpak_actions_are_tagged() {
        let apk = serde_json::to_value(Action::InstallApk {
            package: "niri".into(),
        })
        .unwrap();
        assert_eq!(apk["action"], "install_apk");
        let flatpak = serde_json::to_value(Action::ListFlatpak { system: true }).unwrap();
        assert_eq!(flatpak["action"], "list_flatpak");
    }

    #[test]
    fn config_switch_payload_roundtrips() {
        let a = Action::SetModuleSwitch {
            module: ModuleId::PackagesNix,
            value: crate::TriState::On,
        };
        let s = serde_json::to_string(&a).unwrap();
        let back: Action = serde_json::from_str(&s).unwrap();
        assert_eq!(a, back);
        assert!(s.contains("\"action\":\"set_module_switch\""));
        assert!(s.contains("\"module\":\"packages_nix\""));
        assert!(s.contains("\"value\":true"));
    }

    #[test]
    fn regex_actions_default_to_apply_for_older_clients() {
        let parsed: Action = serde_json::from_value(serde_json::json!({
            "action": "remove_regex",
            "pattern": "^python",
        }))
        .unwrap();
        assert_eq!(
            parsed,
            Action::RemoveRegex {
                pattern: "^python".into(),
                dry_run: false,
            }
        );
        let preview: Action = serde_json::from_value(serde_json::json!({
            "action": "upgrade_regex",
            "pattern": "^python",
            "dry_run": true,
        }))
        .unwrap();
        assert_eq!(
            preview,
            Action::UpgradeRegex {
                pattern: "^python".into(),
                dry_run: true,
            }
        );
    }

    #[test]
    fn prune_expected_generation_is_optional_on_the_wire() {
        let old = serde_json::json!({
            "action": "prune_generations",
            "keep": 3,
            "dry_run": true,
        });
        let parsed: Action = serde_json::from_value(old).unwrap();
        assert_eq!(
            parsed,
            Action::PruneGenerations {
                keep: 3,
                dry_run: true,
                expected_current: None,
                expected_delete: None,
            }
        );
        let with_preview = Action::PruneGenerations {
            keep: 3,
            dry_run: false,
            expected_current: Some(7),
            expected_delete: Some(vec![3, 4]),
        };
        let json = serde_json::to_value(&with_preview).unwrap();
        assert_eq!(json["expected_current"], 7);
        assert_eq!(
            serde_json::from_value::<Action>(json).unwrap(),
            with_preview
        );
    }

    #[test]
    fn job_output_action_roundtrips() {
        let a = Action::JobOutput {
            job_id: "deadbeef".into(),
            offset: 42,
        };
        let s = serde_json::to_string(&a).unwrap();
        assert!(s.contains("\"action\":\"job_output\""));
        assert_eq!(serde_json::from_str::<Action>(&s).unwrap(), a);
        let parsed: Action = serde_json::from_value(serde_json::json!({
            "action": "job_output",
            "job_id": "deadbeef",
        }))
        .unwrap();
        assert_eq!(
            parsed,
            Action::JobOutput {
                job_id: "deadbeef".into(),
                offset: 0,
            }
        );
    }

    #[test]
    fn payload_roundtrip() {
        let a = Action::Install {
            package: "hello".into(),
        };
        let s = serde_json::to_string(&a).unwrap();
        let back: Action = serde_json::from_str(&s).unwrap();
        assert_eq!(a, back);

        let r = Action::RestoreSnapshot {
            snapshot: "@snap".into(),
            target: "/mnt/restore".into(),
            dry_run: true,
        };
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Action>(&s).unwrap(), r);
    }
}
