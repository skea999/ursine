//! Propagation writers: one pass over every target, each result reported.

use std::path::Path;

use ursine_api::{
    ApiError, ApplyOutcome, TargetResult, TargetStatus, ThemeApplyReport, ThemeSnapshot,
    ThemingTarget,
};

use super::apps;
use super::color::{contrast_on, mix, without_alpha};
use super::cosmic;
use super::write::{
    give_away, merge_filesystems, merge_ini, mkdir_owned, read_or_empty, write_owned,
};
use super::{select_source, snapshot_for, target_statuses};

/// Marker line: files we own carry it so a hand-edited file is never stomped.
pub(super) const MARKER: &str = "# Written by Ursine (uniform theming, SPEC-theming)";
pub(super) const BEGIN: &str = "#ursine-theme begin";
pub(super) const END: &str = "#ursine-theme end";

pub(super) fn run_blocking(home: &Path, uid: u32, gid: u32) -> Result<ThemeApplyReport, ApiError> {
    let source = select_source(home, uid).ok_or_else(|| {
        ApiError::unavailable(
            "no supported desktop shell config (Wayle or DankMaterialShell)",
            "install wayle or DankMaterialShell and sign in once",
        )
    })?;
    let snapshot = snapshot_for(source, home)?;
    let statuses = target_statuses(home);
    let mut results = Vec::with_capacity(statuses.len());
    for status in &statuses {
        results.push(write_target(status, home, uid, gid, &snapshot, &statuses));
    }
    Ok(ThemeApplyReport { results })
}

/// What one writer did: `Wrote` (files changed) or `Kept` (nothing touched —
/// foreign content preserved, tool absent, or derivation-only target).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Applied {
    Wrote(String),
    Kept(String),
}

fn write_target(
    status: &TargetStatus,
    home: &Path,
    uid: u32,
    gid: u32,
    snap: &ThemeSnapshot,
    statuses: &[TargetStatus],
) -> TargetResult {
    let target = status.target;
    if !status.available {
        return TargetResult {
            target,
            outcome: ApplyOutcome::Skipped,
            detail: status.detail.clone(),
        };
    }
    let outcome = match target {
        ThemingTarget::Gtk3 => gtk_settings(home, uid, gid, snap, "gtk-3.0"),
        ThemingTarget::Gtk4 => gtk_settings(home, uid, gid, snap, "gtk-4.0"),
        ThemingTarget::Gtk2 => gtk2(home, uid, gid, snap),
        ThemingTarget::Qt5 => qtct(home, uid, gid, snap, "qt5ct"),
        ThemingTarget::Qt6 => qtct(home, uid, gid, snap, "qt6ct"),
        ThemingTarget::Kde => kde(home, uid, gid, snap),
        ThemingTarget::Flatpak => flatpak(home, uid, gid),
        ThemingTarget::Cosmic => return cosmic::run(home, uid, gid, snap),
        ThemingTarget::Environment => environment(home, uid, gid, snap, statuses),
        ThemingTarget::Portal => Ok(Applied::Kept(
            "settings portal derives color/font from the GTK/Qt/KDE layers".to_string(),
        )),
        ThemingTarget::Electron
        | ThemingTarget::Firefox
        | ThemingTarget::Vscodium
        | ThemingTarget::Alacritty
        | ThemingTarget::Kitty
        | ThemingTarget::Ghostty
        | ThemingTarget::Foot
        | ThemingTarget::Wezterm
        | ThemingTarget::Cava => apps::write_target(target, home, uid, gid, snap),
    };
    match outcome {
        Ok(Applied::Wrote(detail)) => TargetResult {
            target,
            outcome: ApplyOutcome::Written,
            detail: Some(detail),
        },
        Ok(Applied::Kept(detail)) => TargetResult {
            target,
            outcome: ApplyOutcome::Skipped,
            detail: Some(detail),
        },
        Err(error) => TargetResult {
            target,
            outcome: ApplyOutcome::Failed,
            detail: Some(error.message),
        },
    }
}

