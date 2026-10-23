//! Minimal schema-driven niri config editing for managed sections.
//!
//! Unknown nodes and formatting are preserved by parsing with the `kdl`
//! document API and serializing the same document.

use kdl::{KdlDocument, KdlNode, KdlValue};
use serde_json::Value as JsonValue;
use ursine_api::{ApiError, NiriValues};

pub(super) fn parse_values(doc: &KdlDocument) -> NiriValues {
    NiriValues {
        layout_gaps: integer(doc, &["layout", "gaps"]),
        animations_off: flag(doc, &["animations", "off"]),
        touchpad_tap: flag(doc, &["input", "touchpad", "tap"]),
        touchpad_natural_scroll: flag(doc, &["input", "touchpad", "natural-scroll"]),
        touchpad_dwt: flag(doc, &["input", "touchpad", "dwt"]),
        cursor_size: integer(doc, &["cursor", "size"]),
        cursor_theme: string(doc, &["cursor", "theme"]),
    }
}

pub(super) fn apply_section(
    doc: &mut KdlDocument,
    section: &str,
    value: &JsonValue,
) -> Result<(), ApiError> {
    let object = value
        .as_object()
        .ok_or_else(|| ApiError::invalid("niri section value must be an object"))?;
    let mut applied = false;
    match section {
        "layout" => {
            reject_unknown(object, &["gaps"])?;
            if let Some(gaps) = object.get("gaps") {
                let gaps = gaps
                    .as_u64()
                    .and_then(|v| u32::try_from(v).ok())
                    .filter(|v| *v <= 256)
                    .ok_or_else(|| ApiError::invalid("layout gaps must be 0..256"))?;
                set_integer(doc, "layout", "gaps", gaps);
                applied = true;
            }
        }
        "animations" => {
            reject_unknown(object, &["off"])?;
            if let Some(off) = object.get("off") {
                let off = off
                    .as_bool()
                    .ok_or_else(|| ApiError::invalid("animations off must be a boolean"))?;
                set_flag(doc, "animations", "off", off);
                applied = true;
            }
        }
        "input.touchpad" => {
            reject_unknown(object, &["tap", "natural_scroll", "dwt"])?;
            for (key, node) in [
                ("tap", "tap"),
                ("natural_scroll", "natural-scroll"),
                ("dwt", "dwt"),
            ] {
                if let Some(on) = object.get(key) {
                    let on = on.as_bool().ok_or_else(|| {
                        ApiError::invalid(format!("touchpad {key} must be boolean"))
                    })?;
                    set_nested_flag(doc, "input", "touchpad", node, on);
                    applied = true;
                }
            }
        }
        "cursor" => {
            reject_unknown(object, &["size", "theme"])?;
            if let Some(size) = object.get("size") {
                let size = size
                    .as_u64()
                    .and_then(|v| u32::try_from(v).ok())
                    .filter(|v| (8..=128).contains(v))
                    .ok_or_else(|| ApiError::invalid("cursor size must be 8..128"))?;
                set_integer(doc, "cursor", "size", size);
                applied = true;
            }
            if let Some(theme) = object.get("theme") {
                let theme = theme
                    .as_str()
                    .filter(|theme| !theme.is_empty())
                    .ok_or_else(|| ApiError::invalid("cursor theme must be a non-empty string"))?;
                if theme.len() > 128 || theme.chars().any(char::is_control) {
                    return Err(ApiError::invalid("invalid cursor theme"));
                }
                set_string(doc, "cursor", "theme", theme);
                applied = true;
            }
        }
        _ => {
            return Err(ApiError::invalid(format!(
                "unsupported niri section: {section}"
            )))
        }
    }
    if !applied {
        return Err(ApiError::invalid(format!(
            "no recognized values for niri section {section}"
        )));
    }
    Ok(())
}

fn reject_unknown(
    object: &serde_json::Map<String, JsonValue>,
    allowed: &[&str],
) -> Result<(), ApiError> {
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(ApiError::invalid("unsupported niri section field"));
    }
    Ok(())
}

fn node<'a>(doc: &'a KdlDocument, name: &str) -> Option<&'a KdlNode> {
    doc.get(name)
}

fn child_node<'a>(doc: &'a KdlDocument, section: &str, key: &str) -> Option<&'a KdlNode> {
    node(doc, section)?.children()?.get(key)
}

fn nested_child<'a>(
    doc: &'a KdlDocument,
    section: &str,
    parent: &str,
    key: &str,
) -> Option<&'a KdlNode> {
    node(doc, section)?
        .children()?
        .get(parent)?
        .children()?
        .get(key)
}

fn integer(doc: &KdlDocument, path: &[&str]) -> Option<u32> {
    let node = match path {
        ["layout", "gaps"] => child_node(doc, "layout", "gaps"),
        ["cursor", "size"] => child_node(doc, "cursor", "size"),
        _ => None,
    }?;
    node.entries()
        .first()?
        .value()
        .as_integer()
        .and_then(|v| u32::try_from(v).ok())
}

fn string(doc: &KdlDocument, path: &[&str]) -> Option<String> {
    let node = match path {
        ["cursor", "theme"] => child_node(doc, "cursor", "theme"),
        _ => None,
    }?;
    node.entries()
        .first()?
        .value()
        .as_string()
        .map(str::to_string)
}

