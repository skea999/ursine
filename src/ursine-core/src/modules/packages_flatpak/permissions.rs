//! Flatpak override keyfile editing.
//!
//! The editor owns four lists (`sockets`, `devices`, `filesystems`,
//! `env`) and must preserve every other group/key the user or another
//! tool wrote ([Session Bus Policy], `shared=…`, comments):
//! `flatpak override --reset` would delete those, so we render the
//! keyfile ourselves and write it atomically.

use ursine_api::{ApiError, FlatpakEnvVar};

/// The four editable lists parsed from an override keyfile.
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

fn encode_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
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

/// Parse the editable lists out of an override keyfile.
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

fn dedup(items: &[String]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for item in items {
        if !seen.contains(item) {
            seen.push(item.clone());
        }
    }
    seen
}

fn list_line(key: &str, items: &[String]) -> Option<String> {
    if items.is_empty() {
        None
    } else {
        Some(format!("{key}={}", items.join(";")))
    }
}

/// True when `line` assigns one of the three owned `[Context]` keys
/// (key compared trimmed, so `sockets = x` is owned too).
fn owned_context_line(line: &str) -> bool {
    split_assignment(line)
        .is_some_and(|(key, _)| matches!(key, "sockets" | "devices" | "filesystems"))
}

/// Render `existing` with the four lists replaced; every other line and
/// group (including comments) is preserved.
pub(super) fn render(
    existing: &str,
    sockets: &[String],
    devices: &[String],
    filesystems: &[String],
    env: &[FlatpakEnvVar],
) -> String {
    let sockets = dedup(sockets);
    let devices = dedup(devices);
    let filesystems = dedup(filesystems);

    let mut preamble: Vec<String> = Vec::new();
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    let mut current: Option<(String, Vec<String>)> = None;
    for line in existing.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            if let Some(section) = current.take() {
                sections.push(section);
            }
            current = Some((name.to_string(), Vec::new()));
            continue;
        }
        match &mut current {
            None => {
                if !trimmed.is_empty() {
                    preamble.push(line.to_string());
                }
            }
            Some((name, lines)) => {
                // The three owned Context keys are rebuilt below (every
                // duplicate dropped); [Environment] is rebuilt from `env`
                // except comments. Foreign keys/groups stay untouched.
                let drop = match name.as_str() {
                    "Context" => owned_context_line(trimmed),
                    "Environment" => !is_comment(trimmed) && split_assignment(trimmed).is_some(),
                    _ => false,
                };
                if !drop {
                    lines.push(line.to_string());
                }
            }
        }
    }
    if let Some(section) = current.take() {
        sections.push(section);
    }

    // Ensure a Context section exists to host the three lists.
    if !sections.iter().any(|(name, _)| name == "Context") {
        sections.push(("Context".to_string(), Vec::new()));
    }
    for (name, lines) in &mut sections {
        if name != "Context" {
            continue;
        }
        for line in [
            list_line("sockets", &sockets),
            list_line("devices", &devices),
            list_line("filesystems", &filesystems),
        ]
        .into_iter()
        .flatten()
        {
            lines.push(line);
        }
    }

    // Rebuild Environment only when the caller set variables; comments
    // inside the section survive either way.
    sections.retain(|(name, lines)| !(name == "Environment" && lines.is_empty()));
    if let Some(section) = sections.iter_mut().find(|(name, _)| name == "Environment") {
        for var in env {
            section
                .1
                .push(format!("{}={}", var.name, encode_value(&var.value)));
        }
    } else if !env.is_empty() {
        let lines = env
            .iter()
            .map(|var| format!("{}={}", var.name, encode_value(&var.value)))
            .collect();
        sections.push(("Environment".to_string(), lines));
    }

    let mut out = String::new();
    for line in preamble {
        out.push_str(&line);
        out.push('\n');
    }
    for (name, lines) in &sections {
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push('[');
        out.push_str(name);
        out.push_str("]\n");
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
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

/// Filesystem grants: paths plus flatpak keywords (`home`, `xdg-*`,
/// `host`, …) and an optional `:ro`/`:rw` suffix. `..` is rejected as a
/// path segment, not as a substring (`foo..bar` is a legal name).
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
    fn render_preserves_foreign_keys_and_groups() {
        let out = render(EXISTING, &[], &[], &[], &[]);
        assert!(out.contains("shared=ipc;"), "{out}");
        assert!(out.contains("[Session Bus Policy]"), "{out}");
        assert!(out.contains("org.freedesktop.Notifications=talk"), "{out}");
        // Owned keys with empty lists are dropped (no override).
        assert!(!out.contains("sockets="), "{out}");
        assert!(!out.contains("devices="), "{out}");
        assert!(!out.contains("filesystems="), "{out}");
    }

    #[test]
    fn render_replaces_whitespace_and_duplicate_owned_keys() {
        let existing = "[Context]\nsockets = x11\nsockets=wayland\nforeign=1\n";
        let out = render(existing, &["pulse".into()], &[], &[], &[]);
        assert_eq!(out.matches("sockets").count(), 1, "{out}");
        assert!(out.contains("sockets=pulse"), "{out}");
        assert!(!out.contains("x11"), "{out}");
        assert!(out.contains("foreign=1"), "{out}");
    }

    #[test]
    fn render_replaces_lists_and_adds_environment() {
        let out = render(
            EXISTING,
            &["wayland".to_string()],
            &["dri".to_string(), "kvm".to_string()],
            &["xdg-download".to_string()],
            &[var("FOO", "bar"), var("EMPTY", "")],
        );
        assert!(out.contains("sockets=wayland"), "{out}");
        assert!(!out.contains("x11"), "{out}");
        assert!(out.contains("devices=dri;kvm"), "{out}");
        assert!(out.contains("filesystems=xdg-download"), "{out}");
        assert!(out.contains("[Environment]"), "{out}");
        assert!(out.contains("FOO=bar"), "{out}");
        assert!(out.contains("EMPTY="), "{out}");
        assert!(out.contains("shared=ipc;"), "{out}");
    }

    #[test]
    fn environment_comments_survive_and_values_roundtrip() {
        let existing = "[Environment]\n# keep me\nFOO=a\\nb\n";
        let out = render(existing, &[], &[], &[], &[var("BAR", "x\ty")]);
        assert!(out.contains("# keep me"), "{out}");
        assert!(!out.contains("FOO="), "{out}");
        assert!(out.contains("BAR=x\\ty"), "{out}");
        let parsed = parse(&out);
        assert_eq!(parsed.env, [var("BAR", "x\ty")]);
        // Escaped pre-existing values decode for the editor.
        let parsed = parse(existing);
        assert_eq!(parsed.env, [var("FOO", "a\nb")]);
    }

    #[test]
    fn render_from_empty_builds_context() {
        let out = render("", &["wayland".into()], &[], &[], &[]);
        assert!(out.starts_with("[Context]\n"), "{out}");
        assert!(out.contains("sockets=wayland"), "{out}");
        assert!(!out.contains("[Environment]"), "{out}");
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
