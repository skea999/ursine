//! End-to-end: spawn the real `ursinectl` binary. Runs as root in the CI
//! container (the same condition the CLI enforces).

// E2E tests spawn processes on purpose (the binary under test, `su` for the
// non-root gate); the disallowed-methods rule targets production code.
#![allow(clippy::disallowed_methods)]
#![allow(clippy::unwrap_used)] // tests

use std::io::Write as _;
use std::process::{Command, Output, Stdio};

/// Root's crontab is a single global resource: tests that SET it must
/// serialize or they last-wins over each other's payloads.
static CRONTAB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_crontab() -> std::sync::MutexGuard<'static, ()> {
    CRONTAB_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ursinectl"))
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().unwrap()
}

fn run_stdin(args: &[&str], stdin: &str) -> Output {
    let mut child = bin()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn version_and_help_succeed_without_side_effects() {
    let out = run(&["--version"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains(env!("CARGO_PKG_VERSION")));

    let out = run(&["--help"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("USAGE"));
}

#[test]
fn no_args_is_usage_error() {
    let out = run(&[]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("USAGE"));
}

#[test]
fn status_prints_version_json() {
    let out = run(&["--json", "system", "status"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(v.get("version").is_some(), "{v}");
    // Wire contract every frontend depends on.
    assert_eq!(v["protocol_version"], 3, "{v}");
}

#[test]
fn unknown_module_and_action_fail_fast() {
    let out = run(&["nope", "list"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let out = run(&["nix", "frobnicate"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
}

#[test]
fn destructive_requires_yes() {
    let out = run(&["crontab", "crontab_set", "content="]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("--yes"), "{}", stderr(&out));
}

#[test]
fn secret_on_argv_is_refused_before_anything_else() {
    let out = run(&[
        "users",
        "set_password",
        "uid=1000",
        "new_password=hunter2",
        "--yes",
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("command line"), "{}", stderr(&out));
}

#[test]
fn crontab_round_trip_as_root() {
    let _serial = lock_crontab();
    // Set a known crontab, read it back, then clear it.
    let payload = "# ursinectl-e2e\n* * * * * /bin/true\n";
    let out = run_stdin(&["crontab", "crontab_set", "content=-", "--yes"], payload);
    assert!(out.status.success(), "{}", stderr(&out));

    let out = run(&["--json", "crontab", "crontab_get"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["crontab"].as_str().unwrap(), payload, "{v}");

    let out = run_stdin(&["crontab", "crontab_set", "content=-", "--yes"], "");
    assert!(out.status.success(), "{}", stderr(&out));
    let out = run(&["--json", "crontab", "crontab_get"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(v["crontab"].is_null(), "cleared crontab must be null: {v}");
}

#[test]
fn module_help_lists_actions() {
    let out = run(&["nix"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("list_installed"));
}

#[cfg(unix)]
#[test]
fn non_root_is_refused() {
    // Drop to `nobody` (uid 65534) via su: the root gate must fire before
    // any side effect.
    let bin_path = env!("CARGO_BIN_EXE_ursinectl");
    let out = Command::new("su")
        .args([
            "-s",
            "/bin/sh",
            "nobody",
            "-c",
            &format!("{bin_path} system status"),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "stderr: {}", stderr(&out));
    assert!(
        stderr(&out).contains("must run as root"),
        "{}",
        stderr(&out)
    );
}

// ---- deeper E2E coverage ----

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../artifacts/test-ursinectl")
        .join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn api_error_exits_1_and_prints_fix() {
    // Nix is not installed in the CI container → capability gate fires:
    // an API error is exit 1 (usage errors are exit 2).
    let out = run(&["nix", "search", "query=niri"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("error:"), "{}", stderr(&out));
    assert!(stderr(&out).contains("fix:"), "{}", stderr(&out));
    assert!(stdout(&out).is_empty(), "errors belong on stderr");
}

#[test]
fn json_flag_is_one_compact_line_pretty_is_multiline() {
    let compact = run(&["--json", "system", "status"]);
    assert!(compact.status.success(), "{}", stderr(&compact));
    let text = stdout(&compact);
    assert!(!text.trim_end().contains('\n'), "compact: {text:?}");
    serde_json::from_str::<serde_json::Value>(text.trim()).unwrap();

    let pretty = run(&["system", "status"]);
    assert!(pretty.status.success(), "{}", stderr(&pretty));
    assert!(stdout(&pretty).contains("\n  "), "pretty must be indented");
}

#[test]
fn module_switch_off_via_config_file_blocks_module() {
    let dir = scratch("e2e-mod-off");
    let cfg = dir.join("config.toml");
    std::fs::write(&cfg, "[modules]\ncrontab = false\n").unwrap();
    let cfg_str = cfg.to_str().unwrap();
    let out = run(&["--config", cfg_str, "crontab", "crontab_get"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("module disabled by configuration"),
        "{}",
        stderr(&out)
    );
    // …while the same command with the default config still works.
    let out = run(&["crontab", "crontab_get"]);
    assert!(
        out.status.success(),
        "default config must not carry the switch: {}",
        stderr(&out)
    );
}

#[test]
fn config_init_get_roundtrip_e2e() {
    let dir = scratch("e2e-init");
    let cfg = dir.join("config.toml");
    let cfg_str = cfg.to_str().unwrap();

    let out = run(&["--config", cfg_str, "config", "init"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(cfg.exists());
    let body = std::fs::read_to_string(&cfg).unwrap();
    assert!(body.contains("[modules]"), "{body}");

    // Second init → Conflict → exit 1, not a silent overwrite.
    let out = run(&["--config", cfg_str, "config", "init"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));

    let out = run(&["--json", "--config", cfg_str, "config", "get_config"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(v["exists"], true, "{v}");

    // Persist a switch and read it back through a fresh call.
    // Full cycle: Off blocks the live gate, Auto unblocks it, and the
    // switch persists in the file for the next invocation.
    let out = run(&[
        "--config",
        cfg_str,
        "config",
        "set_module_switch",
        "module=crontab",
        "value=false",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let out = run(&["--json", "--config", cfg_str, "config", "get_config"]);
    let v: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(v["modules"]["crontab"], false, "{v}");
    let out = run(&["--config", cfg_str, "crontab", "crontab_get"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("module disabled by configuration"),
        "{}",
        stderr(&out)
    );

    let out = run(&[
        "--config",
        cfg_str,
        "config",
        "set_module_switch",
        "module=crontab",
        "value=auto",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let out = run(&["--json", "--config", cfg_str, "config", "get_config"]);
    let v: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(v["modules"]["crontab"], "auto", "{v}");
    let out = run(&["--config", cfg_str, "crontab", "crontab_get"]);
    assert!(out.status.success(), "Auto must unblock: {}", stderr(&out));
}

#[test]
fn as_user_foreign_target_is_rejected_e2e() {
    let out = run(&["crontab", "crontab_get", "--as-user", "definitely-not-root"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("own account"), "{}", stderr(&out));
}

#[test]
fn as_user_outside_user_scoped_module_is_usage_error() {
    // as_user is validated by dispatch (API error, exit 1) — not by the CLI.
    let out = run(&["logs", "log_list", "--as-user", "bob"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("as_user is only valid"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn capabilities_list_is_reachable_e2e() {
    let out = run(&["--json", "caps", "list"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    let arr = v.as_array().expect("caps list is an array");
    assert!(arr.iter().any(|c| c["capability"] == "crontab"), "{v}");
    assert!(arr
        .iter()
        .any(|c| c["capability"] == "btrbk" && c["available"] == false));
}

#[test]
fn logs_search_and_read_e2e() {
    let name = format!("ursine-e2e-{}.log", std::process::id());
    let path = std::path::Path::new("/var/log").join(&name);
    std::fs::write(&path, "alpha\nbeta gamma\ndelta\n").unwrap();

    let out = run(&[
        "--json",
        "logs",
        "log_read",
        &format!("file={name}"),
        "lines=10",
        "cursor={\"mode\":\"end\"}",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert!(v["content"].as_str().unwrap().contains("beta gamma"), "{v}");

    let out = run(&[
        "--json",
        "logs",
        "log_search",
        &format!("file={name}"),
        "query=GAMMA",
        "limit=5",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(v["hits"].as_array().unwrap().len(), 1, "{v}");

    // Escape is an API error (exit 1), never a read.
    let out = run(&[
        "logs",
        "log_read",
        "file=../etc/shadow",
        "cursor={\"mode\":\"end\"}",
    ]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));

    // A malformed value for a typed field → usage error (exit 2).
    let out = run(&["logs", "log_read", "file=x", "cursor=banana"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("invalid arguments"),
        "{}",
        stderr(&out)
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn non_root_can_read_help_and_version() {
    // The gate protects side effects only: help/version are pure output.
    let bin_path = env!("CARGO_BIN_EXE_ursinectl");
    for flag in ["--help", "--version"] {
        let out = Command::new("su")
            .args([
                "-s",
                "/bin/sh",
                "nobody",
                "-c",
                &format!("{bin_path} {flag}"),
            ])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{flag} as nobody: {}",
            stderr(&out)
        );
    }
}

#[test]
fn crontab_set_without_stdin_still_needs_yes_and_valid_fields() {
    // Unknown field → usage exit 2 (fail before any side effect).
    let out = run(&["crontab", "crontab_set", "content=x", "nonsense=1", "--yes"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("unknown field"), "{}", stderr(&out));
}

// ---- deep E2E: the full job path, payload boundaries, unsafe configs ----

#[test]
fn apk_job_mutation_streams_log_and_exits_with_job_code() {
    // The whole production path in one command: parse -> --yes gate ->
    // dispatch -> real job spawned -> log streamed to stdout -> CLI exit
    // code equals the job's exit code. `apk del` of a package that does
    // not exist fails fast against the local db (no network).
    let out = run(&[
        "apk",
        "remove_apk",
        "package=ursine-no-such-pkg-xyz",
        "--yes",
    ]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        !stdout(&out).is_empty(),
        "the job log must reach stdout (terminal-text contract)"
    );
}

#[test]
fn crontab_payload_boundaries_are_enforced_by_the_daemon() {
    // Over the 256 KiB cap: rejected at dispatch (exit 1), never staged.
    let big = "x".repeat(300 * 1024);
    let out = run_stdin(&["crontab", "crontab_set", "content=-", "--yes"], &big);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("too large"), "{}", stderr(&out));

    // NUL byte inside the payload: rejected, never handed to crontab.
    let out = run_stdin(
        &["crontab", "crontab_set", "content=-", "--yes"],
        "line one\0line two\n",
    );
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("NUL"), "{}", stderr(&out));
}

#[test]
fn crontab_set_from_at_file_roundtrip() {
    let _serial = lock_crontab();
    let dir = scratch("e2e-crontab-file");
    let cron = dir.join("payload.cron");
    let payload = "# from-file\n* * * * * /bin/true\n";
    std::fs::write(&cron, payload).unwrap();

    let out = run(&[
        "crontab",
        "crontab_set",
        &format!("content=@{}", cron.display()),
        "--yes",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let out = run(&["--json", "crontab", "crontab_get"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(v["crontab"].as_str().unwrap(), payload, "{v}");

    // Clear again so parallel tests never see a dirty root crontab.
    let out = run_stdin(&["crontab", "crontab_set", "content=-", "--yes"], "");
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn unsafe_config_files_are_refused_before_any_call() {
    use std::os::unix::fs::PermissionsExt as _;
    // World-writable config: the privilege-boundary guard rejects it
    // (exit 1 from daemon construction, before dispatch).
    let dir = scratch("e2e-cfg-0666");
    let cfg = dir.join("config.toml");
    std::fs::write(&cfg, "[modules]\ncrontab = false\n").unwrap();
    std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o666)).unwrap();
    let out = run(&["--config", cfg.to_str().unwrap(), "crontab", "crontab_get"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("writable"), "{}", stderr(&out));

    // Symlinked config: refused as "not a regular file".
    let dir2 = scratch("e2e-cfg-symlink");
    let target = dir2.join("real.toml");
    std::fs::write(&target, "").unwrap();
    let link = dir2.join("link.toml");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let out = run(&["--config", link.to_str().unwrap(), "crontab", "crontab_get"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("not a regular file"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn non_root_mutation_is_refused_at_the_gate_before_anything() {
    let bin_path = env!("CARGO_BIN_EXE_ursinectl");
    let out = Command::new("su")
        .args([
            "-s",
            "/bin/sh",
            "nobody",
            "-c",
            &format!("{bin_path} crontab crontab_set content=x --yes"),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "stderr: {}", stderr(&out));
    assert!(
        stderr(&out).contains("must run as root"),
        "{}",
        stderr(&out)
    );
}
