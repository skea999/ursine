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

/// A dinit unit file exists (enabled or not).
fn dinit_service_exists(name: &str) -> bool {
    ["/etc/dinit.d", "/lib/dinit.d"]
        .iter()
        .any(|dir| Path::new(dir).join(name).is_file())
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
    match op {
        ServiceOp::Start | ServiceOp::Stop | ServiceOp::Restart => {
            let verb = service_verb(op);
            run(&DINITCTL, &[verb, name], "dinit", "apk add dinit").await?;
        }
        ServiceOp::Enable => {
            if !dinit_service_exists(name) {
                return Err(ApiError::new(
                    ErrorCode::NotFound,
                    format!("service {name} does not exist"),
                ));
            }
            if dinit_enabled(name) {
                return Ok(());
            }
            run(&DINITCTL, &["enable", name], "dinit", "apk add dinit").await?;
        }
        ServiceOp::Disable => {
            // "Not enabled" is success only for an existing service: the
            // link test cannot tell it from a missing unit.
            if !dinit_enabled(name) {
                if dinit_service_exists(name) {
                    return Ok(());
                }
                return Err(ApiError::new(
                    ErrorCode::NotFound,
                    format!("service {name} does not exist"),
                ));
            }
            run(&DINITCTL, &["disable", name], "dinit", "apk add dinit").await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
}
