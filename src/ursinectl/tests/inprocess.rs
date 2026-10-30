//! In-process integration: real daemon construction + real dispatch.
//! Runs as root inside the CI container (the same condition the CLI enforces).

#![allow(clippy::unwrap_used)] // tests
#![allow(clippy::panic)] // test failure reporting

use ursine_api::{Action, ModuleId, ResponseBody};
use ursine_core::auth::PeerInfo;
use ursine_core::dispatch::Daemon;

fn root_peer() -> PeerInfo {
    PeerInfo {
        uid: 0,
        user: "root".to_string(),
    }
}

async fn call(
    daemon: &Daemon,
    token: &ursine_api::SessionToken,
    module: ModuleId,
    action: Action,
) -> Result<serde_json::Value, ursine_api::ApiError> {
    ursinectl::call(daemon, token, &root_peer(), module, action, None).await
}

/// Temp config path that does not exist → daemon defaults (audit under the
/// real `/var/log/ursine`, job logs under `/run/ursinectl/jobs`; both are
/// root-owned in the container).
fn build() -> (Daemon, ursine_api::SessionToken) {
    let daemon =
        ursinectl::daemon::build(std::path::Path::new("/nonexistent/ursine-test.toml")).unwrap();
    let token = ursinectl::issue_token(&daemon);
    (daemon, token)
}

#[tokio::test]
async fn status_and_capabilities_through_dispatch() {
    let (daemon, token) = build();
    let status = call(&daemon, &token, ModuleId::System, Action::Status)
        .await
        .unwrap();
    assert!(status.get("version").is_some(), "{status}");

    let caps = call(&daemon, &token, ModuleId::Capabilities, Action::List)
        .await
        .unwrap();
    assert!(caps.is_array(), "{caps}");
}

#[tokio::test]
async fn cli_action_builder_round_trips_through_dispatch() {
    let (daemon, token) = build();
    // Same path a CLI invocation takes: key=value → Action → dispatch.
    let action = ursinectl::cli::build_action("status", &[]).unwrap();
    let result = call(&daemon, &token, ModuleId::System, action)
        .await
        .unwrap();
    assert!(result.get("version").is_some(), "{result}");
}

#[tokio::test]
async fn crontab_get_maps_absence_or_content() {
    let (daemon, token) = build();
    let action = ursinectl::cli::build_action("crontab_get", &[]).unwrap();
    // Either the container root has a crontab or not; both are valid answers.
    match call(&daemon, &token, ModuleId::Crontab, action).await {
        Ok(view) => assert!(view.get("crontab").is_some(), "{view}"),
        Err(e) => {
            // Capability missing (no crontab tool) is the other valid answer.
            assert!(
                e.message.contains("crontab"),
                "unexpected crontab error: {e:?}"
            );
        }
    }
}

#[tokio::test]
async fn response_body_err_shape_is_preserved() {
    // An illegal (module, action) pair must come back as a typed error, the
    // same shape the GUI parses.
    let (daemon, token) = build();
    let err = call(&daemon, &token, ModuleId::Logs, Action::ListInstalled)
        .await
        .unwrap_err();
    assert!(!err.message.is_empty());
    // And the raw envelope path agrees.
    let req = ursine_api::Request {
        v: ursine_api::PROTOCOL_VERSION,
        id: Some(7),
        token: Some(token.clone()),
        body: ursine_api::RequestBody::Call {
            module: ModuleId::Logs,
            action: Action::ListInstalled,
            as_user: None,
        },
    };
    let resp = daemon.handle(req, &root_peer()).await;
    assert!(matches!(resp.body, ResponseBody::Err { .. }));
}

#[tokio::test]
async fn stream_job_delivers_lines_and_exit_code() {
    let (daemon, token) = build();
    let principal = ursine_api::Principal::local("root", vec!["wheel".into()]);
    // A job that writes two lines and exits 0, then a failing one.
    let ok = daemon
        .jobs
        .start(
            ModuleId::PackagesApk,
            "install_apk",
            &principal,
            None,
            |sink| async move {
                sink.append(b"line one\nline two\n");
                Ok(0)
            },
        )
        .unwrap();
    let mut buf: Vec<u8> = Vec::new();
    let code = ursinectl::stream_job(&daemon, &token, &root_peer(), &ok.job_id, &mut buf)
        .await
        .unwrap();
    assert_eq!(code, 0);
    let text = String::from_utf8(buf).unwrap();
    assert!(text.contains("line one"), "{text}");
    assert!(text.contains("line two"), "{text}");

    let bad = daemon
        .jobs
        .start(
            ModuleId::PackagesApk,
            "remove_apk",
            &principal,
            None,
            |sink| async move {
                sink.append(b"kaboom\n");
                Ok(3)
            },
        )
        .unwrap();
    let mut buf: Vec<u8> = Vec::new();
    let code = ursinectl::stream_job(&daemon, &token, &root_peer(), &bad.job_id, &mut buf)
        .await
        .unwrap();
    assert_eq!(code, 3);
    assert!(String::from_utf8(buf).unwrap().contains("kaboom"));
}

