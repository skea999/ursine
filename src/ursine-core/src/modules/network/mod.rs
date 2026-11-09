//! network — `NetworkManager` devices, Wi-Fi and connections.

use std::path::Path;
use std::time::Duration;

use serde_json::json;
use ursine_api::{
    Action, ApiError, NetworkDevice, NetworkIpv4Settings, NetworkProfile, NetworkView, WifiNetwork,
};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const NMCLI: [&str; 2] = ["/usr/bin/nmcli", "/bin/nmcli"];
const TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(90);

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    // nmcli rewrites/deletes NetworkManager keyfiles for these actions:
    // rotate the whole system-connections tree first (uniform slots; the
    // tree keeps its 0700/0600 permissions through the copy).
    let backup = || {
        crate::backup_configs::rotate_async(
            std::path::PathBuf::from("/etc/NetworkManager/system-connections"),
            crate::backup_configs::copies_from(&ctx.config),
            None,
        )
    };
    match action {
        Action::GetNetwork => Ok(json!(read_view().await?)),
        Action::SetWifiEnabled { enabled } => {
            run(&["radio", "wifi", if *enabled { "on" } else { "off" }]).await?;
            Ok(json!(read_view().await?))
        }
        Action::ConnectWifi { ssid, password } => {
            validate_ssid(ssid)?;
            if password.chars().any(|c| matches!(c, '\n' | '\r' | '\0')) {
                return Err(ApiError::invalid("invalid Wi-Fi password"));
            }
            exec::run_root_program_quoted_stdin(
                tool()?,
                &["--ask", "--wait", "30", "device", "wifi", "connect", ssid],
                CONNECT_TIMEOUT,
                password,
            )
            .await?;
            Ok(json!(read_view().await?))
        }
        Action::ConnectNetwork { device } => {
            validate_device(device)?;
            run(&["device", "connect", device]).await?;
            Ok(json!(read_view().await?))
        }
        Action::DisconnectNetwork { device } => {
            validate_device(device)?;
            run(&["device", "disconnect", device]).await?;
            Ok(json!(read_view().await?))
        }
        Action::ForgetNetwork { connection } => {
            validate_connection(connection)?;
            backup().await?;
            run(&["connection", "delete", connection]).await?;
            Ok(json!(read_view().await?))
        }
        Action::SetNetworkConnection { connection, up } => {
            validate_connection(connection)?;
            let verb = if *up { "up" } else { "down" };
            // `connection down` fails on an inactive profile; surface the
            // nmcli message instead of pretending it worked.
            run(&["connection", verb, connection]).await?;
            Ok(json!(read_view().await?))
        }
        Action::SetNetworkIpv4 {
            connection,
            method,
            addresses,
            gateway,
            dns,
        } => {
            backup().await?;
            set_ipv4(connection, method, addresses, gateway.as_deref(), dns).await?;
            Ok(json!(read_view().await?))
        }
        _ => Err(ApiError::invalid("action is not valid for module network")),
    }
}

async fn read_view() -> Result<NetworkView, ApiError> {
    let radio = run(&["radio", "wifi"]).await.unwrap_or_default();
    let devices = run(&[
        "--escape",
        "no",
        "-t",
        "-f",
        "DEVICE,TYPE,STATE,CONNECTION",
        "device",
        "status",
    ])
    .await?;
    let wifi = run(&[
        "--escape",
        "no",
        "-t",
        "-f",
        "IN-USE,SSID,SIGNAL,SECURITY",
        "device",
        "wifi",
        "list",
    ])
    .await?;
    let mut devices = parse_devices(&devices);
    for device in &mut devices {
        if let Ok(output) = run(&[
            "--escape",
            "no",
            "-t",
            "-f",
            "IP4.ADDRESS,IP4.GATEWAY,IP4.DNS",
            "device",
            "show",
            &device.device,
        ])
        .await
        {
            enrich_device(device, &output);
        }
        // Settings fields (`ipv4.method`, …) are only available per
        // name — nmcli refuses them for a blanket listing — so this is
        // one extra call per connected device, same class as the
        // `device show` enrich above.
        if let Some(connection) = device.connection.clone() {
            if let Ok(output) = run(&[
                "-t",
                "-f",
                "ipv4.method,ipv4.addresses,ipv4.gateway,ipv4.dns",
                "connection",
                "show",
                &connection,
            ])
            .await
            {
                device.ipv4_settings = parse_ipv4_settings(&output);
            }
        }
    }
    let profiles = run(&[
        "--escape",
        "no",
        "-t",
        "-f",
        "NAME,TYPE,DEVICE",
        "connection",
        "show",
    ])
    .await?;
    let profiles = parse_profiles(&profiles);
    Ok(NetworkView {
        wifi_enabled: radio.trim() == "enabled",
        devices,
        wifi: parse_wifi(&wifi),
        profiles,
    })
}

