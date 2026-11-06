//! Restore a btrbk snapshot with `btrfs subvolume snapshot`.
//!
//! btrbk 0.32 has no restore command. The spec allows this explicit fallback:
//! validate both paths, preview the exact argv, then run btrfs as root.

use std::path::{Component, Path};

use serde_json::json;
use ursine_api::ApiError;

use super::btrbk;

pub(super) fn preview(snapshot: &str, target: &str) -> Result<serde_json::Value, ApiError> {
    let (_snapshot_path, _target_path, backup) = plan(snapshot, target)?;
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
        let name = target_path
            .file_name()
            .map_or_else(|| "restore".into(), |n| n.to_string_lossy().into_owned());
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let backup = target_path.with_file_name(format!("{name}.ursine-backup-{stamp}"));
        if backup.exists() {
            return Err(ApiError::new(
                ursine_api::ErrorCode::Conflict,
                "restore backup path already exists",
            ));
        }
        Some(backup)
    } else {
        None
    };
    Ok((snapshot_path, target_path, backup))
}

pub(super) async fn apply(snapshot: &str, target: &str) -> Result<serde_json::Value, ApiError> {
    // Re-run the preview checks: the path can change between confirm and apply.
    let (_, target_path, backup) = plan(snapshot, target)?;
    if let Some(backup) = &backup {
        std::fs::rename(target, backup)
            .map_err(|e| ApiError::internal(format!("move existing target aside: {e}")))?;
    }
    let program = btrbk::btrfs_path()?;
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
                let _ = std::fs::rename(backup, target);
            }
            return Err(error);
        }
    };
    // Rotation runs after success: the new backup counts toward the cap.
    prune_restore_backups(&target_path).await;
    Ok(json!({
        "dry_run": false,
        "snapshot": snapshot,
        "target": target,
        "backup": backup.map(|p| p.display().to_string()),
        "stdout": out.stdout,
        "stderr": out.stderr,
    }))
}

/// Restore backups kept per target: the newest snapshot wins, older
/// ones rotate out — at most THREE copies exist at any time.
const MAX_RESTORE_BACKUPS: usize = 3;

/// Best-effort rotation: delete restore backups beyond the newest
/// [`MAX_RESTORE_BACKUPS`]. Runs AFTER a successful restore (the new
/// backup already exists); failures never fail the restore itself —
/// leftover files stay visible next to the target.
///
/// The scan and unlink run on the blocking pool; a backup that plain
/// removal cannot delete (read-only subvolume, EROFS) falls back to
/// `btrfs subvolume delete`.
async fn prune_restore_backups(target: &Path) {
    let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
        return;
    };
    let dir = dir.to_path_buf();
    let prefix = format!("{}.ursine-backup-", name.to_string_lossy());
    let stale = tokio::task::spawn_blocking(move || -> Vec<std::path::PathBuf> {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut backups: Vec<(u64, std::path::PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if let Some(stamp) = file_name.strip_prefix(&prefix) {
                if let Ok(stamp) = stamp.parse::<u64>() {
                    backups.push((stamp, entry.path()));
                }
            }
        }
        // Newest stamp first; drop everything past the cap.
        backups.sort_by_key(|(stamp, _)| std::cmp::Reverse(*stamp));
        backups
            .into_iter()
            .skip(MAX_RESTORE_BACKUPS)
            .map(|(_, path)| path)
            .collect()
    })
    .await
    .unwrap_or_default();
    for path in stale {
        // Backups are moved btrfs subvolumes (directories); fall back to
        // plain file removal for non-directory leftovers.
        let removed = tokio::task::spawn_blocking({
            let path = path.clone();
            move || {
                std::fs::remove_dir_all(&path)
                    .or_else(|_| std::fs::remove_file(&path))
                    .is_ok()
            }
        })
        .await
        .unwrap_or(false);
        if removed {
            continue;
        }
        // Read-only subvolume: unlink fails with EROFS, the btrfs ioctl
        // still deletes it.
        let Ok(program) = btrbk::btrfs_path() else {
            continue;
        };
        let path_arg = path.to_string_lossy().into_owned();
        let _ = crate::modules::packages_nix::exec::run_root_program(
            program,
            &["subvolume", "delete", &path_arg],
            btrbk::MUTATE_TIMEOUT,
        )
        .await;
    }
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

    #[tokio::test]
    async fn restore_backups_rotate_to_three() {
        let dir = std::env::temp_dir().join(format!("ursine-restore-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("config.kdl");
        for stamp in [100u64, 200, 300, 400, 500] {
            std::fs::create_dir_all(dir.join(format!("config.kdl.ursine-backup-{stamp}"))).unwrap();
        }
        prune_restore_backups(&target).await;
        let mut stamps: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("config.kdl.ursine-backup-"))
            .collect();
        stamps.sort();
        assert_eq!(
            stamps,
            vec![
                "config.kdl.ursine-backup-300".to_string(),
                "config.kdl.ursine-backup-400".to_string(),
                "config.kdl.ursine-backup-500".to_string(),
            ],
            "newest three must survive"
        );
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
