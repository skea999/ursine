//! scanners — SANE discovery and managed airscan devices.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, AirscanDevice, ApiError, Scanner, ScannerConfigView};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const SCANIMAGE: [&str; 3] = [
    "/usr/bin/scanimage",
    "/usr/local/bin/scanimage",
    "/bin/scanimage",
];
const DEFAULT_CONFIG: &str = "/etc/sane.d/airscan.conf";
const TIMEOUT: Duration = Duration::from_secs(120);

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::GetScannerConfig => Ok(json!(read_view(ctx).await?)),
        Action::SetScannerDevice {
            name,
            address,
            protocol,
        } => {
            validate_name(name)?;
            validate_address(address)?;
            validate_protocol(protocol)?;
            let mut config = read_config()?;
            config.devices.retain(|device| device.name != *name);
            config.devices.push(AirscanDevice {
                name: name.clone(),
                address: address.clone(),
                protocol: protocol.clone(),
            });
            write_config(&config)?;
            Ok(json!(read_view(ctx).await?))
        }
        Action::RemoveScannerDevice { name } => {
            validate_name(name)?;
            let mut config = read_config()?;
            config.devices.retain(|device| device.name != *name);
            write_config(&config)?;
            Ok(json!(read_view(ctx).await?))
        }
        Action::SetScannerDiscovery { enabled } => {
            let mut config = read_config()?;
            config.discovery_disabled = !enabled;
            write_config(&config)?;
            Ok(json!(read_view(ctx).await?))
        }
        _ => Err(ApiError::invalid("action is not valid for module scanners")),
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct AirscanConfig {
    devices: Vec<AirscanDevice>,
    discovery_disabled: bool,
}

async fn read_view(ctx: &ModuleCtx<'_>) -> Result<ScannerConfigView, ApiError> {
    let config = read_config()?;
    let path = config_path();
    Ok(ScannerConfigView {
        path: path.display().to_string(),
        exists: path.exists(),
        discovery_disabled: config.discovery_disabled,
        devices: config.devices,
        detected: detect(ctx.as_user).await?,
    })
}

fn config_path() -> PathBuf {
    std::env::var_os("URSINE_AIRSCAN_CONFIG")
        .map_or_else(|| PathBuf::from(DEFAULT_CONFIG), PathBuf::from)
}

async fn detect(user: &str) -> Result<Vec<Scanner>, ApiError> {
    let program = tool(&SCANIMAGE, "scanimage")?;
    let output = exec::run_as_user_argv(user, program, &["-L"], TIMEOUT).await?;
    Ok(parse_devices(&output.stdout))
}

fn parse_devices(output: &str) -> Vec<Scanner> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("device `")?;
            let (id, rest) = rest.split_once('\'')?;
            Some(Scanner {
                id: id.to_string(),
                description: rest
                    .strip_prefix(" is a ")
                    .unwrap_or(rest)
                    .trim()
                    .to_string(),
            })
        })
        .collect()
}

fn read_config() -> Result<AirscanConfig, ApiError> {
    let path = config_path();
    if !path.exists() {
        return Ok(AirscanConfig::default());
    }
    let meta = std::fs::symlink_metadata(&path)
        .map_err(|e| ApiError::internal(format!("inspect airscan config: {e}")))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(ApiError::invalid("airscan config is not a regular file"));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| ApiError::internal(format!("read airscan config: {e}")))?;
    Ok(parse_config(&text))
}

fn parse_config(text: &str) -> AirscanConfig {
    let mut config = AirscanConfig::default();
    let mut section = "";
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            section = line;
            continue;
        }
        if section == "[devices]" {
            if let Some((name, value)) = line.split_once('=') {
                let name = name.trim().trim_matches('"').to_string();
                let mut parts = value.splitn(2, ',');
                let address = parts.next().unwrap_or_default().trim().to_string();
                let protocol = parts.next().unwrap_or("WSD").trim().to_string();
                if !name.is_empty() && !address.is_empty() {
                    config.devices.push(AirscanDevice {
                        name,
                        address,
                        protocol,
                    });
                }
            }
        } else if section == "[options]"
            && line
                .replace(' ', "")
                .eq_ignore_ascii_case("discovery=disable")
        {
            config.discovery_disabled = true;
        }
    }
    config
}