/// `nmcli -t -f NAME,TYPE,DEVICE connection show` rows; an empty
/// DEVICE column means the profile is inactive.
fn parse_profiles(output: &str) -> Vec<NetworkProfile> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, ':');
            let name = fields.next()?.to_string();
            let kind = fields.next()?.to_string();
            let device = fields.next().unwrap_or_default();
            if name.is_empty() || kind.is_empty() {
                return None;
            }
            Some(NetworkProfile {
                name,
                kind,
                active: !device.is_empty(),
            })
        })
        .collect()
}

fn parse_devices(output: &str) -> Vec<NetworkDevice> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(4, ':');
            let device = fields.next()?.to_string();
            let kind = fields.next()?.to_string();
            let state = fields.next()?.to_string();
            let connection = fields
                .next()
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            Some(NetworkDevice {
                device,
                kind,
                state,
                connection,
                ip4: Vec::new(),
                gateway: None,
                dns: Vec::new(),
                ipv4_settings: None,
            })
        })
        .collect()
}

fn enrich_device(device: &mut NetworkDevice, output: &str) {
    for line in output.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if key.starts_with("IP4.ADDRESS") && !value.is_empty() {
            device.ip4.push(value.to_string());
        } else if key == "IP4.GATEWAY" && !value.is_empty() {
            device.gateway = Some(value.to_string());
        } else if key.starts_with("IP4.DNS") && !value.is_empty() {
            device.dns.push(value.to_string());
        }
    }
}

fn parse_wifi(output: &str) -> Vec<WifiNetwork> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(4, ':');
            let active = fields.next()? == "*";
            let ssid = fields.next()?.to_string();
            let signal = fields.next()?.parse().unwrap_or(0);
            let security = fields.next().unwrap_or_default().to_string();
            if ssid.is_empty() {
                return None;
            }
            Some(WifiNetwork {
                ssid,
                signal,
                security,
                active,
            })
        })
        .collect()
}

fn validate_ssid(ssid: &str) -> Result<(), ApiError> {
    if ssid.is_empty() || ssid.len() > 128 || ssid.chars().any(|c| matches!(c, '\n' | '\r' | '\0'))
    {
        return Err(ApiError::invalid("invalid Wi-Fi SSID"));
    }
    Ok(())
}

fn validate_device(device: &str) -> Result<(), ApiError> {
    if device.is_empty()
        || device.len() > 64
        || !device
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
    {
        return Err(ApiError::invalid("invalid network device"));
    }
    Ok(())
}

/// Connection profile names: spaces are legal (`Home Wi-Fi`), control
/// characters and a leading `-` (nmcli flag injection) are not. The
/// exec guard additionally rejects `'` (quoted-value rule).
fn validate_connection(connection: &str) -> Result<(), ApiError> {
    if connection.is_empty()
        || connection.len() > 128
        || connection.starts_with('-')
        || connection.chars().any(|c| {
            // `:` is the terse-output column separator: a name containing
            // it would misparse in every `NAME,…` listing.
            c.is_control() || matches!(c, '\\' | '"' | '\'' | ':')
        })
    {
        return Err(ApiError::invalid("invalid connection name"));
    }
    Ok(())
}

/// Strict dotted-quad IPv4 (no prefix).
fn parse_ipv4(value: &str) -> Result<(), ApiError> {
    let octets: Vec<&str> = value.split('.').collect();
    if octets.len() != 4 {
        return Err(ApiError::invalid(format!("invalid IPv4 address: {value}")));
    }
    for octet in octets {
        if octet.is_empty()
            || octet.len() > 3
            || !octet.chars().all(|c| c.is_ascii_digit())
            || octet.parse::<u16>().ok().is_none_or(|n| n > 255)
        {
            return Err(ApiError::invalid(format!("invalid IPv4 address: {value}")));
        }
    }
    Ok(())
}

/// IPv4 with CIDR prefix (`192.168.1.5/24`).
fn parse_cidr(value: &str) -> Result<(), ApiError> {
    let Some((address, prefix)) = value.split_once('/') else {
        return Err(ApiError::invalid(format!("invalid IPv4 CIDR: {value}")));
    };
    parse_ipv4(address)?;
    let Ok(prefix) = prefix.parse::<u8>() else {
        return Err(ApiError::invalid(format!("invalid IPv4 CIDR: {value}")));
    };
    if prefix > 32 {
        return Err(ApiError::invalid(format!("invalid IPv4 CIDR: {value}")));
    }
    Ok(())
}

