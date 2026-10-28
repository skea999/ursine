//! Wayle source: `~/.config/wayle/config.toml` `[styling]` → snapshot.
//!
//! Wayle owns its config; this module only reads it. Dynamic providers
//! (matugen/pywal/wallust) inject the active palette into `[styling].palette`
//! when they run, so the palette table is the single read surface.

use ursine_api::{ApiError, ThemeColors, ThemeFonts, ThemeSnapshot, ThemeSource};

use super::color::{luminance, mix, normalize_hex};

/// Parse a Wayle config into a snapshot.
pub(super) fn snapshot(config_toml: &str) -> Result<ThemeSnapshot, ApiError> {
    let doc: toml::Value = config_toml
        .parse()
        .map_err(|e| ApiError::internal(format!("parse wayle config.toml: {e}")))?;
    let styling = doc
        .get("styling")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| ApiError::internal("wayle config has no [styling] section"))?;
    let palette = styling
        .get("palette")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| ApiError::internal("wayle [styling] has no palette"))?;

    let color = |key: &str| -> Result<String, ApiError> {
        let raw = palette
            .get(key)
            .and_then(toml::Value::as_str)
            .ok_or_else(|| ApiError::internal(format!("wayle palette missing {key}")))?;
        normalize_hex(raw)
    };

    let background = color("bg")?;
    let foreground = color("fg")?;
    let primary = color("primary")?;
    let surface = color("surface")?;
    let error = color("red")?;
    let success = color("green")?;
    let warning = color("yellow")?;
    let secondary = mix(&primary, &background, 0.65);
    let outline = color("fg-muted").ok().map_or_else(
        || mix(&background, &foreground, 0.3),
        |c| mix(&background, &c, 0.4),
    );

    let name = doc
        .get("name")
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            palette
                .get("name")
                .and_then(toml::Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "wayle".into());
    let provider = styling
        .get("theme-provider")
        .and_then(toml::Value::as_str)
        .unwrap_or("wayle");

    let rounding = styling.get("rounding");
    let corner_radius = rounding
        .and_then(toml::Value::as_integer)
        .and_then(|v| u32::try_from(v).ok())
        .or_else(|| {
            rounding
                .and_then(toml::Value::as_str)
                .and_then(|level| match level {
                    "xs" => Some(4),
                    "sm" => Some(6),
                    "md" => Some(12),
                    "lg" => Some(18),
                    "xl" => Some(24),
                    _ => None,
                })
        });

    // Wayle exposes no font section: empty families mean "leave fonts as
    // configured" — writers skip font keys when the family is empty.
    let fonts = ThemeFonts {
        ui: String::new(),
        mono: String::new(),
        weight: 400,
        scale: 1.0,
    };

    let cursor = None;
    let icon_theme = None;

    Ok(ThemeSnapshot {
        source: ThemeSource::Wayle,
        name: format!("{name} ({provider})"),
        dark: luminance(&background) < 0.5,
        colors: ThemeColors {
            background,
            foreground,
            primary,
            secondary,
            surface,
            error,
            success,
            warning,
            outline,
        },
        fonts,
        corner_radius,
        icon_theme,
        cursor,
    })
}

#[cfg(test)]
mod tests {
    use super::super::color::contrast_on;
    use super::*;

    const CONFIG: &str = r##"
[styling]
scale = 1.01
rounding = "sm"
theme-provider = "matugen"

[styling.palette]
bg = "#10140f"
surface = "#1b1f19"
elevated = "#22271f"
fg = "#e0e4db"
fg-muted = "#9aa394"
primary = "#4caf50"
red = "#f2b8b5"
yellow = "#dacb52"
green = "#4daf4c"
blue = "#35a639"
"##;

    #[test]
    fn wayle_snapshot_reads_palette_and_provider() {
        let snapshot = snapshot(CONFIG).unwrap();
        assert_eq!(snapshot.source, ThemeSource::Wayle);
        assert!(snapshot.dark);
        assert_eq!(snapshot.colors.background, "#10140f");
        assert_eq!(snapshot.colors.primary, "#4caf50");
        assert_eq!(snapshot.colors.error, "#f2b8b5");
        assert_eq!(snapshot.colors.success, "#4daf4c");
        assert_eq!(snapshot.colors.warning, "#dacb52");
        assert!(snapshot.name.contains("matugen"));
        // Empty = "leave the system fonts as configured" (writers skip).
        assert_eq!(snapshot.fonts.ui, "");
        // fg-muted drives the outline (matches the shell's muted borders).
        assert_ne!(snapshot.colors.outline, snapshot.colors.background);
        let _ = contrast_on(&snapshot.colors.background);
    }

    #[test]
    fn missing_styling_section_is_an_error() {
        let err = snapshot("[bar]\nlocation = \"top\"\n").unwrap_err();
        assert!(err.message.contains("styling"), "{}", err.message);
    }

    #[test]
    fn missing_palette_key_is_an_error() {
        let err = snapshot("[styling]\ntheme-provider = \"wayle\"\n").unwrap_err();
        assert!(err.message.contains("palette"), "{}", err.message);
    }
}
