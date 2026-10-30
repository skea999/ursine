//! Generic command surface: `ursinectl <module> <action> [key=value ...]`.
//!
//! Values are parsed as JSON when they are valid JSON (`dry_run=true`,
//! `keep=3`, `value={...}`) and fall back to plain strings, then fed to the
//! SAME serde `Action` tag the wire uses — so every action that exists in the
//! API is reachable from the CLI by construction. `key=@path` reads a file,
//! `key=-` reads stdin; `password`/`new_password` MUST use `-` (never argv).

use std::path::PathBuf;

use ursine_api::Action;

/// Parsed command line (everything after argv[0]).
#[derive(Debug)]
pub enum Cmd {
    Help(Option<String>),
    Version,
    Run(RunOpts),
}

#[derive(Debug)]
pub struct RunOpts {
    pub json: bool,
    pub yes: bool,
    pub as_user: Option<String>,
    pub config: PathBuf,
    pub module: String,
    pub action: String,
    pub pairs: Vec<(String, String)>,
}

pub const USAGE: &str = "\
ursinectl — Ursine root CLI (in-process; works in a chroot)

USAGE: sudo ursinectl [--json] [--yes] [--as-user USER] [--config PATH] \\
                      <module> <action> [key=value ...]

  Values are JSON when possible (dry_run=true, keep=3), else plain strings.
  key=@path reads a file; key=- reads stdin.
  password/new_password MUST be key=- (never on the command line).
  Destructive actions require --yes.

MODULES (wire name; short alias in parentheses):
  system              status, job_output
  config              init, get_config, set_module_switch
  capabilities(caps)  list, refresh
  packages_nix(nix)   list_installed, search, install, remove, remove_all,
                      remove_regex, upgrade, upgrade_regex, generations,
                      rollback, wipe_history, diff_closures, store_gc,
                      prune_generations
  packages_apk(apk)   list_apk, search_apk, refresh_apk,
                      list_apk_repositories, install_apk,
                      preview_apk_install, remove_apk, upgrade_apk,
                      list_apk_updates, upgrade_apk_package
  packages_flatpak(flatpak)
                      list_flatpak, list_flatpak_remotes, search_flatpak,
                      install_flatpak, remove_flatpak, update_flatpak,
                      update_flatpak_app, remove_flatpak_orphans
  printers            list_printers, printer_jobs, add_printer,
                      add_printer_manual, remove_printer,
                      set_default_printer, set_printer_options,
                      pause_printer, cancel_printer_jobs
  scanners            list_scanners, get_scanner_config, set_scanner_device,
                      remove_scanner_device, set_scanner_discovery
  network             get_network, set_wifi_enabled, connect_wifi,
                      connect_network, disconnect_network
  bluetooth           get_bluetooth, set_bluetooth_power, scan_bluetooth,
                      bluetooth_action
  disks               list_disks, mount_disk, unmount_disk
  services            list_services, service_action
  firewall            get_firewall, set_default_firewall_zone,
                      add_firewall_service, remove_firewall_service,
                      add_firewall_port, remove_firewall_port,
                      reload_firewall
  snapshots           list_snapshots, stats, create_snapshot,
                      restore_snapshot, get_btrbk_config, init_btrbk_config,
                      set_btrbk_config, delete_snapshot, delete_snapshots
  users               list_users, create_user, delete_user, set_password,
                      list_groups, set_groups
  niri                get_niri_config, validate_niri_config,
                      set_niri_section
  theming             theme_detect, theme_get, theme_apply
  logs                log_list, log_read, log_search
  crontab             crontab_get, crontab_set

EXAMPLES:
  ursinectl system status
  ursinectl caps list
  ursinectl nix search query=niri
  ursinectl apk install_apk package=niri --yes
  ursinectl crontab crontab_set content=@my.cron --yes
  printf 'secret\\n' | ursinectl users set_password uid=1000 new_password=- --yes
  ursinectl logs log_read file=pmelog.log lines=200
";

/// Parse argv (without argv[0]).
pub fn parse(args: &[String]) -> Result<Cmd, String> {
    let mut json = false;
    let mut yes = false;
    let mut as_user = None;
    let mut config = PathBuf::from("/etc/ursine/config.toml");
    let mut positional = Vec::new();
    let mut pairs = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                let module = it.next().map(String::from);
                return Ok(Cmd::Help(module));
            }
            "--version" => return Ok(Cmd::Version),
            "--json" => json = true,
            "--yes" | "-y" => yes = true,
            "--as-user" => {
                let v = it.next().ok_or("--as-user needs a user")?;
                as_user = Some(v.clone());
            }
            "--config" => {
                let v = it.next().ok_or("--config needs a path")?;
                config = PathBuf::from(v);
            }
            other if let Some(v) = other.strip_prefix("--as-user=") => {
                as_user = Some(v.to_string());
            }
            other if let Some(v) = other.strip_prefix("--config=") => {
                config = PathBuf::from(v);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option {other:?}"));
            }
            other if other.contains('=') => {
                let (k, v) = other
                    .split_once('=')
                    .ok_or_else(|| format!("bad pair {other:?}"))?;
                if k.is_empty() {
                    return Err(format!("empty key in {other:?}"));
                }
                pairs.push((k.to_string(), v.to_string()));
            }
            other => positional.push(other.to_string()),
        }
    }
    if positional.len() == 1 && pairs.is_empty() {
        // `ursinectl nix` → module help
        return Ok(Cmd::Help(positional.into_iter().next()));
    }
    if positional.len() != 2 {
        return Err("expected <module> <action> [key=value ...]".to_string());
    }
    let mut module = positional.remove(0);
    let action = positional.remove(0);
    module = resolve_module_alias(&module)?;
    Ok(Cmd::Run(RunOpts {
        json,
        yes,
        as_user,
        config,
        module,
        action,
        pairs,
    }))
}

