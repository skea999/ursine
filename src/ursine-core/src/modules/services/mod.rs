//! services — init service status and lifecycle for `OpenRC` and dinit.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, ApiError, Capability, ErrorCode, Service, ServiceOp};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

// busybox-based FeralOS keeps some rc applets in /bin only (the VM smoke
// found rc-status there) — cover /bin as well as the FHS sbin paths.
const RC_SERVICE: [&str; 4] = [
    "/sbin/rc-service",
    "/usr/sbin/rc-service",
    "/usr/bin/rc-service",
    "/bin/rc-service",
];
const RC_UPDATE: [&str; 4] = [
    "/sbin/rc-update",
    "/usr/sbin/rc-update",
    "/usr/bin/rc-update",
    "/bin/rc-update",
];
const RC_STATUS: [&str; 4] = [
    "/sbin/rc-status",
    "/usr/sbin/rc-status",
    "/usr/bin/rc-status",
    "/bin/rc-status",
];
const DINITCTL: [&str; 4] = [
    "/usr/bin/dinitctl",
    "/sbin/dinitctl",
    "/usr/sbin/dinitctl",
    "/bin/dinitctl",
];
const DINIT_BOOT_DIRS: [&str; 4] = [
    "/etc/dinit.d/boot.d",
    "/lib/dinit.d/boot.d",
    "/usr/lib/dinit.d/boot.d",
    "/usr/local/lib/dinit.d/boot.d",
];
// A dinit service exists exactly when a description file of that name exists
// in one of these directories; the boot.d links point at those descriptions.
const DINIT_DESCRIPTION_DIRS: [&str; 4] = [
    "/etc/dinit.d",
    "/usr/lib/dinit.d",
    "/lib/dinit.d",
    "/usr/local/lib/dinit.d",
];
const TIMEOUT: Duration = Duration::from_secs(60);

/// What a rejected init tool call means for the caller.
///
/// The tools signal a desired state that already holds with the same channel
/// as a real failure (a non-zero exit and a message on stderr), so the answer
/// has to be read off the text instead of the exit code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rejection {
    /// The tool refused because the requested state already holds.
    AlreadyInState,
    /// The init does not know a service by that name.
    UnknownService,
    /// Anything else: a real failure, kept as a typed `internal`.
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backend {
    OpenRc,
    Dinit,
}

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    let backend = select_backend(ctx)?;
    match action {
        Action::ListServices => Ok(json!({"services": list(backend).await?})),
        Action::ServiceAction { name, op } => {
            validate_name(name)?;
            match backend {
                Backend::OpenRc => openrc_action(name, *op).await?,
                Backend::Dinit => dinit_action(name, *op).await?,
            }
            Ok(json!({"services": list(backend).await?}))
        }
        _ => Err(ApiError::invalid("action is not valid for module services")),
    }
}

fn select_backend(ctx: &ModuleCtx<'_>) -> Result<Backend, ApiError> {
    // dinit wins when it is the running init; OpenRC tooling may coexist on
    // FeralOS. The module gate already accepts either backend.
    if ctx.caps.check(Capability::Dinit) {
        Ok(Backend::Dinit)
    } else if ctx.caps.check(Capability::OpenRc) {
        Ok(Backend::OpenRc)
    } else {
        Err(ApiError::unavailable(
            "no supported init service manager",
            "boot OpenRC or dinit",
        ))
    }
}

async fn list(backend: Backend) -> Result<Vec<Service>, ApiError> {
    match backend {
        Backend::OpenRc => list_openrc().await,
        Backend::Dinit => list_dinit().await,
    }
}

async fn list_openrc() -> Result<Vec<Service>, ApiError> {
    let status = run(&RC_STATUS, &["--servicelist"], "OpenRC", "apk add openrc").await?;
    let enabled = run(&RC_UPDATE, &["show"], "OpenRC", "apk add openrc").await?;
    Ok(parse_services(&status, &enabled))
}

fn parse_services(status: &str, enabled: &str) -> Vec<Service> {
    let enabled: Vec<&str> = enabled
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    status
        .lines()
        .filter_map(|line| {
            let name = line.split_whitespace().next()?;
            if !valid_name(name) {
                return None;
            }
            let state = if line.contains("[  started  ]") {
                "started"
            } else if line.contains("[  stopped  ]") {
                "stopped"
            } else if line.contains("[  crashed  ]") {
                "crashed"
            } else {
                "unknown"
            };
            Some(Service {
                name: name.to_string(),
                state: state.to_string(),
                enabled: enabled.contains(&name),
                init: "openrc".to_string(),
            })
        })
        .collect()
}

