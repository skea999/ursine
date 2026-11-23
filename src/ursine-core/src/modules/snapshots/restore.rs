//! Restore a btrbk snapshot with `btrfs subvolume snapshot`.
//!
//! btrbk 0.32 has no restore command. The spec allows this explicit fallback:
//! validate both paths, preview the exact argv, then run btrfs as root.

use std::path::{Component, Path, PathBuf};

use serde_json::json;
use ursine_api::{ApiError, ErrorCode};

use super::btrbk;

/// What a restore would do, decided before anything on disk changes.
#[derive(Debug)]
struct Plan {
    /// Subvolume the snapshot is restored onto.
    target: PathBuf,
    /// Slot the live target rotates into (slot 0), when the target exists.
    backup: Option<PathBuf>,
    /// Trees the rotation deletes to make room, because the backup chain is
    /// capped at `backup_configs.copies`: each restore beyond the cap drops
    /// the oldest saved tree of the target.
    would_delete: Vec<PathBuf>,
}

pub(super) fn preview(
    snapshot: &str,
    target: &str,
    copies: usize,
) -> Result<serde_json::Value, ApiError> {
    let plan = plan(snapshot, target, copies)?;
    let backup_display = plan.backup.as_ref().map(|p| p.display().to_string());
    let command = backup_display.as_ref().map_or_else(
        || format!("btrfs subvolume snapshot {snapshot} {target}"),
        |backup| format!("mv {target} {backup}; btrfs subvolume snapshot {snapshot} {target}"),
    );
    Ok(json!({
        "dry_run": true,
        "snapshot": snapshot,
        "target": target,
        "backup": backup_display,
        "would_delete": displays(&plan.would_delete),
        "command": command,
    }))
}

/// Paths as display strings for the JSON answers.
fn displays(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect()
}

fn plan(snapshot: &str, target: &str, copies: usize) -> Result<Plan, ApiError> {
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
    // A mount point cannot be renamed, so the move-aside step would fail
    // after rotating the backup chain. Refuse at preview time.
    if mountinfo_has_mount_point(&target_path) {
        return Err(ApiError::new(
            ErrorCode::Invalid,
            format!("restore target {} is a mount point", target_path.display()),
        )
        .with_fix("unmount the target before restoring, or restore it from a recovery boot"));
    }
    let backup = if target_path.exists() {
        let meta = std::fs::symlink_metadata(&target_path)
            .map_err(|e| ApiError::invalid(format!("target is not accessible: {e}")))?;
        if !meta.is_dir() {
            return Err(ApiError::invalid("restore target is not a directory"));
        }
        // Uniform slot naming: the live tree rotates into slot 0 (an
        // existing slot 0 is shifted older by rotate_move, so no
        // conflict check here).
        Some(crate::backup_configs::slot_path(&target_path, 0))
    } else {
        None
    };
    // The rotation that the move-aside performs deletes the oldest tree of
    // the chain once the cap is reached; name it instead of losing it
    // silently.
    let would_delete = crate::backup_configs::evicted_slots(&target_path, copies);
    Ok(Plan {
        target: target_path,
        backup,
        would_delete,
    })
}

pub(super) async fn apply(
    snapshot: &str,
    target: &str,
    copies: usize,
) -> Result<serde_json::Value, ApiError> {
    // Re-run the preview checks: the path can change between confirm and apply.
    let plan = plan(snapshot, target, copies)?;
    let target_path = plan.target.clone();
    // Uniform rotation: shift the chain older, move the live tree into
    // slot 0. On a failed snapshot the tree moves back — the chain stays
    // shifted (the oldest backup rotated out; the live target is intact).
    // Resolve btrfs BEFORE rotating: a lookup failure must not strand
    // the live tree in slot 0. The move itself displaces a whole
    // subvolume tree, so it runs on the blocking pool.
    let program = btrbk::btrfs_path()?;
    let backup = if target_path.exists() {
        let move_target = target_path.clone();
        let slot0 = tokio::task::spawn_blocking(move || {
            crate::backup_configs::rotate_move(&move_target, copies)
        })
        .await
        .map_err(|e| ApiError::internal(format!("backup task failed: {e}")))??;
        Some(slot0)
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
            if let Some(backup) = &backup {
                // Best-effort rollback, but never silent: a failed rename
                // (e.g. btrfs left a partial `target` behind) must surface
                // the slot holding the user's tree.
                if let Err(rollback) = std::fs::rename(backup, target) {
                    return Err(ApiError::internal(format!(
                        "restore failed ({error}); rollback of {} also failed ({rollback}) - the live tree is in {}",
                        target,
                        backup.display()
                    )));
                }
            }
            return Err(error);
        }
    };
    Ok(json!({
        "dry_run": false,
        "snapshot": snapshot,
        "target": target,
        "backup": backup.map(|p| p.display().to_string()),
        "would_delete": displays(&plan.would_delete),
        "stdout": out.stdout,
        "stderr": out.stderr,
    }))
}

/// True when `path` is the mount point of a filesystem, read from the
/// kernel's mount table (no D-Bus, no extra tool).
fn mountinfo_has_mount_point(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string("/proc/self/mountinfo") else {
        // No table to read: let the kernel refuse the rename instead.
        return false;
    };
    mountinfo_contains(&content, &path.to_string_lossy())
}