#[tokio::test]
async fn unknown_job_id_is_an_error() {
    let (daemon, token) = build();
    let mut buf: Vec<u8> = Vec::new();
    let err = ursinectl::stream_job(&daemon, &token, &root_peer(), "0000000000000000", &mut buf)
        .await
        .unwrap_err();
    assert!(err.message.contains("unknown job"), "{}", err.message);
}

#[tokio::test]
async fn cross_user_crontab_is_rejected() {
    let (daemon, token) = build();
    // Root may not target another user's crontab (own-account rule).
    let err = ursinectl::call(
        &daemon,
        &token,
        &root_peer(),
        ModuleId::Crontab,
        Action::CrontabGet,
        Some("definitely-not-a-user".into()),
    )
    .await
    .unwrap_err();
    assert!(
        err.message.contains("own account"),
        "unexpected error: {err:?}"
    );
}

#[tokio::test]
async fn as_user_outside_user_scoped_modules_is_rejected() {
    let (daemon, token) = build();
    let err = ursinectl::call(
        &daemon,
        &token,
        &root_peer(),
        ModuleId::Logs,
        Action::LogList,
        Some("bob".into()),
    )
    .await
    .unwrap_err();
    assert!(
        err.message.contains("as_user is only valid"),
        "unexpected error: {err:?}"
    );
}

// ---- deeper coverage: config switches, audit, job binding, logs ----

/// Scratch path under `artifacts/` (host temp dir is off-limits by policy).
fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../artifacts/test-ursinectl")
        .join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Build a daemon around a real config file written under `artifacts/`.
fn build_with_config(tag: &str, toml: &str) -> (Daemon, ursine_api::SessionToken) {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch(tag);
    let path = dir.join("config.toml");
    std::fs::write(&path, toml).unwrap();
    // 0644: the config guard rejects group/world-writable files.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let daemon = ursinectl::daemon::build(&path).unwrap();
    let token = ursinectl::issue_token(&daemon);
    (daemon, token)
}

#[tokio::test]
async fn module_switch_off_blocks_the_module() {
    let (daemon, token) = build_with_config("mod-off", "[modules]\ncrontab = false\n");
    let err = call(&daemon, &token, ModuleId::Crontab, Action::CrontabGet)
        .await
        .unwrap_err();
    assert!(
        err.message.contains("module disabled by configuration"),
        "{err:?}"
    );
    // Other modules are unaffected.
    let ok = call(&daemon, &token, ModuleId::System, Action::Status)
        .await
        .unwrap();
    assert!(ok.get("version").is_some());
}

#[tokio::test]
async fn module_switch_auto_allows_the_module() {
    let (daemon, token) = build_with_config("mod-auto", "[modules]\ncrontab = \"auto\"\n");
    // Capability crontab exists in the container: the call must get past
    // the switch gate (content/absence is covered elsewhere).
    match call(&daemon, &token, ModuleId::Crontab, Action::CrontabGet).await {
        Ok(view) => assert!(view.get("crontab").is_some(), "{view}"),
        Err(e) => assert!(
            e.code != ursine_api::ErrorCode::Unavailable
                || !e.message.contains("disabled by configuration"),
            "switch must not block: {e:?}"
        ),
    }
}