/// Short aliases for the long wire module names.
fn resolve_module_alias(name: &str) -> Result<String, String> {
    let wire = match name {
        "nix" => "packages_nix",
        "apk" => "packages_apk",
        "flatpak" => "packages_flatpak",
        "caps" | "cap" => "capabilities",
        other => other,
    };
    // Fail fast on unknown module names (serde knows the real set).
    let ok =
        serde_json::from_value::<ursine_api::ModuleId>(serde_json::Value::String(wire.to_string()))
            .is_ok();
    if !ok {
        return Err(format!("unknown module {name:?} (see ursinectl --help)"));
    }
    Ok(wire.to_string())
}

/// Build a wire `Action` from the action tag plus `key=value` pairs
/// (secrets read from the process stdin).
pub fn build_action(action: &str, pairs: &[(String, String)]) -> Result<Action, String> {
    build_action_with(action, pairs, read_secret_from_stdin)
}

/// [`build_action`] with an injectable secret reader (tests must never
/// block on the real stdin).
pub fn build_action_with(
    action: &str,
    pairs: &[(String, String)],
    mut read_line: impl FnMut() -> Result<String, String>,
) -> Result<Action, String> {
    let mut map = serde_json::Map::new();
    map.insert(
        "action".to_string(),
        serde_json::Value::String(action.to_string()),
    );
    for (key, raw) in pairs {
        if map.contains_key(key) {
            return Err(format!("duplicate key {key:?}"));
        }
        let value = match key.as_str() {
            // Secrets never ride on argv (visible in /proc).
            "password" | "new_password" => read_secret(raw, &mut read_line)?,
            _ => expand_value(raw)?,
        };
        map.insert(key.clone(), value);
    }
    let provided = map.clone();
    let value: Action =
        parse_action(&map).map_err(|e| format!("invalid arguments for action {action:?}: {e}"))?;
    // serde ignores unknown fields (wire back-compat); a typo'd key on the
    // CLI would otherwise vanish silently (`dry_ru=true` == not dry-run).
    let encoded =
        serde_json::to_value(&value).map_err(|e| format!("encode action {action:?}: {e}"))?;
    if let Some(obj) = encoded.as_object() {
        for (key, val) in &provided {
            if key == "action" || val.is_null() {
                continue;
            }
            if !obj.contains_key(key) {
                return Err(format!("unknown field {key:?} for action {action:?}"));
            }
        }
    }
    Ok(value)
}

/// Deserialize an `Action` from the wire map. When a scalar lands in a
/// string field (`port=8080` — JSON gives a number, the field wants
/// `String`), retry with that single value coerced to its text form; all
/// other values stay typed (`keep=3` remains a number).
fn parse_action(
    map: &serde_json::Map<String, serde_json::Value>,
) -> Result<Action, serde_json::Error> {
    let first = serde_json::from_value::<Action>(serde_json::Value::Object(map.clone()));
    let Err(err) = first else {
        return first;
    };
    for (key, val) in map {
        if key == "action"
            || !matches!(
                val,
                serde_json::Value::Number(_) | serde_json::Value::Bool(_)
            )
        {
            continue;
        }
        let mut coerced = map.clone();
        coerced.insert(key.clone(), serde_json::Value::String(val.to_string()));
        if let Ok(action) = serde_json::from_value::<Action>(serde_json::Value::Object(coerced)) {
            return Ok(action);
        }
    }
    Err(err)
}

/// `key=-` reads one line from the reader; anything else is refused for
/// secrets (argv is visible in /proc).
fn read_secret(
    raw: &str,
    read_line: &mut impl FnMut() -> Result<String, String>,
) -> Result<serde_json::Value, String> {
    if raw != "-" {
        return Err(format!(
            "secrets must not appear on the command line; use {raw}=- and type or pipe the value"
        ));
    }
    let line = read_line()?;
    Ok(serde_json::Value::String(
        line.trim_end_matches(['\n', '\r']).to_string(),
    ))
}

/// The production reader: one line from the process stdin.
fn read_secret_from_stdin() -> Result<String, String> {
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("read secret from stdin: {e}"))?;
    Ok(line)
}

/// `@path` reads a file, `-` reads stdin whole, JSON parses, else string.
fn expand_value(raw: &str) -> Result<serde_json::Value, String> {
    if let Some(path) = raw.strip_prefix('@') {
        if path.is_empty() {
            return Err("@ needs a path".to_string());
        }
        let content = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
        return Ok(serde_json::Value::String(content));
    }
    if raw == "-" {
        use std::io::Read as _;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("read stdin: {e}"))?;
        return Ok(serde_json::Value::String(buf));
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        return Ok(v);
    }
    Ok(serde_json::Value::String(raw.to_string()))
}