/// GTK3/GTK4: `settings.ini` (theme/font/icon/cursor) + colors CSS.
/// An existing `gtk.css` without our marker (shell or hand-written) is never
/// clobbered — no matter which source won.
fn gtk_settings(
    home: &Path,
    uid: u32,
    gid: u32,
    snap: &ThemeSnapshot,
    dir_name: &str,
) -> Result<Applied, ApiError> {
    let dir = home.join(".config").join(dir_name);
    mkdir_owned(home, &dir, uid, gid)?;

    let mut updates: Vec<(&str, String)> = vec![
        ("gtk-theme-name", "Adwaita".to_string()),
        // Classic trio for every app, GNOME/libadwaita included (SPEC:
        // native three-button baseline). Colon = empty left side, all three
        // buttons right-aligned like every other stack.
        (
            "gtk-decoration-layout",
            ":minimize,maximize,close".to_string(),
        ),
    ];
    // Empty family = "leave fonts as configured" (Wayle exposes none).
    if !snap.fonts.ui.is_empty() {
        let font = format!("{} {}", snap.fonts.ui, font_point(snap.fonts.scale));
        updates.push(("gtk-font-name", font));
    }
    if let Some(icon) = &snap.icon_theme {
        updates.push(("gtk-icon-theme-name", icon.clone()));
    }
    if let Some(cursor) = &snap.cursor {
        updates.push(("gtk-cursor-theme-name", cursor.name.clone()));
        updates.push(("gtk-cursor-size", cursor.size.to_string()));
    }
    let updates: Vec<(&str, &str)> = updates.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let ini_path = dir.join("settings.ini");
    let merged = merge_ini(&read_or_empty(&ini_path), "Settings", &updates);
    write_owned(home, &ini_path, merged.as_bytes(), uid, gid)?;

    let css_path = dir.join("gtk.css");
    let existing_css = read_or_empty(&css_path);
    if !existing_css.is_empty() && !existing_css.contains(MARKER) {
        return Ok(Applied::Wrote(
            "settings.ini written; foreign gtk.css preserved".to_string(),
        ));
    }
    write_owned(
        home,
        &css_path,
        gtk_css(snap, dir_name == "gtk-4.0").as_bytes(),
        uid,
        gid,
    )?;
    give_away(&css_path, uid, gid).ok();
    Ok(Applied::Wrote("settings.ini + gtk.css written".to_string()))
}

/// Point size for propagated fonts: scaled 11pt, clamped to sane bounds.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped 6..=32
fn font_point(scale: f64) -> u32 {
    (11.0 * scale).round().clamp(6.0, 32.0) as u32
}

