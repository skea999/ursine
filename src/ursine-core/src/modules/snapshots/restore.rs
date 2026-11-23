//! Restore a btrbk snapshot with `btrfs subvolume snapshot`.
//!
//! btrbk 0.32 has no restore command. The spec allows this explicit fallback:
//! validate both paths, refuse a target this method cannot replace, preview the
//! exact argv, then run btrfs as root.

use std::path::{Component, Path};

use serde_json::json;
use ursine_api::{ApiError, ErrorCode};

use super::btrbk;

/// Restore preview: the checks the real run performs, or its refusal.
pub(super) fn preview(snapshot: &str, target: &str) -> Result<serde_json::Value, ApiError> {
    preview_with(snapshot, target, &mounted_target)
}

/// Preview against an injected mount check, so the refusal is testable without
/// a mounted subvolume (`is_mounted` is the only host fact).
fn preview_with(
    snapshot: &str,
    target: &str,
    is_mounted: &dyn Fn(&Path) -> bool,
) -> Result<serde_json::Value, ApiError> {
    let (_snapshot_path, _target_path, backup) = plan(snapshot, target, is_mounted)?;
    let backup_display = backup.as_ref().map(|p| p.display().to_string());
    let command = backup_display.as_ref().map_or_else(
        || format!("btrfs subvolume snapshot {snapshot} {target}"),
        |backup| format!("mv {target} {backup}; btrfs subvolume snapshot {snapshot} {target}"),
    );
    Ok(json!({
        "dry_run": true,
        "snapshot": snapshot,
        "target": target,
        "backup": backup_display,
        "command": command,
    }))
}

fn plan(
    snapshot: &str,
    target: &str,
    is_mounted: &dyn Fn(&Path) -> bool,
) -> Result<
    (
        std::path::PathBuf,
        std::path::PathBuf,
        Option<std::path::PathBuf>,
    ),
    ApiError,
> {
    let snapshot_path = validated_path(snapshot, "snapshot")?.to_path_buf();
    let target_path = validated_path(target, "target")?.to_path_buf();
    let meta = std::fs::symlink_metadata(&snapshot_path)
        .map_err(|e| ApiError::invalid(format!("snapshot is not accessible: {e}")))?;
    if !meta.is_dir() {
        return Err(ApiError::invalid("snapshot is not a directory"));
    }
    if snapshot_path == target_path {
        return Err(ApiError::invalid("snapshot and target must differ"));
    }
    let backup = if target_path.exists() {
        let meta = std::fs::symlink_metadata(&target_path)
            .map_err(|e| ApiError::invalid(format!("target is not accessible: {e}")))?;
        if !meta.is_dir() {
            return Err(ApiError::invalid("restore target is not a directory"));
        }
        // The previous tree is moved aside, and a mount point cannot be
        // renamed (EBUSY): refuse here, before anything is moved, instead of
        // promising a command that always fails.
        if is_mounted(&target_path) {
            return Err(mounted_target_error(&target_path));
        }
        // Uniform slot naming: the live tree rotates into slot 0 (an
        // existing slot 0 is shifted older by the commit, so no
        // conflict check here).
        Some(crate::backup_configs::slot_path(&target_path, 0))
    } else {
        None
    };
    Ok((snapshot_path, target_path, backup))
}

pub(super) async fn apply(
    snapshot: &str,
    target: &str,
    copies: usize,
) -> Result<serde_json::Value, ApiError> {
    // Re-run the preview checks: the path can change between confirm and apply.
    let (_, target_path, _planned_backup) = plan(snapshot, target, &mounted_target)?;
    // Resolve btrfs BEFORE moving anything: a lookup failure must not strand
    // the live tree in the staging path.
    let program = btrbk::btrfs_path()?;
    // Two phases, so a failed snapshot never shifts the backup chain: park the
    // live tree in a staging sibling first (no slot moves yet), run the
    // snapshot, and only then rotate the chain around it. The old order moved
    // the tree straight into slot 0, which shifted — and dropped the oldest —
    // a whole generation even when the snapshot then failed.
    let staged = if target_path.exists() {
        let move_target = target_path.clone();
        Some(
            tokio::task::spawn_blocking(move || crate::backup_configs::stage_move(&move_target))
                .await
                .map_err(|e| ApiError::internal(format!("backup task failed: {e}")))??,
        )
    } else {
        None
    };
    let result = crate::modules::packages_nix::exec::run_root_program(
        program,
        &["subvolume", "snapshot", snapshot, target],
        btrbk::MUTATE_TIMEOUT,
    )
    .await;
    let out = match result {
        Ok(out) => out,
        Err(error) => {
            if let Some(staged) = &staged {
                // Best-effort rollback, but never silent: a failed rename
                // (e.g. btrfs left a partial `target` behind) must surface
                // the staging path holding the user's tree. No slot moved, so
                // the chain is intact.
                if let Err(rollback) = std::fs::rename(staged, target) {
                    return Err(ApiError::internal(format!(
                        "restore failed ({error}); rollback of {} also failed ({rollback}) - the live tree is in {}",
                        target,
                        staged.display()
                    )));
                }
            }
            return Err(error);
        }
    };
    // The snapshot exists: commit the rotation once, so the previous tree
    // lands in slot 0 and the older ones shift exactly one step.
    let backup = match staged {
        None => None,
        Some(staged) => {
            let staged_display = staged.display().to_string();
            let commit_target = target_path.clone();
            let committed = tokio::task::spawn_blocking(move || {
                crate::backup_configs::commit_move(&commit_target, copies, &staged)
            })
            .await
            .map_err(|e| ApiError::internal(format!("backup task failed: {e}")))?;
            match committed {
                Ok(slot0) => Some(slot0),
                Err(error) => {
                    // The restore itself worked; say so and name the tree.
                    return Err(ApiError::internal(format!(
                        "restore created {target} but the previous tree could not be moved into the backup slot ({}); it is at {staged_display}",
                        error.message
                    )));
                }
            }
        }
    };
    Ok(json!({
        "dry_run": false,
        "snapshot": snapshot,
        "target": target,
        "backup": backup.map(|p| p.display().to_string()),
        "stdout": out.stdout,
        "stderr": out.stderr,
    }))
}