async fn list_dinit() -> Result<Vec<Service>, ApiError> {
    let status = run(&DINITCTL, &["list"], "dinit", "apk add dinit").await?;
    Ok(parse_dinit_services(&status, dinit_enabled))
}

fn parse_dinit_services<F>(status: &str, enabled: F) -> Vec<Service>
where
    F: Fn(&str) -> bool,
{
    status
        .lines()
        .filter_map(|line| {
            let rest = line.rsplit_once(']')?.1.trim();
            let name = rest.split_whitespace().next()?;
            if !valid_name(name) || matches!(name, "boot" | "recovery") {
                return None;
            }
            let state = if line.contains("<<") || line.contains("[!") {
                "failed"
            } else if line.contains("{+}") || line.contains("[+]") {
                "started"
            } else if line.contains("{-}") || line.contains("[ ]") {
                "stopped"
            } else {
                "unknown"
            };
            Some(Service {
                name: name.to_string(),
                state: state.to_string(),
                enabled: enabled(name),
                init: "dinit".to_string(),
            })
        })
        .collect()
}

fn dinit_enabled(name: &str) -> bool {
    DINIT_BOOT_DIRS
        .iter()
        .any(|dir| fs::symlink_metadata(Path::new(dir).join(name)).is_ok())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.starts_with('.')
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(ApiError::invalid("invalid service name"))
    }
}

