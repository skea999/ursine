//! Uniform file-backup rotation: at most `backup_configs.copies` previous
//! versions per file, same naming scheme everywhere.
//!
//! Slots for target `<file>`: `<file>.ursine-backup` (newest), then
//! `<file>.ursine-backup.1` … `<file>.ursine-backup.(copies-1)` (oldest).
//! Every writer rotates immediately BEFORE replacing the live file, so a
//! failed rotation aborts the write (exit 42 in the shell scripts) and the
//! live file stays untouched. Legacy timestamped backups from older
//! releases (`<file>.ursine-backup-<stamp>`) are pruned to the same cap.

use std::path::{Path, PathBuf};

use ursine_api::{ApiError, UrsineConfig};

/// Lower/upper clamp for `backup_configs.copies`.
pub const MIN_COPIES: usize = 1;
pub const MAX_COPIES: usize = 10;

/// Effective copy count from the daemon configuration, clamped to
/// [`MIN_COPIES`]..=[`MAX_COPIES`]. Absent config → 3.
pub fn copies_from(config: &UrsineConfig) -> usize {
    (config.backup_configs.copies as usize).clamp(MIN_COPIES, MAX_COPIES)
}

/// Slot path for `index` 0 (the `.ursine-backup` name) or > 0 (`.N` suffix).
pub fn slot_path(target: &Path, index: usize) -> PathBuf {
    let mut name = target.as_os_str().to_os_string();
    if index == 0 {
        name.push(".ursine-backup");
    } else {
        name.push(format!(".ursine-backup.{index}"));
    }
    PathBuf::from(name)
}

/// Shift the backup chain one step older (slot `i-1` → slot `i` for
/// `i = copies-1 … 1`; directories included — `rename` works for both),
/// drop stale slots left by a higher configured count, then run `insert`
/// which puts the newest backup into slot 0, then prune legacy
/// timestamped backups down to the remaining allowance.
fn rotate_common(
    target: &Path,
    copies: usize,
    insert: impl FnOnce(&Path) -> Result<(), ApiError>,
) -> Result<(), ApiError> {
    let copies = copies.clamp(MIN_COPIES, MAX_COPIES);
    // Oldest slot first so a slot is free before its newer neighbour moves in.
    for index in (1..copies).rev() {
        let from = slot_path(target, index - 1);
        if !from.exists() {
            continue;
        }
        let to = slot_path(target, index);
        if to.exists() {
            let removed = if to.is_dir() {
                std::fs::remove_dir_all(&to)
            } else {
                std::fs::remove_file(&to)
            };
            // A concurrent writer (GUI + ursinectl) may remove the slot
            // between the check and the call: NotFound is benign.
            match removed {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(rotation_error(&to, &e)),
            }
        }
        match std::fs::rename(&from, &to) {
            Ok(()) => {}
            // Same race: the source vanished under us — the other writer
            // already shifted it; keep going instead of failing the write.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(rotation_error(&from, &e)),
        }
    }
    // Slots left over from a higher configured count are stale: drop them.
    for index in copies..=MAX_COPIES {
        let stale = slot_path(target, index);
        if stale.is_dir() {
            let _ = std::fs::remove_dir_all(&stale);
        } else if stale.exists() {
            let _ = std::fs::remove_file(&stale);
        }
    }
    insert(&slot_path(target, 0))?;
    prune_legacy(target, copies);
    Ok(())
}

/// File rotation: copy the live file into slot 0 before it is replaced.
/// `owner` re-chowns slot 0 (theming writes user-owned files as root).
/// Missing target → no-op (first write has nothing to back up).
pub fn rotate(target: &Path, copies: usize, owner: Option<(u32, u32)>) -> Result<(), ApiError> {
    if !target.exists() {
        return Ok(());
    }
    rotate_common(target, copies, |slot0| copy_path(target, slot0, owner))
}

