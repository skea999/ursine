//! Machine-readable action catalogue and the read-only rule.
//!
//! Every [`Action`] variant is classified exactly once, here: what it does to
//! the machine ([`Effect`]) and whether it needs the destructive `--yes`
//! confirmation ([`Gate`]). Frontends, `ursinectl` and any third-party client
//! read the same data ([`ACTIONS`], [`json_document`], `ursinectl actions
//! --json`) instead of keeping their own list, so they cannot disagree about
//! which actions are safe.
//!
//! [`Action::spec`] matches every variant on purpose: a new action does not
//! compile until it is classified, and the `catalogue_covers_every_wire_action`
//! / `every_catalogue_row_is_reachable_from_its_action` tests in
//! `src/ursinectl/tests/samples.rs` fail until the new row is printed too (an
//! action that is not in [`ACTIONS`] never reaches a client).
//!
//! The catalogue is metadata only: it never changes the wire format, an
//! `Action` payload or the daemon's dispatch.

use serde::Serialize;
use serde_json::{json, Value};

use crate::{Action, ModuleId};

/// Wire id of the catalogue document (`ursinectl actions --json`).
pub const CATALOGUE_SCHEMA: &str = "ursine.actions/1";

/// The `dry_run` parameter name: the one flag that turns a mutation into a
/// preview.
const DRY_RUN: &str = "dry_run";

/// What an action does to the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Reads only: no file, profile, service or policy is changed.
    ReadOnly,
    /// Writes, deletes, restarts or re-applies something.
    Mutating,
}

/// Whether an action requires the destructive confirmation (`--yes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    /// Never: reads and additive changes, `--yes` is not required.
    Never,
    /// Always: the request deletes, rolls back, overwrites or unloads policy.
    Always,
    /// The payload decides: a `dry_run` preview, `apparmor_set_mode` with a
    /// mode other than `disable`, `set_firewall_enabled` switched on.
    Request,
}

/// One accepted parameter of an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Param {
    /// Wire field name (`package`, `dry_run`, …).
    pub name: &'static str,
    /// The wire demands the field: it is neither an `Option` nor carries
    /// `#[serde(default)]`, so a request that omits it is rejected.
    pub required: bool,
}

/// The static facts about one action variant: one catalogue row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionSpec {
    /// Wire name (the serde `action` tag).
    pub name: &'static str,
    /// Module the daemon dispatches the action to.
    pub module: ModuleId,
    /// Nominal effect: what a full (non-preview) request does.
    pub effect: Effect,
    /// `--yes` classification of the variant.
    pub gate: Gate,
    /// Accepted parameters, in `Action` declaration order.
    pub params: &'static [Param],
}

impl ActionSpec {
    /// Whether a `dry_run` parameter turns this action into a read-only
    /// preview, i.e. whether the request-level effect can differ from
    /// [`ActionSpec::effect`].
    pub fn previewable(&self) -> bool {
        self.params.iter().any(|p| p.name == DRY_RUN)
    }

    /// The documented `ursine.actions/1` JSON row.
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "module": self.module.as_str(),
            "effect": self.effect,
            "destructive": self.gate,
            "previewable": self.previewable(),
            "params": self
                .params
                .iter()
                .map(|p| json!({ "name": p.name, "required": p.required }))
                .collect::<Vec<Value>>(),
        })
    }
}

/// The whole catalogue as the `ursine.actions/1` JSON document.
pub fn json_document() -> Value {
    json!({
        "schema": CATALOGUE_SCHEMA,
        "count": ACTIONS.len(),
        "actions": ACTIONS.iter().map(ActionSpec::to_json).collect::<Vec<Value>>(),
    })
}

/// Build one row (a `const fn` so every row below is a compile-time constant).
const fn row(
    name: &'static str,
    module: ModuleId,
    effect: Effect,
    gate: Gate,
    params: &'static [Param],
) -> ActionSpec {
    ActionSpec {
        name,
        module,
        effect,
        gate,
        params,
    }
}

/// Build one parameter (a `const fn` so rows stay constant expressions).
const fn param(name: &'static str, required: bool) -> Param {
    Param { name, required }
}

/// One const per `Action` variant: the row `Action::spec` returns and
/// `ACTIONS` lists. `effect` is the nominal effect; a `dry_run` preview is
/// resolved per request by [`Action::effect`].
// -- System --
const STATUS: ActionSpec = row(
    "status",
    ModuleId::System,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);

