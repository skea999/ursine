//! Small color math shared by the theme parsers and writers.

use ursine_api::ApiError;

/// Normalize `#RGB` / `#RRGGBB` / `#RRGGBBAA` to lowercase `#rrggbb[aa]`.
pub(super) fn normalize_hex(input: &str) -> Result<String, ApiError> {
    let raw = input.trim().trim_start_matches('#');
    let valid = !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_hexdigit());
    let len_ok = matches!(raw.len(), 3 | 6 | 8);
    if !valid || !len_ok {
        return Err(ApiError::internal(format!("invalid hex color: {input}")));
    }
    let expanded = if raw.len() == 3 {
        raw.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        raw.to_string()
    };
    Ok(format!("#{}", expanded.to_ascii_lowercase()))
}

/// Byte pair `start..start+2` of a normalized hex body, as a 0..255 value.
fn byte_pair(hex_body: &str, start: usize) -> f64 {
    hex_body
        .as_bytes()
        .get(start..start + 2)
        .and_then(|pair| std::str::from_utf8(pair).ok())
        .and_then(|s| u8::from_str_radix(s, 16).ok())
        .map_or(0.0, |v| f64::from(v) / 255.0)
}

/// Perceptual luminance in 0..1 (sRGB coefficients).
pub(super) fn luminance(hex: &str) -> f64 {
    let Ok(raw) = normalize_hex(hex) else {
        return 0.0;
    };
    let body = raw.trim_start_matches('#');
    let r = byte_pair(body, 0);
    let g = byte_pair(body, 2);
    let b = byte_pair(body, 4);
    0.0722_f64.mul_add(b, 0.7152_f64.mul_add(g, 0.2126 * r))
}

/// Linear mix of two hex colors (`t` = weight of `b`).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped 0..=255
pub(super) fn mix(a: &str, b: &str, t: f64) -> String {
    let body = |hex: &str| -> String {
        normalize_hex(hex)
            .map(|norm| norm.trim_start_matches('#').to_string())
            .unwrap_or_default()
    };
    let (ab, bb) = (body(a), body(b));
    let t = t.clamp(0.0, 1.0);
    let channel = |ai: usize, bi: usize| -> u8 {
        // byte_pair is normalized (0..1); mix runs in 0..255 space.
        let pa = byte_pair(&ab, ai) * 255.0;
        let pb = byte_pair(&bb, bi) * 255.0;
        let value = pa.mul_add(1.0 - t, pb * t);
        value.round().clamp(0.0, 255.0) as u8
    };
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(0, 0),
        channel(2, 2),
        channel(4, 4)
    )
}

/// Readable text color for a given background.
pub(super) fn contrast_on(background: &str) -> String {
    if luminance(background) > 0.5 {
        "#10140f".to_string()
    } else {
        "#ffffff".to_string()
    }
}

/// Strip an optional alpha suffix, keeping `#rrggbb`.
pub(super) fn without_alpha(hex: &str) -> String {
    let Ok(norm) = normalize_hex(hex) else {
        return hex.to_string();
    };
    let body = norm.trim_start_matches('#');
    format!("#{}", body.get(..6).unwrap_or(body))
}

/// A label propagated into INI/env files (font family, icon/cursor theme):
/// no ASCII control bytes (newlines!), no `=`, no surrounding whitespace,
/// bounded length. Blocks config-line injection from shell values.
pub(super) fn sane_label(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 128
        && trimmed == value
        && !value.bytes().any(|b| b.is_ascii_control() || b == b'=')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_handles_short_and_long_forms() {
        assert_eq!(normalize_hex("#ABC").unwrap(), "#aabbcc");
        assert_eq!(normalize_hex("10140f").unwrap(), "#10140f");
        assert_eq!(normalize_hex("#10140f80").unwrap(), "#10140f80");
        assert!(normalize_hex("nope").is_err());
        assert!(normalize_hex("#12345").is_err());
    }

    #[test]
    fn luminance_orders_dark_and_light() {
        assert!(luminance("#000000") < luminance("#808080"));
        assert!(luminance("#ffffff") > 0.99);
    }

    #[test]
    fn mix_midpoint_and_contrast() {
        assert_eq!(mix("#000000", "#ffffff", 0.5), "#808080");
        assert_eq!(contrast_on("#ffffff"), "#10140f");
        assert_eq!(contrast_on("#10140f"), "#ffffff");
        assert_eq!(without_alpha("#10140f80"), "#10140f");
    }
}
