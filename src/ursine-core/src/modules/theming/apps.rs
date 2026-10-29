//! Per-application theme writers (the shells do not cover these):
//! Firefox, VSCodium/VSCode, terminals, cava, Electron flags.
//!
//! Every directory creation goes through `mkdir_owned`; every merge is
//! idempotent; hand-edited files without our marker are preserved.

use std::path::{Component, Path, PathBuf};

use ursine_api::{ApiError, ThemeSnapshot, ThemingTarget};

use super::apply::{Applied, BEGIN, CSS_MARKER, END, MARKER};
use super::color::{contrast_on, mix, without_alpha};
use super::write::{
    merge_ini, merge_json, mkdir_owned, read_or_empty, replace_block, replace_ini_section,
    write_owned,
};
use serde_json::json;

/// Route an application target (availability already checked by `apply`).
pub(super) fn write_target(
    target: ThemingTarget,
    home: &Path,
    uid: u32,
    gid: u32,
    snap: &ThemeSnapshot,
) -> Result<Applied, ApiError> {
    match target {
        ThemingTarget::Firefox => firefox(home, uid, gid, snap),
        ThemingTarget::Vscodium => vscodium(home, uid, gid, snap),
        ThemingTarget::Alacritty => alacritty(home, uid, gid, snap),
        ThemingTarget::Kitty => kitty(home, uid, gid, snap),
        ThemingTarget::Ghostty => ghostty(home, uid, gid, snap),
        ThemingTarget::Foot => foot(home, uid, gid, snap),
        ThemingTarget::Wezterm => Ok(Applied::Kept(
            "wezterm.lua has no safe merge point; colors left to the user (see SPEC)".to_string(),
        )),
        ThemingTarget::Cava => cava(home, uid, gid, snap),
        ThemingTarget::Galculator => galculator(home, uid, gid, snap),
        ThemingTarget::Electron => electron(home, uid, gid),
        _ => Err(ApiError::internal("not an application target")),
    }
}

// -- Firefox ---------------------------------------------------------------

/// Resolve a profile path from `profiles.ini` strictly inside `base` (the
/// profiles directory, itself under home): absolute paths and `..` escapes
/// are rejected (a flatpak-confined Firefox controls its own profiles.ini —
/// never trust it for root writes).
fn resolve_profile(base: &Path, rel: &str, is_relative: bool) -> Option<PathBuf> {
    if !is_relative {
        // Absolute paths are rejected outright: profiles must live in-home.
        return None;
    }
    let candidate = base.join(rel);
    let mut depth = 0usize;
    for component in candidate.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir | Component::RootDir => {}
            Component::ParentDir | Component::Prefix(_) => return None,
        }
    }
    if depth == 0 {
        return None;
    }
    Some(candidate)
}

/// Firefox profiles from `profiles.ini` (host + flatpak layouts), parsed
/// per section so `Path=`/`IsRelative=` order never matters.
fn firefox_profiles(home: &Path) -> Vec<PathBuf> {
    let roots = [
        home.join(".mozilla/firefox"),
        home.join(".var/app/org.mozilla.firefox/.mozilla/firefox"),
    ];
    let mut profiles = Vec::new();
    for root in roots {
        let ini = root.join("profiles.ini");
        if !ini.is_file() {
            continue;
        }
        let text = read_or_empty(&ini);
        let mut current_path: Option<String> = None;
        let mut current_relative = true;
        let flush = |path: &mut Option<String>, relative: bool, out: &mut Vec<PathBuf>| {
            if let Some(p) = path.take() {
                // Relative paths resolve against the profiles directory.
                if let Some(resolved) = resolve_profile(&root, &p, relative) {
                    if resolved.is_dir() {
                        out.push(resolved);
                    }
                }
            }
        };
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                flush(&mut current_path, current_relative, &mut profiles);
                current_relative = true;
                continue;
            }
            if let Some(value) = line.strip_prefix("Path=") {
                current_path = Some(value.trim().to_string());
            } else if let Some(value) = line.strip_prefix("IsRelative=") {
                current_relative = value.trim() == "1";
            }
        }
        flush(&mut current_path, current_relative, &mut profiles);
    }
    profiles
}