fn tool(candidates: &[&'static str], label: &str, fix: &str) -> Result<&'static str, ApiError> {
    crate::tooling::resolve(candidates, label, fix)
}

async fn run(
    candidates: &[&'static str],
    args: &[&str],
    label: &str,
    fix: &str,
) -> Result<String, ApiError> {
    exec::run_root_program(tool(candidates, label, fix)?, args, TIMEOUT)
        .await
        .map(|output| output.stdout)
}

/// The `rc-service`/`dinitctl` verb for a start/stop/restart operation.
fn service_verb(op: ServiceOp) -> &'static str {
    match op {
        ServiceOp::Start => "start",
        ServiceOp::Stop => "stop",
        ServiceOp::Restart => "restart",
        ServiceOp::Enable | ServiceOp::Disable => unreachable!("not a verb operation"),
    }
}

/// Error for a name no service of this init answers to.
fn unknown_service(label: &str, name: &str) -> ApiError {
    ApiError::new(
        ErrorCode::NotFound,
        format!("{label} has no service named {name}"),
    )
    .with_fix("check the service name; list services")
}

/// Classify `rc-update`/`rc-service` stderr for one operation.
fn classify_openrc(stderr: &str, op: ServiceOp) -> Rejection {
    // OpenRC quotes names with a backtick/apostrophe pair (`bluetooth'), so
    // only the verb phrase is matched and the quoting never matters.
    if stderr.contains("does not exist") || stderr.contains("not a valid service") {
        return Rejection::UnknownService;
    }
    let already = match op {
        // `rc-update del` on a service outside the runlevel refuses with
        // "is not in the runlevel" instead of exiting 0 (issue #6).
        ServiceOp::Disable => stderr.contains("is not in the runlevel"),
        ServiceOp::Enable => stderr.contains("is already in the runlevel"),
        ServiceOp::Start | ServiceOp::Stop | ServiceOp::Restart => false,
    };
    if already {
        Rejection::AlreadyInState
    } else {
        Rejection::Failed
    }
}

/// Classify `dinitctl` stderr for one operation.
fn classify_dinit(stderr: &str, op: ServiceOp) -> Rejection {
    if stderr.contains("failed to find service description")
        || stderr.contains("service description file does not exist")
    {
        return Rejection::UnknownService;
    }
    let already = match op {
        ServiceOp::Enable => stderr.contains("already enabled"),
        ServiceOp::Disable => {
            stderr.contains("is not enabled") || stderr.contains("already disabled")
        }
        ServiceOp::Start | ServiceOp::Stop | ServiceOp::Restart => false,
    };
    if already {
        Rejection::AlreadyInState
    } else {
        Rejection::Failed
    }
}

/// Run one init tool call and turn a rejected call into its typed answer.
///
/// `Ok(())` covers both a successful call and a refusal that means the desired
/// state already holds, so enable/disable are idempotent. A real failure keeps
/// the tool's text inside a typed `internal` error.
async fn run_classified(
    name: &str,
    candidates: &[&'static str],
    args: &[&str],
    label: &str,
    fix: &str,
    classify: impl Fn(&str) -> Rejection,
) -> Result<(), ApiError> {
    let program = tool(candidates, label, fix)?;
    let outcome = exec::run_root_program_tolerant(program, args, TIMEOUT, None).await?;
    if outcome.code == 0 {
        return Ok(());
    }
    let detail = if outcome.stderr.trim().is_empty() {
        outcome.stdout.trim().to_string()
    } else {
        outcome.stderr.trim().to_string()
    };
    match classify(&detail) {
        Rejection::AlreadyInState => Ok(()),
        Rejection::UnknownService => Err(unknown_service(label, name)),
        Rejection::Failed => Err(ApiError::internal(format!("command failed: {detail}"))),
    }
}

/// A dinit service exists when a description file carries its name.
fn description_present(name: &str, dirs: &[impl AsRef<Path>]) -> bool {
    dirs.iter().any(|dir| dir.as_ref().join(name).is_file())
}

fn dinit_described(name: &str) -> bool {
    description_present(name, &DINIT_DESCRIPTION_DIRS)
}

/// `OpenRC` answers for a name only when an initscript of that name exists.
///
/// Needs an extra call because `rc-update del` reports a name it has never
/// heard of as "not in the runlevel", which is the idempotent case.
async fn require_openrc_service(name: &str) -> Result<(), ApiError> {
    let program = tool(&RC_SERVICE, "OpenRC", "apk add openrc")?;
    let outcome =
        exec::run_root_program_tolerant(program, &["--exists", name], TIMEOUT, None).await?;
    if outcome.code == 0 {
        return Ok(());
    }
    let stderr = outcome.stderr.trim();
    // An rc-service too old for `--exists` must not turn every action into
    // a not_found: report what the tool said instead.
    if stderr.contains("unknown option") || stderr.contains("unrecognized option") {
        return Err(ApiError::internal(format!("command failed: {stderr}")));
    }
    Err(unknown_service("OpenRC", name))
}

async fn openrc_action(name: &str, op: ServiceOp) -> Result<(), ApiError> {
    require_openrc_service(name).await?;
    match op {
        ServiceOp::Start | ServiceOp::Stop | ServiceOp::Restart => {
            let verb = service_verb(op);
            run_classified(
                name,
                &RC_SERVICE,
                &[name, verb],
                "OpenRC",
                "apk add openrc",
                |stderr| classify_openrc(stderr, op),
            )
            .await
        }
        ServiceOp::Enable => {
            run_classified(
                name,
                &RC_UPDATE,
                &["add", name, "default"],
                "OpenRC",
                "apk add openrc",
                |stderr| classify_openrc(stderr, op),
            )
            .await
        }
        ServiceOp::Disable => {
            run_classified(
                name,
                &RC_UPDATE,
                &["del", name],
                "OpenRC",
                "apk add openrc",
                |stderr| classify_openrc(stderr, op),
            )
            .await
        }
    }
}

async fn dinit_action(name: &str, op: ServiceOp) -> Result<(), ApiError> {
    match op {
        ServiceOp::Start | ServiceOp::Stop | ServiceOp::Restart => {
            let verb = service_verb(op);
            run_classified(
                name,
                &DINITCTL,
                &[verb, name],
                "dinit",
                "apk add dinit",
                |stderr| classify_dinit(stderr, op),
            )
            .await
        }
        ServiceOp::Enable => {
            // The boot-link test cannot tell "already enabled" from "no such
            // service", so the description is checked first (issue #15).
            if !dinit_described(name) {
                return Err(unknown_service("dinit", name));
            }
            if dinit_enabled(name) {
                return Ok(());
            }
            run_classified(
                name,
                &DINITCTL,
                &["enable", name],
                "dinit",
                "apk add dinit",
                |stderr| classify_dinit(stderr, op),
            )
            .await
        }
        ServiceOp::Disable => {
            if !dinit_described(name) {
                return Err(unknown_service("dinit", name));
            }
            if !dinit_enabled(name) {
                return Ok(());
            }
            run_classified(
                name,
                &DINITCTL,
                &["disable", name],
                "dinit",
                "apk add dinit",
                |stderr| classify_dinit(stderr, op),
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir

    #[test]
    fn openrc_candidates_cover_busybox_bin() {
        // The VM smoke found rc-status only at /bin (busybox applet).
        assert!(RC_STATUS.contains(&"/bin/rc-status"));
        assert!(RC_SERVICE.contains(&"/bin/rc-service"));
        assert!(RC_UPDATE.contains(&"/bin/rc-update"));
    }

    use super::*;

    #[test]
    fn parses_openrc_services() {
        let status = " dbus                                                              [  started  ]\n cupsd                                                             [  stopped  ]\n";
        let services = parse_services(status, " dbus | default\n");
        assert_eq!(services.len(), 2);
        assert_eq!(services[0].name, "dbus");
        assert!(services[0].enabled);
        assert_eq!(services[1].state, "stopped");
    }

    #[test]
    fn parses_dinit_services() {
        let status =
            "[[+]     ] boot\n[{+}     ] demo (pid: 11)\n[     {-}] stopped\n[[ ]<<   ] failed\n";
        let services = parse_dinit_services(status, |name| name == "demo");
        assert_eq!(services.len(), 3);
        assert_eq!(services[0].name, "demo");
        assert!(services[0].enabled);
        assert_eq!(services[1].state, "stopped");
        assert_eq!(services[2].state, "failed");
    }

    #[test]
    fn validates_service_names() {
        assert!(validate_name("dbus").is_ok());
        assert!(validate_name("bad name").is_err());
        assert!(validate_name("../dbus").is_err());
        assert!(validate_name("-shutdown").is_err());
        assert!(validate_name(".hidden").is_err());
    }

    #[test]
    fn openrc_refusals_are_classified() {
        // Recorded samples from issue #6 and the OpenRC sources: a refusal
        // that describes the wanted state must not be an error.
        let cases = [
            (
                "* rc-update: service `bluetooth' is not in the runlevel `default'",
                ServiceOp::Disable,
                Rejection::AlreadyInState,
            ),
            (
                "rc-update: service 'bluetooth' is not in the runlevel 'default'",
                ServiceOp::Disable,
                Rejection::AlreadyInState,
            ),
            (
                "* rc-update: service `cupsd' is already in the runlevel `default'",
                ServiceOp::Enable,
                Rejection::AlreadyInState,
            ),
            (
                "rc-service: service `bluetooth' does not exist",
                ServiceOp::Start,
                Rejection::UnknownService,
            ),
            (
                "rc-update: service `bluetooth' does not exist",
                ServiceOp::Enable,
                Rejection::UnknownService,
            ),
            (
                " * ERROR: bluetooth failed to start",
                ServiceOp::Start,
                Rejection::Failed,
            ),
            (
                "* rc-update: failed to add service `x' to runlevel `default': error",
                ServiceOp::Enable,
                Rejection::Failed,
            ),
        ];
        for (stderr, op, want) in cases {
            assert_eq!(classify_openrc(stderr, op), want, "{stderr}");
        }
        // The idempotent phrases are per operation: an enable that reports the
        // disable wording is a real failure, not a no-op.
        assert_eq!(
            classify_openrc(
                "rc-update: service `x' is not in the runlevel `default'",
                ServiceOp::Enable
            ),
            Rejection::Failed
        );
    }

    #[test]
    fn dinit_refusals_are_classified() {
        let missing = "dinitctl: failed to find service description.\ndinitctl: check service description file exists / service name spelling.";
        for op in [
            ServiceOp::Start,
            ServiceOp::Stop,
            ServiceOp::Restart,
            ServiceOp::Enable,
            ServiceOp::Disable,
        ] {
            assert_eq!(classify_dinit(missing, op), Rejection::UnknownService);
        }
        assert_eq!(
            classify_dinit("dinitctl: service is already enabled.", ServiceOp::Enable),
            Rejection::AlreadyInState
        );
        assert_eq!(
            classify_dinit("dinitctl: service is not enabled.", ServiceOp::Disable),
            Rejection::AlreadyInState
        );
        assert_eq!(
            classify_dinit("dinitctl: cannot start service.", ServiceOp::Start),
            Rejection::Failed
        );
        assert_eq!(
            classify_dinit("dinitctl: service is not enabled.", ServiceOp::Enable),
            Rejection::Failed
        );
    }

    #[test]
    fn unknown_service_names_answer_not_found_with_a_fix() {
        for label in ["OpenRC", "dinit"] {
            let error = unknown_service(label, "bluetooth");
            assert_eq!(error.code, ErrorCode::NotFound);
            assert!(error.message.contains("bluetooth"), "{}", error.message);
            assert_eq!(
                error.fix.as_deref(),
                Some("check the service name; list services")
            );
        }
    }

    #[test]
    fn description_lookup_finds_a_service_and_rejects_an_unknown_name() {
        // The description lookup is what tells an unknown dinit name from a
        // disabled service, so it must be pure and dir-driven.
        // nosemgrep: rust.lang.security.temp-dir.temp-dir — tests only; paths are container-local and non-security-sensitive
        let dir = std::env::temp_dir().join(format!("ursine-services-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("cupsd"), "type = process\n").unwrap();
        let empty = dir.join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(description_present("cupsd", std::slice::from_ref(&dir)));
        assert!(description_present("cupsd", &[empty, dir.clone()]));
        assert!(!description_present("cuppsd", std::slice::from_ref(&dir)));
        // A boot.d link is not a description of its own; only the file counts.
        assert!(!description_present("boot.d", std::slice::from_ref(&dir)));
        fs::remove_dir_all(&dir).unwrap();
    }
}
