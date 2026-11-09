//! disks — block devices from the kernel (sysfs + `/proc`) and plain
//! `mount`/`umount` (base system tools — no udisks, no lsblk).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, ApiError, Disk, DiskView, DISK_FSTYPES};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const MOUNT: [&str; 2] = ["/bin/mount", "/usr/bin/mount"];
const UMOUNT: [&str; 2] = ["/bin/umount", "/usr/bin/umount"];
const BLKID: [&str; 3] = ["/usr/sbin/blkid", "/sbin/blkid", "/usr/bin/blkid"];
const DF: [&str; 2] = ["/bin/df", "/usr/bin/df"];
const SMARTCTL: [&str; 3] = ["/usr/sbin/smartctl", "/usr/bin/smartctl", "/sbin/smartctl"];
const PARTED: [&str; 3] = ["/usr/sbin/parted", "/usr/bin/parted", "/sbin/parted"];
const TIMEOUT: Duration = Duration::from_secs(60);

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListDisks => Ok(json!(DiskView {
            disks: list().await?,
            format_enabled: ctx.config.disks_format,
        })),
        Action::MountDisk { device } => {
            validate_device(device)?;
            mount_device(device).await?;
            Ok(json!(DiskView {
                disks: list().await?,
                format_enabled: ctx.config.disks_format,
            }))
        }
        Action::UnmountDisk { device } => {
            validate_device(device)?;
            umount_device(device).await?;
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

/// Mount: use `/etc/fstab` when it names the device (the kernel picks the
/// options and mountpoint); otherwise create `/media/ursine/<device>` and
/// mount there — the plain-tools equivalent of what udisksctl did.
async fn mount_device(device: &str) -> Result<(), ApiError> {
    use std::os::unix::fs::PermissionsExt;
    let program = tool(&MOUNT, "mount")?;
    let by_id = reverse_id(&load_blkid().await);
    if fstab_names(device, &by_id) {
        exec::run_root_program(program, &[device], TIMEOUT)
            .await
            .map_err(|error| ApiError::new(error.code, error.message))?;
        return Ok(());
    }
    let relative = device.trim_start_matches("/dev/");
    let dir = PathBuf::from("/media/ursine").join(relative);
    std::fs::create_dir_all(&dir)
        .map_err(|e| ApiError::internal(format!("create {}: {e}", dir.display())))?;
    // Explicit mode: the mountpoint must be traversable regardless of
    // the daemon's umask.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| ApiError::internal(format!("chmod {}: {e}", dir.display())))?;
    let dir = dir.to_string_lossy().into_owned();
    exec::run_root_program(program, &[device, &dir], TIMEOUT)
        .await
        .map_err(|error| ApiError::new(error.code, error.message))?;
    Ok(())
}

async fn umount_device(device: &str) -> Result<(), ApiError> {
    let program = tool(&UMOUNT, "umount")?;
    exec::run_root_program(program, &[device], TIMEOUT)
        .await
        .map_err(|error| ApiError::new(error.code, error.message))?;
    Ok(())
}

/// True when `/etc/fstab` already names this device (comment lines
/// skipped; `UUID=`/`LABEL=` specs resolve through [`resolve_spec`]).
fn fstab_names(device: &str, by_id: &HashMap<String, String>) -> bool {
    let Ok(raw) = std::fs::read_to_string("/etc/fstab") else {
        return false;
    };
    raw.lines().any(|line| {
        let line = line.trim();
        !line.starts_with('#')
            && line.split_whitespace().next().is_some_and(|spec| {
                spec == device || resolve_spec(spec, by_id).as_deref() == Some(device)
            })
    })
}

/// Build the device list from `/sys/block` (kernel truth: no lsblk),
/// `/proc/mounts` (mountpoint + fstype), `blkid` (label/uuid/fstype of
/// unmounted devices) and `df -P` (use percent of mounted ones). SMART
/// health stays best-effort through `smartctl`.
/// `blkid` output parsed to attrs (empty map when the tool is missing).
async fn load_blkid() -> HashMap<String, BlkidAttrs> {
    match tool(&BLKID, "blkid") {
        Ok(program) => {
            let out = exec::run_root_program(program, &[], TIMEOUT)
                .await
                .map(|output| output.stdout)
                .unwrap_or_default();
            parse_blkid(&out)
        }
        Err(_) => HashMap::new(),
    }
}

/// Reverse UUID/LABEL → device path: `/proc/mounts` and `/etc/fstab`
/// entries declared as `UUID=…` still resolve without udev symlinks.
fn reverse_id(blkid: &HashMap<String, BlkidAttrs>) -> HashMap<String, String> {
    let mut by_id: HashMap<String, String> = HashMap::new();
    for (path, attrs) in blkid {
        if let Some(uuid) = &attrs.uuid {
            by_id.insert(uuid.clone(), path.clone());
        }
        if let Some(label) = &attrs.label {
            by_id.entry(label.clone()).or_insert_with(|| path.clone());
        }
    }
    by_id
}

async fn list() -> Result<Vec<Disk>, ApiError> {
    let mut disks = sysfs_devices();
    let blkid = load_blkid().await;
    let by_id = reverse_id(&blkid);
    let mounts = parse_mounts(&read_or_empty("/proc/mounts"), &by_id);
    let usage = match tool(&DF, "df") {
        Ok(program) => {
            let out = exec::run_root_program(program, &["-P"], TIMEOUT)
                .await
                .map(|output| output.stdout)
                .unwrap_or_default();
            parse_df(&out)
        }
        Err(_) => HashMap::new(),
    };
    for disk in &mut disks {
        if let Some((point, fstype)) = mount_lookup(&mounts, &disk.path) {
            disk.mountpoint = Some(point.clone());
            disk.filesystem = Some(fstype.clone());
            disk.use_percent = usage.get(point).cloned();
        }
        if let Some(attrs) = blkid.get(&disk.path) {
            if disk.label.is_none() {
                disk.label = attrs.label.clone();
            }
            if disk.uuid.is_none() {
                disk.uuid = attrs.uuid.clone();
            }
            if disk.filesystem.is_none() {
                disk.filesystem = attrs.fstype.clone();
            }
        }
    }
    for disk in &mut disks {
        if disk.kind == "disk" {
            disk.smart = smart_health(&disk.path).await;
        }
    }
    Ok(disks)
}

fn read_or_empty(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Find a device's mount entry: exact path first, then its canonical
/// form (sysfs builds `/dev/mapper/x` while mounts may say `/dev/dm-0`,
/// and a spec may be a `/dev/disk/by-*` symlink).
fn mount_lookup<'a>(
    mounts: &'a HashMap<String, (String, String)>,
    path: &str,
) -> Option<&'a (String, String)> {
    if let Some(entry) = mounts.get(path) {
        return Some(entry);
    }
    let canonical = std::fs::canonicalize(path).ok()?;
    mounts.get(&canonical.display().to_string())
}