/// `nmcli -t -f ipv4.method,… connection show <name>` → settings.
fn parse_ipv4_settings(output: &str) -> Option<NetworkIpv4Settings> {
    let mut method = None;
    let mut addresses = Vec::new();
    let mut gateway = None;
    let mut dns = Vec::new();
    for line in output.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key {
            "ipv4.method" => method = Some(value.to_string()),
            "ipv4.addresses" => {
                addresses = value
                    .split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            "ipv4.gateway" => {
                if !value.is_empty() {
                    gateway = Some(value.to_string());
                }
            }
            "ipv4.dns" => {
                dns = value.split_whitespace().map(str::to_string).collect();
            }
            _ => {}
        }
    }
    Some(NetworkIpv4Settings {
        method: method?,
        addresses,
        gateway,
        dns,
    })
}

/// Apply an IPv4 method to a profile; re-activates it when it is the
/// active connection of some device.
/// Validate an IPv4 request and build the `nmcli connection modify`
/// argv (without the leading `nmcli`). Split out so the argv — including
/// the empty strings that clear properties — is unit-testable.
fn build_ipv4_modify(
    connection: &str,
    method: &str,
    addresses: &[String],
    gateway: Option<&str>,
    dns: &[String],
) -> Result<Vec<String>, ApiError> {
    validate_connection(connection)?;
    // An empty gateway from a form means "unset", not "set to nothing".
    let gateway = gateway.filter(|value| !value.is_empty());
    let mut modify: Vec<String> = vec!["connection".into(), "modify".into(), connection.into()];
    match method {
        "dhcp" => {
            if !addresses.is_empty() || gateway.is_some() || !dns.is_empty() {
                return Err(ApiError::invalid("dhcp takes no addresses, gateway or dns"));
            }
            modify.extend(
                [
                    "ipv4.method",
                    "auto",
                    "ipv4.addresses",
                    "",
                    "ipv4.gateway",
                    "",
                    "ipv4.dns",
                    "",
                ]
                .map(String::from),
            );
        }
        "static" => {
            if addresses.is_empty() {
                return Err(ApiError::invalid("static IPv4 needs at least one address"));
            }
            if addresses.len() > 8 || dns.len() > 4 {
                return Err(ApiError::invalid("too many addresses or dns servers"));
            }
            for address in addresses {
                parse_cidr(address)?;
            }
            if let Some(gateway) = gateway {
                parse_ipv4(gateway)?;
            }
            for server in dns {
                parse_ipv4(server)?;
            }
            modify.extend(
                [
                    "ipv4.method",
                    "manual",
                    "ipv4.addresses",
                    &addresses.join(","),
                    "ipv4.gateway",
                    gateway.unwrap_or(""),
                    "ipv4.dns",
                    &dns.join(" "),
                ]
                .map(String::from),
            );
        }
        other => {
            return Err(ApiError::invalid(format!(
                "unknown IPv4 method {other:?} (dhcp or static)"
            )));
        }
    }
    Ok(modify)
}

async fn set_ipv4(
    connection: &str,
    method: &str,
    addresses: &[String],
    gateway: Option<&str>,
    dns: &[String],
) -> Result<(), ApiError> {
    let modify = build_ipv4_modify(connection, method, addresses, gateway, dns)?;
    let args: Vec<&str> = modify.iter().map(String::as_str).collect();
    run(&args).await?;
    // Apply now when the profile is active; an inactive profile only
    // stores the settings for the next activation.
    let active = run(&[
        "--escape",
        "no",
        "-t",
        "-f",
        "NAME",
        "connection",
        "show",
        "--active",
    ])
    .await?
    .lines()
    .any(|line| line.trim() == connection);
    if active {
        run(&["connection", "up", connection]).await?;
    }
    Ok(())
}

fn tool() -> Result<&'static str, ApiError> {
    NMCLI
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| {
            ApiError::unavailable("NetworkManager is not installed", "apk add networkmanager")
        })
}