/// Async wrapper for module handlers: runs [`rotate`] on the blocking
/// pool so a slow directory copy never stalls the async executor.
pub async fn rotate_async(
    target: PathBuf,
    copies: usize,
    owner: Option<(u32, u32)>,
) -> Result<(), ApiError> {
    tokio::task::spawn_blocking(move || rotate(&target, copies, owner))
        .await
        .map_err(|e| ApiError::internal(format!("backup task failed: {e}")))?
}

/// Directory/file MOVE rotation for restores: the live target itself becomes
/// slot 0 (the caller then re-creates the target from a snapshot). Returns
/// the slot-0 path so a failed restore can move it back.
pub fn rotate_move(target: &Path, copies: usize) -> Result<PathBuf, ApiError> {
    if !target.exists() {
        return Err(ApiError::internal(format!(
            "backup rotation target missing: {}",
            target.display()
        )));
    }
    let slot0 = slot_path(target, 0);
    rotate_common(target, copies, |_| {
        std::fs::rename(target, &slot0).map_err(|e| rotation_error(target, &e))?;
        Ok(())
    })?;
    Ok(slot0)
}

/// Content rotation for data with no live file on disk (crontab): shift the
/// chain, then store `bytes` as slot 0 with `mode`.
pub fn rotate_store(target: &Path, copies: usize, bytes: &[u8], mode: u32) -> Result<(), ApiError> {
    rotate_common(target, copies, |slot0| {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        // Create with the final mode up front: no window where the
        // crontab copy is more permissive than requested.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(mode)
            .open(slot0)
            .map_err(|e| rotation_error(slot0, &e))?;
        file.write_all(bytes)
            .map_err(|e| rotation_error(slot0, &e))?;
        file.sync_all().map_err(|e| rotation_error(slot0, &e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(slot0, std::fs::Permissions::from_mode(mode))
                .map_err(|e| rotation_error(slot0, &e))?;
        }
        let _ = mode;
        Ok(())
    })
}

/// Keep legacy `<name>.ursine-backup-<stamp>` siblings (older releases used
/// unbounded timestamped names) only up to the allowance the slot chain did
/// not already consume: `copies - existing_slots`. Newest stamps win.
fn prune_legacy(target: &Path, copies: usize) {
    let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
        return;
    };
    if std::fs::read_dir(dir).is_err() {
        return;
    }
    let name = name.to_string_lossy();
    // Pre-uniform schemes: the old dot-prefixed slots from the previous
    // atomic-write script (the shell scripts drop their own copy of these;
    // Rust writers drop them here) — unconditional, they cannot come back
    // under the single mechanism.
    let dot_prefix = format!(".{name}.ursine-backup");
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        let stale_dot_slot = file_name == dot_prefix
            || file_name
                .strip_prefix(&format!("{dot_prefix}."))
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if stale_dot_slot {
            let path = entry.path();
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    // Old scanners backups were hidden (leading dot): match both forms.
    let prefixes = [
        format!("{name}.ursine-backup-"),
        format!(".{name}.ursine-backup-"),
    ];
    let mut legacy: Vec<(u128, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        let Some(stamp) = prefixes.iter().find_map(|p| file_name.strip_prefix(p)) else {
            continue;
        };
        if let Ok(stamp) = stamp.parse::<u128>() {
            legacy.push((stamp, entry.path()));
        }
    }
    if legacy.is_empty() {
        return;
    }
    let slots_used = (0..copies)
        .filter(|index| slot_path(target, *index).exists())
        .count();
    let allowance = copies.saturating_sub(slots_used);
    legacy.sort_by_key(|(stamp, _)| std::cmp::Reverse(*stamp));
    for (_, path) in legacy.into_iter().skip(allowance) {
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

fn rotation_error(path: &Path, error: &std::io::Error) -> ApiError {
    ApiError::internal(format!(
        "backup rotation failed for {}: {error}",
        path.display()
    ))
}

/// Copy a file or a whole directory tree into slot 0, preserving permission
/// bits (directories included) and optionally re-owning everything to
/// `owner` — root-created backups inside a user home must stay user-owned.
fn copy_path(src: &Path, dst: &Path, owner: Option<(u32, u32)>) -> Result<(), ApiError> {
    if src.is_dir() {
        copy_dir(src, dst, owner)
    } else {
        std::fs::copy(src, dst).map_err(|e| rotation_error(src, &e))?;
        if let Some((uid, gid)) = owner {
            chown(dst, uid, gid)?;
        }
        Ok(())
    }
}

fn copy_dir(src: &Path, dst: &Path, owner: Option<(u32, u32)>) -> Result<(), ApiError> {
    std::fs::create_dir_all(dst).map_err(|e| rotation_error(dst, &e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(src)
            .map_err(|e| rotation_error(src, &e))?
            .permissions()
            .mode();
        let _ = std::fs::set_permissions(dst, std::fs::Permissions::from_mode(mode));
    }
    for entry in std::fs::read_dir(src).map_err(|e| rotation_error(src, &e))? {
        let entry = entry.map_err(|e| rotation_error(src, &e))?;
        copy_path(&entry.path(), &dst.join(entry.file_name()), owner)?;
    }
    if let Some((uid, gid)) = owner {
        chown(dst, uid, gid)?;
    }
    Ok(())
}

fn chown(path: &Path, uid: u32, gid: u32) -> Result<(), ApiError> {
    #[cfg(unix)]
    std::os::unix::fs::chown(path, Some(uid), Some(gid)).map_err(|e| rotation_error(path, &e))?;
    #[cfg(not(unix))]
    let _ = (path, uid, gid);
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ursine-backup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn copies_from_clamps_to_the_supported_range() {
        let mut config = UrsineConfig::default();
        assert_eq!(copies_from(&config), 3);
        config.backup_configs.copies = 0;
        assert_eq!(copies_from(&config), MIN_COPIES);
        config.backup_configs.copies = 999;
        assert_eq!(copies_from(&config), MAX_COPIES);
        config.backup_configs.copies = 5;
        assert_eq!(copies_from(&config), 5);
    }

    #[test]
    fn rotate_keeps_the_newest_copies_and_shifts_older_ones_out() {
        // Real writer flow: rotate (backs up the CURRENT live file), then
        // replace it — five saves with a three-copy cap keep gen2..gen4.
        let dir = dir("shift");
        let target = dir.join("file.conf");
        std::fs::write(&target, "gen1\n").unwrap();
        for generation in 2..=5 {
            rotate(&target, 3, None).unwrap();
            std::fs::write(&target, format!("gen{generation}\n")).unwrap();
        }
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "gen5\n");
        assert_eq!(
            std::fs::read_to_string(slot_path(&target, 0)).unwrap(),
            "gen4\n"
        );
        assert_eq!(
            std::fs::read_to_string(slot_path(&target, 1)).unwrap(),
            "gen3\n"
        );
        assert_eq!(
            std::fs::read_to_string(slot_path(&target, 2)).unwrap(),
            "gen2\n"
        );
        assert!(
            !slot_path(&target, 3).exists(),
            "gen1 must have rotated out of the 3-copy chain"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotate_is_a_noop_for_a_missing_file_and_for_one_copy() {
        let dir = dir("noop");
        let missing = dir.join("never.conf");
        rotate(&missing, 3, None).unwrap();
        assert!(!slot_path(&missing, 0).exists());

        // copies = 1: the single slot always holds the previous version,
        // no .1 ever appears.
        let target = dir.join("one.conf");
        std::fs::write(&target, "a\n").unwrap();
        rotate(&target, 1, None).unwrap();
        std::fs::write(&target, "b\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(slot_path(&target, 0)).unwrap(),
            "a\n"
        );
        rotate(&target, 1, None).unwrap();
        std::fs::write(&target, "c\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(slot_path(&target, 0)).unwrap(),
            "b\n"
        );
        assert!(!slot_path(&target, 1).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lowering_the_count_prunes_stale_higher_slots() {
        let dir = dir("shrink");
        let target = dir.join("file.conf");
        for generation in 1..=4 {
            std::fs::write(&target, format!("gen{generation}\n")).unwrap();
            rotate(&target, 4, None).unwrap();
        }
        assert!(slot_path(&target, 3).exists());
        // Same file now rotated with copies = 2: .2/.3 must disappear.
        std::fs::write(&target, "gen5\n").unwrap();
        rotate(&target, 2, None).unwrap();
        assert!(slot_path(&target, 1).exists());
        assert!(!slot_path(&target, 2).exists());
        assert!(!slot_path(&target, 3).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotate_move_shifts_directories_and_returns_slot_zero() {
        let dir = dir("move");
        let target = dir.join("tree");
        // Two prior generations parked in the chain as directories.
        std::fs::create_dir_all(slot_path(&target, 0)).unwrap();
        std::fs::write(slot_path(&target, 0).join("v"), "older\n").unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("v"), "live\n").unwrap();

        let slot0 = rotate_move(&target, 2).unwrap();
        assert_eq!(slot0, slot_path(&target, 0));
        assert!(!target.exists(), "live tree moved into slot 0");
        assert_eq!(std::fs::read_to_string(slot0.join("v")).unwrap(), "live\n");
        // The previous slot 0 shifted to slot 1; the oldest (none) is gone.
        assert_eq!(
            std::fs::read_to_string(slot_path(&target, 1).join("v")).unwrap(),
            "older\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotate_store_writes_slot_zero_with_the_given_mode() {
        let dir = dir("store");
        let target = dir.join("crontab.alice");
        rotate_store(&target, 2, b"first\n", 0o600).unwrap();
        rotate_store(&target, 2, b"second\n", 0o600).unwrap();
        assert_eq!(
            std::fs::read_to_string(slot_path(&target, 0)).unwrap(),
            "second\n"
        );
        assert_eq!(
            std::fs::read_to_string(slot_path(&target, 1)).unwrap(),
            "first\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(slot_path(&target, 0))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "crontab backups stay private");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotate_copies_whole_directories_recursively() {
        let dir = dir("dir-copy");
        let target = dir.join("entry");
        std::fs::create_dir_all(target.join("v1")).unwrap();
        std::fs::write(target.join("v1/is_dark"), "true\n").unwrap();
        rotate(&target, 2, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(slot_path(&target, 0).join("v1/is_dark")).unwrap(),
            "true\n"
        );
        assert!(target.exists(), "rotate copies, never moves");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_timestamped_backups_are_pruned_to_the_cap() {
        let dir = dir("legacy");
        let target = dir.join("home");
        // Three legacy timestamped dirs from the old scheme (oldest … newest).
        for stamp in [100u128, 200, 300] {
            let path = dir.join(format!("home.ursine-backup-{stamp}"));
            std::fs::create_dir_all(&path).unwrap();
        }
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("v"), "live\n").unwrap();
        // First rotation under the new scheme: slot 0 holds the live tree,
        // allowance for legacy = copies(3) - slots(1) = 2 → oldest drops.
        rotate_move(&target, 3).unwrap();
        assert!(!dir.join("home.ursine-backup-100").exists());
        assert!(dir.join("home.ursine-backup-200").exists());
        assert!(dir.join("home.ursine-backup-300").exists());
        // Chain fills up over the next restores → all legacy goes.
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("v"), "live2\n").unwrap();
        rotate_move(&target, 3).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("v"), "live3\n").unwrap();
        rotate_move(&target, 3).unwrap();
        assert!(!dir.join("home.ursine-backup-200").exists());
        assert!(!dir.join("home.ursine-backup-300").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