/// Generated CSS: palette variables (shared by GTK3/GTK4) plus the uniform
/// window-control restyle for the toolkit of this directory (`gtk4` selects
/// the `windowcontrols` element, GTK3 the `.titlebutton` classes).
fn gtk_css(snap: &ThemeSnapshot, gtk4: bool) -> String {
    let c = &snap.colors;
    let on_primary = contrast_on(&c.primary);
    let on_error = contrast_on(&c.error);
    let muted = mix(&c.background, &c.foreground, 0.55);
    let dark_word = if snap.dark { "dark" } else { "light" };
    // Line array (not a continuation-literal): keeps every line at an even
    // indent for editorconfig-checker.
    let mut lines = vec![
        MARKER.to_string(),
        format!("/* Palette source: {} */", snap.source.as_str()),
        format!("@define-color theme_bg_color {};", c.background),
        format!("@define-color theme_fg_color {};", c.foreground),
        format!("@define-color theme_base_color {};", c.surface),
        format!("@define-color theme_text_color {};", c.foreground),
        format!("@define-color theme_selected_bg_color {};", c.primary),
        format!("@define-color theme_selected_fg_color {on_primary};"),
        format!("@define-color theme_unfocused_bg_color {};", c.background),
        format!("@define-color theme_unfocused_fg_color {muted};"),
        format!("@define-color theme_unfocused_base_color {};", c.surface),
        format!("@define-color theme_unfocused_text_color {muted};"),
        format!(
            "@define-color theme_unfocused_selected_bg_color {};",
            c.outline
        ),
        format!(
            "@define-color theme_unfocused_selected_fg_color {};",
            c.foreground
        ),
        format!("@define-color borders {};", c.outline),
        format!("@define-color error_color {};", c.error),
        format!("@define-color warning_color {};", c.warning),
        format!("@define-color success_color {};", c.success),
        format!("@define-color window_bg_color {};", c.background),
        format!("@define-color window_fg_color {};", c.foreground),
        format!("@define-color view_bg_color {};", c.surface),
        format!("@define-color view_fg_color {};", c.foreground),
        format!("@define-color headerbar_bg_color {};", c.background),
        format!("@define-color headerbar_fg_color {};", c.foreground),
        format!("@define-color sidebar_bg_color {};", c.surface),
        format!("@define-color sidebar_fg_color {};", c.foreground),
        format!("@define-color card_bg_color {};", c.surface),
        format!("@define-color card_fg_color {};", c.foreground),
        format!("@define-color dialog_bg_color {};", c.surface),
        format!("@define-color dialog_fg_color {};", c.foreground),
        format!("@define-color popover_bg_color {};", c.surface),
        format!("@define-color popover_fg_color {};", c.foreground),
        format!("@define-color accent_bg_color {};", c.primary),
        format!("@define-color accent_fg_color {on_primary};"),
        format!("@define-color destructive_bg_color {};", c.error),
        format!("@define-color destructive_fg_color {on_error};"),
        format!("@define-color error_bg_color {};", c.error),
        format!("@define-color error_fg_color {on_error};"),
        format!("color-scheme: {dark_word};"),
        "/* Uniform window controls: same size, radius and palette alpha */".to_string(),
    ];
    // Neutral pill buttons for the trio (close keeps the same neutral look
    // as minimize/maximize so every stack reads as one design).
    let controls = [
        "  min-width: 24px;",
        "  min-height: 24px;",
        "  padding: 4px;",
        "  border-radius: 999px;",
        "  color: @window_fg_color;",
        "  background-color: alpha(@window_fg_color, 0.10);",
    ];
    let states = [
        (":hover", "background-color: alpha(@window_fg_color, 0.20);"),
        (
            ":active",
            "background-color: alpha(@window_fg_color, 0.30);",
        ),
    ];
    if gtk4 {
        lines.push("windowcontrols > button {".to_string());
        lines.extend(controls.iter().map(ToString::to_string));
        lines.push("}".to_string());
        for (state, body) in states {
            lines.push(format!("windowcontrols > button{state} {{"));
            lines.push(format!("  {body}"));
            lines.push("}".to_string());
        }
    } else {
        lines.push(".titlebar .titlebutton, headerbar .titlebutton {".to_string());
        lines.extend(controls.iter().map(ToString::to_string));
        lines.push("}".to_string());
        for (state, body) in states {
            lines.push(format!(".titlebar .titlebutton{state} {{"));
            lines.push(format!("  {body}"));
            lines.push("}".to_string());
        }
    }
    format!("{}\n", lines.join("\n"))
}

/// GTK2: one color-scheme line (guarded by our marker so hand-edited
/// `.gtkrc-2.0` files are preserved).
fn gtk2(home: &Path, uid: u32, gid: u32, snap: &ThemeSnapshot) -> Result<Applied, ApiError> {
    let path = home.join(".gtkrc-2.0");
    let existing = read_or_empty(&path);
    if !existing.is_empty() && !existing.contains(MARKER) {
        return Ok(Applied::Kept(
            "custom ~/.gtkrc-2.0 preserved (no marker)".to_string(),
        ));
    }
    let c = &snap.colors;
    let on_primary = contrast_on(&c.primary);
    let body = format!(
        "{MARKER}\ngtk-color-scheme = \"bg:{bg}:fg:{fg}:base:{surface}:text:{fg}:selected_bg:{primary}:selected_fg:{on_primary}\"\n",
        bg = c.background,
        fg = c.foreground,
        surface = c.surface,
        primary = c.primary,
    );
    let content = format!("# Ursine uniform theme ({})\n{body}", snap.source.as_str());
    write_owned(home, &path, content.as_bytes(), uid, gid)?;
    Ok(Applied::Wrote(".gtkrc-2.0 written".to_string()))
}