fn firefox(home: &Path, uid: u32, gid: u32, snap: &ThemeSnapshot) -> Result<Applied, ApiError> {
    let profiles = firefox_profiles(home);
    if profiles.is_empty() {
        return Ok(Applied::Kept("no Firefox profile yet; skipped".to_string()));
    }
    let c = &snap.colors;
    let scheme = if snap.dark { "dark" } else { "light" };
    let on_primary = contrast_on(&c.primary);
    let css_lines = [
        CSS_MARKER.to_string(),
        "/* Uniform Firefox colors from the shell palette. */".to_string(),
        format!(":root {{ color-scheme: {scheme}; }}"),
        "@-moz-document url-prefix(\"about:\") {".to_string(),
        format!(
            "  :root {{ --in-content-page-background: {}; --in-content-page-color: {};",
            c.background, c.foreground
        ),
        format!("    --in-content-primary-button-background: {};", c.primary),
        format!("    --in-content-primary-button-text: {on_primary};"),
        format!("    --in-content-box-background: {};", c.surface),
        format!("    --in-content-border-color: {}; }}", c.outline),
        "}".to_string(),
    ];
    let css = format!("{}\n", css_lines.join("\n"));
    let prefs: Vec<(&str, &str)> = vec![
        (
            "toolkit.legacyUserProfileCustomizations.stylesheets",
            "true",
        ),
        ("ui.systemUsesDarkTheme", if snap.dark { "1" } else { "0" }),
        (
            "browser.theme.content-theme",
            if snap.dark { "0" } else { "1" },
        ),
    ];
    let mut touched = 0;
    let mut preserved = 0;
    for profile in &profiles {
        let chrome = profile.join("chrome");
        mkdir_owned(home, &chrome, uid, gid)?;
        let user_chrome = chrome.join("userChrome.css");
        let existing = read_or_empty(&user_chrome);
        if existing.is_empty() || existing.contains(CSS_MARKER) {
            write_owned(home, &user_chrome, css.as_bytes(), uid, gid)?;
            touched += 1;
        } else {
            // Shell or hand-written userChrome: never clobber.
            preserved += 1;
        }
        let user_js = profile.join("user.js");
        let merged = merge_user_prefs(&read_or_empty(&user_js), &prefs);
        write_owned(home, &user_js, merged.as_bytes(), uid, gid)?;
    }
    let note = if preserved > 0 {
        format!(", {preserved} custom userChrome.css preserved")
    } else {
        String::new()
    };
    Ok(Applied::Wrote(format!(
        "{touched} Firefox profile(s): chrome/userChrome.css + user.js prefs{note}"
    )))
}