fn mountinfo_contains(content: &str, path: &str) -> bool {
    // `target=/home/` names the same directory as `/home`: without the
    // normalisation the preview passes, the chain rotates, and the rename
    // fails with EBUSY after the backups already moved.
    let path = normalized_mount_path(path);
    content.lines().any(|line| {
        // id parent major:minor root mount-point options [optional ...] - fstype source super
        line.split(' ').nth(4).is_some_and(|mount_point| {
            normalized_mount_path(&unescape_mountinfo(mount_point)) == path
        })
    })
}

/// Mount point paths with the trailing slashes removed (`/` stays `/`), so
/// two spellings of the same directory compare equal.
fn normalized_mount_path(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/"
    } else {
        trimmed
    }
}

/// Mountinfo escapes spaces, tabs, newlines and backslashes as `\NNN`.
fn unescape_mountinfo(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let digits: String = chars.by_ref().take(3).collect();
        match u8::from_str_radix(&digits, 8) {
            Ok(byte) if digits.len() == 3 => out.push(char::from(byte)),
            _ => {
                out.push('\\');
                out.push_str(&digits);
            }
        }
    }
    out
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

    fn temp_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ursine-restore-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A snapshot directory and an empty restore target, as the wire strings
    /// the handler receives.
    fn snapshot_and_target(dir: &Path) -> (String, String) {
        let snap = dir.join("snap");
        let target = dir.join("home");
        std::fs::create_dir_all(&snap).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        (snap.display().to_string(), target.display().to_string())
    }

    #[test]
    fn plan_targets_the_uniform_slot_and_never_conflicts_with_it() {
        let dir = temp_dir("plan");
        let (snap, target) = snapshot_and_target(&dir);
        // A slot 0 from the previous restore must NOT conflict: rotation
        // shifts it older instead of refusing to run.
        std::fs::create_dir_all(crate::backup_configs::slot_path(Path::new(&target), 0)).unwrap();
        let plan = plan(&snap, &target, 3).unwrap();
        assert_eq!(
            plan.backup,
            Some(crate::backup_configs::slot_path(Path::new(&target), 0)),
            "backup path is the uniform slot 0"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preview_names_the_tree_the_next_restore_deletes() {
        // The chain keeps `copies` trees; the next restore overwrites the
        // oldest one, so the preview names it instead of losing it silently.
        let dir = temp_dir("would-delete");
        let (snap, target) = snapshot_and_target(&dir);
        let oldest = crate::backup_configs::slot_path(Path::new(&target), 2);
        for index in 0..2 {
            std::fs::create_dir_all(crate::backup_configs::slot_path(Path::new(&target), index))
                .unwrap();
        }
        assert_eq!(
            preview(&snap, &target, 3).unwrap()["would_delete"],
            json!([]),
            "a chain with room left loses nothing"
        );
        std::fs::create_dir_all(&oldest).unwrap();
        assert_eq!(
            preview(&snap, &target, 3).unwrap()["would_delete"],
            json!([oldest.display().to_string()])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole restore chain: three rotates keep two prior trees plus
    /// the live one moved aside; legacy timestamped backups are pruned
    /// to the same cap (shared `backup_configs` behaviour).
    #[test]
    fn restore_rotation_keeps_the_configured_copies() {
        let dir = temp_dir("rot");
        let target = dir.join("home");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("v"), "1\n").unwrap();
        crate::backup_configs::rotate_move(&target, 2).unwrap();
        for gen in ["2\n", "3\n", "4\n"] {
            std::fs::create_dir_all(&target).unwrap();
            std::fs::write(target.join("v"), gen).unwrap();
            crate::backup_configs::rotate_move(&target, 2).unwrap();
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
    fn mountinfo_detects_mount_points_with_escapes() {
        let table = concat!(
            "25 1 0:22 / / rw,relatime - btrfs /dev/vda1 rw\n",
            "40 25 0:22 /@home /home rw,relatime - btrfs /dev/vda1 rw\n",
            "41 25 0:22 / /space\\040dir rw - btrfs /dev/vda1 rw\n",
        );
        assert!(mountinfo_contains(table, "/"));
        assert!(mountinfo_contains(table, "/home"));
        assert!(mountinfo_contains(table, "/space dir"));
        assert!(!mountinfo_contains(table, "/srv"));
    }

    #[test]
    fn mountinfo_normalises_trailing_slashes() {
        // `target=/home/` is the same directory as `/home`; a plain string
        // compare let it through, the chain rotated and the rename failed
        // with EBUSY after the backups had already moved.
        let table = "40 25 0:22 /@home /home rw,relatime - btrfs /dev/vda1 rw\n";
        for target in ["/home", "/home/", "/home//"] {
            assert!(
                mountinfo_contains(table, target),
                "{target} names the mount point"
            );
        }
        assert!(mountinfo_contains(
            "25 1 0:22 / / rw - btrfs /dev/vda1 rw\n",
            "/"
        ));
        assert!(!mountinfo_contains(table, "/home/user/"));
    }

    #[test]
    fn rejects_relative_and_traversal_paths() {
        assert!(validated_path("relative", "snapshot").is_err());
        assert!(validated_path("/a/../b", "snapshot").is_err());
        assert!(validated_path("/", "target").is_err());
        assert!(validated_path("/snapshots/home.1", "snapshot").is_ok());
    }
}
