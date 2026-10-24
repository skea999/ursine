//! network — `NetworkManager` devices, Wi-Fi and connections.

use std::path::Path;
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, ApiError, NetworkDevice, NetworkView, WifiNetwork};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const NMCLI: [&str; 2] = ["/usr/bin/nmcli", "/bin/nmcli"];
const TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(90);

pub async fn handle(action: &Action, _ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
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
        Action::DisconnectNetwork { device } => {
            validate_device(device)?;
            run(&["device", "disconnect", device]).await?;
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
    Ok(NetworkView {
        wifi_enabled: radio.trim() == "enabled",
        devices: parse_devices(&devices),
        wifi: parse_wifi(&wifi),
    })
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
            })
        })
        .collect()
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
    exec::run_root_program(tool()?, args, TIMEOUT)
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
        let wifi = parse_wifi("*:Home:80:WPA2\nNeighbour:40:WPA2\n");
        assert_eq!(wifi.len(), 2);
        assert!(wifi[0].active);
        assert_eq!(wifi[1].signal, 40);
    }

    #[test]
    fn validates_network_fields() {
        assert!(validate_ssid("Home Wi-Fi").is_ok());
        assert!(validate_ssid("bad\nssid").is_err());
        assert!(validate_device("wlan0").is_ok());
        assert!(validate_device("../wlan0").is_err());
    }
}
