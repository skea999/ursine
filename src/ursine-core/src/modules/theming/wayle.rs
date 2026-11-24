//! Wayle source: `~/.config/wayle/config.toml` + matugen's palette cache.
//!
//! Wayle owns its config; this module only reads it. With
//! `theme-provider = "matugen"` (the `FeralOS` seed) there is NO static
//! palette: Wayle runs `matugen --json hex …` and caches the output at
//! `~/.cache/wayle/matugen-colors.json` (wayle-wallpaper's
//! `ConfigPaths::matugen_colors`). A config that still carries
//! `[styling.palette]` keeps working (older seeds and tests).

use ursine_api::{ApiError, ThemeColors, ThemeFonts, ThemeSnapshot, ThemeSource};

use super::color::{luminance, mix, normalize_hex};

/// Cached matugen output (Wayle's own provider reads the same file).
pub(super) const MATUGEN_CACHE: &str = ".cache/wayle/matugen-colors.json";

/// Parse a Wayle config into a snapshot. `matugen_json` is the cached
/// matugen output when present; it wins for `theme-provider = "matugen"`.
pub(super) fn snapshot(
    config_toml: &str,
    matugen_json: Option<&str>,
) -> Result<ThemeSnapshot, ApiError> {
    let doc: toml::Value = config_toml.parse().map_err(|e| {
        ApiError::unavailable(
            format!("parse wayle config.toml: {e}"),
            "rewrite ~/.config/wayle/config.toml so it parses as TOML; wayle itself needs a readable config",
        )
    })?;
    let styling = doc
        .get("styling")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            ApiError::unavailable(
                "wayle config has no [styling] section",
                "add a [styling] section with theme-provider and [styling.palette] to ~/.config/wayle/config.toml",
            )
        })?;
    let provider = styling
        .get("theme-provider")
        .and_then(toml::Value::as_str)
        .unwrap_or("wayle");
    let corner_radius = corner_radius(styling);

    if provider == "matugen" {
        if let Some(json) = matugen_json {
            let light = styling
                .get("matugen-light")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
            return matugen_snapshot(json, light, corner_radius);
        }
        // Older seeds injected a static palette next to the provider; keep
        // reading it instead of failing outright.
        if let Some(palette) = styling.get("palette").and_then(toml::Value::as_table) {
            return palette_snapshot(&doc, palette, provider, corner_radius);
        }
        return Err(ApiError::unavailable(
            "wayle matugen palette not generated yet",
            "set a wallpaper once in Wayle so matugen writes ~/.cache/wayle/matugen-colors.json",
        ));
    }

    // No palette and no matugen cache: nothing to read. This is the host's
    // configuration, not a bad request, so the code is `unavailable` and the
    // fix names the provider that has to be completed.
    let palette = styling
        .get("palette")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            ApiError::unavailable(
                format!("wayle [styling] has no palette (theme-provider = \"{provider}\")"),
                format!(
                    "set [styling.palette] in ~/.config/wayle/config.toml, or let {provider} write a matugen cache at ~/.cache/wayle/matugen-colors.json"
                ),
            )
        })?;
    palette_snapshot(&doc, palette, provider, corner_radius)
}