/// qt5ct/qt6ct: scheme file + `color_scheme=` selection.
fn qtct(
    home: &Path,
    uid: u32,
    gid: u32,
    snap: &ThemeSnapshot,
    dir_name: &str,
) -> Result<Applied, ApiError> {
    let dir = home.join(".config").join(dir_name);
    mkdir_owned(home, &dir, uid, gid)?;
    let scheme = format!("ursine-{}", snap.source.as_str());
    let colors_path = dir.join("colors").join(format!("{scheme}.conf"));
    let colors_dir = colors_path
        .parent()
        .ok_or_else(|| ApiError::internal("qtct colors path has no parent"))?;
    mkdir_owned(home, colors_dir, uid, gid)?;
    write_owned(home, &colors_path, qtct_colors(snap).as_bytes(), uid, gid)?;

    let conf_path = dir.join(format!("{dir_name}.conf"));
    // Never reset an icon selection the snapshot does not carry.
    let mut updates: Vec<(&str, &str)> = vec![("color_scheme", scheme.as_str())];
    if let Some(icon) = snap.icon_theme.as_deref() {
        updates.push(("icon", icon));
    }
    let merged = merge_ini(&read_or_empty(&conf_path), "Appearance", &updates);
    write_owned(home, &conf_path, merged.as_bytes(), uid, gid)?;
    Ok(Applied::Wrote(format!(
        "{dir_name}: colors/{scheme}.conf selected (qtct palette best-effort)"
    )))
}

/// qtct positional palette (`QPalette` role order, best-effort mapping).
fn qtct_colors(snap: &ThemeSnapshot) -> String {
    let c = &snap.colors;
    let bg = c.background.as_str();
    let fg = c.foreground.as_str();
    let roles: Vec<String> = vec![
        fg.to_string(),           // WindowText
        bg.to_string(),           // Window
        c.surface.clone(),        // Base
        mix(&c.surface, bg, 0.5), // AlternateBase
        c.surface.clone(),        // ToolTipBase
        fg.to_string(),           // ToolTipText
        fg.to_string(),           // Text
        bg.to_string(),           // Button
        contrast_on(bg),          // BrightText
        mix(bg, fg, 0.15),        // Light
        mix(bg, fg, 0.08),        // Midlight
        mix(bg, fg, 0.35),        // Dark
        mix(bg, fg, 0.25),        // Mid
        mix(bg, fg, 0.5),         // Shadow
        c.primary.clone(),        // Highlight
        contrast_on(&c.primary),  // HighlightedText
        c.primary.clone(),        // Link
        mix(&c.primary, fg, 0.4), // LinkVisited
        fg.to_string(),           // LinkOverrideText
        fg.to_string(),           // ButtonText
        mix(fg, bg, 0.5),         // PlaceholderText
        mix(bg, fg, 0.02),        // AlternateBase (alt slot)
        fg.to_string(),           // (extra) text
        bg.to_string(),           // (extra) window
        mix(bg, fg, 0.4),         // (extra) disabled body
        fg.to_string(),           // (extra) disabled text
    ];
    let list = roles.join(", ");
    format!(
        "[ColorScheme]\nColorSchemeName=ursine-{}\nactive_colors={list}\ninactive_colors={list}\ndisabled_colors={list}\n",
        snap.source.as_str()
    )
}

