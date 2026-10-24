//! firewall — firewalld zones and rules.

use std::path::Path;
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, ApiError, FirewallStatus, FirewallZone};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const FIREWALL_CMD: [&str; 3] = [
    "/usr/bin/firewall-cmd",
    "/usr/sbin/firewall-cmd",
    "/bin/firewall-cmd",
];
const TIMEOUT: Duration = Duration::from_secs(120);

pub async fn handle(action: &Action, _ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::GetFirewall => Ok(json!(read_status().await?)),
        Action::SetDefaultFirewallZone { zone } => {
            validate_zone(zone)?;
            run(&[&format!("--set-default-zone={zone}"), "--permanent"]).await?;
            run(&["--reload"]).await?;
            Ok(json!(read_status().await?))
        }
        Action::AddFirewallService { zone, service } => {
            validate_zone(zone)?;
            validate_service(service)?;
            run(&[
                &format!("--zone={zone}"),
                &format!("--add-service={service}"),
                "--permanent",
            ])
            .await?;
            run(&["--reload"]).await?;
            Ok(json!(read_status().await?))
        }
        Action::RemoveFirewallService { zone, service } => {
            validate_zone(zone)?;
            validate_service(service)?;
            run(&[
                &format!("--zone={zone}"),
                &format!("--remove-service={service}"),
                "--permanent",
            ])
            .await?;
            run(&["--reload"]).await?;
            Ok(json!(read_status().await?))
        }
        Action::AddFirewallPort { zone, port } => {
            validate_zone(zone)?;
            validate_port(port)?;
            run(&[
                &format!("--zone={zone}"),
                &format!("--add-port={port}"),
                "--permanent",
            ])
            .await?;
            run(&["--reload"]).await?;
            Ok(json!(read_status().await?))
        }
        Action::RemoveFirewallPort { zone, port } => {
            validate_zone(zone)?;
            validate_port(port)?;
            run(&[
                &format!("--zone={zone}"),
                &format!("--remove-port={port}"),
                "--permanent",
            ])
            .await?;
            run(&["--reload"]).await?;
            Ok(json!(read_status().await?))
        }
        Action::ReloadFirewall => {
            run(&["--reload"]).await?;
            Ok(json!(read_status().await?))
        }
        _ => Err(ApiError::invalid("action is not valid for module firewall")),
    }
}

async fn read_status() -> Result<FirewallStatus, ApiError> {
    let running = run(&["--state"])
        .await
        .is_ok_and(|state| state.trim() == "running");
    if !running {
        return Ok(FirewallStatus::default());
    }
    let default_zone = run(&["--get-default-zone"]).await?.trim().to_string();
    let zones_raw = run(&["--get-zones"]).await?;
    let mut zones = Vec::new();
    for name in zones_raw.split_whitespace() {
        if !valid_zone(name) {
            continue;
        }
        let services = run(&[&format!("--zone={name}"), "--list-services"])
            .await?
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let ports = run(&[&format!("--zone={name}"), "--list-ports"])
            .await?
            .split_whitespace()
            .map(str::to_string)
            .collect();
        zones.push(FirewallZone {
            name: name.to_string(),
            default: name == default_zone,
            services,
            ports,
        });
    }
    Ok(FirewallStatus {
        running,
        default_zone,
        zones,
    })
}

fn valid_zone(zone: &str) -> bool {
    !zone.is_empty()
        && zone.len() <= 64
        && zone
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

fn validate_zone(zone: &str) -> Result<(), ApiError> {
    if valid_zone(zone) {
        Ok(())
    } else {
        Err(ApiError::invalid("invalid firewall zone"))
    }
}

fn validate_service(service: &str) -> Result<(), ApiError> {
    if service.is_empty()
        || service.len() > 64
        || !service
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    {
        return Err(ApiError::invalid("invalid firewall service"));
    }
    Ok(())
}

fn validate_port(port: &str) -> Result<(), ApiError> {
    let Some((number, protocol)) = port.split_once('/') else {
        return Err(ApiError::invalid("port must be number/tcp or number/udp"));
    };
    let number: u16 = number
        .parse()
        .map_err(|_| ApiError::invalid("invalid port number"))?;
    if number == 0 || !matches!(protocol, "tcp" | "udp" | "sctp" | "dccp") {
        return Err(ApiError::invalid("invalid port or protocol"));
    }
    Ok(())
}

fn tool() -> Result<&'static str, ApiError> {
    FIREWALL_CMD
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| ApiError::unavailable("firewalld is not installed", "apk add firewalld"))
}

async fn run(args: &[&str]) -> Result<String, ApiError> {
    exec::run_root_program(tool()?, args, TIMEOUT)
        .await
        .map(|output| output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_firewall_inputs() {
        assert!(validate_zone("public").is_ok());
        assert!(validate_zone("bad zone").is_err());
        assert!(validate_service("https").is_ok());
        assert!(validate_service("bad service").is_err());
        assert!(validate_port("8080/tcp").is_ok());
        assert!(validate_port("0/tcp").is_err());
        assert!(validate_port("8080").is_err());
    }
}