#[tokio::test]
async fn set_module_switch_persists_and_reads_back() {
    let (daemon, token) = build_with_config("switch", "");
    // Empty file → get_config reports exists=true (we wrote it), switches default auto.
    let got = call(&daemon, &token, ModuleId::Config, Action::GetConfig)
        .await
        .unwrap();
    assert_eq!(got["exists"], true, "{got}");

    let res = call(
        &daemon,
        &token,
        ModuleId::Config,
        Action::SetModuleSwitch {
            module: ModuleId::Crontab,
            value: ursine_api::TriState::Off,
        },
    )
    .await
    .unwrap();
    assert_eq!(res["modules"]["crontab"], false, "{res}");

    // The switch is live: crontab is now blocked…
    let err = call(&daemon, &token, ModuleId::Crontab, Action::CrontabGet)
        .await
        .unwrap_err();
    assert!(err.message.contains("disabled by configuration"), "{err:?}");

    // …and persisted: a fresh daemon over the same file agrees.
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../artifacts/test-ursinectl/switch/config.toml");
    let daemon2 = ursinectl::daemon::build(&dir).unwrap();
    let token2 = ursinectl::issue_token(&daemon2);
    let err = call(&daemon2, &token2, ModuleId::Crontab, Action::CrontabGet)
        .await
        .unwrap_err();
    assert!(err.message.contains("disabled by configuration"), "{err:?}");
}

#[tokio::test]
async fn set_module_switch_rejects_non_configurable_module() {
    let (daemon, token) = build_with_config("switch-bad", "");
    let err = call(
        &daemon,
        &token,
        ModuleId::Config,
        Action::SetModuleSwitch {
            module: ModuleId::System,
            value: ursine_api::TriState::On,
        },
    )
    .await
    .unwrap_err();
    assert!(err.message.contains("no configurable switch"), "{err:?}");
}

#[tokio::test]
async fn config_init_refuses_to_overwrite() {
    let dir = scratch("init");
    let path = dir.join("config.toml");
    let daemon = ursinectl::daemon::build(&path).unwrap();
    let token = ursinectl::issue_token(&daemon);
    // First init writes the template…
    call(&daemon, &token, ModuleId::Config, Action::Init)
        .await
        .unwrap();
    assert!(path.exists());
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("[modules]"), "{body}");
    // …second init is a conflict, never a silent overwrite.
    let err = call(&daemon, &token, ModuleId::Config, Action::Init)
        .await
        .unwrap_err();
    assert!(err.code == ursine_api::ErrorCode::Conflict, "{err:?}");
}

#[tokio::test]
async fn every_call_leaves_an_audit_line() {
    let dir = scratch("audit");
    let audit_path = dir.join("audit.jsonl");
    let config = format!("audit_path = \"{}\"\n", audit_path.display());
    let (daemon, token) = build_with_config("audit-cfg", &config);
    call(&daemon, &token, ModuleId::System, Action::Status)
        .await
        .unwrap();
    // A rejected call is audited too (with the error outcome).
    let _ = call(&daemon, &token, ModuleId::Logs, Action::ListInstalled)
        .await
        .unwrap_err();

    let body = std::fs::read_to_string(&audit_path).unwrap();
    let lines: Vec<&str> = body.lines().filter(|l| !l.is_empty()).collect();
    assert!(lines.len() >= 2, "audit lines: {body}");
    let ok_line: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(ok_line["principal"], "root");
    assert_eq!(ok_line["action"], "status");
    assert_eq!(ok_line["module"], "system");
    let err_line: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(err_line["action"], "list_installed");
    assert_eq!(err_line["module"], "logs");
    // Outcome is internally tagged on `result`.
    assert_eq!(err_line["outcome"]["result"], "err");
    assert_eq!(err_line["outcome"]["code"], "invalid");
}

