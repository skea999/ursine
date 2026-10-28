//! `DankMaterialShell` source: `settings.json` + the generated
//! `dank-colors.css` (`@define-color` rules) → one normalized snapshot.
//!
//! DMS owns its config and CSS; this module only reads them.

use std::collections::BTreeMap;

use ursine_api::{ApiError, CursorTheme, ThemeColors, ThemeFonts, ThemeSnapshot, ThemeSource};

use super::color::{luminance, mix, normalize_hex, sane_label};

/// Parse a DMS settings.json + its generated GTK CSS into a snapshot.
pub(super) fn snapshot(settings_json: &str, gtk_css: &str) -> Result<ThemeSnapshot, ApiError> {
    let settings: serde_json::Value = serde_json::from_str(settings_json)
        .map_err(|e| ApiError::internal(format!("parse DMS settings.json: {e}")))?;
    let defines = parse_define_colors(gtk_css);

    let get = |key: &str| -> Result<String, ApiError> {
        settings
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| ApiError::internal(format!("DMS settings missing string key {key}")))
    };
    let pick =
        |names: &[&str]| -> Option<String> { names.iter().find_map(|n| defines.get(*n).cloned()) };

    let background = pick(&["window_bg_color", "theme_bg_color", "view_bg_color"])
        .ok_or_else(|| ApiError::internal("DMS css has no background color"))?;
    let foreground = pick(&["window_fg_color", "theme_fg_color", "view_fg_color"])
        .ok_or_else(|| ApiError::internal("DMS css has no foreground color"))?;
    let primary = pick(&["accent_bg_color", "theme_selected_bg_color"])
        .ok_or_else(|| ApiError::internal("DMS css has no accent color"))?;
    let error =
        pick(&["error_bg_color", "destructive_bg_color"]).unwrap_or_else(|| "#c96a4c".to_string());
    let surface = pick(&["card_bg_color", "view_bg_color", "dialog_bg_color"])
        .unwrap_or_else(|| mix(&background, &foreground, 0.06));
    // DMS defines no success/warning/outline tokens: derive them from the
    // palette we do get so every toolkit receives the same nine colors.
    let success = pick(&["success_bg_color"]).unwrap_or_else(|| primary.clone());
    let warning = mix(&error, &primary, 0.5);
    let outline = mix(&background, &foreground, 0.3);
    let secondary = mix(&primary, &background, 0.65);

    let radius = settings
        .get("cornerRadius")
        .and_then(serde_json::Value::as_u64);
    let corner_radius = radius.and_then(|v| u32::try_from(v).ok());

    let icon_raw = get("iconTheme").ok();
    let icon_theme = non_default(icon_raw.as_deref());
    let cursor = {
        let settings_cursor = settings.get("cursorSettings");
        let name = settings_cursor
            .and_then(|v| v.get("theme"))
            .and_then(|v| v.as_str())
            .and_then(|s| non_default(Some(s)));
        let size = settings_cursor
            .and_then(|v| v.get("size"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or(16);
        name.map(|name| CursorTheme { name, size })
    };

    let weight = settings
        .get("fontWeight")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u16::try_from(v).ok())
        .unwrap_or(400);
    let scale = settings
        .get("fontScale")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(1.0);

    let font_ui = get("fontFamily")?;
    let font_mono = get("monoFontFamily")?;
    Ok(ThemeSnapshot {
        source: ThemeSource::Dms,
        name: get("currentThemeName")?,
        dark: luminance(&background) < 0.5,
        colors: ThemeColors {
            background: normalize_hex(&background)?,
            foreground: normalize_hex(&foreground)?,
            primary: normalize_hex(&primary)?,
            secondary: normalize_hex(&secondary)?,
            surface: normalize_hex(&surface)?,
            error: normalize_hex(&error)?,
            success: normalize_hex(&success)?,
            warning: normalize_hex(&warning)?,
            outline: normalize_hex(&outline)?,
        },
        fonts: ThemeFonts {
            // Shell values land in INI/env files: insane labels fall back to
            // system defaults (newline/`=` injection guard).
            ui: if sane_label(&font_ui) {
                font_ui
            } else {
                "Sans".into()
            },
            mono: if sane_label(&font_mono) {
                font_mono
            } else {
                "Monospace".into()
            },
            weight,
            scale,
        },
        corner_radius,
        icon_theme,
        cursor,
    })
}

/// `"System Default"` means "inherit from the OS": surface it as None.
/// Insane labels (control bytes, `=`) are dropped the same way.
fn non_default(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("System Default"))
        .filter(|v| sane_label(v))
        .map(str::to_string)
}

/// Extract `@define-color name value;` pairs (single or multi line).
fn parse_define_colors(css: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in css.lines() {
        let Some(rest) = line.trim_start().strip_prefix("@define-color") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let (Some(name), Some(value)) = (parts.next(), parts.next()) else {
            continue;
        };
        let value = value.trim_end_matches(';');
        if let Ok(hex) = normalize_hex(value) {
            map.insert(name.to_string(), hex);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS: &str = r#"{
        "currentThemeName": "green",
        "fontFamily": "Inter Variable",
        "monoFontFamily": "Fira Code",
        "fontWeight": 400,
        "fontScale": 1,
        "cornerRadius": 12,
        "iconTheme": "System Default",
        "cursorSettings": {"theme": "System Default", "size": 16}
    }"#;

    const CSS: &str = r"
        /* Generated with Matugen */
        @define-color window_bg_color #10140f;
        @define-color window_fg_color #e0e4db;
        @define-color accent_bg_color #4caf50;
        @define-color error_bg_color #f2b8b5;
        @define-color card_bg_color #1b1f19;
    ";

    #[test]
    fn dms_snapshot_maps_colors_fonts_and_defaults() {
        let snapshot = snapshot(SETTINGS, CSS).unwrap();
        assert_eq!(snapshot.source, ThemeSource::Dms);
        assert_eq!(snapshot.name, "green");
        assert!(snapshot.dark);
        assert_eq!(snapshot.colors.background, "#10140f");
        assert_eq!(snapshot.colors.foreground, "#e0e4db");
        assert_eq!(snapshot.colors.primary, "#4caf50");
        assert_eq!(snapshot.colors.surface, "#1b1f19");
        assert_eq!(snapshot.colors.error, "#f2b8b5");
        assert_eq!(snapshot.fonts.ui, "Inter Variable");
        assert_eq!(snapshot.fonts.mono, "Fira Code");
        assert_eq!(snapshot.fonts.weight, 400);
        assert_eq!(snapshot.corner_radius, Some(12));
        // "System Default" resolves to None: the OS default stays in charge.
        assert!(snapshot.icon_theme.is_none());
        assert!(snapshot.cursor.is_none());
        // Derived colors stay readable against the background.
        assert_eq!(
            crate::modules::theming::color::contrast_on(&snapshot.colors.background),
            "#ffffff"
        );
    }

    #[test]
    fn missing_background_is_an_error() {
        let err = snapshot(SETTINGS, "@define-color accent_bg_color #4caf50;").unwrap_err();
        assert!(err.message.contains("background"), "{}", err.message);
    }

    #[test]
    fn parses_define_colors_across_lines_and_rgba() {
        let map = parse_define_colors("@define-color foo #abc;\n@define-color bar #11223344\n");
        assert_eq!(map.get("foo").map(String::as_str), Some("#aabbcc"));
        assert_eq!(map.get("bar").map(String::as_str), Some("#11223344"));
    }
}
