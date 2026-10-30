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