#[tokio::test]
async fn job_output_is_bound_to_the_starting_principal() {
    let (daemon, _token) = build();
    let root = ursine_api::Principal::local("root", vec!["wheel".into()]);
    let alice = ursine_api::Principal::local("alice", vec![]);
    let started = daemon
        .jobs
        .start(
            ModuleId::PackagesApk,
            "install_apk",
            &alice,
            None,
            |sink| async move {
                sink.append(b"x\n");
                Ok(0)
            },
        )
        .unwrap();
    // The root principal must NOT read alice's job…
    let forbidden = daemon.jobs.output(&root, &started.job_id, 0).unwrap_err();
    assert_eq!(forbidden.code, ursine_api::ErrorCode::Forbidden);
    assert!(
        forbidden.message.contains("another session"),
        "{forbidden:?}"
    );
    // …while the owner can (poll: the worker runs on its own task).
    let mut out = None;
    for _ in 0..200 {
        let poll = daemon.jobs.output(&alice, &started.job_id, 0).unwrap();
        if poll.status == ursine_api::JobStatus::Done {
            out = Some(poll);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let out = out.expect("job never finished");
    assert!(out.lines.contains('x'), "{out:?}");
    assert_eq!(out.exit_code, Some(0), "{out:?}");
}

#[tokio::test]
async fn job_output_rejects_malformed_ids() {
    let (daemon, _token) = build();
    let root = ursine_api::Principal::local("root", vec!["wheel".into()]);
    for bad in ["", "zz", "short", "000000000000000g", "../../etc/passwd"] {
        let err = daemon.jobs.output(&root, bad, 0).unwrap_err();
        assert!(
            err.code == ursine_api::ErrorCode::Invalid,
            "id {bad:?}: {err:?}"
        );
    }
}

#[tokio::test]
async fn logs_read_search_and_hardening_through_dispatch() {
    use std::io::Write as _;
    let (daemon, token) = build();
    // Unique name so parallel test binaries never collide in /var/log.
    let name = format!("ursine-ctl-{}.log", std::process::id());
    let path = std::path::Path::new("/var/log").join(&name);
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "first line here").unwrap();
    writeln!(f, "Needle in a haystack").unwrap();
    writeln!(f, "third").unwrap();
    drop(f);

    // list includes the fresh file (dynamic scan).
    let list = call(&daemon, &token, ModuleId::Logs, Action::LogList)
        .await
        .unwrap();
    let arr = list.as_array().unwrap();
    assert!(
        arr.iter()
            .any(|v| v["path"].as_str() == Some(name.as_str())),
        "list: {list}"
    );

    // tail: End cursor returns the three lines plus offsets.
    let chunk = call(
        &daemon,
        &token,
        ModuleId::Logs,
        Action::LogRead {
            file: name.clone(),
            lines: 10,
            cursor: ursine_api::LogCursor::End,
        },
    )
    .await
    .unwrap();
    assert!(
        chunk["content"].as_str().unwrap().contains("Needle"),
        "{chunk}"
    );
    assert_eq!(chunk["start"], 0);
    let end = chunk["end"].as_u64().unwrap();
    assert!(end > 0);
    assert_eq!(chunk["has_before"], false);

    // Load-previous from the end offset → empty-ish but valid range.
    let prev = call(
        &daemon,
        &token,
        ModuleId::Logs,
        Action::LogRead {
            file: name.clone(),
            lines: 1,
            cursor: ursine_api::LogCursor::Before { offset: end },
        },
    )
    .await
    .unwrap();
    assert!(prev["has_before"].as_bool().unwrap(), "{prev}");

    // search: case-insensitive hit with line number and jump offset.
    let hits = call(
        &daemon,
        &token,
        ModuleId::Logs,
        Action::LogSearch {
            file: name.clone(),
            query: "needle".into(),
            limit: 10,
        },
    )
    .await
    .unwrap();
    let hits_arr = hits["hits"].as_array().unwrap();
    assert_eq!(hits_arr.len(), 1, "{hits}");
    assert_eq!(hits_arr[0]["line_no"], 2, "{hits}");
    // Byte offset of line 2 = len("first line here\n") — the jump cursor
    // the GUI uses to land on the hit.
    assert_eq!(hits_arr[0]["offset"], 16, "{hits}");
    assert_eq!(hits["truncated_scan"], false);

    // Hardening through the real dispatch:
    for evil in ["../etc/shadow", "/etc/passwd", "a/../../etc/passwd"] {
        let err = call(
            &daemon,
            &token,
            ModuleId::Logs,
            Action::LogRead {
                file: evil.into(),
                lines: 10,
                cursor: ursine_api::LogCursor::End,
            },
        )
        .await
        .unwrap_err();
        assert!(!err.message.is_empty(), "escape {evil:?} must fail");
    }

    // Binary content is refused as a log.
    let binname = format!("ursine-ctl-{}-bin.log", std::process::id());
    let binpath = std::path::Path::new("/var/log").join(&binname);
    std::fs::write(&binpath, b"abc\0def\n").unwrap();
    let err = call(
        &daemon,
        &token,
        ModuleId::Logs,
        Action::LogRead {
            file: binname.clone(),
            lines: 10,
            cursor: ursine_api::LogCursor::End,
        },
    )
    .await
    .unwrap_err();
    assert!(
        err.message.contains("binary") || err.message.contains("text"),
        "{err:?}"
    );

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&binpath);
}