/// Merge our `user_pref` lines into `user.js`, preserving every other pref.
fn merge_user_prefs(existing: &str, updates: &[(&str, &str)]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut matched: Vec<&str> = Vec::new();
    if existing.trim().is_empty() {
        out.push(format!("/* {MARKER} */"));
    } else {
        for line in existing.lines() {
            let mut replaced = false;
            for &(key, value) in updates {
                let needle = format!("user_pref(\"{key}\"");
                if line.contains(&needle) {
                    out.push(format!("user_pref(\"{key}\", {value});"));
                    matched.push(key);
                    replaced = true;
                    break;
                }
            }
            if !replaced {
                out.push(line.to_string());
            }
        }
    }
    for &(key, value) in updates {
        if !matched.contains(&key) {
            out.push(format!("user_pref(\"{key}\", {value});"));
        }
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

// -- VSCodium / VSCode -----------------------------------------------------

fn vscodium(home: &Path, uid: u32, gid: u32, snap: &ThemeSnapshot) -> Result<Applied, ApiError> {
    let (rel, label) = if home.join(".config/VSCodium").is_dir() {
        (".config/VSCodium/User/settings.json", "VSCodium")
    } else {
        (".config/Code/User/settings.json", "VSCode")
    };
    let path = home.join(rel);
    let parent = path
        .parent()
        .ok_or_else(|| ApiError::internal("vscodium settings path has no parent"))?;
    if !parent.is_dir() {
        return Ok(Applied::Kept(format!("{label}: no config dir; skipped")));
    }
    let c = &snap.colors;
    let on_primary = contrast_on(&c.primary);
    let ansi = |index: u8| -> String {
        match index {
            0 => without_alpha(&c.background),
            1 => c.error.clone(),
            2 => c.success.clone(),
            3 => c.warning.clone(),
            4 => mix(&c.primary, &c.foreground, 0.35),
            5 => mix(&c.primary, &c.error, 0.5),
            6 => mix(&c.primary, &c.success, 0.5),
            8 => mix(&c.background, &c.foreground, 0.3),
            9 => mix(&c.error, &c.foreground, 0.3),
            10 => mix(&c.success, &c.foreground, 0.3),
            11 => mix(&c.warning, &c.foreground, 0.3),
            12 => mix(&c.primary, &c.foreground, 0.6),
            13 => mix(&c.primary, &c.error, 0.75),
            14 => mix(&c.primary, &c.success, 0.75),
            _ => c.foreground.clone(),
        }
    };
    // Built as a pair list: 40+ keys exceed the json! macro recursion limit.
    let pairs: Vec<(&str, serde_json::Value)> = vec![
        ("editor.background", json!(c.background)),
        ("editor.foreground", json!(c.foreground)),
        ("editorCursor.foreground", json!(c.primary)),
        ("activityBar.background", json!(c.background)),
        ("activityBar.foreground", json!(c.foreground)),
        ("sideBar.background", json!(c.surface)),
        ("sideBar.foreground", json!(c.foreground)),
        ("titleBar.activeBackground", json!(c.background)),
        ("titleBar.activeForeground", json!(c.foreground)),
        ("titleBar.inactiveBackground", json!(c.background)),
        (
            "titleBar.inactiveForeground",
            json!(mix(&c.foreground, &c.background, 0.4)),
        ),
        (
            "statusBar.background",
            json!(mix(&c.background, &c.primary, 0.15)),
        ),
        ("statusBar.foreground", json!(c.foreground)),
        ("panel.background", json!(c.surface)),
        ("panel.border", json!(c.outline)),
        ("tab.activeBackground", json!(c.background)),
        ("tab.activeForeground", json!(c.foreground)),
        ("tab.activeBorderTop", json!(c.primary)),
        ("list.activeSelectionBackground", json!(c.primary)),
        ("list.activeSelectionForeground", json!(on_primary)),
        (
            "list.hoverBackground",
            json!(mix(&c.surface, &c.foreground, 0.08)),
        ),
        ("button.background", json!(c.primary)),
        ("button.foreground", json!(on_primary)),
        (
            "input.background",
            json!(mix(&c.background, &c.foreground, 0.06)),
        ),
        ("input.foreground", json!(c.foreground)),
        ("input.border", json!(c.outline)),
        ("terminal.background", json!(c.background)),
        ("terminal.foreground", json!(c.foreground)),
        ("terminalCursor.foreground", json!(c.primary)),
        ("terminal.ansiBlack", json!(ansi(0))),
        ("terminal.ansiRed", json!(ansi(1))),
        ("terminal.ansiGreen", json!(ansi(2))),
        ("terminal.ansiYellow", json!(ansi(3))),
        ("terminal.ansiBlue", json!(ansi(4))),
        ("terminal.ansiMagenta", json!(ansi(5))),
        ("terminal.ansiCyan", json!(ansi(6))),
        ("terminal.ansiWhite", json!(ansi(7))),
        ("terminal.ansiBrightBlack", json!(ansi(8))),
        ("terminal.ansiBrightRed", json!(ansi(9))),
        ("terminal.ansiBrightGreen", json!(ansi(10))),
        ("terminal.ansiBrightYellow", json!(ansi(11))),
        ("terminal.ansiBrightBlue", json!(ansi(12))),
        ("terminal.ansiBrightMagenta", json!(ansi(13))),
        ("terminal.ansiBrightCyan", json!(ansi(14))),
        ("terminal.ansiBrightWhite", json!(ansi(15))),
    ];
    let mut color_overrides = serde_json::Map::new();
    for (key, value) in pairs {
        color_overrides.insert(key.to_string(), value);
    }
    let mut color_overrides = serde_json::Value::Object(color_overrides);
    if !snap.dark {
        if let Some(map) = color_overrides.as_object_mut() {
            map.insert(
                "editorLineNumber.foreground".to_string(),
                json!(mix(&c.foreground, &c.background, 0.5)),
            );
        }
    }
    // SPEC: only colorCustomizations + the terminal palette — the user's
    // chosen workbench colorTheme stays untouched.
    let existing = read_or_empty(&path);
    let merged = merge_json(
        &existing,
        &[("workbench.colorCustomizations", color_overrides)],
    )?;
    write_owned(home, &path, merged.as_bytes(), uid, gid)?;
    Ok(Applied::Wrote(format!(
        "{label}: workbench.colorCustomizations + terminal palette merged"
    )))
}

// -- Terminals -------------------------------------------------------------

/// Alacritty: DMS's `dank-theme.toml` is only imported when DMS actually
/// won; any other source gets our own generated color file.
fn alacritty(home: &Path, uid: u32, gid: u32, snap: &ThemeSnapshot) -> Result<Applied, ApiError> {
    let dir = home.join(".config/alacritty");
    mkdir_owned(home, &dir, uid, gid)?;
    let config = dir.join("alacritty.toml");
    let dank = dir.join("dank-theme.toml");
    let existing = read_or_empty(&config);

    let import_name = if snap.source == ursine_api::ThemeSource::Dms && dank.is_file() {
        "dank-theme.toml".to_string()
    } else {
        let name = format!("ursine-{}.toml", snap.source.as_str());
        write_owned(
            home,
            &dir.join(&name),
            alacritty_colors(snap).as_bytes(),
            uid,
            gid,
        )?;
        name
    };

    let mut document: toml_edit::DocumentMut = if existing.trim().is_empty() {
        toml_edit::DocumentMut::new()
    } else {
        existing
            .parse()
            .map_err(|e| ApiError::internal(format!("parse alacritty.toml: {e}")))?
    };
    let general = document.entry("general").or_insert_with(toml_edit::table);
    let general_table = general
        .as_table_mut()
        .ok_or_else(|| ApiError::internal("alacritty [general] is not a table"))?;
    let import = general_table.entry("import").or_insert_with(|| {
        toml_edit::Item::Value(toml_edit::Value::Array(toml_edit::Array::new()))
    });
    let array = import
        .as_array_mut()
        .ok_or_else(|| ApiError::internal("alacritty general.import is not an array"))?;
    let present = array
        .iter()
        .any(|v| v.as_str().is_some_and(|s| s == import_name));
    if !present {
        array.push(import_name.clone());
    }
    let rendered = document.to_string();
    write_owned(home, &config, rendered.as_bytes(), uid, gid)?;
    Ok(Applied::Wrote(format!(
        "alacritty.toml imports {import_name}"
    )))
}

fn alacritty_colors(snap: &ThemeSnapshot) -> String {
    let c = &snap.colors;
    let selected = mix(&c.primary, &c.background, 0.6);
    let magenta = mix(&c.primary, &c.error, 0.5);
    let cyan = mix(&c.primary, &c.success, 0.5);
    let dim = mix(&c.background, &c.foreground, 0.3);
    let lines = [
        MARKER.to_string(),
        "[colors.primary]".to_string(),
        format!("background = '{}'", c.background),
        format!("foreground = '{}'", c.foreground),
        String::new(),
        "[colors.cursor]".to_string(),
        format!("text = '{}'", c.background),
        format!("cursor = '{}'", c.primary),
        String::new(),
        "[colors.selection]".to_string(),
        format!("text = '{}'", c.foreground),
        format!("background = '{selected}'"),
        String::new(),
        "[colors.normal]".to_string(),
        format!("black = '{}'", c.background),
        format!("red = '{}'", c.error),
        format!("green = '{}'", c.success),
        format!("yellow = '{}'", c.warning),
        format!("blue = '{}'", c.primary),
        format!("magenta = '{magenta}'"),
        format!("cyan = '{cyan}'"),
        format!("white = '{}'", c.foreground),
        String::new(),
        "[colors.bright]".to_string(),
        format!("black = '{dim}'"),
        format!("red = '{}'", c.error),
        format!("green = '{}'", c.success),
        format!("yellow = '{}'", c.warning),
        format!("blue = '{}'", c.primary),
        format!("magenta = '{magenta}'"),
        format!("cyan = '{cyan}'"),
        format!("white = '{}'", c.foreground),
    ];
    format!("{}\n", lines.join("\n"))
}

/// Kitty: colors in our own file + an uncommented exact `include` line.
fn kitty(home: &Path, uid: u32, gid: u32, snap: &ThemeSnapshot) -> Result<Applied, ApiError> {
    let dir = home.join(".config/kitty");
    mkdir_owned(home, &dir, uid, gid)?;
    let c = &snap.colors;
    let selected = mix(&c.primary, &c.background, 0.6);
    let magenta = mix(&c.primary, &c.error, 0.5);
    let cyan = mix(&c.primary, &c.success, 0.5);
    let dim = mix(&c.background, &c.foreground, 0.3);
    let lines = [
        MARKER.to_string(),
        format!("background {}", c.background),
        format!("foreground {}", c.foreground),
        format!("cursor {}", c.primary),
        format!("selection_background {selected}"),
        format!("selection_foreground {}", c.foreground),
        format!("color0 {}", c.background),
        format!("color1 {}", c.error),
        format!("color2 {}", c.success),
        format!("color3 {}", c.warning),
        format!("color4 {}", c.primary),
        format!("color5 {magenta}"),
        format!("color6 {cyan}"),
        format!("color7 {}", c.foreground),
        format!("color8 {dim}"),
        format!("color9 {}", c.error),
        format!("color10 {}", c.success),
        format!("color11 {}", c.warning),
        format!("color12 {}", c.primary),
        format!("color13 {magenta}"),
        format!("color14 {cyan}"),
        format!("color15 {}", c.foreground),
        format!("active_border_color {}", c.primary),
        format!("inactive_border_color {}", c.outline),
    ];
    let colors = format!("{}\n", lines.join("\n"));
    write_owned(
        home,
        &dir.join("ursine-theme.conf"),
        colors.as_bytes(),
        uid,
        gid,
    )?;
    let conf = dir.join("kitty.conf");
    let existing = read_or_empty(&conf);
    // Exact, uncommented include line (a commented one does not count).
    let included = existing.lines().any(|line| {
        let line = line.trim();
        !line.starts_with('#') && !line.starts_with(';') && line == "include ursine-theme.conf"
    });
    if included {
        return Ok(Applied::Kept(
            "kitty.conf already includes ursine-theme.conf".to_string(),
        ));
    }
    let appended = format!("{existing}\ninclude ursine-theme.conf\n");
    write_owned(home, &conf, appended.as_bytes(), uid, gid)?;
    Ok(Applied::Wrote(
        "kitty: ursine-theme.conf written + included".to_string(),
    ))
}

/// Ghostty: marked block in `~/.config/ghostty/config`.
fn ghostty(home: &Path, uid: u32, gid: u32, snap: &ThemeSnapshot) -> Result<Applied, ApiError> {
    let dir = home.join(".config/ghostty");
    mkdir_owned(home, &dir, uid, gid)?;
    let path = dir.join("config");
    let c = &snap.colors;
    let selected = mix(&c.primary, &c.background, 0.6);
    let lines = [
        format!("background = {}", c.background),
        format!("foreground = {}", c.foreground),
        format!("cursor-color = {}", c.primary),
        format!("selection-background = {selected}"),
        format!("palette = 0={}", c.background),
        format!("palette = 1={}", c.error),
        format!("palette = 2={}", c.success),
        format!("palette = 3={}", c.warning),
        format!("palette = 4={}", c.primary),
        format!("palette = 7={}", c.foreground),
    ];
    let body = format!("{}\n", lines.join("\n"));
    let merged = replace_block(&read_or_empty(&path), BEGIN, END, &body);
    write_owned(home, &path, merged.as_bytes(), uid, gid)?;
    Ok(Applied::Wrote(
        "ghostty config: ursine-theme block written".to_string(),
    ))
}

/// foot: replace the `[colors]` section only when absent or marker-owned.
fn foot(home: &Path, uid: u32, gid: u32, snap: &ThemeSnapshot) -> Result<Applied, ApiError> {
    let dir = home.join(".config/foot");
    mkdir_owned(home, &dir, uid, gid)?;
    let path = dir.join("foot.ini");
    let existing = read_or_empty(&path);
    if let Some(body) = section_bodies(&existing).get("colors") {
        if !body.contains(MARKER) {
            return Ok(Applied::Kept(
                "custom foot [colors] preserved (no marker)".to_string(),
            ));
        }
    }
    let c = &snap.colors;
    let selected = no_hash(&mix(&c.primary, &c.background, 0.6));
    let dim = no_hash(&mix(&c.background, &c.foreground, 0.3));
    let lines = [
        MARKER.to_string(),
        format!("background={}", no_hash(&c.background)),
        format!("foreground={}", no_hash(&c.foreground)),
        format!("cursor={}", no_hash(&c.primary)),
        format!("selection-foreground={}", no_hash(&c.foreground)),
        format!("selection-background={selected}"),
        format!("regular0={}", no_hash(&c.background)),
        format!("regular1={}", no_hash(&c.error)),
        format!("regular2={}", no_hash(&c.success)),
        format!("regular3={}", no_hash(&c.warning)),
        format!("regular4={}", no_hash(&c.primary)),
        format!("regular7={}", no_hash(&c.foreground)),
        format!("bright0={dim}"),
        format!("bright7={}", no_hash(&c.foreground)),
    ];
    let body = format!("{}\n", lines.join("\n"));
    let merged = replace_ini_section(&existing, "colors", &body);
    write_owned(home, &path, merged.as_bytes(), uid, gid)?;
    Ok(Applied::Wrote(
        "foot.ini: [colors] section written".to_string(),
    ))
}

/// cava: replace the `[color]` section only when absent or marker-owned.
fn cava(home: &Path, uid: u32, gid: u32, snap: &ThemeSnapshot) -> Result<Applied, ApiError> {
    let dir = home.join(".config/cava");
    mkdir_owned(home, &dir, uid, gid)?;
    let path = dir.join("config");
    let existing = read_or_empty(&path);
    if let Some(body) = section_bodies(&existing).get("color") {
        if !body.contains(MARKER) {
            return Ok(Applied::Kept(
                "custom cava [color] preserved (no marker)".to_string(),
            ));
        }
    }
    let c = &snap.colors;
    let lines = [
        MARKER.to_string(),
        format!("foreground='{}'", no_hash(&c.foreground)),
        format!("background='{}'", no_hash(&c.background)),
        format!("gradient_color_1='{}'", no_hash(&c.primary)),
        format!("gradient_color_2='{}'", no_hash(&c.secondary)),
    ];
    let body = format!("{}\n", lines.join("\n"));
    let merged = replace_ini_section(&existing, "color", &body);
    write_owned(home, &path, merged.as_bytes(), uid, gid)?;
    Ok(Applied::Wrote("cava: [color] section written".to_string()))
}

/// Collect INI section bodies (header → concatenated content).
fn section_bodies(existing: &str) -> std::collections::HashMap<String, String> {
    let mut map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut current: Option<String> = None;
    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let name = trimmed.trim_matches(['[', ']']).to_string();
            current = Some(name.clone());
            map.entry(name).or_default();
            continue;
        }
        if let Some(name) = &current {
            if let Some(entry) = map.get_mut(name) {
                entry.push_str(line);
                entry.push('\n');
            }
        }
    }
    map
}

