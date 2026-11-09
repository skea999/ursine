//! Flatpak override through the `flatpak override` CLI (no keyfile
//! editing — invariant 3: a CLI exists, so we use it).
//!
//! The editor owns four lists (`sockets`, `devices`, `filesystems`,
//! `env`). [`diff`] turns (current state from `flatpak override --show`,
//! wanted state) into the exact flag set; keys we do not own (`shared=…`,
//! [Session Bus Policy] …) are never touched because the flags only
//! mention our four keys.

use ursine_api::{ApiError, FlatpakEnvVar};

/// The four editable lists parsed from an override (file or `--show`).
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct ParsedOverride {
    pub sockets: Vec<String>,
    pub devices: Vec<String>,
    pub filesystems: Vec<String>,
    pub env: Vec<FlatpakEnvVar>,
}

/// `GLib` keyfile string escapes for environment values (`\n`, `\t`,
/// `\r`, `\\`). Anything else keeps its backslash literally.
fn decode_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') | None => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

/// Split `key = value` (`GLib` allows whitespace around `=`). Returns the
/// trimmed key and the raw value.
fn split_assignment(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    Some((key, value))
}

fn strip_list(line: &str, key: &str) -> Option<Vec<String>> {
    let (found, rest) = split_assignment(line)?;
    if found != key {
        return None;
    }
    Some(
        rest.split(';')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// A `#`/`;` comment line (kept verbatim wherever it appears).
fn is_comment(line: &str) -> bool {
    line.starts_with('#') || line.starts_with(';')
}

/// Parse the editable lists out of an override body (the file or the
/// output of `flatpak override --show` — same keyfile format).
pub(super) fn parse(text: &str) -> ParsedOverride {
    let mut parsed = ParsedOverride::default();
    let mut section = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = name.to_string();
            continue;
        }
        match section.as_str() {
            "Context" => {
                if let Some(items) = strip_list(trimmed, "sockets") {
                    parsed.sockets = items;
                } else if let Some(items) = strip_list(trimmed, "devices") {
                    parsed.devices = items;
                } else if let Some(items) = strip_list(trimmed, "filesystems") {
                    parsed.filesystems = items;
                }
            }
            "Environment" => {
                if let Some((name, value)) = split_assignment(trimmed) {
                    if !is_comment(trimmed) {
                        parsed.env.push(FlatpakEnvVar {
                            name: name.to_string(),
                            value: decode_value(value),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    parsed
}

/// Translate the delta between `current` and `wanted` into
/// `flatpak override` flags: `--socket`/`--nosocket`,
/// `--device`/`--nodevice`, `--filesystem`/`--nofilesystem`,
/// `--env`/`--unset-env`. Deterministic order (sockets, devices,
/// filesystems, env) so identical states produce identical argv.
pub(super) fn diff(current: &ParsedOverride, wanted: &ParsedOverride) -> Vec<String> {
    let mut flags = Vec::new();
    list_diff(
        &current.sockets,
        &wanted.sockets,
        "--socket",
        "--nosocket",
        &mut flags,
    );
    list_diff(
        &current.devices,
        &wanted.devices,
        "--device",
        "--nodevice",
        &mut flags,
    );
    list_diff(
        &current.filesystems,
        &wanted.filesystems,
        "--filesystem",
        "--nofilesystem",
        &mut flags,
    );
    for item in &wanted.env {
        let unchanged = current
            .env
            .iter()
            .any(|entry| entry.name == item.name && entry.value == item.value);
        if !unchanged {
            flags.push(format!("--env={}={}", item.name, item.value));
        }
    }
    for item in &current.env {
        if !wanted.env.iter().any(|entry| entry.name == item.name) {
            flags.push(format!("--unset-env={}", item.name));
        }
    }
    flags
}

fn list_diff(
    current: &[String],
    wanted: &[String],
    add: &str,
    remove: &str,
    out: &mut Vec<String>,
) {
    for item in wanted {
        if !current.contains(item) {
            out.push(format!("{add}={item}"));
        }
    }
    for item in current {
        if !wanted.contains(item) {
            out.push(format!("{remove}={item}"));
        }
    }
}

fn token(value: &str, max: usize) -> bool {
    // The value is written into the keyfile: `;` is the list separator
    // and `=`/newline would corrupt the line structure.
    !value.is_empty()
        && value.len() <= max
        && !value
            .chars()
            .any(|c| matches!(c, ';' | '=' | '\n' | '\r' | '\0'))
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Socket/device names for the override keyfile (see [`token`]).
pub(super) fn validate_tokens(items: &[String], max: usize) -> Result<(), ApiError> {
    if items.len() > 64 {
        return Err(ApiError::invalid("too many entries"));
    }
    if items.iter().any(|item| !token(item, max)) {
        return Err(ApiError::invalid("invalid permission token"));
    }
    Ok(())
}

pub(super) fn validate_filesystems(items: &[String]) -> Result<(), ApiError> {
    if items.len() > 128 {
        return Err(ApiError::invalid("too many filesystem entries"));
    }
    for item in items {
        let no_segment_traversal = item
            .split('/')
            .all(|segment| segment != ".." && segment != ".");
        if item.is_empty()
            || item.len() > 1024
            || !item.chars().all(|c| {
                c.is_ascii_alphanumeric()
                    || matches!(c, '.' | '_' | '-' | '/' | '~' | ':' | '+' | ' ')
            })
            || item.contains("//")
            || !no_segment_traversal
        {
            return Err(ApiError::invalid("invalid filesystem grant"));
        }
    }
    Ok(())
}

pub(super) fn validate_env(env: &[FlatpakEnvVar]) -> Result<(), ApiError> {
    if env.len() > 64 {
        return Err(ApiError::invalid("too many environment variables"));
    }
    let mut names: Vec<&str> = Vec::new();
    for var in env {
        let valid_name = !var.name.is_empty()
            && var.name.len() <= 128
            && var
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !var.name.starts_with(|c: char| c.is_ascii_digit());
        if !valid_name {
            return Err(ApiError::invalid("invalid environment variable name"));
        }
        if names.contains(&var.name.as_str()) {
            return Err(ApiError::invalid("duplicate environment variable"));
        }
        names.push(&var.name);
        // Values are encoded on write; only real control characters are
        // impossible to represent (a raw newline cannot live on a line).
        if var.value.len() > 4096 || var.value.chars().any(|c| matches!(c, '\n' | '\r' | '\0')) {
            return Err(ApiError::invalid("invalid environment variable value"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(name: &str, value: &str) -> FlatpakEnvVar {
        FlatpakEnvVar {
            name: name.to_string(),
            value: value.to_string(),
        }
    }

    // Captured from a real override file plus a Session Bus Policy group.
    const EXISTING: &str = "[Context]\n\
        shared=ipc;\n\
        sockets=x11;wayland;\n\
        devices=dri;\n\
        filesystems=~/.ssh/agent:rw;xdg-download\n\
        \n\
        [Session Bus Policy]\n\
        org.freedesktop.Notifications=talk\n";

    #[test]
    fn parses_editable_lists() {
        let parsed = parse(EXISTING);
        assert_eq!(parsed.sockets, ["x11", "wayland"]);
        assert_eq!(parsed.devices, ["dri"]);
        assert_eq!(parsed.filesystems, ["~/.ssh/agent:rw", "xdg-download"]);
        assert!(parsed.env.is_empty());
    }

    #[test]
    fn parses_whitespace_around_equals() {
        let parsed = parse("[Context]\nsockets = wayland\n");
        assert_eq!(parsed.sockets, ["wayland"]);
    }

    #[test]
    fn diff_adds_and_removes_each_owned_key() {
        let current = parse(EXISTING);
        let wanted = ParsedOverride {
            sockets: vec!["wayland".into(), "pulse".into()],
            devices: vec!["dri".into()],
            filesystems: vec!["home".into()],
            env: vec![var("FOO", "bar")],
        };
        let flags = diff(&current, &wanted);
        // added: pulse socket + home fs + FOO; removed: x11 socket,
        // the old filesystems.
        assert_eq!(
            flags,
            [
                "--socket=pulse",
                "--nosocket=x11",
                "--filesystem=home",
                "--nofilesystem=~/.ssh/agent:rw",
                "--nofilesystem=xdg-download",
                "--env=FOO=bar",
            ]
        );
        // Identical states → no flags at all.
        assert!(diff(&current, &current).is_empty());
    }

    #[test]
    fn diff_updates_and_unsets_environment() {
        let current = ParsedOverride {
            env: vec![var("OLD", "1"), var("CHG", "a")],
            ..ParsedOverride::default()
        };
        let wanted = ParsedOverride {
            env: vec![var("CHG", "b")],
            ..ParsedOverride::default()
        };
        let flags = diff(&current, &wanted);
        assert_eq!(flags, ["--env=CHG=b", "--unset-env=OLD"]);
    }

    #[test]
    fn validation_rejects_bad_input() {
        assert!(validate_tokens(&["wayland".into(), "fallback-x11".into()], 64).is_ok());
        assert!(validate_tokens(&["bad token".into()], 64).is_err());
        assert!(validate_tokens(&["a;b".into()], 64).is_err());
        assert!(validate_tokens(&["x".repeat(65)], 64).is_err());
        assert!(validate_filesystems(&["xdg-config/gtk-3.0:ro".into()]).is_ok());
        assert!(validate_filesystems(&["home".into()]).is_ok());
        assert!(validate_filesystems(&["/mnt/My Disk".into()]).is_ok());
        assert!(validate_filesystems(&["foo..bar".into()]).is_ok());
        assert!(validate_filesystems(&["../etc".into()]).is_err());
        assert!(validate_filesystems(&["a/../b".into()]).is_err());
        assert!(validate_filesystems(&["a//b".into()]).is_err());
        assert!(validate_env(&[var("LANG", "en_US.UTF-8")]).is_ok());
        assert!(validate_env(&[var("1BAD", "x")]).is_err());
        assert!(validate_env(&[var("BAD-NAME", "x")]).is_err());
        assert!(validate_env(&[var("OK", "with=equals")]).is_ok());
        assert!(validate_env(&[var("OK", "back\\slash")]).is_ok());
        assert!(validate_env(&[var("OK", "line\nbreak")]).is_err());
        assert!(validate_env(&[var("DUP", "1"), var("DUP", "2")]).is_err());
    }
}