/// Enumerate `/sys/block` (disk-class devices) and their partition
/// subdirectories (entries carrying a `partition` file).
fn sysfs_devices() -> Vec<Disk> {
    let mut disks = Vec::new();
    let Ok(root) = std::fs::read_dir("/sys/block") else {
        return disks;
    };
    let mut tops: Vec<PathBuf> = root.flatten().map(|entry| entry.path()).collect();
    tops.sort_by_key(|path| {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        name
    });
    tops.sort_by(|a, b| {
        natural_cmp(
            &a.file_name().unwrap_or_default().to_string_lossy(),
            &b.file_name().unwrap_or_default().to_string_lossy(),
        )
    });
    for top in tops {
        let Some(sys_name) = top.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        // Device-mapper: the user-facing name lives in dm/name and the
        // node is /dev/mapper/<name> (what /proc/mounts reports too).
        let fallback_kind = if sys_name.starts_with("loop") {
            "loop"
        } else if sys_name.starts_with("sr") {
            "rom"
        } else {
            "disk"
        };
        let (name, path, kind) = std::fs::read_to_string(top.join("dm/name")).map_or_else(
            |_| {
                (
                    sys_name.clone(),
                    format!("/dev/{sys_name}"),
                    fallback_kind.to_string(),
                )
            },
            |dm_name| {
                let dm_name = dm_name.trim().to_string();
                (
                    dm_name.clone(),
                    format!("/dev/mapper/{dm_name}"),
                    "disk".to_string(),
                )
            },
        );
        disks.push(sysfs_row(&name, &path, &kind, &top));
        // Partitions live as subdirectories carrying a `partition` file.
        let mut parts: Vec<PathBuf> = std::fs::read_dir(&top)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.join("partition").is_file())
                    .collect()
            })
            .unwrap_or_default();
        parts.sort_by(|a, b| {
            natural_cmp(
                &a.file_name().unwrap_or_default().to_string_lossy(),
                &b.file_name().unwrap_or_default().to_string_lossy(),
            )
        });
        for part in parts {
            let Some(part_name) = part.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            disks.push(sysfs_row(
                &part_name,
                &format!("/dev/{part_name}"),
                "part",
                &part,
            ));
        }
    }
    disks
}