/// KDE: write our `.colors` scheme and select it in `kdeglobals` (merged).
fn kde(home: &Path, uid: u32, gid: u32, snap: &ThemeSnapshot) -> Result<Applied, ApiError> {
    let scheme = format!("ursine-{}", snap.source.as_str());
    let colors_dir = home.join(".local/share/color-schemes");
    mkdir_owned(home, &colors_dir, uid, gid)?;
    write_owned(
        home,
        &colors_dir.join(format!("{scheme}.colors")),
        kde_colors(snap, &scheme).as_bytes(),
        uid,
        gid,
    )?;

    let globals = home.join(".config/kdeglobals");
    let existing = read_or_empty(&globals);
    let mut merged = replace_section_value(&existing, "General", "ColorScheme", &scheme);
    // Classic trio on kwin titlebars (KDecoration letters: I=minimize,
    // A=maximize, X=close); left side emptied for right-aligned uniformity.
    merged = replace_section_value(&merged, "WM", "ButtonsOnLeft", "");
    merged = replace_section_value(&merged, "WM", "ButtonsOnRight", "IAX");
    if let Some(icon) = &snap.icon_theme {
        merged = replace_section_value(&merged, "Icons", "Theme", icon);
    }
    if let Some(cursor) = &snap.cursor {
        merged = replace_section_value(&merged, "Mouse", "cursorTheme", &cursor.name);
        merged = replace_section_value(&merged, "Mouse", "cursorSize", &cursor.size.to_string());
    }
    // Empty family = leave the configured font alone (Wayle).
    if !snap.fonts.ui.is_empty() {
        let font = format!(
            "{},{},-1,50,0,0,0,0,0",
            snap.fonts.ui,
            font_point(snap.fonts.scale)
        );
        merged = replace_section_value(&merged, "General", "Font", &font);
    }
    write_owned(home, &globals, merged.as_bytes(), uid, gid)?;
    Ok(Applied::Wrote(format!(
        "kdeglobals + color-schemes/{scheme}.colors"
    )))
}

/// ARGB decimal (`#rrggbb` → `0xAARRGGBB`) for KDE color files.
fn argb(hex: &str) -> u64 {
    let normalized = without_alpha(hex);
    let body = normalized.trim_start_matches('#');
    let v = u64::from_str_radix(body, 16).unwrap_or(0);
    (0xFF << 24) | v
}

fn kde_colors(snap: &ThemeSnapshot, scheme: &str) -> String {
    let c = &snap.colors;
    let section = |name: &str, color: &str, foreground: &str| -> String {
        format!(
            "[Colors:{name}]\nColor={}\nForeground={}\nAlternateBackground={}\nDecorationFocus={}\nDecorationHover={}\n",
            argb(color),
            argb(foreground),
            argb(&mix(color, foreground, 0.15)),
            argb(&c.primary),
            argb(&c.primary),
        )
    };
    format!(
        "[General]\nColorSchemeName={scheme}\nName={scheme}\nshadeSortColumn=true\n\n{}{}{}{}{}",
        section("Window", &c.background, &c.foreground),
        section("View", &c.surface, &c.foreground),
        section("Button", &c.background, &c.foreground),
        section("Selection", &c.primary, &contrast_on(&c.primary)),
        section("ToolTip", &c.surface, &c.foreground),
    )
}

/// Set `key=value` inside INI `section` (created when missing).
fn replace_section_value(existing: &str, section: &str, key: &str, value: &str) -> String {
    let header = format!("[{section}]");
    let updates = [(key, value)];
    if existing.contains(&header) {
        merge_ini(existing, section, &updates)
    } else {
        let mut text = existing.to_string();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        if !text.trim().is_empty() {
            text.push('\n');
        }
        text.push_str(&header);
        text.push('\n');
        text.push_str(key);
        text.push('=');
        text.push_str(value);
        text.push('\n');
        text
    }
}

/// Flatpak: read-only mounts of the host theme config into every sandbox.
fn flatpak(home: &Path, uid: u32, gid: u32) -> Result<Applied, ApiError> {
    let path = home.join(".local/share/flatpak/overrides/global");
    let parent = path
        .parent()
        .ok_or_else(|| ApiError::internal("flatpak override path has no parent"))?;
    mkdir_owned(home, parent, uid, gid)?;
    let existing = read_or_empty(&path);
    let merged = merge_flatpak_context(
        &existing,
        &[
            "xdg-config/gtk-3.0:ro",
            "xdg-config/gtk-4.0:ro",
            "xdg-config/qt5ct:ro",
            "xdg-config/qt6ct:ro",
        ],
    );
    write_owned(home, &path, merged.as_bytes(), uid, gid)?;
    Ok(Applied::Wrote(
        "flatpak overrides/global: host theme dirs mounted ro".to_string(),
    ))
}

