//! In-process integration: real daemon construction + real dispatch.
//! Runs as root inside the CI container (the same condition the CLI enforces).

#![allow(clippy::unwrap_used)] // tests
#![allow(clippy::panic)] // test failure reporting

pub mod common;

use ursine_api::{Action, ErrorCode, ModuleId, ResponseBody};
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
            e.code != ErrorCode::Unavailable || !e.message.contains("disabled by configuration"),
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
    assert!(err.code == ErrorCode::Conflict, "{err:?}");
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
    assert_eq!(forbidden.code, ErrorCode::Forbidden);
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
        assert!(err.code == ErrorCode::Invalid, "id {bad:?}: {err:?}");
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
fn sample_action(s: &common::Sample) -> Action {
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
    let path = std::path::Path::new("/var/log").join(common::LOG_FIXTURE);
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
            for s in common::samples_for($module) {
                let action = sample_action(s);
                assert_eq!(
                    ursine_core::modules::action_name(&action),
                    s.action,
                    "wire tag drift"
                );
                let result = call(&daemon, &token, $module, action).await;
                match (&s.dispatch, result) {
                    (common::Dispatch::Skip, _) => {}
                    (common::Dispatch::Ok, Ok(value)) => {
                        assert!(
                            value.is_object() || value.is_array(),
                            "{}: unexpected payload {value}",
                            s.action
                        );
                    }
                    (common::Dispatch::Ok, Err(e)) => {
                        panic!("{}: expected Ok, got {e:?}", s.action);
                    }
                    (common::Dispatch::Gate, Err(e)) => {
                        assert_eq!(
                            e.code,
                            ErrorCode::Unavailable,
                            "{}: gate must be Unavailable: {e:?}",
                            s.action
                        );
                    }
                    (common::Dispatch::Gate, Ok(value)) => {
                        panic!("{}: expected capability gate, got {value}", s.action);
                    }
                    (common::Dispatch::Any, _) => {}
                    (common::Dispatch::Code(expected), Err(e)) => {
                        assert_eq!(e.code, *expected, "{}: {e:?}", s.action);
                    }
                    (common::Dispatch::Code(expected), Ok(value)) => {
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

// ---- depth: cross-module legality, audit sweep, shapes, boundaries ----

/// Every module id (for the cross-module legality sweep).
const ALL_MODULES: &[ModuleId] = &[
    ModuleId::System,
    ModuleId::Config,
    ModuleId::Capabilities,
    ModuleId::PackagesNix,
    ModuleId::PackagesApk,
    ModuleId::PackagesFlatpak,
    ModuleId::Printers,
    ModuleId::Scanners,
    ModuleId::Firewall,
    ModuleId::Services,
    ModuleId::Disks,
    ModuleId::Bluetooth,
    ModuleId::Network,
    ModuleId::Snapshots,
    ModuleId::Users,
    ModuleId::Niri,
    ModuleId::Theming,
    ModuleId::Logs,
    ModuleId::Crontab,
];

/// Every sample action, called in a DIFFERENT module, must fail: either
/// the capability gate (checked first) or the pair-legality check. An `Ok`
/// here would mean an action executes outside its module — the strongest
/// dispatch invariant there is.
#[tokio::test]
async fn every_sample_action_is_rejected_in_the_wrong_module() {
    let (daemon, token) = build();
    write_log_fixture();
    for (idx, s) in common::SAMPLES.iter().enumerate() {
        // Deterministically pick a module that is not the sample's own.
        let wrong = ALL_MODULES[(idx + 1) % ALL_MODULES.len()];
        if wrong == s.module {
            continue;
        }
        let action = sample_action(s);
        let result = call(&daemon, &token, wrong, action).await;
        let err = match result {
            Ok(value) => panic!(
                "{} executed in module {}: {value}",
                s.action,
                wrong.as_str()
            ),
            Err(e) => e,
        };
        if err.code == ErrorCode::Invalid {
            assert!(
                err.message.contains("not valid for module"),
                "{} in {}: {err:?}",
                s.action,
                wrong.as_str()
            );
        } else {
            // Gate-first modules answer Unavailable before the pair check.
            assert_eq!(
                err.code,
                ErrorCode::Unavailable,
                "{} in {}: {err:?}",
                s.action,
                wrong.as_str()
            );
        }
    }
}

/// Every action the sweep runs must be audited (`job_output` is the one
/// documented exemption; the Skip samples never execute).
#[tokio::test]
async fn every_audited_sample_leaves_a_trace() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch("audit-sweep");
    let audit_path = dir.join("audit.jsonl");
    let cfg = dir.join("config.toml");
    std::fs::write(&cfg, format!("audit_path = \"{}\"\n", audit_path.display())).unwrap();
    std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o644)).unwrap();
    let daemon = ursinectl::daemon::build(&cfg).unwrap();
    let token = ursinectl::issue_token(&daemon);
    let peer = root_peer();

    write_log_fixture();
    let mut ran: Vec<&str> = Vec::new();
    for s in common::SAMPLES {
        if matches!(s.dispatch, common::Dispatch::Skip) || s.action == "job_output" {
            continue;
        }
        let action = sample_action(s);
        let _ = ursinectl::call(&daemon, &token, &peer, s.module, action, None).await;
        ran.push(s.action);
    }

    let body = std::fs::read_to_string(&audit_path).expect("audit file written");
    let mut audited: Vec<String> = Vec::new();
    for line in body.lines().filter(|l| !l.is_empty()) {
        let entry: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("audit line must be JSON: {e}: {line}"));
        audited.push(entry["action"].as_str().unwrap_or_default().to_string());
    }
    for action in &ran {
        assert!(
            audited.iter().any(|a| a == action),
            "action {action:?} missing from audit: {audited:?}"
        );
    }
    assert!(
        !audited.iter().any(|a| a == "job_output"),
        "job_output polls must never be audited: {audited:?}"
    );
    // Skipped samples never executed → never audited.
    for absent in ["init", "set_module_switch", "refresh_apk", "upgrade_apk"] {
        assert!(
            !audited.iter().any(|a| a == absent),
            "{absent} was not run but appears in audit"
        );
    }
}

