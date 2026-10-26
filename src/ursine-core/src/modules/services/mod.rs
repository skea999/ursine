//! services — init service status and lifecycle for `OpenRC` and dinit.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, ApiError, Capability, Service, ServiceOp};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const RC_SERVICE: [&str; 3] = [
    "/sbin/rc-service",
    "/usr/sbin/rc-service",
    "/usr/bin/rc-service",
];
const RC_UPDATE: [&str; 3] = [
    "/sbin/rc-update",
    "/usr/sbin/rc-update",
    "/usr/bin/rc-update",
];
const RC_STATUS: [&str; 3] = [
    "/sbin/rc-status",
    "/usr/sbin/rc-status",
    "/usr/bin/rc-status",
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
    candidates
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| ApiError::unavailable(format!("{label} not found"), fix))
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

async fn openrc_action(name: &str, op: ServiceOp) -> Result<(), ApiError> {
    match op {
        ServiceOp::Start | ServiceOp::Stop | ServiceOp::Restart => {
            let verb = match op {
                ServiceOp::Start => "start",
                ServiceOp::Stop => "stop",
                ServiceOp::Restart => "restart",
                ServiceOp::Enable | ServiceOp::Disable => unreachable!(),
            };
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
            run(&RC_UPDATE, &["del", name], "OpenRC", "apk add openrc").await?;
        }
    }
    Ok(())
}

async fn dinit_action(name: &str, op: ServiceOp) -> Result<(), ApiError> {
    match op {
        ServiceOp::Start | ServiceOp::Stop | ServiceOp::Restart => {
            let verb = match op {
                ServiceOp::Start => "start",
                ServiceOp::Stop => "stop",
                ServiceOp::Restart => "restart",
                ServiceOp::Enable | ServiceOp::Disable => unreachable!(),
            };
            run(&DINITCTL, &[verb, name], "dinit", "apk add dinit").await?;
        }
        ServiceOp::Enable => {
            if dinit_enabled(name) {
                return Ok(());
            }
            run(&DINITCTL, &["enable", name], "dinit", "apk add dinit").await?;
        }
        ServiceOp::Disable => {
            if !dinit_enabled(name) {
                return Ok(());
            }
            run(&DINITCTL, &["disable", name], "dinit", "apk add dinit").await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