/// Refusal for a target that is a mount point: the method needs to rename the
/// live tree aside, which the kernel rejects with EBUSY.
fn mounted_target_error(target: &Path) -> ApiError {
    ApiError::new(
        ErrorCode::Conflict,
        format!(
            "restore target {} is a mounted subvolume; a mount point cannot be renamed",
            target.display()
        ),
    )
    .with_fix(format!(
        "restore from a live environment (boot the install media or another system) so {} can be unmounted, or restore into a directory that is not a mount point",
        target.display()
    ))
}

/// Mount point check for the live system: `/proc/self/mountinfo` first (it also
/// catches bind mounts), the device id of the target against its parent as the
/// fallback when mountinfo cannot be read.
fn mounted_target(path: &Path) -> bool {
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    is_mount_point(path, &mountinfo, device_of)
}

/// Pure mount point decision over injected data: `mountinfo` is a
/// `/proc/self/mountinfo` dump and `device` reads the `st_dev` of a path.
fn is_mount_point(path: &Path, mountinfo: &str, device: impl Fn(&Path) -> Option<u64>) -> bool {
    if mountinfo_lists(mountinfo, path) {
        return true;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    match (device(path), device(parent)) {
        (Some(target), Some(parent)) => target != parent,
        _ => false,
    }
}

/// Fifth field of a mountinfo line is the mount point.
fn mountinfo_lists(mountinfo: &str, path: &Path) -> bool {
    let wanted = comparable(path);
    mountinfo
        .lines()
        .filter_map(|line| line.split_whitespace().nth(4))
        .any(|field| comparable(Path::new(&unescape_octal(field))) == wanted)
}

/// Trailing slashes must not decide the comparison.
fn comparable(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.trim_end_matches('/').to_string()
}

/// mountinfo escapes space, tab, newline and backslash as three octal digits.
fn unescape_octal(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(index) = rest.find('\\') {
        out.push_str(rest.get(..index).unwrap_or(""));
        let tail = rest.get(index + 1..).unwrap_or("");
        let octal = tail
            .get(..3)
            .filter(|digits| digits.chars().all(|c| ('0'..='7').contains(&c)));
        if let Some(digits) = octal {
            if let Ok(byte) = u8::from_str_radix(digits, 8) {
                out.push(char::from(byte));
            }
            rest = tail.get(3..).unwrap_or("");
        } else {
            out.push('\\');
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(unix)]
fn device_of(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt as _;
    Some(std::fs::symlink_metadata(path).ok()?.dev())
}

#[cfg(not(unix))]
fn device_of(_path: &Path) -> Option<u64> {
    None
}

fn validated_path<'a>(value: &'a str, label: &str) -> Result<&'a Path, ApiError> {
    if value.is_empty() || value.len() > 4096 || !value.starts_with('/') {
        return Err(ApiError::invalid(format!(
            "{label} must be an absolute path"
        )));
    }
    let path = Path::new(value);
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(ApiError::invalid(format!(
            "{label} must not contain . or .."
        )));
    }
    if path == Path::new("/") {
        return Err(ApiError::invalid(format!("{label} must not be /")));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir
    use super::*;

    /// FeralOS-shaped layout: `/home` and `/nix` are mounted subvolumes.
    const MOUNTINFO: &str = "\
25 0 8:1 / / rw,relatime shared:1 - btrfs /dev/vda2 rw,ssd
26 25 8:1 /home /home rw,relatime shared:2 - btrfs /dev/vda2 rw,ssd,subvol=@home
27 25 8:1 /nix /nix rw,relatime shared:3 - btrfs /dev/vda2 rw,ssd,subvol=@nix
";

    #[test]
    fn plan_targets_the_uniform_slot_and_never_conflicts_with_it() {
        let dir = std::env::temp_dir().join(format!("ursine-restore-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let snap = dir.join("snap");
        let target = dir.join("home");
        std::fs::create_dir_all(&snap).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        // A slot 0 from the previous restore must NOT conflict: rotation
        // shifts it older instead of refusing to run.
        std::fs::create_dir_all(crate::backup_configs::slot_path(&target, 0)).unwrap();
        let (_s, _t, backup) = plan(
            &snap.display().to_string(),
            &target.display().to_string(),
            &|_| false,
        )
        .unwrap();
        assert_eq!(
            backup,
            Some(crate::backup_configs::slot_path(&target, 0)),
            "backup path is the uniform slot 0"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mountinfo_fixture_marks_the_mounted_subvolume() {
        assert!(is_mount_point(Path::new("/home"), MOUNTINFO, |_| None));
        assert!(is_mount_point(Path::new("/home/"), MOUNTINFO, |_| None));
        assert!(!is_mount_point(Path::new("/var"), MOUNTINFO, |_| None));
        // The root filesystem is a mount point too; it is never a target
        // (validated_path rejects `/`), but the parse must be honest.
        assert!(is_mount_point(Path::new("/"), MOUNTINFO, |_| None));
    }

    #[test]
    fn device_difference_marks_a_mount_point_without_mountinfo() {
        // Same device: a plain directory inside the parent filesystem.
        assert!(!is_mount_point(Path::new("/home"), "", |_| Some(7)));
        // Different device: the target is its own mount.
        assert!(is_mount_point(Path::new("/home"), "", |path| {
            (path == Path::new("/home")).then_some(8).or(Some(7))
        }));
        // Unreadable metadata is not a mount point.
        assert!(!is_mount_point(Path::new("/home"), "", |_| None));
    }

    #[test]
    fn mountinfo_octal_escapes_decode_spaces() {
        let mountinfo = "30 25 8:1 /my\\040home /my\\040home rw - btrfs /dev/vda2 rw\n";
        assert!(is_mount_point(Path::new("/my home"), mountinfo, |_| None));
        assert_eq!(unescape_octal("/a\\040b"), "/a b");
        assert_eq!(unescape_octal("/a\\134b"), "/a\\b");
        assert_eq!(unescape_octal("/plain"), "/plain");
    }

    #[test]
    fn preview_refuses_a_mounted_target_instead_of_a_command() {
        let dir = std::env::temp_dir().join(format!("ursine-restore-mount-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let snap = dir.join("snap");
        let target = dir.join("home");
        std::fs::create_dir_all(&snap).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        let error = preview_with(
            &snap.display().to_string(),
            &target.display().to_string(),
            &|path| path == target,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert!(error.message.contains("mounted subvolume"));
        let fix = error.fix.unwrap_or_default();
        assert!(fix.contains("live environment"), "fix names the way out");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preview_dry_run_prints_the_command_for_a_plain_directory() {
        let dir = std::env::temp_dir().join(format!("ursine-restore-dry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let snap = dir.join("snap");
        let target = dir.join("home");
        std::fs::create_dir_all(&snap).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        let view = preview_with(
            &snap.display().to_string(),
            &target.display().to_string(),
            &|_| false,
        )
        .unwrap();
        assert_eq!(view["dry_run"], true);
        assert!(view["command"]
            .as_str()
            .unwrap_or_default()
            .contains("btrfs subvolume snapshot"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole restore chain: the staging move plus the commit keep two prior
    /// trees; legacy timestamped backups are pruned to the same cap.
    #[test]
    fn restore_rotation_keeps_the_configured_copies() {
        let dir = std::env::temp_dir().join(format!("ursine-restore-rot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("home");
        let rotate = |target: &Path| {
            let staged = crate::backup_configs::stage_move(target).unwrap();
            crate::backup_configs::commit_move(target, 2, &staged).unwrap();
        };
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("v"), "1\n").unwrap();
        rotate(&target);
        for gen in ["2\n", "3\n", "4\n"] {
            std::fs::create_dir_all(&target).unwrap();
            std::fs::write(target.join("v"), gen).unwrap();
            rotate(&target);
        }
        // Live tree sits in slot 0 (moved by the last rotate), slot 1 is
        // the second-newest, older generations rotated out.
        assert_eq!(
            std::fs::read_to_string(crate::backup_configs::slot_path(&target, 0).join("v"))
                .unwrap(),
            "4\n"
        );
        assert_eq!(
            std::fs::read_to_string(crate::backup_configs::slot_path(&target, 1).join("v"))
                .unwrap(),
            "3\n"
        );
        assert!(!crate::backup_configs::slot_path(&target, 2).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_relative_and_traversal_paths() {
        assert!(validated_path("relative", "snapshot").is_err());
        assert!(validated_path("/a/../b", "snapshot").is_err());
        assert!(validated_path("/", "target").is_err());
        assert!(validated_path("/snapshots/home.1", "snapshot").is_ok());
    }
}