/// Expected dispatch outcome for one sample in the CI container (where
/// exactly the tools in `ursine-ci` exist: apk yes, nix/flatpak/dbus no).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// Parse/classify only: covered end-to-end by a dedicated test or the
    /// E2E suite (round-trips, config file writes, full-system upgrades).
    Skip,
    /// Must succeed through the real daemon dispatch.
    Ok,
    /// Must fail at the capability gate (`Unavailable`): the module's tool
    /// is absent from the CI container, so nothing behind it may run.
    Gate,
    /// Ok or any typed error — the path runs but the outcome depends on the
    /// environment (local index state, init tooling).
    Any,
    /// Must fail with exactly this error code.
    Code(ursine_api::ErrorCode),
}

/// One test case per wire action: module, serde tag, sample `key=value`
/// pairs, `is_destructive` expectation and dispatch expectation.
#[derive(Debug)]
pub struct Sample {
    pub module: ursine_api::ModuleId,
    pub action: &'static str,
    pub pairs: &'static [(&'static str, &'static str)],
    pub destructive: bool,
    pub dispatch: Dispatch,
}

/// Every wire action exactly once (drift-guarded against `ALL_ACTIONS`).
pub const SAMPLES: &[Sample] = &[
    // -- system --
    Sample {
        module: ursine_api::ModuleId::System,
        action: "status",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Ok,
    },
    Sample {
        module: ursine_api::ModuleId::System,
        action: "job_output",
        pairs: &[("job_id", "ffffffffffffffff"), ("offset", "0")],
        destructive: false,
        dispatch: Dispatch::Code(ursine_api::ErrorCode::NotFound),
    },
    // -- config --
    // init/set_module_switch write a real file: dedicated tests use a
    // scratch config path, this shared daemon has none.
    Sample {
        module: ursine_api::ModuleId::Config,
        action: "init",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Skip,
    },
    Sample {
        module: ursine_api::ModuleId::Config,
        action: "get_config",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Ok,
    },
    Sample {
        module: ursine_api::ModuleId::Config,
        action: "set_module_switch",
        pairs: &[("module", "crontab"), ("value", "auto")],
        destructive: false,
        dispatch: Dispatch::Skip,
    },
    // -- capabilities --
    Sample {
        module: ursine_api::ModuleId::Capabilities,
        action: "list",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Ok,
    },
    Sample {
        module: ursine_api::ModuleId::Capabilities,
        action: "refresh",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Ok,
    },
    // -- packages.nix (nix absent from the container → gate) --
    Sample {
        module: ursine_api::ModuleId::PackagesNix,
        action: "list_installed",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesNix,
        action: "search",
        pairs: &[("query", "niri")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesNix,
        action: "install",
        pairs: &[("package", "hello")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesNix,
        action: "remove",
        pairs: &[("package", "hello")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesNix,
        action: "remove_all",
        pairs: &[],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesNix,
        action: "remove_regex",
        pairs: &[("pattern", "^py"), ("dry_run", "true")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesNix,
        action: "upgrade",
        pairs: &[("all", "true")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesNix,
        action: "upgrade_regex",
        pairs: &[("pattern", "^py"), ("dry_run", "true")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesNix,
        action: "generations",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesNix,
        action: "rollback",
        pairs: &[("generation", "1")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesNix,
        action: "wipe_history",
        pairs: &[("dry_run", "true")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesNix,
        action: "diff_closures",
        pairs: &[("generation", "1")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesNix,
        action: "store_gc",
        pairs: &[],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesNix,
        action: "prune_generations",
        pairs: &[("keep", "3"), ("dry_run", "true")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    // -- packages.apk (apk IS present) --
    Sample {
        module: ursine_api::ModuleId::PackagesApk,
        action: "list_apk",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Ok,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesApk,
        action: "search_apk",
        pairs: &[("query", "niri")],
        destructive: false,
        dispatch: Dispatch::Any,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesApk,
        action: "refresh_apk",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Skip,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesApk,
        action: "list_apk_repositories",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Ok,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesApk,
        action: "install_apk",
        pairs: &[("package", "ursine-no-such-pkg-xyz")],
        destructive: false,
        dispatch: Dispatch::Ok,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesApk,
        action: "preview_apk_install",
        pairs: &[("package", "ursine-no-such-pkg-xyz")],
        destructive: false,
        dispatch: Dispatch::Any,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesApk,
        action: "remove_apk",
        pairs: &[("package", "ursine-no-such-pkg-xyz")],
        destructive: true,
        dispatch: Dispatch::Ok,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesApk,
        action: "upgrade_apk",
        pairs: &[],
        destructive: true,
        dispatch: Dispatch::Skip,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesApk,
        action: "list_apk_updates",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Any,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesApk,
        action: "upgrade_apk_package",
        pairs: &[("package", "ursine-no-such-pkg-xyz")],
        destructive: true,
        dispatch: Dispatch::Ok,
    },
    // -- packages.flatpak (absent → gate) --
    Sample {
        module: ursine_api::ModuleId::PackagesFlatpak,
        action: "list_flatpak",
        pairs: &[("system", "true")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesFlatpak,
        action: "list_flatpak_remotes",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesFlatpak,
        action: "search_flatpak",
        pairs: &[("query", "firefox"), ("system", "true")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesFlatpak,
        action: "install_flatpak",
        pairs: &[("app_id", "org.x.Y"), ("system", "true")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesFlatpak,
        action: "remove_flatpak",
        pairs: &[("app_id", "org.x.Y"), ("system", "true")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesFlatpak,
        action: "update_flatpak",
        pairs: &[("system", "true")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesFlatpak,
        action: "update_flatpak_app",
        pairs: &[("app_id", "org.x.Y"), ("system", "true")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::PackagesFlatpak,
        action: "remove_flatpak_orphans",
        pairs: &[("system", "true")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    // -- printers (CUPS absent → gate) --
    Sample {
        module: ursine_api::ModuleId::Printers,
        action: "list_printers",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Printers,
        action: "printer_jobs",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Printers,
        action: "add_printer",
        pairs: &[
            ("name", "p"),
            ("uri", "ipp://x"),
            ("description", "d"),
            ("location", "l"),
        ],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Printers,
        action: "add_printer_manual",
        pairs: &[
            ("name", "p"),
            ("uri", "ipp://x"),
            ("driver", "drv"),
            ("ppd", "/tmp/x.ppd"),
            ("description", "d"),
            ("location", "l"),
            ("options", "[\"media=A4\"]"),
        ],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Printers,
        action: "remove_printer",
        pairs: &[("name", "p")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Printers,
        action: "set_default_printer",
        pairs: &[("name", "p")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Printers,
        action: "set_printer_options",
        pairs: &[("name", "p"), ("options", "[\"Duplex=None\"]")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Printers,
        action: "pause_printer",
        pairs: &[("name", "p"), ("pause", "true")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Printers,
        action: "cancel_printer_jobs",
        pairs: &[],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    // -- scanners (SANE absent → gate) --
    Sample {
        module: ursine_api::ModuleId::Scanners,
        action: "list_scanners",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Scanners,
        action: "get_scanner_config",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Scanners,
        action: "set_scanner_device",
        pairs: &[("name", "d"), ("address", "1.2.3.4"), ("protocol", "escl")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Scanners,
        action: "remove_scanner_device",
        pairs: &[("name", "d")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Scanners,
        action: "set_scanner_discovery",
        pairs: &[("enabled", "true")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    // -- network (NetworkManager absent → gate) --
    Sample {
        module: ursine_api::ModuleId::Network,
        action: "get_network",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Network,
        action: "set_wifi_enabled",
        pairs: &[("enabled", "false")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Network,
        action: "connect_wifi",
        pairs: &[("ssid", "lab"), ("password", "-")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Network,
        action: "connect_network",
        pairs: &[("device", "wlan0")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Network,
        action: "disconnect_network",
        pairs: &[("device", "wlan0")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    // -- bluetooth (BlueZ absent → gate) --
    Sample {
        module: ursine_api::ModuleId::Bluetooth,
        action: "get_bluetooth",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Bluetooth,
        action: "set_bluetooth_power",
        pairs: &[("on", "true")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Bluetooth,
        action: "scan_bluetooth",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Bluetooth,
        action: "bluetooth_action",
        pairs: &[("address", "AA:BB:CC:DD:EE:FF"), ("op", "pair")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    // -- disks (UDisks2 absent → gate) --
    Sample {
        module: ursine_api::ModuleId::Disks,
        action: "list_disks",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Disks,
        action: "mount_disk",
        pairs: &[("device", "/dev/sda1")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Disks,
        action: "unmount_disk",
        pairs: &[("device", "/dev/sda1")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    // -- services (no capability; no init tooling in the container) --
    Sample {
        module: ursine_api::ModuleId::Services,
        action: "list_services",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Any,
    },
    Sample {
        module: ursine_api::ModuleId::Services,
        action: "service_action",
        pairs: &[("name", "not-a-service"), ("op", "start")],
        destructive: false,
        dispatch: Dispatch::Any,
    },
    // -- firewall (firewalld absent → gate) --
    Sample {
        module: ursine_api::ModuleId::Firewall,
        action: "get_firewall",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Firewall,
        action: "set_default_firewall_zone",
        pairs: &[("zone", "public")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Firewall,
        action: "add_firewall_service",
        pairs: &[("zone", "public"), ("service", "ssh")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Firewall,
        action: "remove_firewall_service",
        pairs: &[("zone", "public"), ("service", "ssh")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Firewall,
        action: "add_firewall_port",
        pairs: &[("zone", "public"), ("port", "8080")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Firewall,
        action: "remove_firewall_port",
        pairs: &[("zone", "public"), ("port", "8080")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Firewall,
        action: "reload_firewall",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    // -- snapshots (btrbk absent → gate) --
    Sample {
        module: ursine_api::ModuleId::Snapshots,
        action: "list_snapshots",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Snapshots,
        action: "stats",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Snapshots,
        action: "create_snapshot",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Snapshots,
        action: "restore_snapshot",
        pairs: &[("snapshot", "s"), ("target", "t"), ("dry_run", "true")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Snapshots,
        action: "get_btrbk_config",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Snapshots,
        action: "init_btrbk_config",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Snapshots,
        action: "set_btrbk_config",
        pairs: &[("settings", "{}")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Snapshots,
        action: "delete_snapshot",
        pairs: &[("snapshot", "s")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Snapshots,
        action: "delete_snapshots",
        pairs: &[("sources", "[]")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    // -- users (accountsservice absent → gate) --
    Sample {
        module: ursine_api::ModuleId::Users,
        action: "list_users",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Users,
        action: "create_user",
        pairs: &[("name", "t"), ("fullname", "Test"), ("admin", "false")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Users,
        action: "delete_user",
        pairs: &[("uid", "1000"), ("remove_files", "false")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Users,
        action: "set_password",
        pairs: &[("uid", "1000"), ("new_password", "-")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Users,
        action: "list_groups",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Users,
        action: "set_groups",
        pairs: &[("uid", "1000"), ("groups", "[\"wheel\"]")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    // -- niri (absent → gate) --
    Sample {
        module: ursine_api::ModuleId::Niri,
        action: "get_niri_config",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Niri,
        action: "validate_niri_config",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Niri,
        action: "set_niri_section",
        pairs: &[("section", "layout"), ("value", "{}")],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    // -- theming (no shell → gate) --
    Sample {
        module: ursine_api::ModuleId::Theming,
        action: "theme_detect",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Theming,
        action: "theme_get",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Gate,
    },
    Sample {
        module: ursine_api::ModuleId::Theming,
        action: "theme_apply",
        pairs: &[("show_minimize", "true")],
        destructive: true,
        dispatch: Dispatch::Gate,
    },
    // -- logs (always available) --
    Sample {
        module: ursine_api::ModuleId::Logs,
        action: "log_list",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Ok,
    },
    Sample {
        module: ursine_api::ModuleId::Logs,
        action: "log_read",
        pairs: &[
            ("file", LOG_FIXTURE),
            ("lines", "10"),
            ("cursor", "{\"mode\":\"end\"}"),
        ],
        destructive: false,
        dispatch: Dispatch::Ok,
    },
    Sample {
        module: ursine_api::ModuleId::Logs,
        action: "log_search",
        pairs: &[("file", LOG_FIXTURE), ("query", "Needle"), ("limit", "10")],
        destructive: false,
        dispatch: Dispatch::Ok,
    },
    // -- crontab (present; set round-trips in the E2E suite) --
    Sample {
        module: ursine_api::ModuleId::Crontab,
        action: "crontab_get",
        pairs: &[],
        destructive: false,
        dispatch: Dispatch::Ok,
    },
    Sample {
        module: ursine_api::ModuleId::Crontab,
        action: "crontab_set",
        pairs: &[("content", "# sample")],
        destructive: true,
        dispatch: Dispatch::Skip,
    },
];

/// Fixture log created by the dispatch tests before `log_read`/`log_search`.
pub const LOG_FIXTURE: &str = "ursine-ctl-dispatch.log";

/// Samples of one module (one dispatch test per module).
pub fn samples_for(module: ursine_api::ModuleId) -> impl Iterator<Item = &'static Sample> {
    SAMPLES.iter().filter(move |s| s.module == module)
}

/// Actions that delete, roll back or overwrite state: `--yes` required.
///
/// Dry-run/preview variants are not destructive. When a new destructive
/// action lands in the API it must be added here (review checklist).
pub fn is_destructive(action: &Action) -> bool {
    use Action::{
        CancelPrinterJobs, CrontabSet, DeleteSnapshot, DeleteSnapshots, DeleteUser,
        PruneGenerations, Remove, RemoveAll, RemoveApk, RemoveFirewallPort, RemoveFirewallService,
        RemoveFlatpak, RemoveFlatpakOrphans, RemovePrinter, RemoveRegex, RemoveScannerDevice,
        RestoreSnapshot, Rollback, SetPassword, StoreGc, ThemeApply, UnmountDisk, UpdateFlatpak,
        UpdateFlatpakApp, UpgradeApk, UpgradeApkPackage, UpgradeRegex, WipeHistory,
    };
    matches!(
        action,
        Remove { .. }
            | RemoveAll
            | RemoveRegex { dry_run: false, .. }
            | UpgradeRegex { dry_run: false, .. }
            | WipeHistory { dry_run: false, .. }
            | Rollback { .. }
            | StoreGc
            | PruneGenerations { dry_run: false, .. }
            | RemoveApk { .. }
            | UpgradeApk
            | UpgradeApkPackage { .. }
            | RemoveFlatpak { .. }
            | UpdateFlatpak { .. }
            | UpdateFlatpakApp { .. }
            | RemoveFlatpakOrphans { .. }
            | RestoreSnapshot { dry_run: false, .. }
            | DeleteSnapshot { .. }
            | DeleteSnapshots { .. }
            | DeleteUser { .. }
            | SetPassword { .. }
            | CrontabSet { .. }
            | ThemeApply { .. }
            | RemovePrinter { .. }
            | CancelPrinterJobs { .. }
            | RemoveScannerDevice { .. }
            | RemoveFirewallService { .. }
            | RemoveFirewallPort { .. }
            | UnmountDisk { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn parses_module_action_and_pairs() {
        let Cmd::Run(opts) =
            parse(&args(&["--json", "apk", "install_apk", "package=niri"])).unwrap()
        else {
            panic!("want run");
        };
        assert!(opts.json);
        assert_eq!(opts.module, "packages_apk");
        assert_eq!(opts.action, "install_apk");
        let action = build_action(&opts.action, &opts.pairs).unwrap();
        assert_eq!(
            action,
            Action::InstallApk {
                package: "niri".into()
            }
        );
    }

    #[test]
    fn aliases_resolve() {
        let Cmd::Run(opts) = parse(&args(&["nix", "search", "query=x"])).unwrap() else {
            panic!("want run");
        };
        assert_eq!(opts.module, "packages_nix");
        assert!(parse(&args(&["nope", "list"])).is_err());
    }

    #[test]
    fn json_scalars_and_strings() {
        let a = build_action("wipe_history", &[("dry_run".into(), "true".into())]).unwrap();
        assert_eq!(
            a,
            Action::WipeHistory {
                older_than: None,
                dry_run: true
            }
        );
        let a = build_action(
            "prune_generations",
            &[
                ("keep".into(), "3".into()),
                ("dry_run".into(), "false".into()),
            ],
        )
        .unwrap();
        assert_eq!(
            a,
            Action::PruneGenerations {
                keep: 3,
                dry_run: false,
                expected_current: None,
                expected_delete: None,
            }
        );
        // Non-JSON stays a string.
        let a = build_action("search", &[("query".into(), "niri".into())]).unwrap();
        assert_eq!(
            a,
            Action::Search {
                query: "niri".into()
            }
        );
    }

    #[test]
    fn secrets_refuse_argv() {
        let err = build_action(
            "set_password",
            &[
                ("uid".into(), "1000".into()),
                ("new_password".into(), "hunter2".into()),
            ],
        )
        .unwrap_err();
        assert!(err.contains("command line"), "{err}");
    }

    #[test]
    fn unknown_action_is_rejected() {
        assert!(build_action("frobnicate", &[]).is_err());
    }

    #[test]
    fn destructive_classification() {
        assert!(is_destructive(&Action::RemoveAll));
        assert!(is_destructive(&Action::CrontabSet {
            content: String::new()
        }));
        assert!(!is_destructive(&Action::WipeHistory {
            older_than: None,
            dry_run: true
        }));
        assert!(!is_destructive(&Action::ListApk));
    }

    #[test]
    fn equals_forms_and_option_edge_cases() {
        let Cmd::Run(opts) = parse(&args(&[
            "--as-user=bob",
            "--config=/tmp/x.toml",
            "nix",
            "generations",
        ]))
        .unwrap() else {
            panic!("want run");
        };
        assert_eq!(opts.as_user.as_deref(), Some("bob"));
        assert_eq!(opts.config, PathBuf::from("/tmp/x.toml"));
        assert!(parse(&args(&["--nope"])).is_err());
        assert!(parse(&args(&["--as-user"])).is_err());
        assert!(parse(&args(&["--config"])).is_err());
        assert!(parse(&args(&[])).is_err());
        assert!(parse(&args(&["only-module"])).is_ok(), "module help");
        assert!(parse(&args(&["a", "b", "c"])).is_err());
    }

    #[test]
    fn duplicate_and_empty_keys_are_rejected() {
        assert!(build_action(
            "search",
            &[("query".into(), "a".into()), ("query".into(), "b".into())]
        )
        .is_err());
        assert!(build_action("search", &[(String::new(), "x".into())]).is_err());
    }

    #[test]
    fn at_file_expands_to_string_content() {
        let dir = manifest_artifacts("cli-at");
        let file = dir.join("cron.txt");
        std::fs::write(&file, "* * * * * true\n").unwrap();
        let a = build_action(
            "crontab_set",
            &[("content".into(), format!("@{}", file.display()))],
        )
        .unwrap();
        assert_eq!(
            a,
            Action::CrontabSet {
                content: "* * * * * true\n".into()
            }
        );
    }

    #[test]
    fn json_object_values_reach_typed_actions() {
        // LogCursor is internally tagged: {"mode":"end"} / {"mode":"before",...}.
        let a = build_action(
            "log_read",
            &[
                ("file".into(), "pmesg".into()),
                ("cursor".into(), "{\"mode\":\"end\"}".into()),
            ],
        )
        .unwrap();
        assert!(matches!(a, Action::LogRead { .. }));
        let a = build_action(
            "log_read",
            &[
                ("file".into(), "pmesg".into()),
                (
                    "cursor".into(),
                    "{\"mode\":\"before\",\"offset\":123}".into(),
                ),
            ],
        )
        .unwrap();
        assert_eq!(
            a,
            Action::LogRead {
                file: "pmesg".into(),
                lines: ursine_api::logs::default_log_lines(),
                cursor: ursine_api::LogCursor::Before { offset: 123 },
            }
        );
    }

    #[test]
    fn tri_state_and_bool_scalars() {
        let a = build_action(
            "set_module_switch",
            &[
                ("module".into(), "packages_nix".into()),
                ("value".into(), "auto".into()),
            ],
        )
        .unwrap();
        assert_eq!(
            a,
            Action::SetModuleSwitch {
                module: ursine_api::ModuleId::PackagesNix,
                value: ursine_api::TriState::Auto,
            }
        );
        let a = build_action(
            "set_scanner_discovery",
            &[("enabled".into(), "false".into())],
        )
        .unwrap();
        assert_eq!(a, Action::SetScannerDiscovery { enabled: false });
    }

    /// Every wire action name (mirror of `action_name` in ursine-core):
    /// the drift guards below fail when the API grows an action the CLI
    /// help or tag parser does not know about.
    const ALL_ACTIONS: &[&str] = &[
        "add_firewall_port",
        "add_firewall_service",
        "add_printer",
        "add_printer_manual",
        "bluetooth_action",
        "cancel_printer_jobs",
        "connect_network",
        "connect_wifi",
        "create_snapshot",
        "create_user",
        "crontab_get",
        "crontab_set",
        "delete_snapshot",
        "delete_snapshots",
        "delete_user",
        "diff_closures",
        "disconnect_network",
        "generations",
        "get_bluetooth",
        "get_btrbk_config",
        "get_config",
        "get_firewall",
        "get_network",
        "get_niri_config",
        "get_scanner_config",
        "init",
        "init_btrbk_config",
        "install",
        "install_apk",
        "install_flatpak",
        "job_output",
        "list",
        "list_apk",
        "list_apk_repositories",
        "list_apk_updates",
        "list_disks",
        "list_flatpak",
        "list_flatpak_remotes",
        "list_groups",
        "list_installed",
        "list_printers",
        "list_scanners",
        "list_services",
        "list_snapshots",
        "list_users",
        "log_list",
        "log_read",
        "log_search",
        "mount_disk",
        "pause_printer",
        "preview_apk_install",
        "printer_jobs",
        "prune_generations",
        "refresh",
        "refresh_apk",
        "reload_firewall",
        "remove",
        "remove_all",
        "remove_apk",
        "remove_firewall_port",
        "remove_firewall_service",
        "remove_flatpak",
        "remove_flatpak_orphans",
        "remove_printer",
        "remove_regex",
        "remove_scanner_device",
        "restore_snapshot",
        "rollback",
        "scan_bluetooth",
        "search",
        "search_apk",
        "search_flatpak",
        "service_action",
        "set_bluetooth_power",
        "set_btrbk_config",
        "set_default_firewall_zone",
        "set_default_printer",
        "set_groups",
        "set_module_switch",
        "set_niri_section",
        "set_password",
        "set_printer_options",
        "set_scanner_device",
        "set_scanner_discovery",
        "set_wifi_enabled",
        "stats",
        "status",
        "store_gc",
        "theme_apply",
        "theme_detect",
        "theme_get",
        "unmount_disk",
        "update_flatpak",
        "update_flatpak_app",
        "upgrade",
        "upgrade_apk",
        "upgrade_apk_package",
        "upgrade_regex",
        "validate_niri_config",
        "wipe_history",
    ];

    #[test]
    fn every_wire_action_is_a_valid_serde_tag() {
        // The CLI parses actions through the wire serde tag: every name in
        // the registry must resolve to a known variant (missing fields are
        // fine — an unknown variant is not).
        for name in ALL_ACTIONS {
            match serde_json::from_value::<Action>(serde_json::json!({
                "action": name
            })) {
                // All-optional payload: parses from the tag alone.
                Ok(_) => {}
                // Required fields: the ONLY acceptable failure is a missing
                // field — an unknown variant would mean the CLI cannot
                // reach that action at all.
                Err(err) => {
                    let msg = err.to_string();
                    assert!(
                        !msg.contains("unknown variant"),
                        "action tag {name:?} is not reachable from the CLI: {msg}"
                    );
                    assert!(
                        msg.contains("missing field"),
                        "action tag {name:?} failed for an unexpected reason: {msg}"
                    );
                }
            }
        }
        let err = serde_json::from_value::<Action>(serde_json::json!({
            "action": "frobnicate"
        }))
        .unwrap_err();
        assert!(err.to_string().contains("unknown variant"));
    }

    #[test]
    fn usage_documents_every_wire_action() {
        let tokens: Vec<&str> = USAGE
            .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
            .filter(|t| !t.is_empty())
            .collect();
        for name in ALL_ACTIONS {
            assert!(
                tokens.contains(name),
                "action {name:?} missing from ursinectl --help"
            );
        }
    }

    #[test]
    fn unknown_field_is_rejected_not_silently_dropped() {
        // dry_run is required for wipe_history: provide it plus a typo.
        let err = build_action(
            "wipe_history",
            &[
                ("dry_run".into(), "true".into()),
                ("dry_ru".into(), "true".into()),
            ],
        )
        .unwrap_err();
        assert!(err.contains("unknown field"), "{err}");
        let err = build_action(
            "search",
            &[("query".into(), "x".into()), ("queryy".into(), "x".into())],
        )
        .unwrap_err();
        assert!(err.contains("unknown field"), "{err}");
        // Known fields still work, including Option + explicit null skip.
        assert!(build_action("rollback", &[("generation".into(), "42".into())]).is_ok());
    }

    #[test]
    fn tri_state_rejects_garbage() {
        let err = build_action(
            "set_module_switch",
            &[
                ("module".into(), "logs".into()),
                ("value".into(), "banana".into()),
            ],
        )
        .unwrap_err();
        assert!(err.contains("invalid tri-state"), "{err}");
    }

    #[test]
    fn destructive_classification_is_exhaustive() {
        use Action::{
            CancelPrinterJobs, CrontabSet, DeleteSnapshot, DeleteSnapshots, DeleteUser, ListApk,
            PruneGenerations, Remove, RemoveAll, RemoveApk, RemoveFirewallPort,
            RemoveFirewallService, RemoveFlatpak, RemoveFlatpakOrphans, RemovePrinter, RemoveRegex,
            RemoveScannerDevice, RestoreSnapshot, Rollback, Search, SetPassword, StoreGc,
            ThemeApply, UnmountDisk, UpdateFlatpak, UpdateFlatpakApp, Upgrade, UpgradeApk,
            UpgradeApkPackage, UpgradeRegex, WipeHistory,
        };
        let destructive = [
            Remove {
                package: "x".into(),
            },
            RemoveAll,
            RemoveRegex {
                pattern: "x".into(),
                dry_run: false,
            },
            UpgradeRegex {
                pattern: "x".into(),
                dry_run: false,
            },
            WipeHistory {
                older_than: None,
                dry_run: false,
            },
            Rollback { generation: None },
            StoreGc,
            PruneGenerations {
                keep: 1,
                dry_run: false,
                expected_current: None,
                expected_delete: None,
            },
            RemoveApk {
                package: "x".into(),
            },
            UpgradeApk,
            UpgradeApkPackage {
                package: "x".into(),
            },
            RemoveFlatpak {
                app_id: "x".into(),
                system: false,
            },
            UpdateFlatpak { system: false },
            UpdateFlatpakApp {
                app_id: "x".into(),
                system: false,
            },
            RemoveFlatpakOrphans { system: false },
            RestoreSnapshot {
                snapshot: "s".into(),
                target: "t".into(),
                dry_run: false,
            },
            DeleteSnapshot {
                snapshot: "s".into(),
            },
            DeleteSnapshots { sources: vec![] },
            DeleteUser {
                uid: 1000,
                remove_files: false,
            },
            SetPassword {
                uid: 1000,
                new_password: String::new(),
            },
            CrontabSet {
                content: String::new(),
            },
            ThemeApply {
                show_minimize: true,
            },
            RemovePrinter { name: "p".into() },
            CancelPrinterJobs { printer: None },
            RemoveScannerDevice { name: "d".into() },
            RemoveFirewallService {
                zone: "z".into(),
                service: "s".into(),
            },
            RemoveFirewallPort {
                zone: "z".into(),
                port: "53".into(),
            },
            UnmountDisk {
                device: "/dev/sda1".into(),
            },
        ];
        for action in &destructive {
            assert!(is_destructive(action), "must require --yes: {action:?}");
        }
        // Previews/dry-runs and reads never gate on --yes.
        let benign = [
            WipeHistory {
                older_than: None,
                dry_run: true,
            },
            RemoveRegex {
                pattern: "x".into(),
                dry_run: true,
            },
            UpgradeRegex {
                pattern: "x".into(),
                dry_run: true,
            },
            PruneGenerations {
                keep: 1,
                dry_run: true,
                expected_current: None,
                expected_delete: None,
            },
            RestoreSnapshot {
                snapshot: "s".into(),
                target: "t".into(),
                dry_run: true,
            },
            Upgrade {
                package: None,
                all: true,
            },
            ListApk,
            Search { query: "x".into() },
        ];
        for action in &benign {
            assert!(!is_destructive(action), "must NOT gate: {action:?}");
        }
    }

    #[test]
    fn number_in_string_field_is_coerced_not_rejected() {
        // Users type port=8080; JSON yields a number, the field is String.
        let a = build_action(
            "add_firewall_port",
            &[
                ("zone".into(), "public".into()),
                ("port".into(), "8080".into()),
            ],
        )
        .unwrap();
        assert_eq!(
            a,
            Action::AddFirewallPort {
                zone: "public".into(),
                port: "8080".into()
            }
        );
        // A real type error (non-numeric garbage for a number field) still fails.
        assert!(build_action(
            "prune_generations",
            &[
                ("keep".to_string(), "abc".to_string()),
                ("dry_run".to_string(), "false".to_string()),
            ]
        )
        .is_err());
    }

    #[test]
    fn every_wire_action_has_exactly_one_sample() {
        let mut sample_names: Vec<&str> = SAMPLES.iter().map(|s| s.action).collect();
        sample_names.sort_unstable();
        let mut all: Vec<&str> = ALL_ACTIONS.to_vec();
        all.sort_unstable();
        assert_eq!(
            sample_names, all,
            "SAMPLES must cover every wire action exactly once"
        );
        // No duplicate module/action pair.
        let mut pairs: Vec<(&str, &str)> = SAMPLES
            .iter()
            .map(|s| (s.module.as_str(), s.action))
            .collect();
        pairs.sort_unstable();
        let before = pairs.len();
        pairs.dedup();
        assert_eq!(pairs.len(), before, "duplicate (module, action) sample");
    }

    #[test]
    fn every_sample_parses_and_classifies_destructive() {
        for s in SAMPLES {
            let pairs: Vec<(String, String)> = s
                .pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            let action = build_action_with(s.action, &pairs, || Ok("x".to_string()))
                .unwrap_or_else(|e| panic!("{}: {e}", s.action));
            // The tag the CLI parsed must be the tag core audits.
            assert_eq!(
                ursine_core::modules::action_name(&action),
                s.action,
                "{}",
                s.action
            );
            assert_eq!(
                is_destructive(&action),
                s.destructive,
                "destructive expectation for {}",
                s.action
            );
        }
    }

    /// Scratch dir under `artifacts/` (host temp is off-limits by policy).
    fn manifest_artifacts(tag: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../artifacts/test-ursinectl")
            .join(tag);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn help_and_version() {
        assert!(matches!(
            parse(&args(&["--help"])).unwrap(),
            Cmd::Help(None)
        ));
        assert!(matches!(
            parse(&args(&["--help", "nix"])).unwrap(),
            Cmd::Help(Some(m)) if m == "nix"
        ));
        assert!(matches!(
            parse(&args(&["--version"])).unwrap(),
            Cmd::Version
        ));
    }
}
