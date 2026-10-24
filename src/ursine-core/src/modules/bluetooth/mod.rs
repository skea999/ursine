//! bluetooth — `BlueZ` adapter and device setup.

use std::path::Path;
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, ApiError, BluetoothAdapter, BluetoothDevice, BluetoothOp, BluetoothView};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const BLUETOOTHCTL: [&str; 2] = ["/usr/bin/bluetoothctl", "/bin/bluetoothctl"];
const TIMEOUT: Duration = Duration::from_secs(30);
const SCAN_TIMEOUT: Duration = Duration::from_secs(20);

pub async fn handle(action: &Action, _ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::GetBluetooth => Ok(json!(read_view().await?)),
        Action::SetBluetoothPower { on } => {
            run(&["power", if *on { "on" } else { "off" }]).await?;
            Ok(json!(read_view().await?))
        }
        Action::ScanBluetooth => {
            run(&["--timeout", "8", "scan", "on"]).await?;
            Ok(json!(read_view().await?))
        }
        Action::BluetoothAction { address, op } => {
            validate_address(address)?;
            let args = match op {
                BluetoothOp::Pair => vec!["--agent", "NoInputNoOutput", "pair", address.as_str()],
                BluetoothOp::Connect => vec!["connect", address.as_str()],
                BluetoothOp::Disconnect => vec!["disconnect", address.as_str()],
                BluetoothOp::Trust => vec!["trust", address.as_str()],
                BluetoothOp::Untrust => vec!["untrust", address.as_str()],
                BluetoothOp::Forget => vec!["remove", address.as_str()],
            };
            run(&args).await?;
            Ok(json!(read_view().await?))
        }
        _ => Err(ApiError::invalid(
            "action is not valid for module bluetooth",
        )),
    }
}

async fn read_view() -> Result<BluetoothView, ApiError> {
    let show = run(&["show"]).await.unwrap_or_default();
    let adapter = parse_adapter(&show);
    let devices = run(&["devices"]).await?;
    let mut rows = Vec::new();
    for line in devices.lines().take(32) {
        let mut fields = line.splitn(3, ' ');
        let (Some("Device"), Some(address), Some(name)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if !valid_address(address) {
            continue;
        }
        let info = run(&["info", address]).await.unwrap_or_default();
        rows.push(BluetoothDevice {
            address: address.to_string(),
            name: name.to_string(),
            connected: info.contains("Connected: yes"),
            paired: info.contains("Paired: yes"),
            trusted: info.contains("Trusted: yes"),
        });
    }
    Ok(BluetoothView {
        adapter,
        devices: rows,
    })
}

fn parse_adapter(output: &str) -> BluetoothAdapter {
    let address = output
        .lines()
        .find_map(|line| line.strip_prefix("Controller "))
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or_default()
        .to_string();
    let name = output
        .lines()
        .find_map(|line| line.trim().strip_prefix("Name:"))
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    BluetoothAdapter {
        address,
        name,
        powered: output.contains("Powered: yes"),
        discoverable: output.contains("Discoverable: yes"),
    }
}

fn valid_address(address: &str) -> bool {
    address.len() == 17
        && address.chars().enumerate().all(|(index, c)| {
            if matches!(index, 2 | 5 | 8 | 11 | 14) {
                c == ':'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn validate_address(address: &str) -> Result<(), ApiError> {
    if valid_address(address) {
        Ok(())
    } else {
        Err(ApiError::invalid("invalid Bluetooth address"))
    }
}

async fn run(args: &[&str]) -> Result<String, ApiError> {
    let program = BLUETOOTHCTL
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| ApiError::unavailable("bluetoothctl not found", "apk add bluez"))?;
    exec::run_root_program(
        program,
        args,
        if args.contains(&"scan") {
            SCAN_TIMEOUT
        } else {
            TIMEOUT
        },
    )
    .await
    .map(|output| output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_adapter_and_addresses() {
        let adapter = parse_adapter("Controller AA:BB:CC:DD:EE:FF (Public)\n Name: Office\n Powered: yes\n Discoverable: no\n");
        assert_eq!(adapter.address, "AA:BB:CC:DD:EE:FF");
        assert!(adapter.powered);
        assert!(valid_address("AA:BB:CC:DD:EE:FF"));
        assert!(!valid_address("AA:BB:CC:DD:EE"));
        assert!(validate_address("../etc/passwd").is_err());
    }
}
