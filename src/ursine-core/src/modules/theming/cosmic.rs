//! COSMIC/libcosmic target: patch a full `cosmic_theme::Theme` from the
//! snapshot and write the entries every COSMIC app reads via cosmic-config
//! (systemic: all COSMIC windows follow after the entry lands).
//!
//! The daemon must not use `Config::new` (it would resolve the ROOT home):
//! `with_custom_path` pins the target user's config dir, and every created
//! file/dir is handed back to that user.

use std::path::Path;

use cosmic_config::{CosmicConfigEntry, Error as CosmicError};
use cosmic_theme::{Theme, ThemeMode, DARK_THEME_ID, LIGHT_THEME_ID};
use serde_json::{json, Value};
use ursine_api::{ApiError, ApplyOutcome, TargetResult, ThemeSnapshot, ThemingTarget};

use super::color::{contrast_on, mix, normalize_hex, without_alpha};
use super::write::{give_away, mkdir_owned};

const MODE_ID: &str = "com.system76.CosmicTheme.Mode";
/// Alpha for the transparent_* containers (frosted surfaces).
const TRANSPARENT_ALPHA: &str = "d9";

/// Write the COSMIC theme + mode entries for the snapshot's light/dark side.
pub(super) fn run(home: &Path, uid: u32, gid: u32, snap: &ThemeSnapshot) -> TargetResult {
    match run_inner(home, uid, gid, snap) {
        Ok(()) => TargetResult {
            target: ThemingTarget::Cosmic,
            outcome: ApplyOutcome::Written,
            detail: Some(if snap.dark {
                "CosmicTheme.Dark + Mode.is_dark".into()
            } else {
                "CosmicTheme.Light + Mode.is_dark".into()
            }),
        },
        Err(error) => TargetResult {
            target: ThemingTarget::Cosmic,
            outcome: ApplyOutcome::Failed,
            detail: Some(error.message),
        },
    }
}

fn run_inner(home: &Path, uid: u32, gid: u32, snap: &ThemeSnapshot) -> Result<(), ApiError> {
    let base = home.join(".config");
    mkdir_owned(home, &base, uid, gid)?;

    let theme_id = if snap.dark {
        DARK_THEME_ID
    } else {
        LIGHT_THEME_ID
    };
    let config = cosmic_config::Config::with_custom_path(
        theme_id,
        <Theme as CosmicConfigEntry>::VERSION,
        base.clone(),
    )
    .map_err(|e| map_cosmic(&e))?;
    let template = if snap.dark {
        Theme::dark_default()
    } else {
        Theme::light_default()
    };
    let patched = patched(&template, snap)?;
    patched.write_entry(&config).map_err(|e| map_cosmic(&e))?;

    let mode_config = cosmic_config::Config::with_custom_path(
        MODE_ID,
        <ThemeMode as CosmicConfigEntry>::VERSION,
        base.clone(),
    )
    .map_err(|e| map_cosmic(&e))?;
    // Preserve auto_switch; only the dark/light side is ours to set.
    let previous = ThemeMode::get_entry(&mode_config).unwrap_or_else(|(_, mode)| mode);
    let mode = ThemeMode {
        is_dark: snap.dark,
        auto_switch: previous.auto_switch,
    };
    mode.write_entry(&mode_config).map_err(|e| map_cosmic(&e))?;
    // Whole cosmic tree (incl. the `.config/cosmic` root cosmic-config just
    // created as root) goes back to the user.
    give_tree(&base.join("cosmic"), uid, gid)?;
    Ok(())
}

fn map_cosmic(error: &CosmicError) -> ApiError {
    ApiError::internal(format!("cosmic-config: {error}"))
}

/// Chown a freshly written config subtree back to the target user.
/// Symlinks are never followed (user-writable tree: a planted link must not
/// hand `/etc/shadow` to the user).
fn give_tree(path: &Path, uid: u32, gid: u32) -> Result<(), ApiError> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(());
    };
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    give_away(path, uid, gid)
        .map_err(|e| ApiError::internal(format!("own {}: {e}", path.display())))?;
    if meta.is_dir() {
        let entries = std::fs::read_dir(path)
            .map_err(|e| ApiError::internal(format!("read {}: {e}", path.display())))?;
        for entry in entries.flatten() {
            give_tree(&entry.path(), uid, gid)?;
        }
    }
    Ok(())
}