// -- Config --
const INIT: ActionSpec = row("init", ModuleId::Config, Effect::Mutating, Gate::Never, &[]);
const GET_CONFIG: ActionSpec = row(
    "get_config",
    ModuleId::Config,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const SET_MODULE_SWITCH: ActionSpec = row(
    "set_module_switch",
    ModuleId::Config,
    Effect::Mutating,
    Gate::Never,
    &[param("module", true), param("value", true)],
);
const SET_UPDATE_AUTO: ActionSpec = row(
    "set_update_auto",
    ModuleId::Config,
    Effect::Mutating,
    Gate::Never,
    &[param("manager", true), param("enabled", true)],
);

// -- Capabilities --
const LIST: ActionSpec = row(
    "list",
    ModuleId::Capabilities,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
// re-runs capability detection in memory; no file or service changes
const REFRESH: ActionSpec = row(
    "refresh",
    ModuleId::Capabilities,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);

// -- System --
const JOB_OUTPUT: ActionSpec = row(
    "job_output",
    ModuleId::System,
    Effect::ReadOnly,
    Gate::Never,
    &[param("job_id", true), param("offset", false)],
);

// -- PackagesNix --
const LIST_INSTALLED: ActionSpec = row(
    "list_installed",
    ModuleId::PackagesNix,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const SEARCH: ActionSpec = row(
    "search",
    ModuleId::PackagesNix,
    Effect::ReadOnly,
    Gate::Never,
    &[param("query", true), param("repo", false)],
);
const LIST_NIX_REPOSITORIES: ActionSpec = row(
    "list_nix_repositories",
    ModuleId::PackagesNix,
    Effect::ReadOnly,
    Gate::Never,
    &[param("system", false)],
);
const ADD_NIX_REPOSITORY: ActionSpec = row(
    "add_nix_repository",
    ModuleId::PackagesNix,
    Effect::Mutating,
    Gate::Never,
    &[
        param("name", true),
        param("url", true),
        param("system", false),
    ],
);
const REMOVE_NIX_REPOSITORY: ActionSpec = row(
    "remove_nix_repository",
    ModuleId::PackagesNix,
    Effect::Mutating,
    Gate::Always,
    &[
        param("name", true),
        param("system", false),
        param("source", false),
    ],
);
const INSTALL: ActionSpec = row(
    "install",
    ModuleId::PackagesNix,
    Effect::Mutating,
    Gate::Never,
    &[param("package", true)],
);
const REMOVE: ActionSpec = row(
    "remove",
    ModuleId::PackagesNix,
    Effect::Mutating,
    Gate::Always,
    &[param("package", true)],
);
const REMOVE_ALL: ActionSpec = row(
    "remove_all",
    ModuleId::PackagesNix,
    Effect::Mutating,
    Gate::Always,
    &[],
);
const REMOVE_REGEX: ActionSpec = row(
    "remove_regex",
    ModuleId::PackagesNix,
    Effect::Mutating,
    Gate::Request,
    &[param("pattern", true), param("dry_run", false)],
);
const UPGRADE: ActionSpec = row(
    "upgrade",
    ModuleId::PackagesNix,
    Effect::Mutating,
    Gate::Never,
    &[param("package", false), param("all", false)],
);
const UPGRADE_REGEX: ActionSpec = row(
    "upgrade_regex",
    ModuleId::PackagesNix,
    Effect::Mutating,
    Gate::Request,
    &[param("pattern", true), param("dry_run", false)],
);
const LIST_NIX_UPDATES: ActionSpec = row(
    "list_nix_updates",
    ModuleId::PackagesNix,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const GENERATIONS: ActionSpec = row(
    "generations",
    ModuleId::PackagesNix,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const ROLLBACK: ActionSpec = row(
    "rollback",
    ModuleId::PackagesNix,
    Effect::Mutating,
    Gate::Always,
    &[param("generation", false)],
);
const WIPE_HISTORY: ActionSpec = row(
    "wipe_history",
    ModuleId::PackagesNix,
    Effect::Mutating,
    Gate::Request,
    &[param("older_than", false), param("dry_run", true)],
);
const DIFF_CLOSURES: ActionSpec = row(
    "diff_closures",
    ModuleId::PackagesNix,
    Effect::ReadOnly,
    Gate::Never,
    &[param("generation", false)],
);
const STORE_GC: ActionSpec = row(
    "store_gc",
    ModuleId::PackagesNix,
    Effect::Mutating,
    Gate::Always,
    &[],
);
const PRUNE_GENERATIONS: ActionSpec = row(
    "prune_generations",
    ModuleId::PackagesNix,
    Effect::Mutating,
    Gate::Request,
    &[
        param("keep", true),
        param("dry_run", true),
        param("expected_current", false),
        param("expected_delete", false),
    ],
);

// -- PackagesApk --
const LIST_APK: ActionSpec = row(
    "list_apk",
    ModuleId::PackagesApk,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const SEARCH_APK: ActionSpec = row(
    "search_apk",
    ModuleId::PackagesApk,
    Effect::ReadOnly,
    Gate::Never,
    &[param("query", true)],
);
const LIST_APK_REPOSITORIES: ActionSpec = row(
    "list_apk_repositories",
    ModuleId::PackagesApk,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const ADD_APK_REPOSITORY: ActionSpec = row(
    "add_apk_repository",
    ModuleId::PackagesApk,
    Effect::Mutating,
    Gate::Never,
    &[param("url", true), param("tag", false)],
);
const REMOVE_APK_REPOSITORY: ActionSpec = row(
    "remove_apk_repository",
    ModuleId::PackagesApk,
    Effect::Mutating,
    Gate::Always,
    &[param("url", true)],
);
const INSTALL_APK: ActionSpec = row(
    "install_apk",
    ModuleId::PackagesApk,
    Effect::Mutating,
    Gate::Never,
    &[param("package", true)],
);
const PREVIEW_APK_INSTALL: ActionSpec = row(
    "preview_apk_install",
    ModuleId::PackagesApk,
    Effect::ReadOnly,
    Gate::Never,
    &[param("package", true)],
);
const REMOVE_APK: ActionSpec = row(
    "remove_apk",
    ModuleId::PackagesApk,
    Effect::Mutating,
    Gate::Always,
    &[param("package", true)],
);
const UPGRADE_APK: ActionSpec = row(
    "upgrade_apk",
    ModuleId::PackagesApk,
    Effect::Mutating,
    Gate::Always,
    &[],
);
// the cached index keeps the default a read; `refresh = true` runs
// `apk update`, which rewrites the index files, and is mutating
const LIST_APK_UPDATES: ActionSpec = row(
    "list_apk_updates",
    ModuleId::PackagesApk,
    Effect::ReadOnly,
    Gate::Never,
    &[param("refresh", false)],
);
const UPGRADE_APK_PACKAGE: ActionSpec = row(
    "upgrade_apk_package",
    ModuleId::PackagesApk,
    Effect::Mutating,
    Gate::Always,
    &[param("package", true)],
);

// -- PackagesFlatpak --
const LIST_FLATPAK: ActionSpec = row(
    "list_flatpak",
    ModuleId::PackagesFlatpak,
    Effect::ReadOnly,
    Gate::Never,
    &[param("system", false)],
);
const LIST_FLATPAK_REMOTES: ActionSpec = row(
    "list_flatpak_remotes",
    ModuleId::PackagesFlatpak,
    Effect::ReadOnly,
    Gate::Never,
    &[param("system", false)],
);
const LIST_FLATPAK_UPDATES: ActionSpec = row(
    "list_flatpak_updates",
    ModuleId::PackagesFlatpak,
    Effect::ReadOnly,
    Gate::Never,
    &[param("system", false)],
);
const ADD_FLATPAK_REMOTE: ActionSpec = row(
    "add_flatpak_remote",
    ModuleId::PackagesFlatpak,
    Effect::Mutating,
    Gate::Never,
    &[
        param("name", true),
        param("url", true),
        param("system", false),
    ],
);
const REMOVE_FLATPAK_REMOTE: ActionSpec = row(
    "remove_flatpak_remote",
    ModuleId::PackagesFlatpak,
    Effect::Mutating,
    Gate::Always,
    &[param("name", true), param("system", false)],
);
const SEARCH_FLATPAK: ActionSpec = row(
    "search_flatpak",
    ModuleId::PackagesFlatpak,
    Effect::ReadOnly,
    Gate::Never,
    &[param("query", true), param("system", false)],
);
const INSTALL_FLATPAK: ActionSpec = row(
    "install_flatpak",
    ModuleId::PackagesFlatpak,
    Effect::Mutating,
    Gate::Never,
    &[
        param("app_id", true),
        param("system", false),
        param("remote", false),
    ],
);
const GET_FLATPAK_INFO: ActionSpec = row(
    "get_flatpak_info",
    ModuleId::PackagesFlatpak,
    Effect::ReadOnly,
    Gate::Never,
    &[
        param("app_id", true),
        param("remote", false),
        param("system", false),
    ],
);
const REMOVE_FLATPAK: ActionSpec = row(
    "remove_flatpak",
    ModuleId::PackagesFlatpak,
    Effect::Mutating,
    Gate::Always,
    &[param("app_id", true), param("system", false)],
);

// -- PackagesApk --
const APK_REMOVE_REGEX: ActionSpec = row(
    "apk_remove_regex",
    ModuleId::PackagesApk,
    Effect::Mutating,
    Gate::Request,
    &[param("pattern", true), param("dry_run", false)],
);
const APK_UPGRADE_REGEX: ActionSpec = row(
    "apk_upgrade_regex",
    ModuleId::PackagesApk,
    Effect::Mutating,
    Gate::Request,
    &[param("pattern", true), param("dry_run", false)],
);

// -- PackagesFlatpak --
const FLATPAK_REMOVE_REGEX: ActionSpec = row(
    "flatpak_remove_regex",
    ModuleId::PackagesFlatpak,
    Effect::Mutating,
    Gate::Request,
    &[
        param("pattern", true),
        param("dry_run", false),
        param("system", false),
    ],
);
const FLATPAK_UPGRADE_REGEX: ActionSpec = row(
    "flatpak_upgrade_regex",
    ModuleId::PackagesFlatpak,
    Effect::Mutating,
    Gate::Request,
    &[
        param("pattern", true),
        param("dry_run", false),
        param("system", false),
    ],
);
const UPDATE_FLATPAK: ActionSpec = row(
    "update_flatpak",
    ModuleId::PackagesFlatpak,
    Effect::Mutating,
    Gate::Always,
    &[param("system", false)],
);
const UPDATE_FLATPAK_APP: ActionSpec = row(
    "update_flatpak_app",
    ModuleId::PackagesFlatpak,
    Effect::Mutating,
    Gate::Always,
    &[param("app_id", true), param("system", false)],
);
const REMOVE_FLATPAK_ORPHANS: ActionSpec = row(
    "remove_flatpak_orphans",
    ModuleId::PackagesFlatpak,
    Effect::Mutating,
    Gate::Always,
    &[param("system", false)],
);

// -- Printers --
const LIST_PRINTERS: ActionSpec = row(
    "list_printers",
    ModuleId::Printers,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const PRINTER_JOBS: ActionSpec = row(
    "printer_jobs",
    ModuleId::Printers,
    Effect::ReadOnly,
    Gate::Never,
    &[param("printer", false)],
);
const ADD_PRINTER: ActionSpec = row(
    "add_printer",
    ModuleId::Printers,
    Effect::Mutating,
    Gate::Never,
    &[
        param("name", true),
        param("uri", true),
        param("description", true),
        param("location", true),
    ],
);
const ADD_PRINTER_MANUAL: ActionSpec = row(
    "add_printer_manual",
    ModuleId::Printers,
    Effect::Mutating,
    Gate::Never,
    &[
        param("name", true),
        param("uri", true),
        param("driver", true),
        param("ppd", true),
        param("description", true),
        param("location", true),
        param("options", true),
    ],
);
const REMOVE_PRINTER: ActionSpec = row(
    "remove_printer",
    ModuleId::Printers,
    Effect::Mutating,
    Gate::Always,
    &[param("name", true)],
);
const SET_DEFAULT_PRINTER: ActionSpec = row(
    "set_default_printer",
    ModuleId::Printers,
    Effect::Mutating,
    Gate::Never,
    &[param("name", true)],
);
const SET_PRINTER_OPTIONS: ActionSpec = row(
    "set_printer_options",
    ModuleId::Printers,
    Effect::Mutating,
    Gate::Never,
    &[param("name", true), param("options", true)],
);
const PAUSE_PRINTER: ActionSpec = row(
    "pause_printer",
    ModuleId::Printers,
    Effect::Mutating,
    Gate::Never,
    &[param("name", true), param("pause", true)],
);
const CANCEL_PRINTER_JOBS: ActionSpec = row(
    "cancel_printer_jobs",
    ModuleId::Printers,
    Effect::Mutating,
    Gate::Always,
    &[param("printer", false)],
);

// -- Scanners --
const LIST_SCANNERS: ActionSpec = row(
    "list_scanners",
    ModuleId::Scanners,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const GET_SCANNER_CONFIG: ActionSpec = row(
    "get_scanner_config",
    ModuleId::Scanners,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const SET_SCANNER_DEVICE: ActionSpec = row(
    "set_scanner_device",
    ModuleId::Scanners,
    Effect::Mutating,
    Gate::Never,
    &[
        param("name", true),
        param("address", true),
        param("protocol", true),
    ],
);
const REMOVE_SCANNER_DEVICE: ActionSpec = row(
    "remove_scanner_device",
    ModuleId::Scanners,
    Effect::Mutating,
    Gate::Always,
    &[param("name", true)],
);
const SET_SCANNER_DISCOVERY: ActionSpec = row(
    "set_scanner_discovery",
    ModuleId::Scanners,
    Effect::Mutating,
    Gate::Never,
    &[param("enabled", true)],
);

// -- Network --
const GET_NETWORK: ActionSpec = row(
    "get_network",
    ModuleId::Network,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const SET_WIFI_ENABLED: ActionSpec = row(
    "set_wifi_enabled",
    ModuleId::Network,
    Effect::Mutating,
    Gate::Never,
    &[param("enabled", true)],
);
const CONNECT_WIFI: ActionSpec = row(
    "connect_wifi",
    ModuleId::Network,
    Effect::Mutating,
    Gate::Never,
    &[param("ssid", true), param("password", true)],
);
const CONNECT_NETWORK: ActionSpec = row(
    "connect_network",
    ModuleId::Network,
    Effect::Mutating,
    Gate::Never,
    &[param("device", true)],
);
const DISCONNECT_NETWORK: ActionSpec = row(
    "disconnect_network",
    ModuleId::Network,
    Effect::Mutating,
    Gate::Never,
    &[param("device", true)],
);
const FORGET_NETWORK: ActionSpec = row(
    "forget_network",
    ModuleId::Network,
    Effect::Mutating,
    Gate::Always,
    &[param("connection", true)],
);
const SET_NETWORK_CONNECTION: ActionSpec = row(
    "set_network_connection",
    ModuleId::Network,
    Effect::Mutating,
    Gate::Never,
    &[param("connection", true), param("up", true)],
);
const SET_NETWORK_IPV4: ActionSpec = row(
    "set_network_ipv4",
    ModuleId::Network,
    Effect::Mutating,
    Gate::Always,
    &[
        param("connection", true),
        param("method", true),
        param("addresses", false),
        param("gateway", false),
        param("dns", false),
    ],
);

// -- Disks --
const FORMAT_DISK: ActionSpec = row(
    "format_disk",
    ModuleId::Disks,
    Effect::Mutating,
    Gate::Always,
    &[
        param("device", true),
        param("fstype", true),
        param("confirm_device", true),
    ],
);
const PARTITION_CREATE: ActionSpec = row(
    "partition_create",
    ModuleId::Disks,
    Effect::Mutating,
    Gate::Always,
    &[
        param("device", true),
        param("start_mib", true),
        param("end_mib", true),
        param("confirm_device", true),
    ],
);
const PARTITION_DELETE: ActionSpec = row(
    "partition_delete",
    ModuleId::Disks,
    Effect::Mutating,
    Gate::Always,
    &[
        param("device", true),
        param("number", true),
        param("confirm_device", true),
    ],
);

// -- Bluetooth --
const GET_BLUETOOTH: ActionSpec = row(
    "get_bluetooth",
    ModuleId::Bluetooth,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const SET_BLUETOOTH_POWER: ActionSpec = row(
    "set_bluetooth_power",
    ModuleId::Bluetooth,
    Effect::Mutating,
    Gate::Never,
    &[param("on", true)],
);
// discovery only: no pairing, trust or config state is written
const SCAN_BLUETOOTH: ActionSpec = row(
    "scan_bluetooth",
    ModuleId::Bluetooth,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const BLUETOOTH_ACTION: ActionSpec = row(
    "bluetooth_action",
    ModuleId::Bluetooth,
    Effect::Mutating,
    Gate::Never,
    &[param("address", true), param("op", true)],
);

// -- Disks --
const LIST_DISKS: ActionSpec = row(
    "list_disks",
    ModuleId::Disks,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const MOUNT_DISK: ActionSpec = row(
    "mount_disk",
    ModuleId::Disks,
    Effect::Mutating,
    Gate::Never,
    &[param("device", true)],
);
const UNMOUNT_DISK: ActionSpec = row(
    "unmount_disk",
    ModuleId::Disks,
    Effect::Mutating,
    Gate::Always,
    &[param("device", true)],
);

// -- Services --
const LIST_SERVICES: ActionSpec = row(
    "list_services",
    ModuleId::Services,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const SERVICE_ACTION: ActionSpec = row(
    "service_action",
    ModuleId::Services,
    Effect::Mutating,
    Gate::Never,
    &[param("name", true), param("op", true)],
);

// -- Firewall --
const GET_FIREWALL: ActionSpec = row(
    "get_firewall",
    ModuleId::Firewall,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const SET_DEFAULT_FIREWALL_ZONE: ActionSpec = row(
    "set_default_firewall_zone",
    ModuleId::Firewall,
    Effect::Mutating,
    Gate::Never,
    &[param("zone", true)],
);
const ADD_FIREWALL_SERVICE: ActionSpec = row(
    "add_firewall_service",
    ModuleId::Firewall,
    Effect::Mutating,
    Gate::Never,
    &[param("zone", true), param("service", true)],
);
const REMOVE_FIREWALL_SERVICE: ActionSpec = row(
    "remove_firewall_service",
    ModuleId::Firewall,
    Effect::Mutating,
    Gate::Always,
    &[param("zone", true), param("service", true)],
);
const ADD_FIREWALL_PORT: ActionSpec = row(
    "add_firewall_port",
    ModuleId::Firewall,
    Effect::Mutating,
    Gate::Never,
    &[param("zone", true), param("port", true)],
);
const REMOVE_FIREWALL_PORT: ActionSpec = row(
    "remove_firewall_port",
    ModuleId::Firewall,
    Effect::Mutating,
    Gate::Always,
    &[param("zone", true), param("port", true)],
);
const SET_FIREWALL_ENABLED: ActionSpec = row(
    "set_firewall_enabled",
    ModuleId::Firewall,
    Effect::Mutating,
    Gate::Request,
    &[param("enabled", true)],
);
const CREATE_FIREWALL_SERVICE: ActionSpec = row(
    "create_firewall_service",
    ModuleId::Firewall,
    Effect::Mutating,
    Gate::Never,
    &[param("service", true), param("ports", false)],
);
const DELETE_FIREWALL_SERVICE: ActionSpec = row(
    "delete_firewall_service",
    ModuleId::Firewall,
    Effect::Mutating,
    Gate::Always,
    &[param("service", true)],
);
const RELOAD_FIREWALL: ActionSpec = row(
    "reload_firewall",
    ModuleId::Firewall,
    Effect::Mutating,
    Gate::Never,
    &[],
);

// -- Snapshots --
const LIST_SNAPSHOTS: ActionSpec = row(
    "list_snapshots",
    ModuleId::Snapshots,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const STATS: ActionSpec = row(
    "stats",
    ModuleId::Snapshots,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const CREATE_SNAPSHOT: ActionSpec = row(
    "create_snapshot",
    ModuleId::Snapshots,
    Effect::Mutating,
    Gate::Never,
    &[param("section", false)],
);
const RESTORE_SNAPSHOT: ActionSpec = row(
    "restore_snapshot",
    ModuleId::Snapshots,
    Effect::Mutating,
    Gate::Request,
    &[
        param("snapshot", true),
        param("target", true),
        param("dry_run", true),
    ],
);
const GET_BTRBK_CONFIG: ActionSpec = row(
    "get_btrbk_config",
    ModuleId::Snapshots,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const INIT_BTRBK_CONFIG: ActionSpec = row(
    "init_btrbk_config",
    ModuleId::Snapshots,
    Effect::Mutating,
    Gate::Never,
    &[],
);
const SET_BTRBK_CONFIG: ActionSpec = row(
    "set_btrbk_config",
    ModuleId::Snapshots,
    Effect::Mutating,
    Gate::Never,
    &[param("settings", true)],
);
const DELETE_SNAPSHOT: ActionSpec = row(
    "delete_snapshot",
    ModuleId::Snapshots,
    Effect::Mutating,
    Gate::Always,
    &[param("snapshot", true)],
);
const DELETE_SNAPSHOTS: ActionSpec = row(
    "delete_snapshots",
    ModuleId::Snapshots,
    Effect::Mutating,
    Gate::Always,
    &[param("sources", true)],
);

// -- Users --
const LIST_USERS: ActionSpec = row(
    "list_users",
    ModuleId::Users,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const CREATE_USER: ActionSpec = row(
    "create_user",
    ModuleId::Users,
    Effect::Mutating,
    Gate::Never,
    &[
        param("name", true),
        param("fullname", true),
        param("admin", true),
    ],
);
const DELETE_USER: ActionSpec = row(
    "delete_user",
    ModuleId::Users,
    Effect::Mutating,
    Gate::Always,
    &[
        param("uid", false),
        param("remove_files", false),
        param("name", false),
    ],
);
const SET_PASSWORD: ActionSpec = row(
    "set_password",
    ModuleId::Users,
    Effect::Mutating,
    Gate::Always,
    &[
        param("uid", false),
        param("new_password", true),
        param("name", false),
    ],
);
const LIST_GROUPS: ActionSpec = row(
    "list_groups",
    ModuleId::Users,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const SET_GROUPS: ActionSpec = row(
    "set_groups",
    ModuleId::Users,
    Effect::Mutating,
    Gate::Never,
    &[
        param("uid", false),
        param("groups", true),
        param("name", false),
    ],
);

// -- Niri --
const GET_NIRI_CONFIG: ActionSpec = row(
    "get_niri_config",
    ModuleId::Niri,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
// checks a proposal with `niri validate`; nothing is written
const VALIDATE_NIRI_CONFIG: ActionSpec = row(
    "validate_niri_config",
    ModuleId::Niri,
    Effect::ReadOnly,
    Gate::Never,
    &[param("proposed", false)],
);
const SET_NIRI_SECTION: ActionSpec = row(
    "set_niri_section",
    ModuleId::Niri,
    Effect::Mutating,
    Gate::Never,
    &[param("section", true), param("value", true)],
);

// -- Theming --
const THEME_DETECT: ActionSpec = row(
    "theme_detect",
    ModuleId::Theming,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const THEME_GET: ActionSpec = row(
    "theme_get",
    ModuleId::Theming,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const THEME_APPLY: ActionSpec = row(
    "theme_apply",
    ModuleId::Theming,
    Effect::Mutating,
    Gate::Always,
    &[param("show_minimize", false)],
);

// -- Logs --
const LOG_LIST: ActionSpec = row(
    "log_list",
    ModuleId::Logs,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const LOG_READ: ActionSpec = row(
    "log_read",
    ModuleId::Logs,
    Effect::ReadOnly,
    Gate::Never,
    &[
        param("file", true),
        param("lines", false),
        param("cursor", true),
    ],
);
const LOG_SEARCH: ActionSpec = row(
    "log_search",
    ModuleId::Logs,
    Effect::ReadOnly,
    Gate::Never,
    &[
        param("file", true),
        param("query", true),
        param("limit", false),
    ],
);

// -- Crontab --
const CRONTAB_GET: ActionSpec = row(
    "crontab_get",
    ModuleId::Crontab,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const CRONTAB_SET: ActionSpec = row(
    "crontab_set",
    ModuleId::Crontab,
    Effect::Mutating,
    Gate::Always,
    &[param("content", true)],
);

// -- Booster --
const BOOSTER_CONFIG_GET: ActionSpec = row(
    "booster_config_get",
    ModuleId::Booster,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const BOOSTER_CONFIG_SET: ActionSpec = row(
    "booster_config_set",
    ModuleId::Booster,
    Effect::Mutating,
    Gate::Never,
    &[param("keys", true)],
);
const BOOSTER_REGENERATE: ActionSpec = row(
    "booster_regenerate",
    ModuleId::Booster,
    Effect::Mutating,
    Gate::Always,
    &[],
);

// -- Apparmor --
const APPARMOR_STATUS: ActionSpec = row(
    "apparmor_status",
    ModuleId::Apparmor,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const APPARMOR_SET_MODE: ActionSpec = row(
    "apparmor_set_mode",
    ModuleId::Apparmor,
    Effect::Mutating,
    Gate::Request,
    &[param("path", true), param("mode", true)],
);
const APPARMOR_LOAD: ActionSpec = row(
    "apparmor_load",
    ModuleId::Apparmor,
    Effect::Mutating,
    Gate::Never,
    &[param("path", true)],
);
// syntax-checks a profile file, never loads it
const APPARMOR_CHECK: ActionSpec = row(
    "apparmor_check",
    ModuleId::Apparmor,
    Effect::ReadOnly,
    Gate::Never,
    &[param("path", true)],
);
const APPARMOR_REMOVE: ActionSpec = row(
    "apparmor_remove",
    ModuleId::Apparmor,
    Effect::Mutating,
    Gate::Never,
    &[param("path", true)],
);
const APPARMOR_RELOAD_ALL: ActionSpec = row(
    "apparmor_reload_all",
    ModuleId::Apparmor,
    Effect::Mutating,
    Gate::Never,
    &[],
);
const APPARMOR_UNCONFINED: ActionSpec = row(
    "apparmor_unconfined",
    ModuleId::Apparmor,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const APPARMOR_REMOVE_UNKNOWN: ActionSpec = row(
    "apparmor_remove_unknown",
    ModuleId::Apparmor,
    Effect::Mutating,
    Gate::Always,
    &[],
);
const APPARMOR_PROFILE_GET: ActionSpec = row(
    "apparmor_profile_get",
    ModuleId::Apparmor,
    Effect::ReadOnly,
    Gate::Never,
    &[param("path", true)],
);
const APPARMOR_PROFILE_SET: ActionSpec = row(
    "apparmor_profile_set",
    ModuleId::Apparmor,
    Effect::Mutating,
    Gate::Never,
    &[param("path", true), param("content", true)],
);
const APPARMOR_NOTIFICATIONS_ENABLE: ActionSpec = row(
    "apparmor_notifications_enable",
    ModuleId::Apparmor,
    Effect::Mutating,
    Gate::Never,
    &[],
);
const APPARMOR_CLEANPROF: ActionSpec = row(
    "apparmor_cleanprof",
    ModuleId::Apparmor,
    Effect::Mutating,
    Gate::Always,
    &[param("name", true)],
);
// writes hardlink dependency lines into the profile file
const APPARMOR_AUTODEP: ActionSpec = row(
    "apparmor_autodep",
    ModuleId::Apparmor,
    Effect::Mutating,
    Gate::Never,
    &[param("path", true)],
);
const APPARMOR_FEATURES_ABI: ActionSpec = row(
    "apparmor_features_abi",
    ModuleId::Apparmor,
    Effect::ReadOnly,
    Gate::Never,
    &[],
);
const APPARMOR_DECODE: ActionSpec = row(
    "apparmor_decode",
    ModuleId::Apparmor,
    Effect::ReadOnly,
    Gate::Never,
    &[param("hex", true)],
);
const APPARMOR_EASYPROF: ActionSpec = row(
    "apparmor_easyprof",
    ModuleId::Apparmor,
    Effect::ReadOnly,
    Gate::Never,
    &[
        param("binary", true),
        param("template", false),
        param("policy_groups", false),
        param("template_vars", false),
    ],
);
const APPARMOR_EASYPROF_LIST: ActionSpec = row(
    "apparmor_easyprof_list",
    ModuleId::Apparmor,
    Effect::ReadOnly,
    Gate::Never,
    &[param("what", true)],
);

/// The catalogue, one row per `Action` variant, in declaration order.
pub static ACTIONS: &[ActionSpec] = &[
    // -- System --
    STATUS,
    // -- System --
    INIT,
    GET_CONFIG,
    SET_MODULE_SWITCH,
    SET_UPDATE_AUTO,
    // -- Config --
    LIST,
    REFRESH,
    // -- Capabilities --
    JOB_OUTPUT,
    // -- System --
    LIST_INSTALLED,
    SEARCH,
    LIST_NIX_REPOSITORIES,
    ADD_NIX_REPOSITORY,
    REMOVE_NIX_REPOSITORY,
    INSTALL,
    REMOVE,
    REMOVE_ALL,
    REMOVE_REGEX,
    UPGRADE,
    UPGRADE_REGEX,
    LIST_NIX_UPDATES,
    GENERATIONS,
    ROLLBACK,
    WIPE_HISTORY,
    DIFF_CLOSURES,
    STORE_GC,
    PRUNE_GENERATIONS,
    // -- PackagesNix --
    LIST_APK,
    SEARCH_APK,
    LIST_APK_REPOSITORIES,
    ADD_APK_REPOSITORY,
    REMOVE_APK_REPOSITORY,
    INSTALL_APK,
    PREVIEW_APK_INSTALL,
    REMOVE_APK,
    UPGRADE_APK,
    LIST_APK_UPDATES,
    UPGRADE_APK_PACKAGE,
    // -- PackagesApk --
    LIST_FLATPAK,
    LIST_FLATPAK_REMOTES,
    LIST_FLATPAK_UPDATES,
    ADD_FLATPAK_REMOTE,
    REMOVE_FLATPAK_REMOTE,
    SEARCH_FLATPAK,
    INSTALL_FLATPAK,
    GET_FLATPAK_INFO,
    REMOVE_FLATPAK,
    // -- PackagesFlatpak --
    APK_REMOVE_REGEX,
    APK_UPGRADE_REGEX,
    // -- PackagesApk --
    FLATPAK_REMOVE_REGEX,
    FLATPAK_UPGRADE_REGEX,
    UPDATE_FLATPAK,
    UPDATE_FLATPAK_APP,
    REMOVE_FLATPAK_ORPHANS,
    // -- PackagesFlatpak --
    LIST_PRINTERS,
    PRINTER_JOBS,
    ADD_PRINTER,
    ADD_PRINTER_MANUAL,
    REMOVE_PRINTER,
    SET_DEFAULT_PRINTER,
    SET_PRINTER_OPTIONS,
    PAUSE_PRINTER,
    CANCEL_PRINTER_JOBS,
    // -- Printers --
    LIST_SCANNERS,
    GET_SCANNER_CONFIG,
    SET_SCANNER_DEVICE,
    REMOVE_SCANNER_DEVICE,
    SET_SCANNER_DISCOVERY,
    // -- Scanners --
    GET_NETWORK,
    SET_WIFI_ENABLED,
    CONNECT_WIFI,
    CONNECT_NETWORK,
    DISCONNECT_NETWORK,
    FORGET_NETWORK,
    SET_NETWORK_CONNECTION,
    SET_NETWORK_IPV4,
    // -- Network --
    FORMAT_DISK,
    PARTITION_CREATE,
    PARTITION_DELETE,
    // -- Disks --
    GET_BLUETOOTH,
    SET_BLUETOOTH_POWER,
    SCAN_BLUETOOTH,
    BLUETOOTH_ACTION,
    // -- Bluetooth --
    LIST_DISKS,
    MOUNT_DISK,
    UNMOUNT_DISK,
    // -- Disks --
    LIST_SERVICES,
    SERVICE_ACTION,
    // -- Services --
    GET_FIREWALL,
    SET_DEFAULT_FIREWALL_ZONE,
    ADD_FIREWALL_SERVICE,
    REMOVE_FIREWALL_SERVICE,
    ADD_FIREWALL_PORT,
    REMOVE_FIREWALL_PORT,
    SET_FIREWALL_ENABLED,
    CREATE_FIREWALL_SERVICE,
    DELETE_FIREWALL_SERVICE,
    RELOAD_FIREWALL,
    // -- Firewall --
    LIST_SNAPSHOTS,
    STATS,
    CREATE_SNAPSHOT,
    RESTORE_SNAPSHOT,
    GET_BTRBK_CONFIG,
    INIT_BTRBK_CONFIG,
    SET_BTRBK_CONFIG,
    DELETE_SNAPSHOT,
    DELETE_SNAPSHOTS,
    // -- Snapshots --
    LIST_USERS,
    CREATE_USER,
    DELETE_USER,
    SET_PASSWORD,
    LIST_GROUPS,
    SET_GROUPS,
    // -- Users --
    GET_NIRI_CONFIG,
    VALIDATE_NIRI_CONFIG,
    SET_NIRI_SECTION,
    // -- Niri --
    THEME_DETECT,
    THEME_GET,
    THEME_APPLY,
    // -- Theming --
    LOG_LIST,
    LOG_READ,
    LOG_SEARCH,
    // -- Logs --
    CRONTAB_GET,
    CRONTAB_SET,
    // -- Crontab --
    BOOSTER_CONFIG_GET,
    BOOSTER_CONFIG_SET,
    BOOSTER_REGENERATE,
    // -- Booster --
    APPARMOR_STATUS,
    APPARMOR_SET_MODE,
    APPARMOR_LOAD,
    APPARMOR_CHECK,
    APPARMOR_REMOVE,
    APPARMOR_RELOAD_ALL,
    APPARMOR_UNCONFINED,
    APPARMOR_REMOVE_UNKNOWN,
    APPARMOR_PROFILE_GET,
    APPARMOR_PROFILE_SET,
    APPARMOR_NOTIFICATIONS_ENABLE,
    APPARMOR_CLEANPROF,
    APPARMOR_AUTODEP,
    APPARMOR_FEATURES_ABI,
    APPARMOR_DECODE,
    APPARMOR_EASYPROF,
    APPARMOR_EASYPROF_LIST,
];

impl Action {
    /// The catalogue row of this action.
    ///
    /// Every variant is matched: a new `Action` does not compile until it is
    /// classified, which is what keeps [`ACTIONS`] complete.
    pub fn spec(&self) -> &'static ActionSpec {
        match self {
            Self::Status => &STATUS,
            Self::Init => &INIT,
            Self::GetConfig => &GET_CONFIG,
            Self::SetModuleSwitch { .. } => &SET_MODULE_SWITCH,
            Self::SetUpdateAuto { .. } => &SET_UPDATE_AUTO,
            Self::List => &LIST,
            Self::Refresh => &REFRESH,
            Self::JobOutput { .. } => &JOB_OUTPUT,
            Self::ListInstalled => &LIST_INSTALLED,
            Self::Search { .. } => &SEARCH,
            Self::ListNixRepositories { .. } => &LIST_NIX_REPOSITORIES,
            Self::AddNixRepository { .. } => &ADD_NIX_REPOSITORY,
            Self::RemoveNixRepository { .. } => &REMOVE_NIX_REPOSITORY,
            Self::Install { .. } => &INSTALL,
            Self::Remove { .. } => &REMOVE,
            Self::RemoveAll => &REMOVE_ALL,
            Self::RemoveRegex { .. } => &REMOVE_REGEX,
            Self::Upgrade { .. } => &UPGRADE,
            Self::UpgradeRegex { .. } => &UPGRADE_REGEX,
            Self::ListNixUpdates => &LIST_NIX_UPDATES,
            Self::Generations => &GENERATIONS,
            Self::Rollback { .. } => &ROLLBACK,
            Self::WipeHistory { .. } => &WIPE_HISTORY,
            Self::DiffClosures { .. } => &DIFF_CLOSURES,
            Self::StoreGc => &STORE_GC,
            Self::PruneGenerations { .. } => &PRUNE_GENERATIONS,
            Self::ListApk => &LIST_APK,
            Self::SearchApk { .. } => &SEARCH_APK,
            Self::ListApkRepositories => &LIST_APK_REPOSITORIES,
            Self::AddApkRepository { .. } => &ADD_APK_REPOSITORY,
            Self::RemoveApkRepository { .. } => &REMOVE_APK_REPOSITORY,
            Self::InstallApk { .. } => &INSTALL_APK,
            Self::PreviewApkInstall { .. } => &PREVIEW_APK_INSTALL,
            Self::RemoveApk { .. } => &REMOVE_APK,
            Self::UpgradeApk => &UPGRADE_APK,
            Self::ListApkUpdates { .. } => &LIST_APK_UPDATES,
            Self::UpgradeApkPackage { .. } => &UPGRADE_APK_PACKAGE,
            Self::ListFlatpak { .. } => &LIST_FLATPAK,
            Self::ListFlatpakRemotes { .. } => &LIST_FLATPAK_REMOTES,
            Self::ListFlatpakUpdates { .. } => &LIST_FLATPAK_UPDATES,
            Self::AddFlatpakRemote { .. } => &ADD_FLATPAK_REMOTE,
            Self::RemoveFlatpakRemote { .. } => &REMOVE_FLATPAK_REMOTE,
            Self::SearchFlatpak { .. } => &SEARCH_FLATPAK,
            Self::InstallFlatpak { .. } => &INSTALL_FLATPAK,
            Self::GetFlatpakInfo { .. } => &GET_FLATPAK_INFO,
            Self::RemoveFlatpak { .. } => &REMOVE_FLATPAK,
            Self::ApkRemoveRegex { .. } => &APK_REMOVE_REGEX,
            Self::ApkUpgradeRegex { .. } => &APK_UPGRADE_REGEX,
            Self::FlatpakRemoveRegex { .. } => &FLATPAK_REMOVE_REGEX,
            Self::FlatpakUpgradeRegex { .. } => &FLATPAK_UPGRADE_REGEX,
            Self::UpdateFlatpak { .. } => &UPDATE_FLATPAK,
            Self::UpdateFlatpakApp { .. } => &UPDATE_FLATPAK_APP,
            Self::RemoveFlatpakOrphans { .. } => &REMOVE_FLATPAK_ORPHANS,
            Self::ListPrinters => &LIST_PRINTERS,
            Self::PrinterJobs { .. } => &PRINTER_JOBS,
            Self::AddPrinter { .. } => &ADD_PRINTER,
            Self::AddPrinterManual { .. } => &ADD_PRINTER_MANUAL,
            Self::RemovePrinter { .. } => &REMOVE_PRINTER,
            Self::SetDefaultPrinter { .. } => &SET_DEFAULT_PRINTER,
            Self::SetPrinterOptions { .. } => &SET_PRINTER_OPTIONS,
            Self::PausePrinter { .. } => &PAUSE_PRINTER,
            Self::CancelPrinterJobs { .. } => &CANCEL_PRINTER_JOBS,
            Self::ListScanners => &LIST_SCANNERS,
            Self::GetScannerConfig => &GET_SCANNER_CONFIG,
            Self::SetScannerDevice { .. } => &SET_SCANNER_DEVICE,
            Self::RemoveScannerDevice { .. } => &REMOVE_SCANNER_DEVICE,
            Self::SetScannerDiscovery { .. } => &SET_SCANNER_DISCOVERY,
            Self::GetNetwork => &GET_NETWORK,
            Self::SetWifiEnabled { .. } => &SET_WIFI_ENABLED,
            Self::ConnectWifi { .. } => &CONNECT_WIFI,
            Self::ConnectNetwork { .. } => &CONNECT_NETWORK,
            Self::DisconnectNetwork { .. } => &DISCONNECT_NETWORK,
            Self::ForgetNetwork { .. } => &FORGET_NETWORK,
            Self::SetNetworkConnection { .. } => &SET_NETWORK_CONNECTION,
            Self::SetNetworkIpv4 { .. } => &SET_NETWORK_IPV4,
            Self::FormatDisk { .. } => &FORMAT_DISK,
            Self::PartitionCreate { .. } => &PARTITION_CREATE,
            Self::PartitionDelete { .. } => &PARTITION_DELETE,
            Self::GetBluetooth => &GET_BLUETOOTH,
            Self::SetBluetoothPower { .. } => &SET_BLUETOOTH_POWER,
            Self::ScanBluetooth => &SCAN_BLUETOOTH,
            Self::BluetoothAction { .. } => &BLUETOOTH_ACTION,
            Self::ListDisks => &LIST_DISKS,
            Self::MountDisk { .. } => &MOUNT_DISK,
            Self::UnmountDisk { .. } => &UNMOUNT_DISK,
            Self::ListServices => &LIST_SERVICES,
            Self::ServiceAction { .. } => &SERVICE_ACTION,
            Self::GetFirewall => &GET_FIREWALL,
            Self::SetDefaultFirewallZone { .. } => &SET_DEFAULT_FIREWALL_ZONE,
            Self::AddFirewallService { .. } => &ADD_FIREWALL_SERVICE,
            Self::RemoveFirewallService { .. } => &REMOVE_FIREWALL_SERVICE,
            Self::AddFirewallPort { .. } => &ADD_FIREWALL_PORT,
            Self::RemoveFirewallPort { .. } => &REMOVE_FIREWALL_PORT,
            Self::SetFirewallEnabled { .. } => &SET_FIREWALL_ENABLED,
            Self::CreateFirewallService { .. } => &CREATE_FIREWALL_SERVICE,
            Self::DeleteFirewallService { .. } => &DELETE_FIREWALL_SERVICE,
            Self::ReloadFirewall => &RELOAD_FIREWALL,
            Self::ListSnapshots => &LIST_SNAPSHOTS,
            Self::Stats => &STATS,
            Self::CreateSnapshot { .. } => &CREATE_SNAPSHOT,
            Self::RestoreSnapshot { .. } => &RESTORE_SNAPSHOT,
            Self::GetBtrbkConfig => &GET_BTRBK_CONFIG,
            Self::InitBtrbkConfig => &INIT_BTRBK_CONFIG,
            Self::SetBtrbkConfig { .. } => &SET_BTRBK_CONFIG,
            Self::DeleteSnapshot { .. } => &DELETE_SNAPSHOT,
            Self::DeleteSnapshots { .. } => &DELETE_SNAPSHOTS,
            Self::ListUsers => &LIST_USERS,
            Self::CreateUser { .. } => &CREATE_USER,
            Self::DeleteUser { .. } => &DELETE_USER,
            Self::SetPassword { .. } => &SET_PASSWORD,
            Self::ListGroups => &LIST_GROUPS,
            Self::SetGroups { .. } => &SET_GROUPS,
            Self::GetNiriConfig => &GET_NIRI_CONFIG,
            Self::ValidateNiriConfig { .. } => &VALIDATE_NIRI_CONFIG,
            Self::SetNiriSection { .. } => &SET_NIRI_SECTION,
            Self::ThemeDetect => &THEME_DETECT,
            Self::ThemeGet => &THEME_GET,
            Self::ThemeApply { .. } => &THEME_APPLY,
            Self::LogList => &LOG_LIST,
            Self::LogRead { .. } => &LOG_READ,
            Self::LogSearch { .. } => &LOG_SEARCH,
            Self::CrontabGet => &CRONTAB_GET,
            Self::CrontabSet { .. } => &CRONTAB_SET,
            Self::BoosterConfigGet => &BOOSTER_CONFIG_GET,
            Self::BoosterConfigSet { .. } => &BOOSTER_CONFIG_SET,
            Self::BoosterRegenerate => &BOOSTER_REGENERATE,
            Self::ApparmorStatus => &APPARMOR_STATUS,
            Self::ApparmorSetMode { .. } => &APPARMOR_SET_MODE,
            Self::ApparmorLoad { .. } => &APPARMOR_LOAD,
            Self::ApparmorCheck { .. } => &APPARMOR_CHECK,
            Self::ApparmorRemove { .. } => &APPARMOR_REMOVE,
            Self::ApparmorReloadAll => &APPARMOR_RELOAD_ALL,
            Self::ApparmorUnconfined => &APPARMOR_UNCONFINED,
            Self::ApparmorRemoveUnknown => &APPARMOR_REMOVE_UNKNOWN,
            Self::ApparmorProfileGet { .. } => &APPARMOR_PROFILE_GET,
            Self::ApparmorProfileSet { .. } => &APPARMOR_PROFILE_SET,
            Self::ApparmorNotificationsEnable => &APPARMOR_NOTIFICATIONS_ENABLE,
            Self::ApparmorCleanprof { .. } => &APPARMOR_CLEANPROF,
            Self::ApparmorAutodep { .. } => &APPARMOR_AUTODEP,
            Self::ApparmorFeaturesAbi => &APPARMOR_FEATURES_ABI,
            Self::ApparmorDecode { .. } => &APPARMOR_DECODE,
            Self::ApparmorEasyprof { .. } => &APPARMOR_EASYPROF,
            Self::ApparmorEasyprofList { .. } => &APPARMOR_EASYPROF_LIST,
        }
    }

    /// The `dry_run` flag of the actions that can be previewed.
    fn dry_run(&self) -> Option<bool> {
        match self {
            Self::RemoveRegex { dry_run, .. }
            | Self::UpgradeRegex { dry_run, .. }
            | Self::ApkRemoveRegex { dry_run, .. }
            | Self::ApkUpgradeRegex { dry_run, .. }
            | Self::FlatpakRemoveRegex { dry_run, .. }
            | Self::FlatpakUpgradeRegex { dry_run, .. }
            | Self::WipeHistory { dry_run, .. }
            | Self::PruneGenerations { dry_run, .. }
            | Self::RestoreSnapshot { dry_run, .. } => Some(*dry_run),
            _ => None,
        }
    }

    /// Effect of THIS request: a dry run or preview changes nothing, so it is
    /// read-only even when the variant mutates.
    pub fn effect(&self) -> Effect {
        // The inverse of `dry_run`: `list_apk_updates` refreshes the index
        // only when the caller asks, and that one payload writes the cache.
        if self.refreshes_index() {
            return Effect::Mutating;
        }
        match (self.spec().effect, self.dry_run()) {
            (Effect::Mutating, Some(true)) => Effect::ReadOnly,
            (effect, _) => effect,
        }
    }

    /// Whether this request refreshes a cached package index on purpose.
    fn refreshes_index(&self) -> bool {
        matches!(self, Self::ListApkUpdates { refresh: true })
    }

    /// Whether the request only reads (previews included).
    pub fn is_read_only(&self) -> bool {
        self.effect() == Effect::ReadOnly
    }

    /// Whether the request changes the machine.
    pub fn is_mutating(&self) -> bool {
        !self.is_read_only()
    }

    /// Whether the request needs the destructive confirmation (`--yes`).
    pub fn is_destructive(&self) -> bool {
        match self.spec().gate {
            Gate::Never => false,
            Gate::Always => true,
            Gate::Request => self.request_is_destructive(),
        }
    }

    /// The payload decides the confirmation (see [`Gate::Request`]).
    fn request_is_destructive(&self) -> bool {
        match self {
            // Only unloading policy counts; enforce/complain write nothing
            // the same call cannot restore.
            Self::ApparmorSetMode { mode, .. } => mode == "disable",
            // firewalld has no global switch: `enabled = false` sets the
            // default zone target to ACCEPT, i.e. filtering off.
            Self::SetFirewallEnabled { enabled, .. } => !*enabled,
            // Preview-capable actions: only the real run is destructive.
            other => other.dry_run() == Some(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_named(name: &str) -> &'static ActionSpec {
        ACTIONS
            .iter()
            .find(|spec| spec.name == name)
            .unwrap_or_else(|| panic!("no catalogue row for {name}"))
    }

    #[test]
    fn every_row_has_a_unique_wire_name() {
        let mut names: Vec<&str> = ACTIONS.iter().map(|spec| spec.name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "duplicate action name in the catalogue");
        assert!(
            total > 100,
            "the catalogue shrank unexpectedly: {total} rows"
        );
    }

    #[test]
    fn optional_scope_flags_are_not_required() {
        // `system` is optional on every flatpak action (serde default false =
        // the user installation); `all` is optional on nix `upgrade`.
        let flatpak_with_scope = [
            "list_flatpak",
            "list_flatpak_updates",
            "add_flatpak_remote",
            "remove_flatpak_remote",
            "search_flatpak",
            "install_flatpak",
            "get_flatpak_info",
            "remove_flatpak",
            "flatpak_remove_regex",
            "flatpak_upgrade_regex",
            "update_flatpak",
            "update_flatpak_app",
            "remove_flatpak_orphans",
        ];
        for name in flatpak_with_scope {
            let system = row_named(name)
                .params
                .iter()
                .find(|p| p.name == "system")
                .unwrap_or_else(|| panic!("{name} has no system param"));
            assert!(!system.required, "{name}: system must be optional");
        }
        let all = row_named("upgrade")
            .params
            .iter()
            .find(|p| p.name == "all")
            .expect("upgrade has an all param");
        assert!(!all.required, "upgrade: all must be optional");
    }

    #[test]
    fn destructive_deletions_keep_dry_run_required() {
        // Documented on the action: a deletion never previews by default.
        for name in ["wipe_history", "prune_generations"] {
            let dry_run = row_named(name)
                .params
                .iter()
                .find(|p| p.name == DRY_RUN)
                .unwrap_or_else(|| panic!("{name} has no dry_run param"));
            assert!(dry_run.required, "{name}: dry_run must stay required");
        }
    }

    #[test]
    fn read_only_rows_never_need_confirmation() {
        for spec in ACTIONS {
            if spec.effect == Effect::ReadOnly {
                assert_eq!(
                    spec.gate,
                    Gate::Never,
                    "{} reads only and must not be gated",
                    spec.name
                );
            }
            // The other direction: a gated action cannot be read-only.
            if spec.gate != Gate::Never {
                assert_eq!(
                    spec.effect,
                    Effect::Mutating,
                    "{} is gated but classified read-only",
                    spec.name
                );
            }
        }
    }

    #[test]
    fn previews_are_flagged_and_gated_by_the_payload() {
        let mut previews = Vec::new();
        for spec in ACTIONS {
            if spec.previewable() {
                assert_eq!(
                    spec.gate,
                    Gate::Request,
                    "{} previews, so --yes must depend on the payload",
                    spec.name
                );
                assert_eq!(
                    spec.effect,
                    Effect::Mutating,
                    "{} previews the real run, which mutates",
                    spec.name
                );
                previews.push(spec.name);
            }
        }
        assert_eq!(previews.len(), 9, "previewable actions: {previews:?}");
    }

    #[test]
    fn dry_runs_are_read_only_and_ungated() {
        let preview = Action::RemoveRegex {
            pattern: "^python".into(),
            dry_run: true,
        };
        assert!(preview.is_read_only());
        assert!(!preview.is_destructive());
        let apply = Action::RemoveRegex {
            pattern: "^python".into(),
            dry_run: false,
        };
        assert!(apply.is_mutating());
        assert!(apply.is_destructive());
        let restore = Action::RestoreSnapshot {
            snapshot: "@snap".into(),
            target: "/mnt/restore".into(),
            dry_run: true,
        };
        assert!(restore.is_read_only());
        assert!(!restore.is_destructive());
    }

    #[test]
    fn payload_gated_actions_follow_the_payload() {
        // Unloading policy is the destructive half of apparmor_set_mode.
        assert!(Action::ApparmorSetMode {
            path: "/etc/apparmor.d/local/usr.bin.niri".into(),
            mode: "disable".into(),
        }
        .is_destructive());
        assert!(!Action::ApparmorSetMode {
            path: "/etc/apparmor.d/local/usr.bin.niri".into(),
            mode: "enforce".into(),
        }
        .is_destructive());
        // firewalld has no switch: off means the default zone accepts all.
        assert!(Action::SetFirewallEnabled { enabled: false }.is_destructive());
        assert!(!Action::SetFirewallEnabled { enabled: true }.is_destructive());
    }

    #[test]
    fn list_apk_updates_only_writes_when_asked_to_refresh() {
        // The cached-index diff is a read; `refresh = true` runs `apk update`
        // (rewrites the index files), so that request must be visible as
        // mutating to clients polling on a timer.
        let cached = Action::ListApkUpdates { refresh: false };
        assert_eq!(cached.spec().effect, Effect::ReadOnly);
        assert!(cached.is_read_only());
        assert!(!cached.is_destructive());
        let refreshed = Action::ListApkUpdates { refresh: true };
        assert!(refreshed.is_mutating());
        assert!(!refreshed.is_destructive());
    }

    #[test]
    fn row_json_shape_is_stable() {
        assert_eq!(
            row_named("install_apk").to_json(),
            json!({
                "name": "install_apk",
                "module": "packages_apk",
                "effect": "mutating",
                "destructive": "never",
                "previewable": false,
                "params": [{ "name": "package", "required": true }],
            })
        );
        // A preview action: dry_run is optional on the wire (serde default
        // false) and the gate is decided per request.
        assert_eq!(
            row_named("remove_regex").to_json(),
            json!({
                "name": "remove_regex",
                "module": "packages_nix",
                "effect": "mutating",
                "destructive": "request",
                "previewable": true,
                "params": [
                    { "name": "pattern", "required": true },
                    { "name": "dry_run", "required": false },
                ],
            })
        );
    }

    #[test]
    fn document_lists_every_row() {
        let doc = json_document();
        assert_eq!(doc["schema"], CATALOGUE_SCHEMA);
        assert_eq!(doc["count"], ACTIONS.len());
        assert_eq!(doc["actions"].as_array().map(Vec::len), Some(ACTIONS.len()));
        assert!(doc["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["name"] == "log_read"));
    }
}