// ---- one dispatch test per module, driven by the 100-action sample table ----

/// Build an action from a static sample with a fake secret reader (tests
/// must never touch the real stdin).
fn sample_action(s: &ursinectl::cli::Sample) -> Action {
    let pairs: Vec<(String, String)> = s
        .pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    ursinectl::cli::build_action_with(s.action, &pairs, || Ok("x".to_string()))
        .unwrap_or_else(|e| panic!("{}: {e}", s.action))
}

/// Shared fixture for `log_read`/`log_search` samples.
fn write_log_fixture() {
    use std::io::Write as _;
    let path = std::path::Path::new("/var/log").join(ursinectl::cli::LOG_FIXTURE);
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "fixture line one").unwrap();
    writeln!(f, "Needle in a fixture").unwrap();
}

/// Run every sample of `$module` through the real daemon and check the
/// declared expectation. `$setup` runs before the loop (fixtures).
macro_rules! dispatch_per_module {
    ($fn_name:ident => $module:expr, setup $setup:expr) => {
        #[tokio::test]
        async fn $fn_name() {
            let (daemon, token) = build();
            ($setup)();
            for s in ursinectl::cli::samples_for($module) {
                let action = sample_action(s);
                assert_eq!(
                    ursine_core::modules::action_name(&action),
                    s.action,
                    "wire tag drift"
                );
                let result = call(&daemon, &token, $module, action).await;
                match (&s.dispatch, result) {
                    (ursinectl::cli::Dispatch::Skip, _) => {}
                    (ursinectl::cli::Dispatch::Ok, Ok(value)) => {
                        assert!(
                            value.is_object() || value.is_array(),
                            "{}: unexpected payload {value}",
                            s.action
                        );
                    }
                    (ursinectl::cli::Dispatch::Ok, Err(e)) => {
                        panic!("{}: expected Ok, got {e:?}", s.action);
                    }
                    (ursinectl::cli::Dispatch::Gate, Err(e)) => {
                        assert_eq!(
                            e.code,
                            ursine_api::ErrorCode::Unavailable,
                            "{}: gate must be Unavailable: {e:?}",
                            s.action
                        );
                    }
                    (ursinectl::cli::Dispatch::Gate, Ok(value)) => {
                        panic!("{}: expected capability gate, got {value}", s.action);
                    }
                    (ursinectl::cli::Dispatch::Any, _) => {}
                    (ursinectl::cli::Dispatch::Code(expected), Err(e)) => {
                        assert_eq!(e.code, *expected, "{}: {e:?}", s.action);
                    }
                    (ursinectl::cli::Dispatch::Code(expected), Ok(value)) => {
                        panic!(
                            "{}: expected error {expected:?}, got {value}",
                            s.action
                        );
                    }
                }
            }
        }
    };
    ($fn_name:ident => $module:expr) => {
        dispatch_per_module!($fn_name => $module, setup || ());
    };
}

dispatch_per_module!(dispatch_system => ModuleId::System);
dispatch_per_module!(dispatch_config => ModuleId::Config);
dispatch_per_module!(dispatch_capabilities => ModuleId::Capabilities);
dispatch_per_module!(dispatch_packages_nix => ModuleId::PackagesNix);
dispatch_per_module!(dispatch_packages_apk => ModuleId::PackagesApk);
dispatch_per_module!(dispatch_packages_flatpak => ModuleId::PackagesFlatpak);
dispatch_per_module!(dispatch_printers => ModuleId::Printers);
dispatch_per_module!(dispatch_scanners => ModuleId::Scanners);
dispatch_per_module!(dispatch_network => ModuleId::Network);
dispatch_per_module!(dispatch_bluetooth => ModuleId::Bluetooth);
dispatch_per_module!(dispatch_disks => ModuleId::Disks);
dispatch_per_module!(dispatch_services => ModuleId::Services);
dispatch_per_module!(dispatch_firewall => ModuleId::Firewall);
dispatch_per_module!(dispatch_snapshots => ModuleId::Snapshots);
dispatch_per_module!(dispatch_users => ModuleId::Users);
dispatch_per_module!(dispatch_niri => ModuleId::Niri);
dispatch_per_module!(dispatch_theming => ModuleId::Theming);
dispatch_per_module!(dispatch_logs => ModuleId::Logs, setup write_log_fixture);
dispatch_per_module!(dispatch_crontab => ModuleId::Crontab);