/// galculator paints its display from its OWN config (`display_bkg_color`
/// etc.) — the shell theme never reaches it. Merge the five display colors
/// from the palette; everything else in the file is preserved.
fn galculator(home: &Path, uid: u32, gid: u32, snap: &ThemeSnapshot) -> Result<Applied, ApiError> {
    let dir = home.join(".config/galculator");
    mkdir_owned(home, &dir, uid, gid)?;
    let path = dir.join("galculator.conf");
    let existing = read_or_empty(&path);
    let c = &snap.colors;
    let (bg, fg) = (c.background.as_str(), c.foreground.as_str());
    let updates = [
        ("display_bkg_color", format!("\"{bg}\"")),
        ("display_result_color", format!("\"{fg}\"")),
        ("display_stack_color", format!("\"{}\"", mix(fg, bg, 0.65))),
        ("display_module_active_color", format!("\"{}\"", c.primary)),
        (
            "display_module_inactive_color",
            format!("\"{}\"", mix(fg, bg, 0.45)),
        ),
    ];
    let refs: Vec<(&str, &str)> = updates.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let merged = merge_ini(&existing, "general", &refs);
    write_owned(home, &path, merged.as_bytes(), uid, gid)?;
    Ok(Applied::Wrote(
        "galculator display colors from shell palette".to_string(),
    ))
}