/// Matugen's JSON (`--json hex`): every role carries a dark and a light
/// variant, either a plain string or `{ "color": "…" }`.
fn matugen_snapshot(
    json: &str,
    light: bool,
    corner_radius: Option<u32>,
) -> Result<ThemeSnapshot, ApiError> {
    let doc: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| ApiError::internal(format!("parse matugen palette cache: {e}")))?;
    let colors = doc
        .get("colors")
        .ok_or_else(|| ApiError::internal("matugen cache has no colors object"))?;
    let color = |role: &str| -> Result<String, ApiError> {
        let variants = colors
            .get(role)
            .ok_or_else(|| ApiError::internal(format!("matugen palette missing {role}")))?;
        let side = if light { "light" } else { "dark" };
        let raw = variants
            .get(side)
            .ok_or_else(|| ApiError::internal(format!("matugen {role} missing {side}")))
            .and_then(|value| {
                value
                    .as_str()
                    .or_else(|| value.get("color").and_then(serde_json::Value::as_str))
                    .ok_or_else(|| ApiError::internal(format!("matugen {role} has no color")))
            })?;
        normalize_hex(raw)
    };

    let background = color("background")?;
    let foreground = color("on_background")?;
    let primary = color("primary")?;
    let secondary = color("secondary")?;
    let error = color("error")?;
    // Wayle maps tertiary → yellow and secondary → green.
    let warning = color("tertiary")?;
    let success = secondary.clone();
    let surface = mix(&background, &foreground, 0.06);
    let muted = color("on_surface_variant").map_or_else(
        |_| mix(&background, &foreground, 0.3),
        |value| mix(&background, &value, 0.4),
    );

    Ok(ThemeSnapshot {
        source: ThemeSource::Wayle,
        name: "matugen".to_string(),
        dark: !light,
        colors: ThemeColors {
            background,
            foreground,
            primary,
            secondary,
            surface,
            error,
            success,
            warning,
            outline: muted,
        },
        fonts: ThemeFonts {
            ui: String::new(),
            mono: String::new(),
            weight: 400,
            scale: 1.0,
        },
        corner_radius,
        icon_theme: None,
        cursor: None,
    })
}

/// Static `[styling.palette]` table (older seeds and tests).
fn palette_snapshot(
    doc: &toml::Value,
    palette: &toml::map::Map<String, toml::Value>,
    provider: &str,
    corner_radius: Option<u32>,
) -> Result<ThemeSnapshot, ApiError> {
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
        fonts: ThemeFonts {
            ui: String::new(),
            mono: String::new(),
            weight: 400,
            scale: 1.0,
        },
        corner_radius,
        icon_theme: None,
        cursor: None,
    })
}

