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
    let (_, _, backup) = plan(snapshot, target)?;
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
    use super::*;

    #[test]
    fn rejects_relative_and_traversal_paths() {
        assert!(validated_path("relative", "snapshot").is_err());
        assert!(validated_path("/a/../b", "snapshot").is_err());
        assert!(validated_path("/", "target").is_err());
        assert!(validated_path("/snapshots/home.1", "snapshot").is_ok());
    }
}