fn merge_flatpak_context(existing: &str, add: &[&str]) -> String {
    let header = "[Context]";
    let mut out: Vec<String> = Vec::new();
    let mut in_context = false;
    let mut merged = false;
    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_context && !merged {
                out.push(format!("filesystems={}", merge_filesystems("", add)));
                merged = true;
            }
            in_context = trimmed == header;
            out.push(line.to_string());
            continue;
        }
        if in_context && trimmed.starts_with("filesystems=") {
            let current = trimmed.trim_start_matches("filesystems=");
            out.push(format!("filesystems={}", merge_filesystems(current, add)));
            merged = true;
            continue;
        }
        out.push(line.to_string());
    }
    if in_context && !merged {
        out.push(format!("filesystems={}", merge_filesystems("", add)));
    }
    if !in_context && !merged {
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(header.to_string());
        out.push(format!("filesystems={}", merge_filesystems("", add)));
    }
    let mut text = out.join("\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// Session environment: cursor/icon/platform theme for every login.
fn environment(
    home: &Path,
    uid: u32,
    gid: u32,
    snap: &ThemeSnapshot,
    statuses: &[TargetStatus],
) -> Result<Applied, ApiError> {
    let dir = home.join(".config/environment.d");
    mkdir_owned(home, &dir, uid, gid)?;
    let available = |target: ThemingTarget| -> bool {
        statuses.iter().any(|s| s.target == target && s.available)
    };
    let mut lines = vec![MARKER.to_string()];
    if let Some(cursor) = &snap.cursor {
        lines.push(format!("XCURSOR_THEME={}", cursor.name));
        lines.push(format!("XCURSOR_SIZE={}", cursor.size));
    }
    if let Some(icon) = &snap.icon_theme {
        lines.push(format!("ICON_THEME={icon}"));
    }
    // Platform theme only where the tool exists; KDE apps read kdeglobals
    // themselves (no plugin guesswork).
    if available(ThemingTarget::Qt6) {
        lines.push("QT_QPA_PLATFORMTHEME=qt6ct".to_string());
    } else if available(ThemingTarget::Qt5) {
        lines.push("QT_QPA_PLATFORMTHEME=qt5ct".to_string());
    }
    let content = format!("{}\n", lines.join("\n"));
    write_owned(
        home,
        &dir.join("95-ursine-theme.conf"),
        content.as_bytes(),
        uid,
        gid,
    )?;
    Ok(Applied::Wrote(
        "environment.d/95-ursine-theme.conf written (relogin to apply)".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir
    use super::*;
    use ursine_api::ThemeSource;

    fn snapshot(source: ThemeSource) -> ThemeSnapshot {
        ThemeSnapshot {
            source,
            name: "green".into(),
            dark: true,
            colors: ursine_api::ThemeColors {
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
            fonts: ursine_api::ThemeFonts {
                ui: "Inter Variable".into(),
                mono: "Fira Code".into(),
                weight: 400,
                scale: 1.0,
            },
            corner_radius: Some(12),
            icon_theme: Some("Papirus".into()),
            cursor: Some(ursine_api::CursorTheme {
                name: "Bibata".into(),
                size: 24,
            }),
        }
    }

    fn wrote(applied: Applied) -> String {
        match applied {
            Applied::Wrote(detail) => detail,
            Applied::Kept(detail) => panic!("expected Wrote, got Kept: {detail}"),
        }
    }

    fn kept(applied: Applied) -> String {
        match applied {
            Applied::Kept(detail) => detail,
            Applied::Wrote(detail) => panic!("expected Kept, got Wrote: {detail}"),
        }
    }

    fn temp_home(tag: &str) -> std::path::PathBuf {
        let home = std::env::temp_dir().join(format!("ursine-apply-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        home
    }

    #[test]
    fn gtk_settings_writes_ini_and_css_for_wayle_source() {
        let home = temp_home("gtk");
        let detail =
            wrote(gtk_settings(&home, 0, 0, &snapshot(ThemeSource::Wayle), "gtk-3.0").unwrap());
        assert!(detail.contains("gtk.css"));
        let ini = std::fs::read_to_string(home.join(".config/gtk-3.0/settings.ini")).unwrap();
        assert!(ini.contains("gtk-theme-name=Adwaita"));
        assert!(ini.contains("gtk-icon-theme-name=Papirus"));
        assert!(ini.contains("gtk-cursor-theme-name=Bibata"));
        assert!(ini.contains("gtk-cursor-size=24"));
        assert!(ini.contains("gtk-font-name=Inter Variable 11"));
        // Native three-button baseline (SPEC): classic trio, right side.
        assert!(ini.contains("gtk-decoration-layout=:minimize,maximize,close"));
        let css = std::fs::read_to_string(home.join(".config/gtk-3.0/gtk.css")).unwrap();
        assert!(css.contains("@define-color theme_bg_color #10140f;"));
        assert!(css.contains("@define-color window_bg_color #10140f;"));
        assert!(css.contains("color-scheme: dark;"));
        // GTK3 uniform restyle targets .titlebutton, not the GTK4 element.
        assert!(css.contains(".titlebar .titlebutton"));
        assert!(!css.contains("windowcontrols >"));
        // GTK4/libadwaita directory gets the element selector instead.
        wrote(gtk_settings(&home, 0, 0, &snapshot(ThemeSource::Wayle), "gtk-4.0").unwrap());
        let css4 = std::fs::read_to_string(home.join(".config/gtk-4.0/gtk.css")).unwrap();
        assert!(css4.contains("windowcontrols > button"));
        assert!(!css4.contains(".titlebar .titlebutton"));
        let ini4 = std::fs::read_to_string(home.join(".config/gtk-4.0/settings.ini")).unwrap();
        assert!(ini4.contains("gtk-decoration-layout=:minimize,maximize,close"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn gtk_settings_never_clobbers_the_shell_css() {
        let home = temp_home("gtk-dms");
        let dir = home.join(".config/gtk-3.0");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("gtk.css"),
            "@define-color window_bg_color #000000;\n",
        )
        .unwrap();
        let detail =
            wrote(gtk_settings(&home, 0, 0, &snapshot(ThemeSource::Dms), "gtk-3.0").unwrap());
        assert!(detail.contains("foreign gtk.css preserved"));
        // The trio baseline lives in settings.ini, independent of who owns
        // the CSS file.
        let ini = std::fs::read_to_string(home.join(".config/gtk-3.0/settings.ini")).unwrap();
        assert!(ini.contains("gtk-decoration-layout=:minimize,maximize,close"));
        let css = std::fs::read_to_string(dir.join("gtk.css")).unwrap();
        assert!(css.contains("#000000"), "shell css was overwritten: {css}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn gtk2_preserves_hand_edited_rc() {
        let home = temp_home("gtk2");
        let path = home.join(".gtkrc-2.0");
        std::fs::write(&path, "my custom style\n").unwrap();
        let detail = kept(gtk2(&home, 0, 0, &snapshot(ThemeSource::Dms)).unwrap());
        assert!(detail.contains("preserved"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "my custom style\n");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn environment_writes_platform_theme_for_available_qtct_only() {
        let home = temp_home("env");
        let statuses = vec![
            TargetStatus::available(ThemingTarget::Qt6, "qt6ct"),
            TargetStatus::unavailable(ThemingTarget::Qt5, "missing"),
        ];
        environment(&home, 0, 0, &snapshot(ThemeSource::Dms), &statuses).unwrap();
        let text = std::fs::read_to_string(home.join(".config/environment.d/95-ursine-theme.conf"))
            .unwrap();
        assert!(text.contains("QT_QPA_PLATFORMTHEME=qt6ct"));
        assert!(!text.contains("qt5ct"));
        assert!(text.contains("XCURSOR_THEME=Bibata"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn flatpak_merges_filesystems_and_is_idempotent() {
        let first = merge_flatpak_context("", &["xdg-config/gtk-3.0:ro"]);
        assert!(first.contains("[Context]"));
        assert!(first.contains("xdg-config/gtk-3.0:ro"));
        let second =
            merge_flatpak_context(&first, &["xdg-config/gtk-3.0:ro", "xdg-config/qt6ct:ro"]);
        assert_eq!(second.matches("xdg-config/gtk-3.0:ro").count(), 1);
        assert!(second.contains("xdg-config/qt6ct:ro"));
        // Foreign section preserved.
        let other = "[Context]\nshared=network\n\n[Session]\napps=foo\n";
        let merged = merge_flatpak_context(other, &["xdg-config/gtk-4.0:ro"]);
        assert!(merged.contains("apps=foo"));
        assert!(merged.contains("shared=network"));
        assert!(merged.contains("xdg-config/gtk-4.0:ro"));
    }

    #[test]
    fn kde_writes_scheme_and_selects_it_in_kdeglobals() {
        let home = temp_home("kde");
        std::fs::create_dir_all(home.join(".config")).unwrap();
        std::fs::write(
            home.join(".config/kdeglobals"),
            "[General]\nSomeKey=keep\n\n[Icons]\nTheme=OldIcon\n",
        )
        .unwrap();
        kde(&home, 0, 0, &snapshot(ThemeSource::Dms)).unwrap();
        let globals = std::fs::read_to_string(home.join(".config/kdeglobals")).unwrap();
        assert!(globals.contains("ColorScheme=ursine-dms"));
        // Native three-button baseline on kwin (I=minimize A=maximize X=close).
        let wm = globals
            .lines()
            .skip_while(|l| l.trim() != "[WM]")
            .skip(1) // consume the header itself
            .take_while(|l| !l.trim().starts_with('['))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(wm.contains("ButtonsOnLeft="), "{globals}");
        assert!(wm.contains("ButtonsOnRight=IAX"), "{globals}");
        assert!(globals.contains("SomeKey=keep"), "foreign key lost");
        assert!(globals.contains("Theme=Papirus"));
        let colors =
            std::fs::read_to_string(home.join(".local/share/color-schemes/ursine-dms.colors"))
                .unwrap();
        assert!(colors.contains("[Colors:Window]"));
        assert!(colors.contains("[Colors:Selection]"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn portal_target_reports_derivation_not_a_write() {
        let status = TargetStatus::available(ThemingTarget::Portal, "portal");
        let result = write_target(
            &status,
            Path::new("/nonexistent"),
            0,
            0,
            &snapshot(ThemeSource::Dms),
            std::slice::from_ref(&status),
        );
        assert_eq!(result.outcome, ApplyOutcome::Skipped);
        assert!(result.detail.unwrap().contains("settings portal derives"));
    }

    #[test]
    fn unavailable_targets_are_skipped_with_detail() {
        let status = TargetStatus::unavailable(ThemingTarget::Kde, "KDE tools not installed");
        let result = write_target(
            &status,
            Path::new("."),
            0,
            0,
            &snapshot(ThemeSource::Dms),
            &[],
        );
        assert_eq!(result.outcome, ApplyOutcome::Skipped);
        assert_eq!(result.detail.as_deref(), Some("KDE tools not installed"));
    }

    #[test]
    fn qtct_writes_scheme_file_and_selection() {
        let home = temp_home("qtct");
        qtct(&home, 0, 0, &snapshot(ThemeSource::Dms), "qt5ct").unwrap();
        let scheme = home.join(".config/qt5ct/colors/ursine-dms.conf");
        let text = std::fs::read_to_string(scheme).unwrap();
        let active = text
            .lines()
            .find(|l| l.starts_with("active_colors="))
            .unwrap();
        assert!(
            active.matches(',').count() >= 20,
            "palette too short: {active}"
        );
        let conf = std::fs::read_to_string(home.join(".config/qt5ct/qt5ct.conf")).unwrap();
        assert!(conf.contains("color_scheme=ursine-dms"));
        let _ = std::fs::remove_dir_all(&home);
    }
}