/// One `Disk` from a sysfs directory (size in 512-byte sectors, flags,
/// model). Content fields are filled later from mounts/blkid.
fn sysfs_row(name: &str, path: &str, kind: &str, sys: &Path) -> Disk {
    let read = |file: &str| -> Option<String> {
        std::fs::read_to_string(sys.join(file))
            .ok()
            .map(|raw| raw.trim().to_string())
    };
    let sectors: u64 = read("size").and_then(|s| s.parse().ok()).unwrap_or(0);
    Disk {
        name: name.to_string(),
        path: path.to_string(),
        kind: kind.to_string(),
        size: format_size(sectors),
        filesystem: None,
        label: None,
        uuid: None,
        use_percent: None,
        smart: None,
        mountpoint: None,
        removable: read("removable").as_deref() == Some("1"),
        read_only: read("ro").as_deref() == Some("1"),
        model: read("device/model"),
    }
}

/// Human size in 1024-based units (`512M`, `1G`) — integer math, no
/// float casts (the label never needs fractional precision).
fn format_size(sectors: u64) -> String {
    const UNITS: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    let mut bytes = sectors.saturating_mul(512);
    let mut unit = 0;
    while bytes >= 1024 && unit < UNITS.len() - 1 {
        bytes /= 1024;
        unit += 1;
    }
    match UNITS.get(unit).copied() {
        Some(label) if unit > 0 => format!("{bytes}{label}"),
        // `unit == 0` (bytes) and the unreachable overflow fallback.
        _ => format!("{bytes}B"),
    }
}

/// Natural ordering so partitions sort `sda1, sda2, … sda10` (not
/// lexicographic `sda1, sda10, sda2`). Splits both sides into
/// alternating digit / non-digit runs and compares run by run — no
/// byte indexing, every slice comes from a fresh iterator.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let runs = |s: &str| -> Vec<(bool, String)> {
        let mut out = Vec::new();
        let mut chars = s.chars().peekable();
        while let Some(&first) = chars.peek() {
            let numeric = first.is_ascii_digit();
            let mut buf = String::new();
            while let Some(&next) = chars.peek() {
                if next.is_ascii_digit() == numeric {
                    buf.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push((numeric, buf));
        }
        out
    };
    let (ra, rb) = (runs(a), runs(b));
    for (index, (seg_a, seg_b)) in ra.iter().zip(rb.iter()).enumerate() {
        let _ = index;
        if seg_a.0 && seg_b.0 {
            let va: u128 = seg_a.1.parse().unwrap_or(u128::MAX);
            let vb: u128 = seg_b.1.parse().unwrap_or(u128::MAX);
            match va.cmp(&vb) {
                Ordering::Equal => {}
                other => return other,
            }
        } else {
            let order = seg_a.1.cmp(&seg_b.1);
            if order != Ordering::Equal {
                return order;
            }
        }
    }
    ra.len().cmp(&rb.len())
}

/// Decode `/proc/mounts` escapes (`\040` = space …).
fn unescape_proc(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let mut octal = String::new();
        for _ in 0..3 {
            match chars.next() {
                Some(c) if c.is_ascii_digit() => octal.push(c),
                other => {
                    if let Some(other) = other {
                        out.push(other);
                    }
                    break;
                }
            }
        }
        match u32::from_str_radix(&octal, 8) {
            Ok(code) => out.push(char::from_u32(code).unwrap_or('\u{fffd}')),
            Err(_) => out.push('\u{fffd}'),
        }
    }
    out
}