/// `[styling].rounding`: integer pixels or the named scale.
fn corner_radius(styling: &toml::map::Map<String, toml::Value>) -> Option<u32> {
    let rounding = styling.get("rounding")?;
    rounding
        .as_integer()
        .and_then(|v| u32::try_from(v).ok())
        .or_else(|| {
            rounding.as_str().and_then(|level| match level {
                "xs" => Some(4),
                "sm" => Some(6),
                "md" => Some(12),
                "lg" => Some(18),
                "xl" => Some(24),
                _ => None,
            })
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

    /// Invariants shared by the static-palette and matugen snapshots.
    fn assert_common_dark(snapshot: &ThemeSnapshot) {
        assert_eq!(snapshot.source, ThemeSource::Wayle);
        assert!(snapshot.dark);
        assert_eq!(snapshot.colors.background, "#10140f");
        assert_eq!(snapshot.colors.primary, "#4caf50");
        assert_eq!(snapshot.colors.error, "#f2b8b5");
        assert_eq!(snapshot.colors.warning, "#dacb52");
        // Empty = "leave the system fonts as configured" (writers skip).
        assert_eq!(snapshot.fonts.ui, "");
    }

    #[test]
    fn wayle_snapshot_reads_palette_and_provider() {
        let snapshot = snapshot(CONFIG, None).unwrap();
        assert_common_dark(&snapshot);
        assert_eq!(snapshot.colors.success, "#4daf4c");
        assert!(snapshot.name.contains("matugen"));
        // fg-muted drives the outline (matches the shell's muted borders).
        assert_ne!(snapshot.colors.outline, snapshot.colors.background);
        let _ = contrast_on(&snapshot.colors.background);
    }

    #[test]
    fn missing_styling_section_is_an_error() {
        let err = snapshot("[bar]\nlocation = \"top\"\n", None).unwrap_err();
        assert_eq!(err.code, ursine_api::ErrorCode::Unavailable);
        assert!(err.message.contains("styling"), "{}", err.message);
        assert!(
            err.fix
                .as_deref()
                .is_some_and(|fix| fix.contains("styling")),
            "{:?}",
            err.fix
        );
    }

    #[test]
    fn unparsable_config_is_unavailable_not_internal() {
        let err = snapshot("[styling\ntheme-provider = \"pywal\"\n", None).unwrap_err();
        assert_eq!(err.code, ursine_api::ErrorCode::Unavailable);
        assert!(
            err.fix
                .as_deref()
                .is_some_and(|fix| fix.contains("config.toml")),
            "{:?}",
            err.fix
        );
    }

    #[test]
    fn missing_palette_key_is_an_error() {
        let err = snapshot("[styling]\ntheme-provider = \"wayle\"\n", None).unwrap_err();
        assert_eq!(err.code, ursine_api::ErrorCode::Unavailable);
        assert!(err.message.contains("palette"), "{}", err.message);
    }

    #[test]
    fn other_provider_without_a_palette_names_the_provider_and_the_fix() {
        let err = snapshot("[styling]\ntheme-provider = \"pywal\"\n", None).unwrap_err();
        assert_eq!(err.code, ursine_api::ErrorCode::Unavailable);
        // The message stays recognisable and names the provider.
        assert!(err.message.contains("palette"), "{}", err.message);
        assert!(err.message.contains("pywal"), "{}", err.message);
        let fix = err.fix.as_deref().unwrap_or_default();
        assert!(!fix.is_empty(), "an unavailable answer needs a fix");
        assert!(fix.contains("pywal"), "{fix}");
        assert!(fix.contains("styling.palette"), "{fix}");
    }

    const MATUGEN_JSON: &str = r##"{
        "colors": {
            "background": { "dark": { "color": "#10140f" }, "light": { "color": "#fafbfc" } },
            "surface": { "dark": { "color": "#1b1f19" }, "light": { "color": "#eaebec" } },
            "on_background": { "dark": { "color": "#e0e4db" }, "light": { "color": "#101112" } },
            "on_surface_variant": { "dark": { "color": "#9aa394" }, "light": { "color": "#505152" } },
            "primary": { "dark": { "color": "#4caf50" }, "light": { "color": "#2060cc" } },
            "secondary": { "dark": { "color": "#4daf4c" }, "light": { "color": "#20cc60" } },
            "tertiary": { "dark": { "color": "#dacb52" }, "light": { "color": "#cc9f20" } },
            "error": { "dark": { "color": "#f2b8b5" }, "light": { "color": "#cc2020" } }
        }
    }"##;

    #[test]
    fn matugen_provider_reads_the_cache_without_a_static_palette() {
        let config = "[styling]\ntheme-provider = \"matugen\"\nrounding = \"lg\"\n";
        let snapshot = snapshot(config, Some(MATUGEN_JSON)).unwrap();
        assert_common_dark(&snapshot);
        assert_eq!(snapshot.colors.success, "#4daf4c");
        assert_eq!(snapshot.corner_radius, Some(18));
        assert_eq!(snapshot.name, "matugen");
    }

    #[test]
    fn matugen_light_picks_the_light_variants() {
        let config = "[styling]\ntheme-provider = \"matugen\"\nmatugen-light = true\n";
        let snapshot = snapshot(config, Some(MATUGEN_JSON)).unwrap();
        assert!(!snapshot.dark);
        assert_eq!(snapshot.colors.background, "#fafbfc");
        assert_eq!(snapshot.colors.primary, "#2060cc");
    }

    #[test]
    fn matugen_without_cache_is_an_actionable_error() {
        let err = snapshot("[styling]\ntheme-provider = \"matugen\"\n", None).unwrap_err();
        assert!(err.message.contains("matugen"), "{}", err.message);
        assert!(
            err.fix
                .as_deref()
                .is_some_and(|fix| fix.contains("wallpaper")),
            "{:?}",
            err.fix
        );
    }

    #[test]
    fn old_matugen_shape_with_plain_strings_parses() {
        let json = r##"{"colors": {
            "background": { "dark": "#101112", "light": "#fafbfc" },
            "on_background": { "dark": "#f0f1f2", "light": "#101112" },
            "on_surface_variant": { "dark": "#a0a1a2", "light": "#505152" },
            "primary": { "dark": "#4090ff", "light": "#2060cc" },
            "secondary": { "dark": "#40ff90", "light": "#20cc60" },
            "tertiary": { "dark": "#ffcf40", "light": "#cc9f20" },
            "error": { "dark": "#ff4040", "light": "#cc2020" }
        }}"##;
        let snapshot = snapshot("[styling]\ntheme-provider = \"matugen\"\n", Some(json)).unwrap();
        assert_eq!(snapshot.colors.primary, "#4090ff");
    }
}