fn write_config(config: &AirscanConfig) -> Result<(), ApiError> {
    let path = config_path();
    let parent = path
        .parent()
        .ok_or_else(|| ApiError::invalid("airscan config has no parent"))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| ApiError::internal(format!("create sane config dir: {e}")))?;
    if path.exists() {
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|e| ApiError::internal(format!("inspect airscan config: {e}")))?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(ApiError::invalid("airscan config is not a regular file"));
        }
        let backup = path.with_file_name(format!(".airscan.conf.ursine-backup-{}", nanos()));
        std::fs::copy(&path, &backup)
            .map_err(|e| ApiError::internal(format!("backup airscan config: {e}")))?;
    }
    let temp = path.with_file_name(format!(
        ".airscan.conf.ursine-{}-{}",
        std::process::id(),
        nanos()
    ));
    let text = render_config(config);
    std::fs::write(&temp, text)
        .map_err(|e| ApiError::internal(format!("write airscan temp: {e}")))?;
    std::fs::rename(&temp, &path)
        .map_err(|e| ApiError::internal(format!("replace airscan config: {e}")))?;
    Ok(())
}

fn render_config(config: &AirscanConfig) -> String {
    let mut text = String::from("[devices]\n");
    for device in &config.devices {
        text.push_str(&format!(
            "\"{}\" = {}, {}\n",
            device.name, device.address, device.protocol
        ));
    }
    text.push_str("\n[options]\n");
    if config.discovery_disabled {
        text.push_str("discovery = disable\n");
    }
    text
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty()
        || name.len() > 128
        || name.chars().any(|c| matches!(c, '"' | '\n' | '\r' | '\0'))
    {
        return Err(ApiError::invalid("invalid scanner name"));
    }
    Ok(())
}

fn validate_address(address: &str) -> Result<(), ApiError> {
    let valid = address.starts_with("http://") || address.starts_with("https://");
    if !valid || address.len() > 2048 || address.chars().any(char::is_control) {
        return Err(ApiError::invalid("scanner address must be an http(s) URL"));
    }
    Ok(())
}

fn validate_protocol(protocol: &str) -> Result<(), ApiError> {
    if !matches!(protocol, "WSD" | "eSCL") {
        return Err(ApiError::invalid("scanner protocol must be WSD or eSCL"));
    }
    Ok(())
}

fn tool(candidates: &[&'static str], label: &str) -> Result<&'static str, ApiError> {
    candidates
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| ApiError::unavailable(format!("{label} not found"), "apk add sane-utils"))
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_airscan_config() {
        let config = parse_config(
            "[devices]\n\"Brother MFC-1910W\" = http://192.168.1.2:80/WebServices/ScannerService, WSD\n\n[options]\ndiscovery = disable\n",
        );
        assert_eq!(config.devices.len(), 1);
        assert_eq!(config.devices[0].protocol, "WSD");
        assert!(config.discovery_disabled);
    }

    #[test]
    fn renders_airscan_config() {
        let config = AirscanConfig {
            devices: vec![AirscanDevice {
                name: "Office".into(),
                address: "https://scanner.local/eSCL".into(),
                protocol: "eSCL".into(),
            }],
            discovery_disabled: true,
        };
        let text = render_config(&config);
        assert!(text.contains("\"Office\" = https://scanner.local/eSCL, eSCL"));
        assert!(text.contains("discovery = disable"));
    }

    #[test]
    fn validates_manual_fields() {
        assert!(validate_name("Brother MFC").is_ok());
        assert!(validate_name("bad\nname").is_err());
        assert!(validate_address("http://scanner.local/eSCL").is_ok());
        assert!(validate_address("file:///etc/passwd").is_err());
        assert!(validate_protocol("WSD").is_ok());
        assert!(validate_protocol("raw").is_err());
    }
}
