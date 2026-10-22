//! Restore a btrbk snapshot with `btrfs subvolume snapshot`.
//!
//! btrbk 0.32 has no restore command. The spec allows this explicit fallback:
//! validate both paths, preview the exact argv, then run btrfs as root.

use std::path::{Component, Path};

use serde_json::json;
use ursine_api::ApiError;

use super::btrbk;

pub(super) fn preview(snapshot: &str, target: &str) -> Result<serde_json::Value, ApiError> {
    let snapshot_path = validated_path(snapshot, "snapshot")?;
    let target_path = validated_path(target, "target")?;

    let meta = std::fs::symlink_metadata(snapshot_path)
        .map_err(|e| ApiError::invalid(format!("snapshot is not accessible: {e}")))?;
    if !meta.is_dir() {
        return Err(ApiError::invalid("snapshot is not a directory"));
    }
    if target_path.exists() {
        return Err(ApiError::new(
            ursine_api::ErrorCode::Conflict,
            "restore target already exists",
        ));
    }

    Ok(json!({
        "dry_run": true,
        "snapshot": snapshot,
        "target": target,
        "command": format!("btrfs subvolume snapshot {snapshot} {target}"),
    }))
}

pub(super) async fn apply(snapshot: &str, target: &str) -> Result<serde_json::Value, ApiError> {
    // Re-run the preview checks: the path can change between confirm and apply.
    let _ = preview(snapshot, target)?;
    let program = btrbk::btrfs_path()?;
    let out = crate::modules::packages_nix::exec::run_root_program(
        program,
        &["subvolume", "snapshot", snapshot, target],
        btrbk::MUTATE_TIMEOUT,
    )
    .await?;
    Ok(json!({
        "dry_run": false,
        "snapshot": snapshot,
        "target": target,
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