/// Exact response shapes for the reads every frontend consumes.
#[tokio::test]
async fn response_shapes_match_the_contract() {
    let (daemon, token) = build();
    write_log_fixture();

    let status = call(&daemon, &token, ModuleId::System, Action::Status)
        .await
        .unwrap();
    assert_eq!(status["protocol_version"], 3, "{status}");
    assert!(status["version"].is_string(), "{status}");
    assert!(status["auth"].is_string(), "{status}");

    let cfg = call(&daemon, &token, ModuleId::Config, Action::GetConfig)
        .await
        .unwrap();
    assert!(
        cfg["path"].is_string() && cfg["exists"].is_boolean(),
        "{cfg}"
    );
    let modules = cfg["modules"].as_object().expect("modules object");
    // Every configurable module key present (GUI settings renders them all).
    for key in [
        "packages_nix",
        "packages_apk",
        "packages_flatpak",
        "printers",
        "scanners",
        "firewall",
        "services",
        "disks",
        "bluetooth",
        "network",
        "snapshots",
        "users",
        "niri",
        "theming",
        "crontab",
    ] {
        assert!(modules.contains_key(key), "missing module key {key}: {cfg}");
    }

    let caps = call(&daemon, &token, ModuleId::Capabilities, Action::List)
        .await
        .unwrap();
    let arr = caps.as_array().expect("caps array");
    assert!(arr.len() >= 15, "capability list thin: {caps}");
    for c in arr {
        assert!(c["capability"].is_string(), "{c}");
        assert!(c["available"].is_boolean(), "{c}");
        // Cockpit contract: unavailable ⇒ reason + fix, available ⇒ neither.
        if c["available"] == false {
            assert!(
                c["reason"].as_str().is_some_and(|r| !r.is_empty()),
                "unavailable without reason: {c}"
            );
            assert!(
                c["fix"].as_str().is_some_and(|f| !f.is_empty()),
                "unavailable without fix: {c}"
            );
        } else {
            assert!(c["reason"].is_null(), "available with reason: {c}");
            assert!(c["fix"].is_null(), "available with fix: {c}");
        }
    }

    let list = call(&daemon, &token, ModuleId::Logs, Action::LogList)
        .await
        .unwrap();
    let files = list.as_array().expect("log list array");
    let ours = files
        .iter()
        .find(|f| f["path"] == common::LOG_FIXTURE)
        .expect("fixture listed");
    assert!(ours["size"].is_u64() && ours["modified"].is_u64(), "{ours}");

    let search = call(
        &daemon,
        &token,
        ModuleId::Logs,
        Action::LogSearch {
            file: common::LOG_FIXTURE.to_string(),
            query: "Needle".into(),
            limit: 10,
        },
    )
    .await
    .unwrap();
    assert!(search["truncated_scan"].is_boolean(), "{search}");
    let hit = &search["hits"].as_array().unwrap()[0];
    assert!(hit["line_no"].is_u64() && hit["offset"].is_u64(), "{hit}");
    assert!(hit["line"].is_string(), "{hit}");

    let view = call(&daemon, &token, ModuleId::Crontab, Action::CrontabGet)
        .await
        .unwrap();
    assert!(
        view["crontab"].is_null() || view["crontab"].is_string(),
        "{view}"
    );

    let apk = call(&daemon, &token, ModuleId::PackagesApk, Action::ListApk)
        .await
        .unwrap();
    assert!(apk["packages"].is_array(), "{apk}");
    let repos = call(
        &daemon,
        &token,
        ModuleId::PackagesApk,
        Action::ListApkRepositories,
    )
    .await
    .unwrap();
    assert!(repos["repositories"].is_array(), "{repos}");
}

