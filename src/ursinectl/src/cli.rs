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
  ursinectl status
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

/// Build a wire `Action` from the action tag plus `key=value` pairs.
pub fn build_action(action: &str, pairs: &[(String, String)]) -> Result<Action, String> {
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
            "password" | "new_password" => read_secret(raw)?,
            _ => expand_value(raw)?,
        };
        map.insert(key.clone(), value);
    }
    serde_json::from_value(serde_json::Value::Object(map))
        .map_err(|e| format!("invalid arguments for action {action:?}: {e}"))
}

/// `key=-` reads one line from stdin; anything else is refused for secrets.
fn read_secret(raw: &str) -> Result<serde_json::Value, String> {
    if raw != "-" {
        return Err(format!(
            "secrets must not appear on the command line; use {raw}=- and type or pipe the value"
        ));
    }
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("read secret from stdin: {e}"))?;
    Ok(serde_json::Value::String(
        line.trim_end_matches(['\n', '\r']).to_string(),
    ))
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