async fn run(args: &[&str]) -> Result<String, ApiError> {
    // Direct exec, no shell: the quoted-value guard keeps spaces legal
    // (SSIDs and connection names such as `Home Wi-Fi`).
    exec::run_root_program_value(tool()?, args, TIMEOUT)
        .await
        .map(|output| output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_networkmanager_output() {
        let devices = parse_devices("wlan0:wifi:connected:Home\neth0:ethernet:disconnected:\n");
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].connection.as_deref(), Some("Home"));
        let wifi = parse_wifi("*:Home:80:WPA2\n:Neighbour:40:WPA2\n");
        assert_eq!(wifi.len(), 2);
        assert!(wifi[0].active);
        assert_eq!(wifi[1].signal, 40);
    }

    #[test]
    fn parses_saved_profiles() {
        let profiles = parse_profiles(concat!(
            "eth0:802-3-ethernet:eth0\n",
            "Home Wi-Fi:802-11-wireless:wlan0\n",
            "niuma:wireguard:\n",
            "vpn work:vpn:\n",
        ));
        assert_eq!(profiles.len(), 4);
        assert!(profiles[0].active);
        assert_eq!(profiles[1].name, "Home Wi-Fi");
        assert!(profiles[1].active);
        assert!(!profiles[2].active);
        assert_eq!(profiles[3].kind, "vpn");
    }

    #[test]
    fn validates_network_fields() {
        assert!(validate_ssid("Home Wi-Fi").is_ok());
        assert!(validate_ssid("bad\nssid").is_err());
        assert!(validate_device("wlan0").is_ok());
        assert!(validate_device("../wlan0").is_err());
        assert!(validate_connection("Home Wi-Fi").is_ok());
        assert!(validate_connection("Wired connection 1").is_ok());
        assert!(validate_connection("").is_err());
        assert!(validate_connection("-evil").is_err());
        assert!(validate_connection("bad\"name").is_err());
        assert!(validate_connection("a:b").is_err());
        assert!(validate_connection("Bob's Net").is_err());
    }

    #[test]
    fn validates_ipv4_inputs() {
        assert!(parse_ipv4("192.168.1.1").is_ok());
        assert!(parse_ipv4("0.0.0.0").is_ok());
        assert!(parse_ipv4("256.1.1.1").is_err());
        assert!(parse_ipv4("1.2.3").is_err());
        assert!(parse_ipv4("1.2.3.4.5").is_err());
        assert!(parse_cidr("192.168.1.5/24").is_ok());
        assert!(parse_cidr("192.168.1.5/33").is_err());
        assert!(parse_cidr("192.168.1.5").is_err());
    }

    #[test]
    fn builds_modify_argv_including_clears() {
        let dhcp = build_ipv4_modify("Home Wi-Fi", "dhcp", &[], None, &[]).unwrap();
        assert_eq!(
            dhcp,
            [
                "connection",
                "modify",
                "Home Wi-Fi",
                "ipv4.method",
                "auto",
                "ipv4.addresses",
                "",
                "ipv4.gateway",
                "",
                "ipv4.dns",
                "",
            ]
        );
        // The empty strings survive the argv guard (they clear values).
        for arg in &dhcp {
            assert!(
                arg.is_empty() || !arg.chars().any(char::is_control),
                "{arg:?}"
            );
        }
        let static_ip = build_ipv4_modify(
            "eth0",
            "static",
            &["192.168.1.5/24".to_string()],
            Some("192.168.1.1"),
            &["1.1.1.1".to_string()],
        )
        .unwrap();
        assert_eq!(
            static_ip,
            [
                "connection",
                "modify",
                "eth0",
                "ipv4.method",
                "manual",
                "ipv4.addresses",
                "192.168.1.5/24",
                "ipv4.gateway",
                "192.168.1.1",
                "ipv4.dns",
                "1.1.1.1",
            ]
        );
        assert!(build_ipv4_modify("eth0", "dhcp", &["1.2.3.4/24".into()], None, &[]).is_err());
        assert!(build_ipv4_modify("eth0", "static", &[], None, &[]).is_err());
        assert!(build_ipv4_modify("eth0", "weird", &[], None, &[]).is_err());
        assert!(build_ipv4_modify("-evil", "dhcp", &[], None, &[]).is_err());
        assert!(build_ipv4_modify("eth0", "static", &["bad".into()], None, &[]).is_err());
    }

    #[test]
    fn parses_ipv4_settings_terse_output() {
        // Captured from `nmcli -t -f ipv4.method,… connection show eth0`.
        let settings = parse_ipv4_settings(concat!(
            "ipv4.method:manual\n",
            "ipv4.addresses:192.168.111.132/24,10.0.0.5/24\n",
            "ipv4.gateway:192.168.111.1\n",
            "ipv4.dns:192.168.111.1 1.1.1.1\n",
        ))
        .unwrap();
        assert_eq!(settings.method, "manual");
        assert_eq!(settings.addresses, ["192.168.111.132/24", "10.0.0.5/24"]);
        assert_eq!(settings.gateway.as_deref(), Some("192.168.111.1"));
        assert_eq!(settings.dns, ["192.168.111.1", "1.1.1.1"]);
        // Empty values (internal has no gateway/dns).
        let settings =
            parse_ipv4_settings("ipv4.method:auto\nipv4.addresses:\nipv4.gateway:\nipv4.dns:\n")
                .unwrap();
        assert_eq!(settings.method, "auto");
        assert!(settings.addresses.is_empty());
        assert!(settings.gateway.is_none());
        assert!(settings.dns.is_empty());
        assert!(parse_ipv4_settings("").is_none());
    }
}
