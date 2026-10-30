//! File writes for theming: atomic replace owned by the target user, plus
//! small merge helpers (INI, flatpak overrides, JSON settings).
//!
//! The daemon runs as root but every artifact belongs to the target user:
//! each temp file is chowned before the rename so no root-owned file ever
//! lands in a user home.

use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path};

use ursine_api::ApiError;

/// Linux `O_NOFOLLOW` (asm-generic/fcntl.h): the temp file must not be a
/// symlink when opened.
const O_NOFOLLOW: i32 = 0o400_000;
/// Read cap for user config files (memory pressure guard).
const READ_CAP: u64 = 8 * 1024 * 1024;

/// Verify `dir` lies under `home` component by component: reject symlinks,
/// reject `..`, create missing directories (owned by the target user).
///
/// Residual TOCTOU: a local attacker racing a directory swap between this
/// check and the rename can still redirect a write; narrowing it fully needs
/// `openat`/`renameat` (out of scope without new deps). Symlinks are never
/// followed at check time.
fn ensure_dir_under(home: &Path, dir: &Path, uid: u32, gid: u32) -> Result<(), ApiError> {
    let home_real = home
        .canonicalize()
        .map_err(|e| ApiError::internal(format!("resolve home {}: {e}", home.display())))?;
    let rel = dir
        .strip_prefix(home)
        .map_err(|_| ApiError::invalid(format!("theming path escapes home: {}", dir.display())))?;
    let mut current = home_real;
    for component in rel.components() {
        let Component::Normal(part) = component else {
            return Err(ApiError::invalid(format!(
                "theming path escapes home: {}",
                dir.display()
            )));
        };
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(ApiError::invalid(format!(
                    "refusing symlinked theming path: {}",
                    current.display()
                )));
            }
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(ApiError::internal(format!(
                    "not a directory: {}",
                    current.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current).map_err(|e| {
                    ApiError::internal(format!("create {}: {e}", current.display()))
                })?;
                give_away(&current, uid, gid)
                    .map_err(|e| ApiError::internal(format!("own {}: {e}", current.display())))?;
            }
            Err(error) => {
                return Err(ApiError::internal(format!(
                    "stat {}: {error}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

/// Write `bytes` to `path` atomically (same-directory temp + rename) and
/// give the file to `uid`/`gid`. Every parent component is verified under
/// `home` first (no symlinks, no `..`).
pub(super) fn write_owned(
    home: &Path,
    path: &Path,
    bytes: &[u8],
    uid: u32,
    gid: u32,
) -> Result<(), ApiError> {
    let dir = path.parent().ok_or_else(|| {
        ApiError::internal(format!("theming path has no parent: {}", path.display()))
    })?;
    ensure_dir_under(home, dir, uid, gid)?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ApiError::internal("theming path has no file name"))?;
    // Per-process temp name: two concurrent writers (GUI + ursinectl) must
    // never delete each other's in-flight file. The stale cleanup below can
    // then only touch this process's own leftovers (crash + pid reuse).
    let tmp = dir.join(format!(".{file_name}.ursine-tmp-{}", std::process::id()));
    // A stale temp from a crashed run must not block the target forever.
    if std::fs::symlink_metadata(&tmp).is_ok() {
        let _ = std::fs::remove_file(&tmp);
    }

    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        give_away(&tmp, uid, gid)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(ApiError::internal(format!(
            "write {}: {error}",
            path.display()
        )));
    }
    Ok(())
}

/// Hand a file to its owner. Already-owned files (dev/test running as the
/// owner) pass; a root daemon that genuinely cannot chown fails loudly.
pub(super) fn give_away(path: &Path, uid: u32, gid: u32) -> std::io::Result<()> {
    match std::os::unix::fs::chown(path, Some(uid), Some(gid)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            if std::fs::metadata(path).is_ok_and(|meta| {
                use std::os::unix::fs::MetadataExt;
                meta.uid() == uid
            }) {
                Ok(())
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

/// Create a directory chain under `home` (no symlinks) owned by `uid`/`gid`.
pub(super) fn mkdir_owned(home: &Path, path: &Path, uid: u32, gid: u32) -> Result<(), ApiError> {
    ensure_dir_under(home, path, uid, gid)
}

/// Read a user file (≤ 8 MiB), treating "missing" as empty.
pub(super) fn read_or_empty(path: &Path) -> String {
    let Ok(file) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut bytes = Vec::new();
    let _ = file.take(READ_CAP).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Merge `updates` into the INI `section` of `existing` (kept lines outside
/// the section are preserved; keys in the section are replaced in place and
/// new keys appended).
pub(super) fn merge_ini(existing: &str, section: &str, updates: &[(&str, &str)]) -> String {
    let header = format!("[{section}]");
    let mut out: Vec<String> = Vec::new();
    let mut matched: Vec<&str> = Vec::new();
    let mut in_section = false;
    let mut section_done = false;

    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_section {
                // Leaving the section: append any update keys not present.
                for &(key, value) in updates {
                    if !matched.contains(&key) {
                        out.push(format!("{key}={value}"));
                        matched.push(key);
                    }
                }
                in_section = false;
                section_done = true;
            }
            if trimmed == header && !section_done {
                // Only the FIRST occurrence of the section is merged;
                // later duplicates pass through untouched.
                in_section = true;
            }
            out.push(line.to_string());
            continue;
        }
        if in_section {
            let mut replaced = false;
            for &(key, value) in updates {
                if let Some(rest) = line.trim_start().strip_prefix(key) {
                    if rest.trim_start().starts_with('=') {
                        out.push(format!("{key}={value}"));
                        matched.push(key);
                        replaced = true;
                        break;
                    }
                }
            }
            if replaced {
                continue;
            }
        }
        out.push(line.to_string());
    }

    if in_section {
        for &(key, value) in updates {
            if !matched.contains(&key) {
                out.push(format!("{key}={value}"));
            }
        }
    } else if !section_done {
        if !out.is_empty() && out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push(header);
        for (key, value) in updates {
            out.push(format!("{key}={value}"));
        }
    }

    let mut text = out.join("\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// Merge a flatpak override `filesystems=` value: existing entries kept,
/// new `entry:ro` entries added (idempotent).
pub(super) fn merge_filesystems(existing: &str, add: &[&str]) -> String {
    let mut entries: Vec<String> = existing
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    for &wanted in add {
        let wanted_path = wanted.split(':').next().unwrap_or(wanted);
        let mut present = false;
        for entry in &mut entries {
            let entry_path = entry.split(':').next().unwrap_or(entry.as_str());
            if entry_path == wanted_path {
                present = true;
                if entry != wanted {
                    // Same path, weaker mode (e.g. rw): upgrade to wanted.
                    *entry = wanted.to_string();
                }
            }
        }
        if !present {
            entries.push(wanted.to_string());
        }
    }
    entries.join(";")
}

/// Strip `//` / `/* */` comments outside of strings (JSONC as written by
/// VSCode/VSCodium settings.json). Trailing commas are dropped in a second
/// pass ([`drop_trailing_commas`]).
fn strip_jsonc(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escape = false;
    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => {
                in_string = true;
                out.push(ch);
            }
            '/' => match chars.peek() {
                Some('/') => {
                    for next in chars.by_ref() {
                        if next == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                }
                Some('*') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '*' && chars.peek() == Some(&'/') {
                            chars.next();
                            out.push('\n');
                            break;
                        }
                    }
                }
                _ => out.push(ch),
            },
            _ => out.push(ch),
        }
    }
    out
}

/// Drop commas whose next non-whitespace character (outside strings) is `}`
/// or `]` — the trailing commas valid JSON never contains.
fn drop_trailing_commas(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escape = false;
    let chars: Vec<char> = input.chars().collect();
    for (index, ch) in chars.iter().copied().enumerate() {
        if in_string {
            out.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push(ch);
            continue;
        }
        if ch == ',' {
            let mut probe = chars.iter().skip(index + 1).copied();
            let mut next = probe.next();
            while next.is_some_and(char::is_whitespace) {
                next = probe.next();
            }
            if matches!(next, Some('}' | ']')) {
                continue; // drop this comma
            }
        }
        out.push(ch);
    }
    out
}

/// Replace (or append) a marked block delimited by `begin`/`end` markers.
///
/// Only the FIRST balanced `begin`..`end` block is ours: a `begin` without a
/// matching `end` leaves the file untouched (never eat user content), and
/// anything after the first `end` passes through verbatim.
pub(super) fn replace_block(existing: &str, begin: &str, end: &str, body: &str) -> String {
    let lines: Vec<&str> = existing.lines().collect();
    let start = lines.iter().position(|l| l.trim() == begin);
    let Some(start) = start else {
        let mut out = existing.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(begin);
        out.push('\n');
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(end);
        out.push('\n');
        return out;
    };
    let stop = lines
        .get(start + 1..)
        .and_then(|tail| tail.iter().position(|l| l.trim() == end))
        .map(|offset| offset + start + 1);
    let Some(stop) = stop else {
        // Unbalanced markers: not safely ours to modify.
        return existing.to_string();
    };

    let mut out = String::new();
    for line in lines.iter().take(start) {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(begin);
    out.push('\n');
    out.push_str(body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(end);
    out.push('\n');
    for line in lines.iter().skip(stop + 1) {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Replace the body of INI `section` with `body` (lines, no header), or
/// append the section when missing. Other sections stay untouched.
pub(super) fn replace_ini_section(existing: &str, section: &str, body: &str) -> String {
    let header = format!("[{section}]");
    let mut out: Vec<String> = Vec::new();
    let mut skipping = false;
    let mut seen = false;
    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if skipping {
                skipping = false;
            }
            // Replace only the FIRST occurrence of the section; later
            // duplicates and their bodies pass through untouched.
            if trimmed == header && !seen {
                skipping = true;
                seen = true;
                out.push(header.clone());
                for body_line in body.lines() {
                    out.push(body_line.to_string());
                }
            } else {
                out.push(line.to_string());
            }
            continue;
        }
        if !skipping {
            out.push(line.to_string());
        }
    }
    if !seen {
        if !out.is_empty() && out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push(header);
        for body_line in body.lines() {
            out.push(body_line.to_string());
        }
    }
    let mut text = out.join("\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// Merge a JSON object file: `updates` replace/add keys, everything else is
/// preserved. Returns the pretty-printed document. JSONC input (comments,
/// trailing commas — `VSCode` settings.json) is tolerated.
pub(super) fn merge_json(
    existing: &str,
    updates: &[(&str, serde_json::Value)],
) -> Result<String, ApiError> {
    let cleaned = drop_trailing_commas(&strip_jsonc(existing));
    let mut value: serde_json::Value = if cleaned.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(&cleaned)
            .map_err(|e| ApiError::internal(format!("parse json settings: {e}")))?
    };
    if !value.is_object() {
        return Err(ApiError::internal("json settings root is not an object"));
    }
    if let Some(object) = value.as_object_mut() {
        for (key, update) in updates {
            object.insert((*key).to_string(), update.clone());
        }
    }
    let mut text = serde_json::to_string_pretty(&value)
        .map_err(|e| ApiError::internal(format!("encode json settings: {e}")))?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir
    use super::*;

    #[test]
    fn ini_replaces_and_appends_keys() {
        let existing = "[Settings]\ngtk-theme-name=Old\nkeep-me=1\n";
        let merged = merge_ini(
            existing,
            "Settings",
            &[("gtk-theme-name", "Adwaita"), ("gtk-font-name", "Inter 11")],
        );
        assert!(merged.contains("gtk-theme-name=Adwaita"));
        assert!(!merged.contains("gtk-theme-name=Old"));
        assert!(merged.contains("keep-me=1"));
        assert!(merged.contains("gtk-font-name=Inter 11"));
    }

    #[test]
    fn ini_creates_missing_section_and_keeps_others() {
        let existing = "[Other]\nfoo=bar\n";
        let merged = merge_ini(existing, "Settings", &[("gtk-cursor-size", "16")]);
        assert!(merged.contains("[Other]\nfoo=bar"));
        assert!(merged.contains("[Settings]\ngtk-cursor-size=16"));
    }

    #[test]
    fn filesystems_merge_is_idempotent() {
        let first = merge_filesystems("", &["xdg-config/gtk-3.0:ro", "xdg-config/gtk-4.0:ro"]);
        let second = merge_filesystems(&first, &["xdg-config/gtk-3.0:ro", "xdg-config/qt5ct:ro"]);
        assert_eq!(second.matches("xdg-config/gtk-3.0:ro").count(), 1);
        assert!(second.contains("xdg-config/qt5ct:ro"));
        assert!(second.contains("xdg-config/gtk-4.0:ro"));
    }

    #[test]
    fn block_replace_is_owned_and_idempotent() {
        let begin = "#ursine-theme begin";
        let end = "#ursine-theme end";
        let first = replace_block("keep=1\n", begin, end, "a=1");
        assert!(first.contains(begin));
        assert!(first.contains("a=1"));
        let second = replace_block(&first, begin, end, "a=2");
        assert_eq!(second.matches(begin).count(), 1);
        assert!(second.contains("a=2"));
        assert!(!second.contains("a=1"));
        assert!(second.contains("keep=1"));
    }

    #[test]
    fn ini_section_replace_keeps_other_sections() {
        let existing = "[colors]\nold=1\n\n[general]\nsize=2\n";
        let replaced = replace_ini_section(existing, "colors", "foreground=#10140f");
        assert!(replaced.contains("[colors]\nforeground=#10140f"));
        assert!(!replaced.contains("old=1"));
        assert!(replaced.contains("[general]\nsize=2"));
        let appended = replace_ini_section("size=2\n", "colors", "foreground=#10140f");
        assert!(appended.contains("[colors]\nforeground=#10140f"));
    }

    #[test]
    fn json_merge_preserves_foreign_keys() {
        let existing = r#"{ "files.exclude": { "**/.git": true } }"#;
        let merged = merge_json(
            existing,
            &[("workbench.colorCustomizations", serde_json::json!({"a": 1}))],
        )
        .unwrap();
        assert!(merged.contains("workbench.colorCustomizations"));
        assert!(merged.contains("**/.git"));
    }

    #[test]
    fn write_owned_is_atomic_and_replaces() {
        let dir = std::env::temp_dir().join(format!("ursine-theme-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nested/out.ini");
        let home = dir.clone();
        write_owned(&home, &path, b"first\n", 0, 0).unwrap();
        write_owned(&home, &path, b"second\n", 0, 0).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second\n");
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains("ursine-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp file leaked: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