/// Wire-protocol boundaries of the shared envelope (raw handle — the CLI
/// never takes this path, the daemon socket does).
#[tokio::test]
async fn protocol_and_auth_boundaries() {
    let (daemon, token) = build();
    let peer = root_peer();

    // Wrong protocol version.
    let req = ursine_api::Request {
        v: 999,
        id: Some(1),
        token: Some(token.clone()),
        body: ursine_api::RequestBody::Call {
            module: ModuleId::System,
            action: Action::Status,
            as_user: None,
        },
    };
    let resp = daemon.handle(req, &peer).await;
    match resp.body {
        ResponseBody::Err { error } => {
            assert!(error.message.contains("protocol version"), "{error:?}");
        }
        other => panic!("want err, got {other:?}"),
    }

    // Missing token.
    let req = ursine_api::Request {
        v: ursine_api::PROTOCOL_VERSION,
        id: Some(2),
        token: None,
        body: ursine_api::RequestBody::Call {
            module: ModuleId::System,
            action: Action::Status,
            as_user: None,
        },
    };
    let resp = daemon.handle(req, &peer).await;
    assert!(
        matches!(resp.body, ResponseBody::Err { ref error } if error.code == ErrorCode::Unauthorized)
    );

    // Garbage token.
    let req = ursine_api::Request {
        v: ursine_api::PROTOCOL_VERSION,
        id: Some(3),
        token: Some(ursine_api::SessionToken::new("deadbeefdeadbeef")),
        body: ursine_api::RequestBody::Call {
            module: ModuleId::System,
            action: Action::Status,
            as_user: None,
        },
    };
    let resp = daemon.handle(req, &peer).await;
    assert!(
        matches!(resp.body, ResponseBody::Err { ref error } if error.code == ErrorCode::Unauthorized)
    );

    // The CLI's authenticator rejects every auth attempt outright.
    let req = ursine_api::Request {
        v: ursine_api::PROTOCOL_VERSION,
        id: Some(4),
        token: None,
        // Any credential works as input: the CLI's NoAuth rejects all of
        // them unconditionally (value kept short: it is not a secret).
        body: ursine_api::RequestBody::Auth {
            user: "root".into(),
            password: "x".into(),
        },
    };
    let resp = daemon.handle(req, &peer).await;
    assert!(
        matches!(resp.body, ResponseBody::Err { ref error } if error.code == ErrorCode::Unauthorized),
        "NoAuth must reject: {:?}",
        resp.body
    );
}