/// Rebuild every visual field of `template` from the snapshot colors.
fn patched(template: &Theme, snap: &ThemeSnapshot) -> Result<Theme, ApiError> {
    let mut value = serde_json::to_value(template)
        .map_err(|e| ApiError::internal(format!("serialize cosmic theme: {e}")))?;
    let colors = &snap.colors;
    let bg = colors.background.as_str();
    let fg = colors.foreground.as_str();
    let primary = colors.primary.as_str();

    let container = |base: &str, on: &str| -> Value {
        json!({
            "base": base,
            "component": neutral(base, on, primary),
            "divider": mix(base, on, 0.25),
            "on": on,
            "small_widget": mix(base, on, 0.06),
        })
    };

    set(
        &mut value,
        "name",
        json!(format!("ursine-{}", snap.source.as_str())),
    );
    set(&mut value, "is_dark", json!(snap.dark));
    set(&mut value, "is_high_contrast", json!(false));
    set(&mut value, "shade", json!(mix(bg, "#000000", 0.45)));
    set(&mut value, "accent_text", Value::Null);
    set(&mut value, "control_tint", Value::Null);
    set(&mut value, "text_tint", Value::Null);

    let opaque = [
        ("background", container(bg, fg)),
        ("primary", container(primary, &contrast_on(primary))),
        (
            "secondary",
            container(&colors.secondary, &contrast_on(&colors.secondary)),
        ),
    ];
    for (key, built) in opaque {
        set(&mut value, key, built.clone());
        let mut transparent = built;
        apply_alpha(&mut transparent, TRANSPARENT_ALPHA);
        set(&mut value, &format!("transparent_{key}"), transparent);
    }

    let plain = neutral(bg, fg, primary);
    for key in [
        "button",
        "icon_button",
        "link_button",
        "list_button",
        "text_button",
    ] {
        set(&mut value, key, plain.clone());
    }
    let accents = [
        ("accent", primary.to_string()),
        ("accent_button", primary.to_string()),
        ("success", colors.success.clone()),
        ("success_button", colors.success.clone()),
        ("destructive", colors.error.clone()),
        ("destructive_button", colors.error.clone()),
        ("warning", colors.warning.clone()),
        ("warning_button", colors.warning.clone()),
    ];
    for (key, color) in accents {
        set(&mut value, key, accented(&color));
    }

    // Palette: utility + surface + neutral ramp; brand slots stay template.
    let palette_name = format!("ursine-{}", snap.source.as_str());
    let palette = value
        .get_mut("palette")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| ApiError::internal("cosmic theme template has no palette"))?;
    palette.insert("name".into(), json!(palette_name));
    palette.insert("gray_1".into(), json!(colors.surface.clone()));
    palette.insert("gray_2".into(), json!(mix(&colors.surface, fg, 0.15)));
    palette.insert("bright_red".into(), json!(colors.error.clone()));
    palette.insert("bright_green".into(), json!(colors.success.clone()));
    palette.insert("bright_orange".into(), json!(colors.warning.clone()));
    palette.insert("accent_red".into(), json!(colors.error.clone()));
    palette.insert("accent_green".into(), json!(colors.success.clone()));
    palette.insert("accent_yellow".into(), json!(colors.warning.clone()));
    palette.insert("accent_warm_grey".into(), json!(colors.outline.clone()));
    for i in 0..=10u32 {
        palette.insert(
            format!("neutral_{i}"),
            json!(mix(bg, fg, f64::from(i) / 10.0)),
        );
    }

    // Corner radius from the source (every scale gets the same radius).
    if let Some(radius) = snap.corner_radius {
        let corner = value
            .get_mut("corner_radii")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| ApiError::internal("cosmic theme template has no corner_radii"))?;
        let keys: Vec<String> = corner
            .keys()
            .filter(|k| k.starts_with("radius_"))
            .cloned()
            .collect();
        #[allow(clippy::cast_precision_loss)] // radius is a small UI value
        let r = radius as f32;
        for key in keys {
            corner.insert(key, json!([r, r, r, r]));
        }
    }

    // ColorRepr only deserializes 8-digit hex (`#rrggbbaa`): pad everything.
    pad_hex_colors(&mut value);

    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("rebuild cosmic theme: {e}")))
}