/// `by_id` maps blkid UUID/LABEL → device path: a mount declared as
/// `UUID=…` still matches when `/dev/disk/by-*` is missing (mdev
/// systems) — without it a mounted device could look unmounted.
fn parse_mounts(raw: &str, by_id: &HashMap<String, String>) -> HashMap<String, (String, String)> {
    let mut map = HashMap::new();
    for line in raw.lines() {
        let mut fields = line.split_whitespace();
        let (Some(spec), Some(point), Some(fstype)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let Some(path) = resolve_spec(spec, by_id) else {
            continue;
        };
        map.insert(path, (unescape_proc(point), fstype.to_string()));
    }
    map
}

fn resolve_spec(spec: &str, by_id: &HashMap<String, String>) -> Option<String> {
    if let Some(path) = spec.strip_prefix("/dev/") {
        let raw = format!("/dev/{path}");
        // Canonicalize symlinked specs (`/dev/disk/by-*`, `/dev/root`);
        // keep the raw form when the node is already gone.
        return Some(
            std::fs::canonicalize(&raw)
                .map(|target| target.display().to_string())
                .unwrap_or(raw),
        );
    }
    let id = spec
        .strip_prefix("UUID=")
        .or_else(|| spec.strip_prefix("LABEL="))?;
    // Prefer the udev symlink, fall back to the blkid reverse map.
    let kind = if spec.starts_with("UUID=") {
        "uuid"
    } else {
        "label"
    };
    if let Ok(target) = std::fs::canonicalize(format!("/dev/disk/by-{kind}/{id}")) {
        return Some(target.display().to_string());
    }
    by_id.get(id).cloned()
}

#[derive(Debug, Default, Clone)]
struct BlkidAttrs {
    label: Option<String>,
    uuid: Option<String>,
    fstype: Option<String>,
}

/// `blkid` default output: `/dev/sda1: LABEL="x" UUID="y" TYPE="ext4"`.
fn parse_blkid(raw: &str) -> HashMap<String, BlkidAttrs> {
    let mut map = HashMap::new();
    for line in raw.lines() {
        let Some((device, rest)) = line.split_once(": ") else {
            continue;
        };
        let mut attrs = BlkidAttrs::default();
        let mut cursor = rest;
        while let Some(eq) = cursor.find('=') {
            let key = cursor.get(..eq).map(str::trim).unwrap_or_default();
            let Some(after) = cursor.get(eq + 1..).and_then(|s| s.strip_prefix('"')) else {
                break;
            };
            let Some(end) = after.find('"') else {
                break;
            };
            let Some(value) = after.get(..end) else {
                break;
            };
            match key {
                "LABEL" => attrs.label = Some(value.to_string()),
                "UUID" => attrs.uuid = Some(value.to_string()),
                "TYPE" => attrs.fstype = Some(value.to_string()),
                _ => {}
            }
            match after.get(end + 1..) {
                Some(rest) => cursor = rest,
                None => break,
            }
        }
        if attrs.label.is_some() || attrs.uuid.is_some() || attrs.fstype.is_some() {
            map.insert(device.trim().to_string(), attrs);
        }
    }
    map
}

/// `df -P` → mountpoint → capacity (`50%`). The mount column is
/// everything after the capacity field (mountpoints may contain spaces).
fn parse_df(raw: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in raw.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let (Some(capacity), Some(rest)) = (fields.get(4), fields.get(5..)) else {
            continue;
        };
        if !capacity.ends_with('%') {
            continue;
        }
        map.insert(rest.join(" "), capacity.to_string());
    }
    map
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
            "set disks_format = true in /etc/ursine/config.toml and restartursined",
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
    // A mounted child blocks whole-disk operations too — but only a
    // real child (sda → sda1, nvme0n1 → nvme0n1p1), never a different
    // disk that merely shares the prefix (sda → sdaa).
    for row in &disks {
        if row.name != target.name
            && is_partition_of(&target.name, &row.name)
            && row.mountpoint.is_some()
        {
            return Err(ApiError::invalid(format!(
                "a mounted partition of {device} is in use ({}); unmount it first",
                row.path
            )));
        }
    }
    Ok(())
}

