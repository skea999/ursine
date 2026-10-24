//! scanners — SANE one-page scans to the user's Pictures directory.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, ApiError, ScanResult, Scanner};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const SCANIMAGE: [&str; 3] = [
    "/usr/bin/scanimage",
    "/usr/local/bin/scanimage",
    "/bin/scanimage",
];
const MKDIR: [&str; 3] = ["/bin/mkdir", "/usr/bin/mkdir", "/sbin/mkdir"];
const TIMEOUT: Duration = Duration::from_secs(300);

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListScanners => Ok(json!({"scanners": list(ctx.as_user).await?})),
        Action::ScanDocument {
            device,
            resolution,
            mode,
            source,
        } => Ok(json!(
            scan(ctx.as_user, device, *resolution, mode, source).await?
        )),
        _ => Err(ApiError::invalid("action is not valid for module scanners")),
    }
}

async fn list(user: &str) -> Result<Vec<Scanner>, ApiError> {
    let program = tool(&SCANIMAGE, "scanimage")?;
    let output = exec::run_as_user_argv(user, program, &["-L"], TIMEOUT).await?;
    Ok(parse_devices(&output.stdout))
}

async fn scan(
    user: &str,
    device: &str,
    resolution: u32,
    mode: &str,
    source: &str,
) -> Result<ScanResult, ApiError> {
    validate_device(device)?;
    if !matches!(resolution, 75 | 100 | 150 | 200 | 300 | 600 | 1200) {
        return Err(ApiError::invalid("unsupported scan resolution"));
    }
    if !matches!(mode, "Color" | "Gray" | "Lineart") {
        return Err(ApiError::invalid("unsupported scan mode"));
    }
    validate_source(source)?;
    let home = home_for_user(user)?;
    let directory = home.join("Pictures");
    let mkdir = tool(&MKDIR, "mkdir")?;
    exec::run_as_user_argv(user, mkdir, &["-p", &directory.to_string_lossy()], TIMEOUT).await?;
    let path = directory.join(format!("ursine-scan-{}.png", nanos()));
    let resolution = resolution.to_string();
    let path_text = path.to_string_lossy();
    let scanimage = tool(&SCANIMAGE, "scanimage")?;
    exec::run_as_user_argv(
        user,
        scanimage,
        &[
            "--device-name",
            device,
            "--resolution",
            &resolution,
            "--mode",
            mode,
            "--source",
            source,
            "--format=png",
            "--output-file",
            &path_text,
        ],
        TIMEOUT,
    )
    .await?;
    Ok(ScanResult {
        path: path.display().to_string(),
    })
}

fn parse_devices(output: &str) -> Vec<Scanner> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("device `")?;
            let (id, rest) = rest.split_once('\'')?;
            let description = rest
                .strip_prefix(" is a ")
                .unwrap_or(rest)
                .trim()
                .to_string();
            Some(Scanner {
                id: id.to_string(),
                description,
            })
        })
        .collect()
}

fn validate_device(device: &str) -> Result<(), ApiError> {
    if device.is_empty() || device.len() > 512 || device.starts_with('-') {
        return Err(ApiError::invalid("invalid scanner device"));
    }
    Ok(())
}

fn validate_source(source: &str) -> Result<(), ApiError> {
    if source.is_empty()
        || source.len() > 64
        || source.starts_with('-')
        || !source
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' '))
    {
        return Err(ApiError::invalid("invalid scan source"));
    }
    Ok(())
}

fn tool(candidates: &[&'static str], label: &str) -> Result<&'static str, ApiError> {
    candidates
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| ApiError::unavailable(format!("{label} not found"), "apk add sane"))
}

fn home_for_user(user: &str) -> Result<PathBuf, ApiError> {
    let raw = std::fs::read_to_string("/etc/passwd")
        .map_err(|e| ApiError::internal(format!("read /etc/passwd: {e}")))?;
    let home = raw.lines().find_map(|line| {
        let mut fields = line.split(':');
        if fields.next()? != user {
            return None;
        }
        fields.next()?;
        fields.next()?;
        fields.next()?;
        fields.next()?;
        fields.next().map(PathBuf::from)
    });
    home.filter(|home| home.is_absolute())
        .ok_or_else(|| ApiError::new(ursine_api::ErrorCode::NotFound, "user home not found"))
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
    fn parses_sane_devices() {
        let devices = parse_devices(
            "device `epson2:libusb:001:002' is a Epson Perfection V39 flatbed scanner\n",
        );
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "epson2:libusb:001:002");
        assert!(devices[0].description.contains("Epson"));
    }

    #[test]
    fn validates_scan_options() {
        assert!(validate_device("epson2:libusb:001:002").is_ok());
        assert!(validate_device("-bad").is_err());
        assert!(validate_source("Flatbed").is_ok());
        assert!(validate_source("--bad").is_err());
    }
}
