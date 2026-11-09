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
        // Uniform slot naming: the live tree rotates into slot 0 (an
        // existing slot 0 is shifted older by rotate_move, so no
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
    let (_, target_path, _planned_backup) = plan(snapshot, target)?;
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
        "stdout": out.stdout,
        "stderr": out.stderr,
    }))
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
        let (_s, _t, backup) =
            plan(&snap.display().to_string(), &target.display().to_string()).unwrap();
        assert_eq!(
            backup,
            Some(crate::backup_configs::slot_path(&target, 0)),
            "backup path is the uniform slot 0"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole restore chain: three rotates keep two prior trees plus
    /// the live one moved aside; legacy timestamped backups are pruned
    /// to the same cap (shared `backup_configs` behaviour).
    #[test]
    fn restore_rotation_keeps_the_configured_copies() {
        let dir = std::env::temp_dir().join(format!("ursine-restore-rot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
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
    fn rejects_relative_and_traversal_paths() {
        assert!(validated_path("relative", "snapshot").is_err());
        assert!(validated_path("/a/../b", "snapshot").is_err());
        assert!(validated_path("/", "target").is_err());
        assert!(validated_path("/snapshots/home.1", "snapshot").is_ok());
    }
}