fn flag(doc: &KdlDocument, path: &[&str]) -> Option<bool> {
    let node = match path {
        ["animations", "off"] => child_node(doc, "animations", "off"),
        ["input", "touchpad", "tap"] => nested_child(doc, "input", "touchpad", "tap"),
        ["input", "touchpad", "natural-scroll"] => {
            nested_child(doc, "input", "touchpad", "natural-scroll")
        }
        ["input", "touchpad", "dwt"] => nested_child(doc, "input", "touchpad", "dwt"),
        _ => None,
    }?;
    Some(
        node.entries()
            .first()
            .and_then(|entry| entry.value().as_bool())
            .unwrap_or(true),
    )
}

fn ensure_node<'a>(doc: &'a mut KdlDocument, name: &str) -> &'a mut KdlNode {
    if doc.get(name).is_none() {
        doc.nodes_mut().push(KdlNode::new(name));
    }
    let Some(node) = doc.get_mut(name) else {
        unreachable!("node was just inserted")
    };
    node
}

fn ensure_child<'a>(doc: &'a mut KdlDocument, section: &str, key: &str) -> &'a mut KdlNode {
    let children = ensure_node(doc, section).ensure_children();
    if children.get(key).is_none() {
        children.nodes_mut().push(KdlNode::new(key));
    }
    let Some(node) = children.get_mut(key) else {
        unreachable!("child was just inserted")
    };
    node
}

fn ensure_nested_child<'a>(
    doc: &'a mut KdlDocument,
    section: &str,
    parent: &str,
    key: &str,
) -> &'a mut KdlNode {
    let children = ensure_node(doc, section).ensure_children();
    if children.get(parent).is_none() {
        children.nodes_mut().push(KdlNode::new(parent));
    }
    let Some(parent) = children.get_mut(parent) else {
        unreachable!("parent was just inserted")
    };
    let nested = parent.ensure_children();
    if nested.get(key).is_none() {
        nested.nodes_mut().push(KdlNode::new(key));
    }
    let Some(node) = nested.get_mut(key) else {
        unreachable!("nested key was just inserted")
    };
    node
}

fn set_arg(node: &mut KdlNode, value: KdlValue) {
    let repr = value.to_string();
    if let Some(entry) = node.entries_mut().first_mut() {
        *entry.value_mut() = value;
        if let Some(format) = entry.format_mut() {
            format.value_repr = repr;
        }
    } else {
        node.push(value);
    }
}

fn set_integer(doc: &mut KdlDocument, section: &str, key: &str, value: u32) {
    set_arg(
        ensure_child(doc, section, key),
        KdlValue::Integer(i128::from(value)),
    );
}

fn set_string(doc: &mut KdlDocument, section: &str, key: &str, value: &str) {
    set_arg(
        ensure_child(doc, section, key),
        KdlValue::String(value.to_string()),
    );
}

fn set_flag(doc: &mut KdlDocument, section: &str, key: &str, on: bool) {
    let children = ensure_node(doc, section).ensure_children();
    if on {
        if children.get(key).is_none() {
            children.nodes_mut().push(KdlNode::new(key));
        }
    } else {
        children
            .nodes_mut()
            .retain(|node| node.name().value() != key);
    }
}

fn set_nested_flag(doc: &mut KdlDocument, section: &str, parent: &str, key: &str, on: bool) {
    if on {
        ensure_nested_child(doc, section, parent, key);
    } else {
        let children = ensure_node(doc, section).ensure_children();
        if let Some(parent) = children
            .get_mut(parent)
            .and_then(|n| n.children_mut().as_mut())
        {
            parent.nodes_mut().retain(|node| node.name().value() != key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"layout {
    gaps 16
}
animations {
    off
}
input {
    touchpad {
        tap
        dwt
    }
}
// user comment
spawn-at-startup "foo"
"#;

    #[test]
    fn parses_and_roundtrips_unknown_nodes() {
        let mut doc: KdlDocument = SAMPLE.parse().unwrap();
        let values = parse_values(&doc);
        assert_eq!(values.layout_gaps, Some(16));
        assert_eq!(values.animations_off, Some(true));
        assert_eq!(values.touchpad_tap, Some(true));
        assert_eq!(values.touchpad_dwt, Some(true));
        assert_eq!(values.touchpad_natural_scroll, None);

        apply_section(&mut doc, "layout", &serde_json::json!({"gaps": 24})).unwrap();
        let output = doc.to_string();
        assert!(output.contains("gaps 24"));
        assert!(output.contains("spawn-at-startup"));
        assert!(output.contains("user comment"));
    }

    #[test]
    fn rejects_bad_sections_and_values() {
        let mut doc = KdlDocument::new();
        assert!(apply_section(&mut doc, "nope", &serde_json::json!({})).is_err());
        assert!(apply_section(&mut doc, "layout", &serde_json::json!({"gaps": 9999})).is_err());
        assert!(apply_section(&mut doc, "cursor", &serde_json::json!({"size": 4})).is_err());
        assert!(apply_section(&mut doc, "layout", &serde_json::json!({"gaps": "16"})).is_err());
        assert!(apply_section(&mut doc, "animations", &serde_json::json!({})).is_err());
        assert!(apply_section(&mut doc, "animations", &serde_json::json!({"off": "yes"})).is_err());
        assert!(apply_section(&mut doc, "cursor", &serde_json::json!({"theme": ""})).is_err());
        assert!(apply_section(&mut doc, "cursor", &serde_json::json!({"nope": 1})).is_err());
    }
}
