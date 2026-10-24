//! services — `OpenRC` service status and lifecycle.

use std::path::Path;
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, ApiError, Service, ServiceOp};

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
const TIMEOUT: Duration = Duration::from_secs(60);

pub async fn handle(action: &Action, _ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListServices => Ok(json!({"services": list().await?})),
        Action::ServiceAction { name, op } => {
            validate_name(name)?;
            match op {
                ServiceOp::Start | ServiceOp::Stop | ServiceOp::Restart => {
                    let verb = match op {
                        ServiceOp::Start => "start",
                        ServiceOp::Stop => "stop",
                        ServiceOp::Restart => "restart",
                        _ => unreachable!(),
                    };
                    run(&RC_SERVICE, &[name, verb]).await?;
                }
                ServiceOp::Enable => {
                    run(&RC_UPDATE, &["add", name, "default"]).await?;
                }
                ServiceOp::Disable => {
                    run(&RC_UPDATE, &["del", name]).await?;
                }
            }
            Ok(json!({"services": list().await?}))
        }
        _ => Err(ApiError::invalid("action is not valid for module services")),
    }
}

async fn list() -> Result<Vec<Service>, ApiError> {
    let status = run(&RC_STATUS, &["--servicelist"]).await?;
    let enabled = run(&RC_UPDATE, &["show"]).await?;
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

fn valid_name(name: &str) -> bool {
    !name.is_empty()
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

fn tool(candidates: &[&'static str], label: &str) -> Result<&'static str, ApiError> {
    candidates
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| ApiError::unavailable(format!("{label} not found"), "apk add openrc"))
}

async fn run(candidates: &[&'static str], args: &[&str]) -> Result<String, ApiError> {
    exec::run_root_program(tool(candidates, "OpenRC")?, args, TIMEOUT)
        .await
        .map(|output| output.stdout)
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
    fn validates_service_names() {
        assert!(validate_name("dbus").is_ok());
        assert!(validate_name("bad name").is_err());
        assert!(validate_name("../dbus").is_err());
    }
}