/// Ensure every `#hex` string in the tree is `#rrggbbaa` (opaque = `ff`).
fn pad_hex_colors(value: &mut Value) {
    match value {
        Value::String(hex) => {
            if hex.starts_with('#') {
                if let Ok(mut norm) = normalize_hex(hex) {
                    if norm.len() == 7 {
                        norm.push_str("ff");
                        *hex = norm;
                    } else if norm.len() == 9 {
                        *hex = norm;
                    }
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(pad_hex_colors),
        Value::Object(map) => map.values_mut().for_each(pad_hex_colors),
        _ => {}
    }
}

/// Widget colors sitting on `base` (neutral surface widgets).
fn neutral(base: &str, on: &str, primary: &str) -> Value {
    json!({
        "base": mix(base, on, 0.08),
        "hover": mix(base, on, 0.12),
        "pressed": mix(base, on, 0.16),
        "selected": primary,
        "selected_text": contrast_on(primary),
        "focus": primary,
        "divider": mix(base, on, 0.20),
        "on": on,
        "disabled": mix(base, on, 0.10),
        "on_disabled": mix(on, base, 0.5),
        "border": mix(base, on, 0.25),
        "disabled_border": mix(base, on, 0.12),
    })
}

/// Accent-style widgets whose whole body is `color`.
fn accented(color: &str) -> Value {
    let on = contrast_on(color);
    json!({
        "base": color,
        "hover": mix(color, &on, 0.12),
        "pressed": mix(color, &on, 0.22),
        "selected": color,
        "selected_text": on,
        "focus": color,
        "divider": mix(color, &on, 0.35),
        "on": on,
        "disabled": mix(color, &on, 0.45),
        "on_disabled": mix(&on, color, 0.3),
        "border": mix(color, &on, 0.4),
        "disabled_border": mix(color, &on, 0.2),
    })
}

/// Overwrite a theme key (template shape is known; missing key = bug).
fn set(value: &mut Value, key: &str, replacement: Value) {
    if let Some(object) = value.as_object_mut() {
        object.insert(key.to_string(), replacement);
    }
}

/// Append an alpha suffix (`#rrggbb` → `#rrggbbdd`) to every color string.
fn apply_alpha(container: &mut Value, alpha: &str) {
    fn patch(value: &mut Value, alpha: &str) {
        match value {
            Value::String(hex) => {
                if let Ok(base) = normalize_hex(hex) {
                    if base.len() == 8 {
                        *hex = base;
                    } else {
                        *hex = format!("{}{alpha}", without_alpha(&base));
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| patch(v, alpha)),
            Value::Object(map) => map.values_mut().for_each(|v| patch(v, alpha)),
            _ => {}
        }
    }
    patch(container, alpha);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir
    use super::*;
    use ursine_api::{ThemeColors, ThemeFonts, ThemeSource};

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

    #[test]
    fn patched_theme_keeps_structure_and_takes_snapshot_colors() {
        let theme = patched(&Theme::dark_default(), &snapshot()).unwrap();
        assert_eq!(theme.name, "ursine-dms");
        assert!(theme.is_dark);
        assert!(!theme.is_high_contrast);
        assert_eq!(theme.corner_radii.radius_m, [12.0; 4]);
        // Round-trip stays a valid Theme; JSON carries 8-digit hex colors
        // (ColorRepr only deserializes `#rrggbbaa`; hex_color emits upper).
        let value = serde_json::to_value(&theme).unwrap();
        let hex = |v: &Value| v.as_str().unwrap_or_default().to_ascii_lowercase();
        assert_eq!(value["name"], "ursine-dms");
        assert_eq!(value["palette"]["name"], "ursine-dms");
        assert_eq!(hex(&value["palette"]["gray_1"]), "#1b1f19ff");
        assert_eq!(hex(&value["palette"]["neutral_10"]), "#e0e4dbff");
        // Accent flows into the accent component base.
        assert_eq!(hex(&value["accent"]["base"]), "#4caf50ff");
        // Opaque container carries the snapshot background.
        assert_eq!(hex(&value["background"]["base"]), "#10140fff");
        assert_eq!(hex(&value["background"]["on"]), "#e0e4dbff");
        // Transparent variant keeps an alpha suffix for frosted surfaces.
        let transparent = value["transparent_background"]["base"].as_str().unwrap();
        assert_eq!(transparent.len(), 9, "{transparent}");
    }

    #[test]
    fn neutral_components_stay_readably_contrasted() {
        let component = neutral("#10140f", "#e0e4db", "#4caf50");
        let on = component["on"].as_str().unwrap();
        assert_eq!(on, "#e0e4db");
        assert_ne!(component["base"], component["on"]);
        assert_eq!(component["selected_text"], contrast_on("#4caf50"));
    }

    #[test]
    fn write_entry_targets_the_user_home_not_root() {
        let dir = std::env::temp_dir().join(format!("ursine-cosmic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".config/cosmic")).unwrap();
        let result = run_inner(&dir, std::process::id(), std::process::id(), &snapshot());
        assert!(result.is_ok(), "{result:?}");
        let dark = dir.join(".config/cosmic/com.system76.CosmicTheme.Dark");
        assert!(dark.is_dir(), "dark entry missing at {}", dark.display());
        let mode = dir.join(".config/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark");
        assert_eq!(std::fs::read_to_string(&mode).unwrap().trim(), "true");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