/// Electron: keep Chromium on GTK4 so the GTK layers drive its colors.
fn electron(home: &Path, uid: u32, gid: u32) -> Result<Applied, ApiError> {
    let path = home.join(".config/electron-flags.conf");
    let existing = read_or_empty(&path);
    if existing.lines().any(|l| l.trim() == "--gtk-version=4") {
        return Ok(Applied::Kept(
            "electron-flags.conf already requests GTK4".to_string(),
        ));
    }
    let mut text = existing;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("{MARKER}\n--gtk-version=4\n"));
    write_owned(home, &path, text.as_bytes(), uid, gid)?;
    Ok(Applied::Wrote(
        "electron-flags.conf: --gtk-version=4 appended".to_string(),
    ))
}

/// Hex without the leading `#` (foot/cava color syntax).
fn no_hash(hex: &str) -> String {
    without_alpha(hex).trim_start_matches('#').to_string()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir
    use super::*;
    use crate::modules::theming::apply::Applied as Out;
    use ursine_api::{ThemeColors, ThemeFonts, ThemeSource};

    fn wrote(applied: Out) -> String {
        match applied {
            Out::Wrote(detail) => detail,
            Out::Kept(detail) => panic!("expected Wrote, got Kept: {detail}"),
        }
    }

    fn kept(applied: Out) -> String {
        match applied {
            Out::Kept(detail) => detail,
            Out::Wrote(detail) => panic!("expected Kept, got Wrote: {detail}"),
        }
    }

    fn snapshot() -> ThemeSnapshot {
        ThemeSnapshot {
            source: ThemeSource::Dms,
            name: "green".into(),
            dark: true,
            colors: ThemeColors {
                background: "#10140f".into(),
                foreground: "#e0e4db".into(),
                primary: "#4caf50".into(),
                secondary: "#2e7d32".into(),
                surface: "#1b1f19".into(),
                error: "#f2b8b5".into(),
                success: "#4daf4c".into(),
                warning: "#dacb52".into(),
                outline: "#35a639".into(),
            },
            fonts: ThemeFonts {
                ui: "Inter Variable".into(),
                mono: "Fira Code".into(),
                weight: 400,
                scale: 1.0,
            },
            corner_radius: Some(12),
            icon_theme: None,
            cursor: None,
        }
    }

    fn temp_home(tag: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!("ursine-apps-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        home
    }

    #[test]
    fn firefox_writes_chrome_and_merges_prefs() {
        let home = temp_home("ff");
        let profile = home.join(".mozilla/firefox/abc.default");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(
            home.join(".mozilla/firefox/profiles.ini"),
            "[Profile0]\nName=default\nPath=abc.default\nIsRelative=1\n",
        )
        .unwrap();
        std::fs::write(profile.join("user.js"), "user_pref(\"keep.me\", 1);\n").unwrap();
        let detail = wrote(firefox(&home, 0, 0, &snapshot()).unwrap());
        assert!(detail.contains("1 Firefox profile"));
        let css = std::fs::read_to_string(profile.join("chrome/userChrome.css")).unwrap();
        assert!(css.contains("--in-content-page-background: #10140f"));
        let prefs = std::fs::read_to_string(profile.join("user.js")).unwrap();
        assert!(prefs.contains("keep.me"));
        assert!(prefs.contains("toolkit.legacyUserProfileCustomizations"));
        assert!(prefs.contains("ui.systemUsesDarkTheme"));
        // Second run: no duplicate prefs.
        firefox(&home, 0, 0, &snapshot()).unwrap();
        let prefs = std::fs::read_to_string(profile.join("user.js")).unwrap();
        assert_eq!(
            prefs
                .matches("toolkit.legacyUserProfileCustomizations")
                .count(),
            1
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn firefox_rejects_absolute_and_escaping_profiles() {
        // Absolute Path (IsRelative=0) must never be written to.
        assert!(resolve_profile(Path::new("/home/u"), "/etc", false).is_none());
        // `..` escapes are rejected even when marked relative.
        assert!(resolve_profile(Path::new("/home/u"), "../etc", true).is_none());
        assert!(resolve_profile(Path::new("/home/u"), "a/../b", true).is_none());
        let ok = resolve_profile(Path::new("/home/u"), "rel.profile", true);
        assert_eq!(ok, Some(PathBuf::from("/home/u/rel.profile")));
    }

    #[test]
    fn firefox_profiles_ini_order_does_not_matter() {
        let home = temp_home("ff-order");
        let root = home.join(".mozilla/firefox");
        std::fs::create_dir_all(root.join("abs.profile")).unwrap();
        std::fs::create_dir_all(root.join("rel.profile")).unwrap();
        // IsRelative BEFORE Path.
        std::fs::write(
            root.join("profiles.ini"),
            "[Profile0]\nIsRelative=0\nPath=/tmp/evil\n",
        )
        .unwrap();
        // Path first, relative after.
        std::fs::write(
            root.join("profiles1.ini"),
            "[Profile0]\nPath=rel.profile\nIsRelative=1\n",
        )
        .unwrap();
        let profiles = firefox_profiles(&home);
        // The absolute one is rejected; no /tmp write possible.
        assert!(!profiles.iter().any(|p| p.starts_with("/tmp")));
        // Relative from the second ini: the parser reads profiles.ini only,
        // so seed the canonical name for the relative case.
        std::fs::write(
            root.join("profiles.ini"),
            "[Profile0]\nPath=rel.profile\nIsRelative=1\n",
        )
        .unwrap();
        let profiles = firefox_profiles(&home);
        assert_eq!(profiles, vec![root.join("rel.profile")]);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn firefox_preserves_foreign_userchrome() {
        let home = temp_home("ff-custom");
        let profile = home.join(".mozilla/firefox/abc.default");
        std::fs::create_dir_all(profile.join("chrome")).unwrap();
        std::fs::write(
            home.join(".mozilla/firefox/profiles.ini"),
            "[Profile0]\nPath=abc.default\nIsRelative=1\n",
        )
        .unwrap();
        let custom = profile.join("chrome/userChrome.css");
        std::fs::write(&custom, "/* my own chrome */\n").unwrap();
        let detail = wrote(firefox(&home, 0, 0, &snapshot()).unwrap());
        assert!(detail.contains("preserved"));
        assert_eq!(
            std::fs::read_to_string(&custom).unwrap(),
            "/* my own chrome */\n"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn vscodium_merges_colors_without_dropping_user_keys() {
        let home = temp_home("vsc");
        let dir = home.join(".config/VSCodium/User");
        std::fs::create_dir_all(&dir).unwrap();
        // Real-world JSONC: comments + trailing comma.
        std::fs::write(
            dir.join("settings.json"),
            "{\n  // keep autosave\n  \"files.autoSave\": \"afterDelay\",\n}\n",
        )
        .unwrap();
        wrote(vscodium(&home, 0, 0, &snapshot()).unwrap());
        let text = std::fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(text.contains("workbench.colorCustomizations"));
        assert!(text.contains("files.autoSave"));
        assert!(text.contains("\"terminal.ansiRed\": \"#f2b8b5\""));
        // The user's colorTheme choice is never forced.
        assert!(!text.contains("workbench.colorTheme"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn alacritty_import_matches_the_winning_source() {
        let home = temp_home("alac");
        let dir = home.join(".config/alacritty");
        std::fs::create_dir_all(&dir).unwrap();
        // Stale DMS theme file, but WAYLE won: our own file must be imported.
        std::fs::write(
            dir.join("dank-theme.toml"),
            "[colors.primary]\nbackground = '#000'\n",
        )
        .unwrap();
        let mut wayle = snapshot();
        wayle.source = ThemeSource::Wayle;
        wrote(alacritty(&home, 0, 0, &wayle).unwrap());
        let config = std::fs::read_to_string(dir.join("alacritty.toml")).unwrap();
        assert!(config.contains("ursine-wayle.toml"), "{config}");
        assert!(!config.contains("dank-theme.toml"));
        // DMS winner imports the dank file (the original bug fix).
        wrote(alacritty(&home, 0, 0, &snapshot()).unwrap());
        let config = std::fs::read_to_string(dir.join("alacritty.toml")).unwrap();
        assert!(config.contains("dank-theme.toml"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn kitty_appends_include_once_and_ignores_comments() {
        let home = temp_home("kitty");
        let dir = home.join(".config/kitty");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("kitty.conf"),
            "font_size 12\n# include ursine-theme.conf\n",
        )
        .unwrap();
        wrote(kitty(&home, 0, 0, &snapshot()).unwrap());
        let detail = kept(kitty(&home, 0, 0, &snapshot()).unwrap());
        assert!(detail.contains("already"));
        let conf = std::fs::read_to_string(dir.join("kitty.conf")).unwrap();
        let active = conf
            .lines()
            .filter(|l| l.trim() == "include ursine-theme.conf")
            .count();
        assert_eq!(active, 1, "commented include must not count:\n{conf}");
        assert!(conf.contains("font_size 12"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn foot_cava_replace_only_marker_owned_sections() {
        let home = temp_home("foot-cava");
        let foot_dir = home.join(".config/foot");
        std::fs::create_dir_all(&foot_dir).unwrap();
        std::fs::write(
            foot_dir.join("foot.ini"),
            "[main]\nfont=x\n\n[colors]\nbackground=old\n",
        )
        .unwrap();
        let detail = kept(foot(&home, 0, 0, &snapshot()).unwrap());
        assert!(detail.contains("preserved"));
        let text = std::fs::read_to_string(foot_dir.join("foot.ini")).unwrap();
        assert!(text.contains("background=old"), "custom colors stomped");

        let cava_dir = home.join(".config/cava");
        std::fs::create_dir_all(&cava_dir).unwrap();
        wrote(cava(&home, 0, 0, &snapshot()).unwrap());
        let text = std::fs::read_to_string(cava_dir.join("config")).unwrap();
        assert!(text.contains("[color]"));
        assert!(text.contains("foreground='e0e4db'"));
        // Marker-owned cava config re-applies cleanly.
        wrote(cava(&home, 0, 0, &snapshot()).unwrap());
        let text = std::fs::read_to_string(cava_dir.join("config")).unwrap();
        assert_eq!(text.matches("[color]").count(), 1);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn galculator_display_colors_merge_preserve_the_rest() {
        let home = temp_home("galculator");
        let dir = home.join(".config/galculator");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("galculator.conf"),
            "[general]\ndisplay_bkg_color=\"rgb(255,255,255)\"\nbutton_width=40\nmode=0\n",
        )
        .unwrap();
        let detail = wrote(galculator(&home, 0, 0, &snapshot()).unwrap());
        assert!(detail.contains("display colors"));
        let text = std::fs::read_to_string(dir.join("galculator.conf")).unwrap();
        // Quoted like galculator's own writer.
        assert!(text.contains("display_bkg_color=\"#10140f\""), "{text}");
        assert!(text.contains("display_result_color=\"#e0e4db\""));
        assert!(text.contains("display_module_active_color=\"#4caf50\""));
        assert!(!text.contains("rgb(255,255,255)"));
        // Foreign keys untouched.
        assert!(text.contains("button_width=40"));
        assert!(text.contains("mode=0"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn electron_flags_are_added_once() {
        let home = temp_home("electron");
        wrote(electron(&home, 0, 0).unwrap());
        let detail = kept(electron(&home, 0, 0).unwrap());
        assert!(detail.contains("already"));
        let text = std::fs::read_to_string(home.join(".config/electron-flags.conf")).unwrap();
        assert_eq!(text.matches("--gtk-version=4").count(), 1);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn wezterm_reports_no_safe_merge_point() {
        let applied = write_target(
            ThemingTarget::Wezterm,
            Path::new("/nonexistent"),
            0,
            0,
            &snapshot(),
        )
        .unwrap();
        match applied {
            Out::Kept(detail) => assert!(detail.contains("no safe merge point")),
            Out::Wrote(detail) => panic!("wezterm must not write: {detail}"),
        }
    }
}