/// True when `name` is a partition carved from `parent`: the suffix is
/// all digits after a letter-ended parent (`sda1` from `sda`), or the
/// letter `p` followed by digits after a digit-ended parent
/// (`nvme0n1p1` from `nvme0n1`). A bare prefix match would also hit a
/// different disk (`sdaa`).
fn is_partition_of(parent: &str, name: &str) -> bool {
    let Some(rest) = name.strip_prefix(parent) else {
        return false;
    };
    let digits = |segment: &str| !segment.is_empty() && segment.chars().all(|c| c.is_ascii_digit());
    if parent.ends_with(|c: char| c.is_ascii_digit()) {
        rest.strip_prefix('p').is_some_and(digits)
    } else {
        digits(rest)
    }
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
        .ok_or_else(|| {
            ApiError::unavailable(
                format!("{label} not found"),
                "apk add util-linux  # provides mount/umount/blkid/df",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_mounts_with_uuid_and_escapes() {
        let raw = concat!(
            "/dev/sda1 / ext4 rw,relatime 0 0\n",
            "UUID=1234-ABCD /media/usb\\040stick vfat rw 0 0\n",
            "/dev/mapper/vg-lv /home btrfs rw 0 0\n",
            "tmpfs /run tmpfs rw 0 0\n",
        );
        // UUID= resolves through the blkid reverse map (no udev here).
        let mut by_id = HashMap::new();
        by_id.insert("1234-ABCD".to_string(), "/dev/sdb1".to_string());
        let mounts = parse_mounts(raw, &by_id);
        assert_eq!(mounts.get("/dev/sda1").unwrap().0, "/");
        assert_eq!(mounts.get("/dev/sda1").unwrap().1, "ext4");
        assert_eq!(mounts.get("/dev/mapper/vg-lv").unwrap().1, "btrfs");
        // Escaped mountpoint decodes; the UUID spec resolved via blkid.
        assert_eq!(mounts.get("/dev/sdb1").unwrap().0, "/media/usb stick");
        assert_eq!(mounts.get("/dev/sdb1").unwrap().1, "vfat");
        // An unresolvable spec is skipped, never guessed.
        let empty = HashMap::new();
        let mounts = parse_mounts("UUID=zz /mnt/x ext4 rw 0 0\n", &empty);
        assert!(mounts.is_empty());
    }

    #[test]
    fn parses_blkid_lines_with_spaces_in_labels() {
        let raw = concat!(
            "/dev/sda1: LABEL=\"My Disk\" UUID=\"abcd-1234\" TYPE=\"ext4\" PARTUUID=\"x\"\n",
            "/dev/sdb1: LABEL=\"\" UUID=\"\" TYPE=\"vfat\"\n",
            "no-colon-line\n",
        );
        let map = parse_blkid(raw);
        let attrs = map.get("/dev/sda1").unwrap();
        assert_eq!(attrs.label.as_deref(), Some("My Disk"));
        assert_eq!(attrs.uuid.as_deref(), Some("abcd-1234"));
        assert_eq!(attrs.fstype.as_deref(), Some("ext4"));
        assert_eq!(map.get("/dev/sdb1").unwrap().label.as_deref(), Some(""));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn parses_df_capacity_per_mount() {
        let raw = concat!(
            "Filesystem     1024-blocks     Used Available Capacity Mounted on\n",
            "/dev/sda1            100        50       50       50% /\n",
            "/dev/sdb1            200       100       100       50% /media/my usb\n",
        );
        let map = parse_df(raw);
        assert_eq!(map.get("/").unwrap(), "50%");
        assert_eq!(map.get("/media/my usb").unwrap(), "50%");
    }

    #[test]
    fn formats_sizes_like_lsblk_and_sorts_naturally() {
        // Input is 512-byte sectors: 2_048 sectors = 1 MiB.
        assert_eq!(format_size(2_048), "1M");
        assert_eq!(format_size(2_048 * 1024), "1G");
        assert_eq!(format_size(2 * 2_048 * 1024), "2G");
        assert_eq!(format_size(0), "0B");
        assert_eq!(format_size(1_000_000), "488M");
        // partitions must not sort lexicographically
        let mut names = vec!["sda10", "sda2", "sda1"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["sda1", "sda2", "sda10"]);
        assert_eq!(
            natural_cmp("nvme0n1", "nvme0n1p1"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn validates_devices() {
        assert!(validate_device("/dev/sda1").is_ok());
        assert!(validate_device("/dev/../etc/passwd").is_err());
        assert!(validate_device("sda1").is_err());
    }

    #[test]
    fn partition_of_distinguishes_children_from_siblings() {
        assert!(is_partition_of("sda", "sda1"));
        assert!(is_partition_of("nvme0n1", "nvme0n1p1"));
        assert!(is_partition_of("mmcblk0", "mmcblk0p1"));
        // Different disks that merely share the prefix.
        assert!(!is_partition_of("sda", "sdaa"));
        assert!(!is_partition_of("sda", "sdaa1"));
        // A different nvme namespace, not a partition of nvme0n1.
        assert!(!is_partition_of("nvme0n1", "nvme0n11"));
        // Not a child at all.
        assert!(!is_partition_of("sda", "sdb1"));
        assert!(!is_partition_of("sda", "sda"));
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
