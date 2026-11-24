//! Atomic file and symlink replacement shared by the profile/config editors.

#![allow(clippy::redundant_pub_crate)] // private module: pub(crate) is the real visibility

use std::path::Path;

use ursine_api::ApiError;

/// Replace `path` through a pid-unique temp file in the same directory.
///
/// The full file name is part of the temp name (`usr.bin.foo` and
/// `usr.bin.bar` must never share a temp path inside one process). Write,
/// chmod and rename all clean the temp up on failure; callers rotate their
/// backups before calling this.
pub(crate) fn write(path: &Path, text: &str) -> Result<(), ApiError> {
    replace(path, Some(0o644), |tmp| std::fs::write(tmp, text))
}

/// Point the symlink at `path` to `target`, replacing it atomically.
///
/// `/etc/localtime` is read by every process that asks for the local zone and
/// is usually a symlink already; `rename` over it swaps the link itself, so no
/// reader ever resolves a missing link. No mode is set: `chmod` would follow
/// the link and rewrite the permissions of the zoneinfo file it points at.
pub(crate) fn symlink(path: &Path, target: &Path) -> Result<(), ApiError> {
    replace(path, None, |tmp| std::os::unix::fs::symlink(target, tmp))
}

/// The one replacement sequence: build the temp path, let `put` create it,
/// set the mode when the result is a regular file, then rename it over the
/// target. Every failure removes the temp file, so a retry never trips over
/// a leftover.
fn replace(
    path: &Path,
    mode: Option<u32>,
    put: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<(), ApiError> {
    let file_name = path
        .file_name()
        .map_or_else(|| "file".to_string(), |n| n.to_string_lossy().into_owned());
    let tmp = path.with_file_name(format!("{file_name}.ursine-new.{}", std::process::id()));
    let cleanup = |error: ApiError| -> ApiError {
        let _ = std::fs::remove_file(&tmp);
        error
    };
    put(&tmp).map_err(|e| cleanup(ApiError::internal(format!("write {}: {e}", tmp.display()))))?;
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))
            .map_err(|e| cleanup(ApiError::internal(format!("chmod {}: {e}", tmp.display()))))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        cleanup(ApiError::internal(format!(
            "replace {}: {e}",
            path.display()
        )))
    })
}
