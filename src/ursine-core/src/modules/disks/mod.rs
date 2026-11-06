//! disks — block devices and `UDisks2` mount/unmount.

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use ursine_api::{Action, ApiError, Disk, DiskView, DISK_FSTYPES};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const LSBLK: [&str; 2] = ["/usr/bin/lsblk", "/bin/lsblk"];
const UDISKSCTL: [&str; 2] = ["/usr/bin/udisksctl", "/bin/udisksctl"];
const SMARTCTL: [&str; 3] = ["/usr/sbin/smartctl", "/usr/bin/smartctl", "/sbin/smartctl"];
const PARTED: [&str; 3] = ["/usr/sbin/parted", "/usr/bin/parted", "/sbin/parted"];
const TIMEOUT: Duration = Duration::from_secs(60);

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListDisks => Ok(json!(DiskView {
            disks: list().await?,
            format_enabled: ctx.config.disks_format,
        })),
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
            Ok(json!(DiskView {
                disks: list().await?,
                format_enabled: ctx.config.disks_format,
            }))
        }
        Action::FormatDisk {
            device,
            fstype,
            confirm_device,
        } => {
            require_format_enabled(&ctx.config)?;
            confirm_device_matches(device, confirm_device)?;
            ensure_target_unmounted(device).await?;
            let program = mkfs_program(fstype)?;
            let flags = mkfs_args(fstype);
            let mut argv: Vec<&str> = flags.iter().map(String::as_str).collect();
            argv.push(device);
            exec::run_root_program(&program, &argv, TIMEOUT)
                .await
                .map_err(|error| ApiError::new(error.code, error.message))?;
            Ok(json!(DiskView {
                disks: list().await?,
                format_enabled: ctx.config.disks_format,
            }))
        }
        Action::PartitionCreate {
            device,
            start_mib,
            end_mib,
            confirm_device,
        } => {
            require_format_enabled(&ctx.config)?;
            confirm_device_matches(device, confirm_device)?;
            ensure_target_unmounted(device).await?;
            ensure_partition_table(device).await?;
            if *start_mib == 0 || *end_mib <= *start_mib {
                return Err(ApiError::invalid(
                    "partition range needs 0 < start < end (MiB)",
                ));
            }
            let start = format!("{start_mib}MiB");
            let end = format!("{end_mib}MiB");
            let program = tool(&PARTED, "parted")?;
            exec::run_root_program(
                program,
                &["-s", device, "mkpart", "primary", &start, &end],
                TIMEOUT,
            )
            .await
            .map_err(|error| ApiError::new(error.code, error.message))?;
            Ok(json!(DiskView {
                disks: list().await?,
                format_enabled: ctx.config.disks_format,
            }))
        }
        Action::PartitionDelete {
            device,
            number,
            confirm_device,
        } => {
            require_format_enabled(&ctx.config)?;
            confirm_device_matches(device, confirm_device)?;
            ensure_target_unmounted(device).await?;
            ensure_partition_table(device).await?;
            if *number == 0 {
                return Err(ApiError::invalid("partition number must be >= 1"));
            }
            let number = number.to_string();
            let program = tool(&PARTED, "parted")?;
            exec::run_root_program(program, &["-s", device, "rm", &number], TIMEOUT)
                .await
                .map_err(|error| ApiError::new(error.code, error.message))?;
            Ok(json!(DiskView {
                disks: list().await?,
                format_enabled: ctx.config.disks_format,
            }))
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

/// Explicit enablement gate: formatting stays off until an admin sets
/// `disks_format = true` (and restarts the daemon).
fn require_format_enabled(config: &ursine_api::UrsineConfig) -> Result<(), ApiError> {
    if config.disks_format {
        Ok(())
    } else {
        Err(ApiError::unavailable(
            "destructive disk operations are disabled",
            "set disks_format = true in /etc/ursine/config.toml and restart ursined",
        ))
    }
}

/// The typed confirmation must name the exact device (plan: "exact
/// device typed" before any write to raw devices).
fn confirm_device_matches(device: &str, confirm: &str) -> Result<(), ApiError> {
    validate_device(device)?;
    if confirm != device {
        return Err(ApiError::invalid(
            "type the exact device path to confirm (confirm_device must equal device)",
        ));
    }
    Ok(())
}

/// Refuse writes to a mounted target: the device itself or any
/// partition carved from it must be unmounted first (mkfs/parted on a
/// live mount destroys data in use).
async fn ensure_target_unmounted(device: &str) -> Result<(), ApiError> {
    let disks = list().await?;
    let Some(target) = disks.iter().find(|row| row.path == device) else {
        return Err(ApiError::invalid(format!(
            "device {device} is not present in the system"
        )));
    };
    if target.mountpoint.is_some() {
        return Err(ApiError::invalid(format!(
            "{device} is mounted; unmount it first"
        )));
    }
    // Children share the parent name prefix (sda → sda1, nvme0n1 →
    // nvme0n1p1): a mounted child blocks whole-disk operations too.
    for row in &disks {
        if row.name != target.name && row.name.starts_with(&target.name) && row.mountpoint.is_some()
        {
            return Err(ApiError::invalid(format!(
                "a mounted partition of {device} is in use ({}); unmount it first",
                row.path
            )));
        }
    }
    Ok(())
}

/// `parted mkpart` needs an existing partition table; probe first so a
/// blank disk gets a clear error instead of "unrecognised disk label".
async fn ensure_partition_table(device: &str) -> Result<(), ApiError> {
    let program = tool(&PARTED, "parted")?;
    let probe = exec::run_root_program(program, &["-s", device, "print"], TIMEOUT).await;
    if probe.is_err() {
        return Err(ApiError::invalid(format!(
            "{device} has no readable partition table; create one first (parted -s {device} mklabel gpt)"
        )));
    }
    Ok(())
}

/// Absolute `mkfs.<fstype>` path from the whitelist (never user input).
fn mkfs_program(fstype: &str) -> Result<String, ApiError> {
    if !DISK_FSTYPES.contains(&fstype) {
        return Err(ApiError::invalid(format!(
            "unsupported filesystem type {fstype:?} ({})",
            DISK_FSTYPES.join(", ")
        )));
    }
    for dir in ["/sbin", "/usr/sbin"] {
        let path = format!("{dir}/mkfs.{fstype}");
        if Path::new(&path).is_file() {
            return Ok(path);
        }
    }
    Err(ApiError::unavailable(
        "mkfs tool is not installed",
        format!("install mkfs.{fstype} (e2fsprogs, xfsprogs, btrfs-progs, dosfstools, …)"),
    ))
}

/// Force flag per mkfs tool (destructive by design: the confirm gate
/// already matched the exact device).
fn mkfs_args(fstype: &str) -> Vec<String> {
    match fstype {
        "ext4" | "btrfs" | "xfs" => vec!["-f".to_string()],
        "ntfs" => vec!["-F".to_string()],
        _ => Vec::new(),
    }
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

    #[test]
    fn format_gate_fails_closed_by_default() {
        let cfg = ursine_api::UrsineConfig::default();
        assert!(!cfg.disks_format);
        let err = require_format_enabled(&cfg).unwrap_err();
        assert_eq!(err.code, ursine_api::ErrorCode::Unavailable, "{err:?}");
        assert!(err.message.contains("disabled"), "{}", err.message);
        let enabled = ursine_api::UrsineConfig {
            disks_format: true,
            ..ursine_api::UrsineConfig::default()
        };
        assert!(require_format_enabled(&enabled).is_ok());
    }

    #[test]
    fn gates_and_validators_work() {
        assert!(confirm_device_matches("/dev/sda1", "/dev/sda1").is_ok());
        assert!(confirm_device_matches("/dev/sda1", "/dev/sda").is_err());
        assert!(confirm_device_matches("../dev", "../dev").is_err());
        assert_eq!(mkfs_args("ext4"), ["-f"]);
        assert_eq!(mkfs_args("xfs"), ["-f"]);
        assert!(mkfs_args("vfat").is_empty());
        assert_eq!(mkfs_args("ntfs"), ["-F"]);
        assert!(mkfs_program("swap").is_err());
    }
}
