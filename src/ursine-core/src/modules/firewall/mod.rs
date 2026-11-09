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

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    let copies = crate::backup_configs::copies_from(&ctx.config);
    // Every mutation below is `firewall-cmd --permanent`: rotate the
    // whole /etc/firewalld tree first (uniform slot backups of the XML
    // files the CLI rewrites).
    let backup = || {
        crate::backup_configs::rotate_async(
            std::path::PathBuf::from("/etc/firewalld"),
            copies,
            None,
        )
    };
    match action {
        Action::GetFirewall => Ok(json!(read_status().await?)),
        Action::SetDefaultFirewallZone { zone } => {
            validate_zone(zone)?;
            backup().await?;
            run(&[&format!("--set-default-zone={zone}"), "--permanent"]).await?;
            run(&["--reload"]).await?;
            Ok(json!(read_status().await?))
        }
        Action::AddFirewallService { zone, service } => {
            validate_zone(zone)?;
            validate_service(service)?;
            backup().await?;
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
            backup().await?;
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
            backup().await?;
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
            backup().await?;
            run(&[
                &format!("--zone={zone}"),
                &format!("--remove-port={port}"),
                "--permanent",
            ])
            .await?;
            run(&["--reload"]).await?;
            Ok(json!(read_status().await?))
        }
        Action::SetFirewallEnabled { enabled } => {
            let status = read_status().await?;
            if !status.running {
                return Err(ApiError::unavailable(
                    "firewalld is not running",
                    "rc-service firewalld start",
                ));
            }
            if !valid_zone(&status.default_zone) {
                return Err(ApiError::internal(
                    "firewalld reported an invalid default zone",
                ));
            }
            let target = if *enabled { "REJECT" } else { "ACCEPT" };
            backup().await?;
            run(&[
                &format!("--zone={}", status.default_zone),
                &format!("--set-target={target}"),
                "--permanent",
            ])
            .await?;
            run(&["--reload"]).await?;
            Ok(json!(read_status().await?))
        }
        Action::CreateFirewallService { service, ports } => {
            validate_service(service)?;
            for port in ports {
                validate_port(port)?;
            }
            backup().await?;
            run(&[&format!("--new-service={service}"), "--permanent"]).await?;
            for port in ports {
                run(&[
                    &format!("--service={service}"),
                    &format!("--add-port={port}"),
                    "--permanent",
                ])
                .await?;
            }
            run(&["--reload"]).await?;
            Ok(json!(read_status().await?))
        }
        Action::DeleteFirewallService { service } => {
            validate_service(service)?;
            backup().await?;
            run(&[&format!("--delete-service={service}"), "--permanent"]).await?;
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
        // One call carries target/services/ports/interfaces; rich rules
        // get their own list (values contain spaces).
        let list_all = run(&[&format!("--zone={name}"), "--list-all"]).await?;
        let rich_rules = run(&[&format!("--zone={name}"), "--list-rich-rules"])
            .await?
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        let mut zone = FirewallZone {
            name: name.to_string(),
            default: name == default_zone,
            services: Vec::new(),
            ports: Vec::new(),
            target: String::new(),
            interfaces: Vec::new(),
            rich_rules,
        };
        parse_list_all(&list_all, &mut zone);
        zones.push(zone);
    }
    let default_target = zones
        .iter()
        .find(|zone| zone.default)
        .map(|zone| zone.target.clone())
        .unwrap_or_default();
    let enabled = !default_target.eq_ignore_ascii_case("ACCEPT");
    let custom_services = list_custom_services().await?;
    Ok(FirewallStatus {
        running,
        default_zone,
        zones,
        enabled,
        custom_services,
    })
}

/// Parse `firewall-cmd --list-all` into the zone (values are
/// space-separated tokens; a key line is `  key: value`).
fn parse_list_all(output: &str, zone: &mut FirewallZone) {
    for line in output.lines() {
        let trimmed = line.trim();
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "target" => zone.target = value.to_string(),
            "services" => {
                zone.services = value.split_whitespace().map(str::to_string).collect();
            }
            "ports" => {
                zone.ports = value.split_whitespace().map(str::to_string).collect();
            }
            "interfaces" => {
                zone.interfaces = value.split_whitespace().map(str::to_string).collect();
            }
            _ => {}
        }
    }
}

/// Service definitions that live under `/etc/firewalld` (custom) as
/// opposed to the distro's `/usr/lib/firewalld` predefined set. One
/// directory listing instead of a `--path-service` call per service.
async fn list_custom_services() -> Result<Vec<String>, ApiError> {
    let outcome = exec::run_root_program("/bin/ls", &["/etc/firewalld/services"], TIMEOUT).await;
    let listing = match outcome {
        Ok(output) => output.stdout,
        // No directory means no custom services (fresh install).
        Err(_) => String::new(),
    };
    let mut custom: Vec<String> = listing
        .lines()
        .filter_map(|line| line.trim().strip_suffix(".xml"))
        .filter(|name| valid_service(name))
        .map(str::to_string)
        .collect();
    custom.sort();
    Ok(custom)
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

fn valid_service(service: &str) -> bool {
    !service.is_empty()
        && service.len() <= 64
        && service
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

fn validate_service(service: &str) -> Result<(), ApiError> {
    if valid_service(service) {
        Ok(())
    } else {
        Err(ApiError::invalid("invalid firewall service"))
    }
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
    fn parses_list_all_into_zone() {
        let mut zone = FirewallZone {
            name: "public".into(),
            default: true,
            rich_rules: vec!["rule family=ipv4 drop".into()],
            ..FirewallZone::default()
        };
        parse_list_all(
            concat!(
                "public (active)\n",
                "  target: default\n",
                "  services: ssh dhcpv6-client\n",
                "  ports: 8080/tcp 443/tcp\n",
                "  interfaces: eth0\n",
                "  sources:\n",
                "  rich rules:\n",
            ),
            &mut zone,
        );
        assert_eq!(zone.target, "default");
        assert_eq!(zone.services, ["ssh", "dhcpv6-client"]);
        assert_eq!(zone.ports, ["8080/tcp", "443/tcp"]);
        assert_eq!(zone.interfaces, ["eth0"]);
        // list-all does not carry rich rules; they stay untouched here.
        assert_eq!(zone.rich_rules, ["rule family=ipv4 drop"]);
        // ACCEPT = firewall off for the default zone.
        let mut zone = FirewallZone::default();
        parse_list_all("target: ACCEPT\nports:\n", &mut zone);
        assert_eq!(zone.target, "ACCEPT");
        assert!(zone.ports.is_empty());
    }

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
