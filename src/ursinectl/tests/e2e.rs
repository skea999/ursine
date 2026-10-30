//! End-to-end: spawn the real `ursinectl` binary. Runs as root in the CI
//! container (the same condition the CLI enforces).

// E2E tests spawn processes on purpose (the binary under test, `su` for the
// non-root gate); the disallowed-methods rule targets production code.
#![allow(clippy::disallowed_methods)]
#![allow(clippy::unwrap_used)] // tests

use std::io::Write as _;
use std::process::{Command, Output, Stdio};

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
