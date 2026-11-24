//! Per-module action payloads.
//!
//! One flat enum keeps the wire format simple; `dispatch` validates that the
//! `(ModuleId, Action)` pair is legal. Variants cover the v1 spec surface even
//! where the handler still returns `NotImplemented`.
//!
//! Package actions carry their module in the wire name (`install_apk`,
//! `install_nix`, …), so an audit line or a client log stays unambiguous
//! without the module next to it. The nix actions used bare names before that
//! rule existed; each renamed variant keeps its old name as a deserialization
//! alias, so a client or script written against the bare name keeps working.

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
    /// Persist the per-manager "check updates automatically" preference
    /// (`manager` = apk | flatpak | nix) and apply it live.
    SetUpdateAuto {
        manager: String,
        enabled: bool,
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
    #[serde(rename = "list_installed_nix", alias = "list_installed")]
    ListInstalled,
    /// Search a registry flake (default `nixpkgs`; slow: UI shows progress).
    #[serde(rename = "search_nix", alias = "search")]
    Search {
        query: String,
        /// Registry flake to search (default `nixpkgs`); bare installables
        /// in the results come back prefixed with it.
        #[serde(default)]
        repo: Option<String>,
    },
    /// Configured repositories for one scope: registry entries
    /// (`nix registry list`) plus channels (`nix-channel --list`).
    /// Builtin registry defaults are never returned.
    ListNixRepositories {
        /// `true` = the system registry and root's channels; `false` =
        /// the target user's.
        #[serde(default)]
        system: bool,
    },
    /// Add or replace a registry entry in the chosen scope.
    AddNixRepository {
        name: String,
        url: String,
        /// `true` = system registry file (root-owned); `false` = the
        /// target user's registry via `nix registry add`.
        #[serde(default)]
        system: bool,
    },
    /// Remove one configured repository entry.
    RemoveNixRepository {
        name: String,
        /// `true` = system scope, `false` = the target user's.
        #[serde(default)]
        system: bool,
        /// Registry entry or channel (the row says which).
        #[serde(default)]
        source: crate::NixRepoSource,
    },
    /// Install a package into the target user's profile.
    #[serde(rename = "install_nix", alias = "install")]
    Install {
        package: String,
    },
    /// Remove a package from the target user's profile.
    #[serde(rename = "remove_nix", alias = "remove")]
    Remove {
        package: String,
    },
    /// Remove every package from the profile.
    #[serde(rename = "remove_all_nix", alias = "remove_all")]
    RemoveAll,
    /// Remove all packages whose name matches a regex. `dry_run` lists matches.
    #[serde(rename = "remove_regex_nix", alias = "remove_regex")]
    RemoveRegex {
        pattern: String,
        #[serde(default)]
        dry_run: bool,
    },
    /// Upgrade one package (or all when `all`).
    ///
    /// `all` is optional on the wire and defaults to `false` (upgrade the one
    /// named package), so a client that only names a package does not have to
    /// send the flag. A request that names neither is refused with `invalid`
    /// rather than silently upgrading nothing.
    #[serde(rename = "upgrade_nix", alias = "upgrade")]
    Upgrade {
        package: Option<String>,
        #[serde(default)]
        all: bool,
    },
    /// Upgrade all packages whose name matches a regex. `dry_run` lists matches.
    #[serde(rename = "upgrade_regex_nix", alias = "upgrade_regex")]
    UpgradeRegex {
        pattern: String,
        #[serde(default)]
        dry_run: bool,
    },
    /// Report upgradable packages via `nix profile list` + one
    /// `nix flake metadata` lock compare per flake reference
    /// (network; on demand).
    ListNixUpdates,
    /// Profile history (generations).
    #[serde(rename = "generations_nix", alias = "generations")]
    Generations,
    /// Roll the profile back to a previous generation (default: previous).
    #[serde(rename = "rollback_nix", alias = "rollback")]
    Rollback {
        generation: Option<u32>,
    },
    /// Delete old profile generations (destructive). Dry-run supported.
    ///
    /// `dry_run` stays **required** on purpose: a destructive call has to
    /// state whether it previews or applies, and a silent `false` default is
    /// the one default that cannot be taken back.
    #[serde(rename = "wipe_history_nix", alias = "wipe_history")]
    WipeHistory {
        older_than: Option<String>,
        dry_run: bool,
    },
    /// Show what changed between generations (human-readable closure diff).
    #[serde(rename = "diff_closures_nix", alias = "diff_closures")]
    DiffClosures {
        generation: Option<u32>,
    },
    /// Garbage-collect the whole nix store (unreachable paths only).
    #[serde(rename = "store_gc_nix", alias = "store_gc")]
    StoreGc,
    /// Prune profile generations, keeping the last `keep` (`dry_run` = no changes).
    ///
    /// `dry_run` stays **required** for the same reason as [`Action::WipeHistory`]:
    /// a deletion is never the implied default.
    #[serde(rename = "prune_generations_nix", alias = "prune_generations")]
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
    /// Configured APK repositories.
    ListApkRepositories,
    /// Add an APK repository line (`tag` masks it: `@tag url` is
    /// ignored by apk unless a world entry pins `pkg@tag`). If the URL
    /// already exists the line's tag is updated (`None` unmasks).
    AddApkRepository {
        url: String,
        #[serde(default)]
        tag: Option<String>,
    },
    /// Delete an APK repository line by URL.
    RemoveApkRepository {
        url: String,
    },
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
    ///
    /// Read-only by default: the diff is answered from the cached index, so
    /// polling the action writes nothing and needs no network. `refresh =
    /// true` runs `apk update` first (that request rewrites the index cache),
    /// which is what the explicit "Check for updates" control sends.
    ListApkUpdates {
        #[serde(default)]
        refresh: bool,
    },
    /// Upgrade one installed APK package (`apk upgrade <package>`; a package
    /// that is not installed answers `not_found`).
    UpgradeApkPackage {
        package: String,
    },

    // -- packages.flatpak --
    // Every action here selects the installation with `system`; the field is
    // optional on the wire and defaults to `false` (the target user's
    // installation, the same value an explicit `--user` request carries).
    // `ListFlatpakRemotes` is the exception: its default is `None`, which
    // keeps the legacy both-installations listing.
    /// Installed Flatpak applications for system or user installation.
    ListFlatpak {
        #[serde(default)]
        system: bool,
    },
    /// Configured Flatpak remotes. `system` selects the installation;
    /// `None` keeps the legacy both-installations listing.
    ListFlatpakRemotes {
        #[serde(default)]
        system: Option<bool>,
    },
    /// Refs with updates available on their remotes
    /// (`flatpak remote-ls --updates`, apps and runtimes).
    ListFlatpakUpdates {
        #[serde(default)]
        system: bool,
    },
    /// Add a Flatpak remote to the selected installation
    /// (`flatpak remote-add`; errors when the name exists).
    AddFlatpakRemote {
        name: String,
        url: String,
        #[serde(default)]
        system: bool,
    },
    /// Remove a Flatpak remote (`flatpak remote-delete`; refuses while
    /// packages still come from it unless flatpak itself allows it).
    RemoveFlatpakRemote {
        name: String,
        #[serde(default)]
        system: bool,
    },
    /// Search the configured Flatpak remotes.
    SearchFlatpak {
        query: String,
        #[serde(default)]
        system: bool,
    },
    /// Install a Flatpak application from a configured remote (default
    /// `flathub`).
    InstallFlatpak {
        app_id: String,
        #[serde(default)]
        system: bool,
        #[serde(default)]
        remote: Option<String>,
    },
    /// Resolve one ref on a remote: name, version and download/installed
    /// size for the install confirmation.
    GetFlatpakInfo {
        app_id: String,
        #[serde(default)]
        remote: Option<String>,
        #[serde(default)]
        system: bool,
    },
    /// Remove a Flatpak application.
    RemoveFlatpak {
        app_id: String,
        #[serde(default)]
        system: bool,
    },
    /// Preview/apply removing installed apk packages matching a regex
    /// (the apk CLI has no regex: the daemon matches names itself).
    ApkRemoveRegex {
        pattern: String,
        #[serde(default)]
        dry_run: bool,
    },
    /// Preview/apply upgrading installed apk packages matching a regex.
    ApkUpgradeRegex {
        pattern: String,
        #[serde(default)]
        dry_run: bool,
    },
    /// Preview/apply removing the installed Flatpak refs that match a regex.
    ///
    /// The pattern is tested against the ref id and its display name, over
    /// applications AND runtimes — a pattern such as `Platform` selects
    /// runtimes. The dry run answers `would_change` (the ids) plus `matches`
    /// (one row per ref with `app_id`, `name`, `branch` and `kind`, so a
    /// runtime and an id installed on several branches stay visible).
    FlatpakRemoveRegex {
        pattern: String,
        #[serde(default)]
        dry_run: bool,
        #[serde(default)]
        system: bool,
    },
    /// Preview/apply updating the installed Flatpak refs that match a regex.
    ///
    /// Same matching and same dry-run answer as [`Action::FlatpakRemoveRegex`]:
    /// applications and runtimes both match, and the preview names each ref's
    /// `kind` and `branch`.
    FlatpakUpgradeRegex {
        pattern: String,
        #[serde(default)]
        dry_run: bool,
        #[serde(default)]
        system: bool,
    },
    /// Update every Flatpak application in the selected installation.
    UpdateFlatpak {
        #[serde(default)]
        system: bool,
    },
    /// Update one Flatpak application in the selected installation.
    UpdateFlatpakApp {
        app_id: String,
        #[serde(default)]
        system: bool,
    },
    /// Remove unused Flatpak runtimes.
    RemoveFlatpakOrphans {
        #[serde(default)]
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
    /// Delete a saved connection profile (Wi-Fi passwords included).
    ForgetNetwork {
        connection: String,
    },
    /// Activate or deactivate a saved connection profile (VPN and
    /// generic connection enable/disable).
    SetNetworkConnection {
        connection: String,
        up: bool,
    },
    /// Set IPv4 on a connection profile: `dhcp` (clears addresses,
    /// gateway and DNS) or `static` (requires addresses). An active
    /// connection is re-activated to apply the change.
    SetNetworkIpv4 {
        connection: String,
        /// `dhcp` or `static`.
        method: String,
        #[serde(default)]
        addresses: Vec<String>,
        #[serde(default)]
        gateway: Option<String>,
        #[serde(default)]
        dns: Vec<String>,
    },

    /// Format a block device. Requires `disks_format = true` in the
    /// daemon config, `confirm_device` to equal `device` (typed exactly)
    /// and `--yes` in the CLI.
    FormatDisk {
        device: String,
        /// `ext4`, `xfs`, `btrfs`, `vfat`, `exfat`, `f2fs`, `ntfs`.
        fstype: String,
        /// Must equal `device`.
        confirm_device: String,
    },
    /// Create a partition (MiB start/end on an existing partition table).
    PartitionCreate {
        device: String,
        start_mib: u64,
        end_mib: u64,
        /// Must equal `device`.
        confirm_device: String,
    },
    /// Delete a partition by number.
    PartitionDelete {
        device: String,
        number: u32,
        /// Must equal `device`.
        confirm_device: String,
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
    ///
    /// `start`, `stop` and `restart` only touch the running state. `enable`
    /// and `disable` change the boot set, and under dinit the backend command
    /// changes the running state as well, so these three results are part of
    /// the contract:
    ///
    /// - `enable` on a stopped unit that is not enabled yet: the boot link is
    ///   created **and dinit starts the service**.
    /// - `enable` on a unit whose boot link exists: success without a backend
    ///   call, and the running state is untouched (a stopped unit stays
    ///   stopped).
    /// - `disable` on a running enabled unit: the boot link is removed and
    ///   **the service keeps running**.
    ///
    /// `enable` and `disable` therefore answer with the service list re-read
    /// afterwards: read `state` there instead of assuming the operation left
    /// it alone. `OpenRC` behaves differently (`rc-update` never starts or
    /// stops anything).
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
    /// Toggle filtering on the default zone: `enabled=false` sets the
    /// target to `ACCEPT` (traffic passes), `enabled=true` restores
    /// `REJECT`. firewalld has no global on/off switch, so the toggle
    /// is the default zone's target.
    SetFirewallEnabled {
        enabled: bool,
    },
    /// Create a custom service definition (ports optional); add it to
    /// zones afterwards with `add_firewall_service`.
    CreateFirewallService {
        service: String,
        #[serde(default)]
        ports: Vec<String>,
    },
    /// Delete a custom service definition.
    DeleteFirewallService {
        service: String,
    },
    /// Reload firewalld (applies pending permanent changes).
    ReloadFirewall,

    // -- snapshots (btrbk) --
    /// List btrbk-managed snapshots.
    ListSnapshots,
    /// btrbk stats output.
    Stats,
    /// Run a snapshot-only btrbk pass for a config section.
    ///
    /// btrbk then applies the retention rules of the configuration
    /// (`snapshot_preserve`, `snapshot_preserve_min`) and deletes the
    /// snapshots they no longer keep, so this needs the destructive
    /// confirmation (`Gate::Always`). The deletion is only visible in
    /// btrbk's own output.
    CreateSnapshot {
        section: Option<String>,
    },
    /// Restore a snapshot to a target. `dry_run` returns the preview.
    ///
    /// The live target is moved aside into the uniform backup chain, which
    /// keeps at most `backup_configs.copies` backups of it (3 by default) —
    /// the oldest one is deleted to make room. The preview and the apply
    /// answer name the deleted tree as `would_delete`.
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
    ///
    /// No password is set: the account is created locked and cannot log in
    /// until `SetPassword` gives it one.
    CreateUser {
        name: String,
        fullname: String,
        admin: bool,
    },
    /// Delete a user (confirmation enforced by the frontend).
    DeleteUser {
        /// Numeric id; pass this or `name`, not both.
        #[serde(default)]
        uid: Option<u32>,
        #[serde(default)]
        remove_files: bool,
        /// Login name; resolves the uid in one call.
        #[serde(default)]
        name: Option<String>,
    },
    /// Set a user's password (value never logged or audited).
    ///
    /// The password is hashed in-process and the hash is stored as-is, so
    /// the account accepts exactly the password supplied here; an account
    /// created by `CreateUser` stays locked until this runs (or while it
    /// fails).
    SetPassword {
        /// Numeric id; pass this or `name`, not both.
        #[serde(default)]
        uid: Option<u32>,
        new_password: String,
        /// Login name; resolves the uid in one call.
        #[serde(default)]
        name: Option<String>,
    },
    /// List system groups.
    ListGroups,
    /// Replace a user's supplementary groups.
    SetGroups {
        /// Numeric id; pass this or `name`, not both.
        #[serde(default)]
        uid: Option<u32>,
        groups: Vec<String>,
        /// Login name; resolves the uid in one call.
        #[serde(default)]
        name: Option<String>,
    },

    // -- niri --
    /// Schema-typed view of the user's config.kdl.
    GetNiriConfig,
    /// Run `niri validate` against a proposed config (nothing written).
    /// The check uses a hidden temporary file beside the config — resolved
    /// `include` paths need that location — and removes it afterwards; the
    /// config folder is never created.
    ValidateNiriConfig {
        proposed: Option<String>,
    },
    /// Apply one section of form values (schema-driven).
    SetNiriSection {
        section: String,
        value: serde_json::Value,
    },

    // -- theming (uniform desktop) --
    /// Detect the winning shell source (Wayle/DMS) and per-target availability.
    ThemeDetect,
    /// Normalized theme snapshot from the winning source.
    ThemeGet,
    /// Propagate the snapshot to every available target (writes + report).
    /// `show_minimize` toggles the minimize button of the three-button
    /// baseline (default on).
    ThemeApply {
        #[serde(default = "default_show_minimize")]
        show_minimize: bool,
    },

    // -- logs (SPEC-logs) --
    /// Discover log files under `/var/log` (fresh scan — new files show up).
    LogList,
    /// Read a chunk of a `/var/log` file (tail / previous / follow).
    LogRead {
        /// Path relative to `/var/log`.
        file: String,
        #[serde(default = "crate::logs::default_log_lines")]
        lines: u32,
        cursor: crate::logs::LogCursor,
    },
    /// Substring search over a `/var/log` file (case-insensitive).
    LogSearch {
        /// Path relative to `/var/log`.
        file: String,
        query: String,
        #[serde(default = "crate::logs::default_log_search_limit")]
        limit: u32,
    },

    // -- crontab (SPEC-crontab) --
    /// Current crontab of the session user (`None` when absent).
    CrontabGet,
    /// Replace the session user's crontab (empty string clears it).
    CrontabSet {
        content: String,
    },

    // -- booster / initramfs (SPEC-booster) --
    /// Read `/etc/booster.yaml` (raw bytes + known keys).
    BoosterConfigGet,
    /// Surgically update the known keys of `/etc/booster.yaml`.
    BoosterConfigSet {
        // Boxed: the patch is 18 optional keys — keeping it inline would
        // dwarf every other Action variant (clippy large_enum_variant).
        keys: Box<crate::BoosterConfigPatch>,
    },
    /// Rebuild the initramfs through the distro's own kernel-update
    /// process (`apk fix kernel-hooks` first, `booster build` fallback).
    BoosterRegenerate,

    // -- apparmor (SPEC-apparmor) --
    /// Current `AppArmor` state: aa-status parse + profile files + the
    /// notifications activation state.
    ApparmorStatus,
    /// Switch a profile file's mode (enforce|complain|disable|enable).
    ApparmorSetMode {
        path: String,
        mode: String,
    },
    /// Load/replace a profile file (`apparmor_parser -r -W`).
    ApparmorLoad {
        path: String,
    },
    /// Syntax-check a profile file without loading it.
    ApparmorCheck {
        path: String,
    },
    /// Unload the profiles defined in a file (`apparmor_parser -R`).
    ApparmorRemove {
        path: String,
    },
    /// Reload every profile under /etc/apparmor.d.
    ApparmorReloadAll,
    /// Processes without a profile (`aa-unconfined`).
    ApparmorUnconfined,
    /// Drop loaded profiles that have no backing file (destructive).
    ApparmorRemoveUnknown,
    /// Raw content of one profile file (the editor's source).
    ApparmorProfileGet {
        path: String,
    },
    /// Write edited profile content (syntax-checked before any write).
    ApparmorProfileSet {
        path: String,
        content: String,
    },
    /// Enable DENIED notifications: notify.conf switch + aa-notify
    /// autostart entry for the session user (active from next login).
    ApparmorNotificationsEnable,
    /// Clean a profile file in place (destructive: drops comments).
    ApparmorCleanprof {
        name: String,
    },
    /// Add hardlink dependencies to a profile file (`aa-autodep`).
    ApparmorAutodep {
        path: String,
    },
    /// Extract the kernel's ABI features (`aa-features-abi --extract`).
    ApparmorFeaturesAbi,
    /// Decode one hex-encoded `AppArmor` log token (`aa-decode <hex>`).
    ApparmorDecode {
        hex: String,
    },
    /// Generate a profile from templates/policy groups — stdout only,
    /// nothing is written (`--output-directory` is never passed).
    ApparmorEasyprof {
        binary: String,
        #[serde(default)]
        template: Option<String>,
        #[serde(default)]
        policy_groups: Option<String>,
        /// `@{NAME}=VALUE` declarations passed as `--template-var`.
        #[serde(default)]
        template_vars: Vec<String>,
    },
    /// List aa-easyprof templates or policy groups.
    ApparmorEasyprofList {
        what: String,
    },
}

/// Wire default for `ThemeApply.show_minimize` (trio on).
fn default_show_minimize() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_tag_is_present_and_snake_case() {
        let json = serde_json::to_value(&Action::ListInstalled).unwrap();
        assert_eq!(json["action"], "list_installed_nix");
        let search = serde_json::to_value(&Action::Search {
            query: "niri".into(),
            repo: None,
        })
        .unwrap();
        assert_eq!(search["action"], "search_nix");
    }

    /// Every nix action that used a bare wire name before the rename, with the
    /// module-qualified name it answers to now and one payload that builds it.
    fn renamed_nix_actions() -> Vec<(&'static str, &'static str, Action)> {
        vec![
            (
                "list_installed",
                "list_installed_nix",
                Action::ListInstalled,
            ),
            (
                "search",
                "search_nix",
                Action::Search {
                    query: "niri".into(),
                    repo: None,
                },
            ),
            (
                "install",
                "install_nix",
                Action::Install {
                    package: "hello".into(),
                },
            ),
            (
                "remove",
                "remove_nix",
                Action::Remove {
                    package: "hello".into(),
                },
            ),
            ("remove_all", "remove_all_nix", Action::RemoveAll),
            (
                "remove_regex",
                "remove_regex_nix",
                Action::RemoveRegex {
                    pattern: "^py".into(),
                    dry_run: true,
                },
            ),
            (
                "upgrade",
                "upgrade_nix",
                Action::Upgrade {
                    package: Some("hello".into()),
                    all: false,
                },
            ),
            (
                "upgrade_regex",
                "upgrade_regex_nix",
                Action::UpgradeRegex {
                    pattern: "^py".into(),
                    dry_run: true,
                },
            ),
            ("generations", "generations_nix", Action::Generations),
            (
                "rollback",
                "rollback_nix",
                Action::Rollback {
                    generation: Some(7),
                },
            ),
            (
                "wipe_history",
                "wipe_history_nix",
                Action::WipeHistory {
                    older_than: None,
                    dry_run: true,
                },
            ),
            (
                "diff_closures",
                "diff_closures_nix",
                Action::DiffClosures {
                    generation: Some(3),
                },
            ),
            ("store_gc", "store_gc_nix", Action::StoreGc),
            (
                "prune_generations",
                "prune_generations_nix",
                Action::PruneGenerations {
                    keep: 3,
                    dry_run: true,
                    expected_current: None,
                    expected_delete: None,
                },
            ),
        ]
    }

    #[test]
    fn nix_actions_serialize_with_the_module_in_the_name() {
        for (old, new, action) in renamed_nix_actions() {
            let json = serde_json::to_value(&action).unwrap();
            assert_eq!(json["action"], new, "serialized tag of {old}");
        }
    }

    #[test]
    fn legacy_nix_action_names_deserialize_to_the_same_variant() {
        // A client or script written before the rename sends the bare name;
        // the alias keeps it on the same variant, so nothing breaks.
        for (old, new, action) in renamed_nix_actions() {
            let mut renamed = serde_json::to_value(&action).unwrap();
            renamed["action"] = serde_json::Value::String(new.to_string());
            assert_eq!(
                serde_json::from_value::<Action>(renamed).unwrap(),
                action,
                "new name {new}"
            );
            let mut legacy = serde_json::to_value(&action).unwrap();
            legacy["action"] = serde_json::Value::String(old.to_string());
            assert_eq!(
                serde_json::from_value::<Action>(legacy).unwrap(),
                action,
                "legacy alias {old}"
            );
        }
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
    fn list_apk_updates_refresh_is_optional_and_off_by_default() {
        // Old clients send the tag alone: the field must default to the
        // cached-index read, never to a write.
        let cached: Action =
            serde_json::from_value(serde_json::json!({ "action": "list_apk_updates" })).unwrap();
        assert_eq!(cached, Action::ListApkUpdates { refresh: false });
        assert!(cached.is_read_only());
        let refreshed: Action = serde_json::from_value(serde_json::json!({
            "action": "list_apk_updates",
            "refresh": true,
        }))
        .unwrap();
        assert_eq!(refreshed, Action::ListApkUpdates { refresh: true });
        assert!(refreshed.is_mutating());
    }

    #[test]
    fn flatpak_actions_default_new_fields_for_older_clients() {
        // Old clients omit `remote` / `system`: the wire must still parse.
        let install: Action = serde_json::from_value(serde_json::json!({
            "action": "install_flatpak",
            "app_id": "org.mozilla.firefox",
            "system": true,
        }))
        .unwrap();
        assert_eq!(
            install,
            Action::InstallFlatpak {
                app_id: "org.mozilla.firefox".into(),
                system: true,
                remote: None,
            }
        );
        let remotes: Action =
            serde_json::from_value(serde_json::json!({ "action": "list_flatpak_remotes" }))
                .unwrap();
        assert_eq!(remotes, Action::ListFlatpakRemotes { system: None });
        let with_remote: Action = serde_json::from_value(serde_json::json!({
            "action": "install_flatpak",
            "app_id": "org.mozilla.firefox",
            "system": false,
            "remote": "flathub-beta",
        }))
        .unwrap();
        assert_eq!(
            with_remote,
            Action::InstallFlatpak {
                app_id: "org.mozilla.firefox".into(),
                system: false,
                remote: Some("flathub-beta".into()),
            }
        );
    }

    #[test]
    fn flatpak_system_defaults_to_the_user_installation() {
        // Every flatpak action answers the user's installation when a client
        // omits `system`; the catalogue says `required: false` for it.
        let listed: Action =
            serde_json::from_value(serde_json::json!({ "action": "list_flatpak" })).unwrap();
        assert_eq!(listed, Action::ListFlatpak { system: false });

        let installed: Action = serde_json::from_value(serde_json::json!({
            "action": "install_flatpak",
            "app_id": "org.mozilla.firefox",
        }))
        .unwrap();
        assert_eq!(
            installed,
            Action::InstallFlatpak {
                app_id: "org.mozilla.firefox".into(),
                system: false,
                remote: None,
            }
        );

        let updated: Action = serde_json::from_value(serde_json::json!({
            "action": "update_flatpak_app",
            "app_id": "org.mozilla.firefox",
        }))
        .unwrap();
        assert_eq!(
            updated,
            Action::UpdateFlatpakApp {
                app_id: "org.mozilla.firefox".into(),
                system: false,
            }
        );

        let orphans: Action =
            serde_json::from_value(serde_json::json!({ "action": "remove_flatpak_orphans" }))
                .unwrap();
        assert_eq!(orphans, Action::RemoveFlatpakOrphans { system: false });

        // The regex pair keeps the required/optional split it had: preview is
        // optional, the installation selector is optional too.
        let regex: Action = serde_json::from_value(serde_json::json!({
            "action": "flatpak_remove_regex",
            "pattern": "^org\\.mozilla",
        }))
        .unwrap();
        assert_eq!(
            regex,
            Action::FlatpakRemoveRegex {
                pattern: "^org\\.mozilla".into(),
                dry_run: false,
                system: false,
            }
        );

        // `system: true` keeps the explicit system scope on the wire.
        let explicit: Action = serde_json::from_value(serde_json::json!({
            "action": "list_flatpak",
            "system": true,
        }))
        .unwrap();
        assert_eq!(explicit, Action::ListFlatpak { system: true });
    }

    #[test]
    fn upgrade_all_defaults_to_false_and_dry_run_stays_required() {
        let one: Action = serde_json::from_value(serde_json::json!({
            "action": "upgrade",
            "package": "hello",
        }))
        .unwrap();
        assert_eq!(
            one,
            Action::Upgrade {
                package: Some("hello".into()),
                all: false,
            }
        );

        // Destructive calls keep an explicit choice: a request without
        // `dry_run` is rejected instead of defaulting to apply.
        let wipe = serde_json::from_value::<Action>(serde_json::json!({
            "action": "wipe_history",
        }));
        assert!(wipe.is_err());
        let prune = serde_json::from_value::<Action>(serde_json::json!({
            "action": "prune_generations",
            "keep": 3,
        }));
        assert!(prune.is_err());
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
