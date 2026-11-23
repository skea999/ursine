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
/// Directories dinit searches for unit files, in lookup order.
///
/// Upstream's default service directory is `/etc/dinit.d`; distribution
/// packages also install units under `/lib` and `/usr/lib` (a `FeralOS` box
/// keeps `acpid`-style units in `/usr/lib/dinit.d`), and an admin-added tree
/// lives under `/usr/local/lib`. The list is a constant so the lookup is one
/// place to extend and can be tested against a temporary root.
const DINIT_SERVICE_DIRS: [&str; 4] = [
    "/etc/dinit.d",
    "/lib/dinit.d",
    "/usr/lib/dinit.d",
    "/usr/local/lib/dinit.d",
];
const TIMEOUT: Duration = Duration::from_secs(60);

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

/// Whether a unit file named `name` exists in one of `dirs`.
///
/// Pure over the directory list so a test can point it at a temporary root:
/// the real directories are root-owned and never touched from a test.
fn unit_file_exists<S: AsRef<Path>>(dirs: &[S], name: &str) -> bool {
    dirs.iter().any(|dir| dir.as_ref().join(name).is_file())
}

/// A dinit unit file exists (enabled or not) in a directory dinit searches.
fn dinit_service_exists(name: &str) -> bool {
    unit_file_exists(&DINIT_SERVICE_DIRS, name)
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

async fn openrc_action(name: &str, op: ServiceOp) -> Result<(), ApiError> {
    match op {
        ServiceOp::Start | ServiceOp::Stop | ServiceOp::Restart => {
            let verb = service_verb(op);
            run(&RC_SERVICE, &[name, verb], "OpenRC", "apk add openrc").await?;
        }
        ServiceOp::Enable => {
            run(
                &RC_UPDATE,
                &["add", name, "default"],
                "OpenRC",
                "apk add openrc",
            )
            .await?;
        }
        ServiceOp::Disable => {
            // `rc-update del` fails when the service is not in the runlevel;
            // the desired state already holds, so that answer is success.
            let program = tool(&RC_UPDATE, "OpenRC", "apk add openrc")?;
            let out =
                exec::run_root_program_tolerant(program, &["del", name], TIMEOUT, None).await?;
            if out.code != 0 {
                let text = format!("{}{}", out.stdout, out.stderr);
                if !text.contains("is not in the runlevel") {
                    return Err(ApiError::internal(format!(
                        "command failed: {}",
                        text.trim()
                    )));
                }
            }
        }
    }
    Ok(())
}

async fn dinit_action(name: &str, op: ServiceOp) -> Result<(), ApiError> {
    let exists = match op {
        // start/stop/restart go to dinitctl, which answers for every unit it
        // can load: no file test in front of them.
        ServiceOp::Start | ServiceOp::Stop | ServiceOp::Restart => true,
        ServiceOp::Enable | ServiceOp::Disable => dinit_service_known(name).await,
    };
    match dinit_step(op, exists, dinit_enabled(name)) {
        DinitStep::Done => Ok(()),
        DinitStep::Missing => Err(ApiError::new(
            ErrorCode::NotFound,
            format!("service {name} does not exist"),
        )
        .with_fix("check the unit name, or install the unit file under /etc/dinit.d")),
        DinitStep::Run(verb) => {
            run(&DINITCTL, &[verb, name], "dinit", "apk add dinit").await?;
            Ok(())
        }
    }
}

/// Whether dinit knows the unit at all, by file or by a loaded service.
///
/// The file test covers the directories in [`DINIT_SERVICE_DIRS`]; a unit kept
/// in a directory outside that list is still valid for dinit, so the miss path
/// asks `dinitctl list` before answering `not_found`. Without the fallback a
/// working `start`/`stop` target could answer `not_found` for `enable`.
async fn dinit_service_known(name: &str) -> bool {
    if dinit_service_exists(name) {
        return true;
    }
    let Ok(program) = tool(&DINITCTL, "dinit", "apk add dinit") else {
        return false;
    };
    match exec::run_root_program_tolerant(program, &["list"], TIMEOUT, None).await {
        // `dinitctl list` names every loaded service; reuse the list parser so
        // the accepted line format stays the one `list_services` reads.
        Ok(out) if out.code == 0 => parse_dinit_services(&out.stdout, |_| false)
            .iter()
            .any(|service| service.name == name),
        _ => false,
    }
}

/// One dinit operation, decided from what the host reports about the unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DinitStep {
    /// The requested state already holds: answer success, call nothing.
    Done,
    /// No unit file and no loaded service: answer `not_found`.
    Missing,
    /// Run `dinitctl <verb> <name>`.
    Run(&'static str),
}

/// Pure decision table for a dinit operation.
///
/// `enable`/`disable` under dinit are not pure boot-set edits (see
/// `Action::ServiceAction`): `dinitctl enable` starts a stopped service and
/// `dinitctl disable` leaves a running one running. The table keeps the three
/// documented cases in one place and testable without a dinit host.
fn dinit_step(op: ServiceOp, exists: bool, enabled: bool) -> DinitStep {
    match op {
        ServiceOp::Start | ServiceOp::Stop | ServiceOp::Restart => DinitStep::Run(service_verb(op)),
        ServiceOp::Enable => {
            if !exists {
                DinitStep::Missing
            } else if enabled {
                // The boot link already exists. Calling `dinitctl enable` here
                // would start a stopped service, so `enable` is a no-op.
                DinitStep::Done
            } else {
                DinitStep::Run("enable")
            }
        }
        ServiceOp::Disable => {
            if enabled {
                DinitStep::Run("disable")
            } else if exists {
                // "Not enabled" is the desired state of a real unit; the link
                // test cannot tell it from a missing one, hence `exists`.
                DinitStep::Done
            } else {
                DinitStep::Missing
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // the unit-root fixture needs a temp dir

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
    fn unit_lookup_covers_every_dinit_service_directory() {
        // A unit kept in /usr/lib/dinit.d (or any added directory) must not
        // answer not_found for enable/disable while start/stop work.
        assert!(DINIT_SERVICE_DIRS.contains(&"/etc/dinit.d"));
        assert!(DINIT_SERVICE_DIRS.contains(&"/usr/lib/dinit.d"));
        assert!(DINIT_SERVICE_DIRS.contains(&"/usr/local/lib/dinit.d"));
    }

    #[test]
    fn unit_file_found_in_an_added_directory() {
        let root = std::env::temp_dir().join(format!("ursine-units-{}", std::process::id()));
        let etc = root.join("etc");
        let usr_lib = root.join("usr-lib");
        fs::create_dir_all(&etc).unwrap();
        fs::create_dir_all(&usr_lib).unwrap();
        fs::write(usr_lib.join("acpid"), "type = process\n").unwrap();

        let dirs = [etc.as_path(), usr_lib.as_path()];
        assert!(unit_file_exists(&dirs, "acpid"));
        assert!(!unit_file_exists(&dirs, "missing"));
        // Only the second directory holds it: the lookup is not ordered-only.
        assert!(unit_file_exists(&[usr_lib.as_path()], "acpid"));
        assert!(!unit_file_exists(&[etc.as_path()], "acpid"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn dinit_enable_and_disable_keep_the_documented_running_state() {
        // The three cases from ISSUES.md: enable creates the link and starts a
        // stopped unit; an existing link is a no-op; disable removes the link
        // and leaves a running unit running.
        assert_eq!(
            dinit_step(ServiceOp::Enable, true, false),
            DinitStep::Run("enable")
        );
        assert_eq!(dinit_step(ServiceOp::Enable, true, true), DinitStep::Done);
        assert_eq!(
            dinit_step(ServiceOp::Disable, true, true),
            DinitStep::Run("disable")
        );
        // Existing unit, no boot link: already disabled.
        assert_eq!(dinit_step(ServiceOp::Disable, true, false), DinitStep::Done);
        // No unit file and no loaded service: not_found for both.
        assert_eq!(
            dinit_step(ServiceOp::Enable, false, false),
            DinitStep::Missing
        );
        assert_eq!(
            dinit_step(ServiceOp::Disable, false, false),
            DinitStep::Missing
        );
    }

    #[test]
    fn dinit_lifecycle_verbs_reach_dinitctl_unchanged() {
        assert_eq!(
            dinit_step(ServiceOp::Start, true, false),
            DinitStep::Run("start")
        );
        assert_eq!(
            dinit_step(ServiceOp::Stop, true, true),
            DinitStep::Run("stop")
        );
        assert_eq!(
            dinit_step(ServiceOp::Restart, true, true),
            DinitStep::Run("restart")
        );
    }

    #[test]
    fn validates_service_names() {
        assert!(validate_name("dbus").is_ok());
        assert!(validate_name("bad name").is_err());
        assert!(validate_name("../dbus").is_err());
        assert!(validate_name("-shutdown").is_err());
        assert!(validate_name(".hidden").is_err());
    }
}