/// Cross-user scoping split: nix profile actions ACCEPT a foreign `as_user`
/// (they are profile-scoped), crontab/theming REJECT it — verified above.
#[tokio::test]
async fn nix_accepts_foreign_as_user_while_gated() {
    let (daemon, token) = build();
    let err = ursinectl::call(
        &daemon,
        &token,
        &root_peer(),
        ModuleId::PackagesNix,
        Action::ListInstalled,
        Some("someone-else".into()),
    )
    .await
    .unwrap_err();
    // The scope check let it through; the capability gate (nix absent) fired.
    assert_eq!(err.code, ErrorCode::Unavailable, "{err:?}");
    assert!(!err.message.contains("as_user is only valid"), "{err:?}");
}

/// Exact byte semantics of the three log cursors over a known file.
#[tokio::test]
async fn log_cursors_serve_exact_byte_ranges() {
    let (daemon, token) = build();
    let name = format!("ursine-ctl-cursor-{}.log", std::process::id());
    let path = std::path::Path::new("/var/log").join(&name);
    std::fs::write(&path, b"L1\nL2\nL3\n").unwrap();

    // End: whole file.
    let end = call(
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
    assert_eq!(end["content"], "L1\nL2\nL3\n", "{end}");
    assert_eq!(end["start"], 0, "{end}");
    assert_eq!(end["end"], 9, "{end}");
    assert_eq!(end["size"], 9, "{end}");
    assert_eq!(end["has_before"], false, "{end}");

    // Before: the last line, ending at the file end.
    let prev = call(
        &daemon,
        &token,
        ModuleId::Logs,
        Action::LogRead {
            file: name.clone(),
            lines: 1,
            cursor: ursine_api::LogCursor::Before { offset: 9 },
        },
    )
    .await
    .unwrap();
    assert_eq!(prev["content"], "L3\n", "{prev}");
    assert_eq!(prev["start"], 6, "{prev}");
    assert_eq!(prev["end"], 9, "{prev}");
    assert_eq!(prev["has_before"], true, "{prev}");

    // From: complete lines starting at a mid-file offset (follow loop).
    let from = call(
        &daemon,
        &token,
        ModuleId::Logs,
        Action::LogRead {
            file: name.clone(),
            lines: 10,
            cursor: ursine_api::LogCursor::From { offset: 3 },
        },
    )
    .await
    .unwrap();
    assert_eq!(from["content"], "L2\nL3\n", "{from}");
    assert_eq!(from["start"], 3, "{from}");
    assert_eq!(from["end"], 9, "{from}");
    assert_eq!(from["has_before"], true, "{from}");

    // Missing file → typed NotFound, not a panic.
    let err = call(
        &daemon,
        &token,
        ModuleId::Logs,
        Action::LogRead {
            file: "definitely-missing-9f8e.log".into(),
            lines: 10,
            cursor: ursine_api::LogCursor::End,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound, "{err:?}");

    let _ = std::fs::remove_file(&path);
}

/// Search limit enforcement and the empty-result contract.
#[tokio::test]
async fn log_search_limit_and_miss_contract() {
    let (daemon, token) = build();
    let name = format!("ursine-ctl-search-{}.log", std::process::id());
    let path = std::path::Path::new("/var/log").join(&name);
    std::fs::write(&path, "aaa\nbbb\naaa\n").unwrap();

    let hits = call(
        &daemon,
        &token,
        ModuleId::Logs,
        Action::LogSearch {
            file: name.clone(),
            query: "aaa".into(),
            limit: 1,
        },
    )
    .await
    .unwrap();
    assert_eq!(hits["hits"].as_array().unwrap().len(), 1, "limit=1: {hits}");
    assert_eq!(hits["hits"][0]["line_no"], 1, "{hits}");

    let miss = call(
        &daemon,
        &token,
        ModuleId::Logs,
        Action::LogSearch {
            file: name.clone(),
            query: "zzz-not-there".into(),
            limit: 10,
        },
    )
    .await
    .unwrap();
    assert_eq!(miss["hits"].as_array().unwrap().len(), 0, "{miss}");
    assert_eq!(miss["truncated_scan"], false, "{miss}");

    let _ = std::fs::remove_file(&path);
}

/// Config switches cycle through all three states with the live gate
/// following each transition (Off blocks, On forces, Auto decides).
#[tokio::test]
async fn config_switch_cycle_drives_the_live_gate() {
    let (daemon, token) = build_with_config("switch-cycle", "");

    for (value, blocked) in [
        (ursine_api::TriState::Off, true),
        (ursine_api::TriState::On, false),
        (ursine_api::TriState::Auto, false),
    ] {
        let set = call(
            &daemon,
            &token,
            ModuleId::Config,
            Action::SetModuleSwitch {
                module: ModuleId::Crontab,
                value,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            set["modules"]["crontab"],
            serde_json::to_value(value).unwrap()
        );

        let result = call(&daemon, &token, ModuleId::Crontab, Action::CrontabGet).await;
        if blocked {
            let err = result.unwrap_err();
            assert!(
                err.message.contains("disabled by configuration"),
                "Off must block: {err:?}"
            );
        } else {
            result.unwrap_or_else(|e| panic!("On/Auto must allow: {e:?}"));
        }
    }
}

/// A worker error closes the job without an exit code: the streamer must
/// surface it as a failure (exit code 1) after delivering the log.
#[tokio::test]
async fn stream_job_reports_worker_error_as_failure() {
    let (daemon, token) = build();
    let principal = ursine_api::Principal::local("root", vec!["wheel".into()]);
    let started = daemon
        .jobs
        .start(
            ModuleId::PackagesApk,
            "install_apk",
            &principal,
            None,
            |sink| async move {
                sink.append(b"about to fail\n");
                Err(ursine_api::ApiError::internal("worker boom"))
            },
        )
        .unwrap();
    let mut buf: Vec<u8> = Vec::new();
    let code = ursinectl::stream_job(&daemon, &token, &root_peer(), &started.job_id, &mut buf)
        .await
        .unwrap();
    assert_eq!(code, 1, "worker error must exit non-zero");
    let text = String::from_utf8(buf).unwrap();
    assert!(text.contains("about to fail"), "{text}");
}

/// Poll-boundary correctness: bytes written across two polls are delivered
/// exactly once each (offset resume) and a multi-byte UTF-8 sequence split
/// between polls is reassembled losslessly.
#[tokio::test]
async fn stream_job_resumes_across_polls_without_loss_or_duplication() {
    let (daemon, token) = build();
    let principal = ursine_api::Principal::local("root", vec!["wheel".into()]);
    let started = daemon
        .jobs
        .start(
            ModuleId::PackagesApk,
            "install_apk",
            &principal,
            None,
            |sink| async move {
                // Cross the streamer's 300 ms poll cadence twice.
                sink.append(b"part1\n");
                sink.append(&[b'a', 0xC3]); // torn multi-byte char at the boundary
                tokio::time::sleep(std::time::Duration::from_millis(450)).await;
                sink.append(&[0xA9, b'\n']); // completes 'é'
                sink.append(b"part2\n");
                tokio::time::sleep(std::time::Duration::from_millis(450)).await;
                Ok(0)
            },
        )
        .unwrap();
    let mut buf: Vec<u8> = Vec::new();
    let code = ursinectl::stream_job(&daemon, &token, &root_peer(), &started.job_id, &mut buf)
        .await
        .unwrap();
    assert_eq!(code, 0);
    let text = String::from_utf8(buf).expect("stream must be lossless UTF-8");
    assert_eq!(text.matches("part1").count(), 1, "no duplication: {text:?}");
    assert_eq!(text.matches("part2").count(), 1, "no duplication: {text:?}");
    assert!(text.contains("a\u{e9}"), "torn char reassembled: {text:?}");
}
