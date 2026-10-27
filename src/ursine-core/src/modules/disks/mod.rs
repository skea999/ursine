//! disks — block devices and `UDisks2` mount/unmount.

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use ursine_api::{Action, ApiError, Disk};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const LSBLK: [&str; 2] = ["/usr/bin/lsblk", "/bin/lsblk"];
const UDISKSCTL: [&str; 2] = ["/usr/bin/udisksctl", "/bin/udisksctl"];
const SMARTCTL: [&str; 3] = ["/usr/sbin/smartctl", "/usr/bin/smartctl", "/sbin/smartctl"];
const TIMEOUT: Duration = Duration::from_secs(60);

pub async fn handle(action: &Action, _ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListDisks => Ok(json!({"disks": list().await?})),
        Action::MountDisk { device } | Action::UnmountDisk { device } => {
            validate_device(device)?;
            let verb = if matches!(action, Action::MountDisk { .. }) {
                "mount"
            } else {
                "unmount"
            };
            let program = tool(&UDISKSCTL, "udisksctl")?;
            exec::run_root_program(program, &[verb, "-b", device], TIMEOUT)
                .await
                .map_err(|error| ApiError::new(error.code, error.message))?;
            Ok(json!({"disks": list().await?}))
        }
        _ => Err(ApiError::invalid("action is not valid for module disks")),
    }
}

async fn list() -> Result<Vec<Disk>, ApiError> {
    let program = tool(&LSBLK, "lsblk")?;
    let output = exec::run_root_program(
        program,
        &[
            "-J",
            "-o",
            "NAME,PATH,TYPE,SIZE,FSTYPE,LABEL,UUID,FSUSE%,MOUNTPOINT,RM,RO,MODEL",
        ],
        TIMEOUT,
    )
    .await?;
    let parsed: Lsblk = serde_json::from_str(&output.stdout)
        .map_err(|e| ApiError::internal(format!("parse lsblk: {e}")))?;
    let mut disks = Vec::new();
    for device in parsed.blockdevices {
        flatten(device, &mut disks);
    }
    for disk in &mut disks {
        if disk.kind == "disk" {
            disk.smart = smart_health(&disk.path).await;
        }
    }
    Ok(disks)
}

#[derive(Debug, Deserialize)]
struct Lsblk {
    #[serde(default)]
    blockdevices: Vec<Block>,
}

#[derive(Debug, Deserialize)]
struct Block {
    name: String,
    path: String,
    #[serde(rename = "type")]
    kind: String,
    size: String,
    #[serde(default)]
    fstype: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(rename = "fsuse%", default)]
    use_percent: Option<String>,
    #[serde(default)]
    mountpoint: Option<String>,
    #[serde(default)]
    rm: bool,
    #[serde(default)]
    ro: bool,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    children: Vec<Self>,
}

fn flatten(block: Block, disks: &mut Vec<Disk>) {
    disks.push(Disk {
        name: block.name,
        path: block.path,
        kind: block.kind,
        size: block.size,
        filesystem: block.fstype,
        label: block.label,
        uuid: block.uuid,
        use_percent: block.use_percent,
        smart: None,
        mountpoint: block.mountpoint,
        removable: block.rm,
        read_only: block.ro,
        model: block.model,
    });
    for child in block.children {
        flatten(child, disks);
    }
}

async fn smart_health(path: &str) -> Option<String> {
    let program = SMARTCTL.iter().find(|path| Path::new(path).is_file())?;
    let output = exec::run_root_program(program, &["-H", path], TIMEOUT)
        .await
        .ok()?;
    if output.stdout.contains("PASSED") {
        Some("PASSED".to_string())
    } else if output.stdout.contains("FAILED") {
        Some("FAILED".to_string())
    } else {
        None
    }
}

fn validate_device(device: &str) -> Result<(), ApiError> {
    if !device.starts_with("/dev/")
        || device.len() > 256
        || device.contains("..")
        || !Path::new(device).is_absolute()
    {
        return Err(ApiError::invalid("invalid block device"));
    }
    Ok(())
}

fn tool(candidates: &[&'static str], label: &str) -> Result<&'static str, ApiError> {
    candidates
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| ApiError::unavailable(format!("{label} not found"), "apk add udisks2 lsblk"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_lsblk() {
        let parsed: Lsblk = serde_json::from_str(
            r#"{"blockdevices":[{"name":"sda","path":"/dev/sda","type":"disk","size":"1G","rm":false,"ro":false,"children":[{"name":"sda1","path":"/dev/sda1","type":"part","size":"512M","fstype":"ext4","mountpoint":"/","rm":false,"ro":false}]}]}"#,
        )
        .unwrap();
        let mut disks = Vec::new();
        for device in parsed.blockdevices {
            flatten(device, &mut disks);
        }
        assert_eq!(disks.len(), 2);
        assert_eq!(disks[1].filesystem.as_deref(), Some("ext4"));
    }

    #[test]
    fn validates_devices() {
        assert!(validate_device("/dev/sda1").is_ok());
        assert!(validate_device("/dev/../etc/passwd").is_err());
        assert!(validate_device("sda1").is_err());
    }
}
