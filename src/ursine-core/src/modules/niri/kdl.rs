//! Minimal schema-driven niri config editing for managed sections.
//!
//! Unknown nodes and formatting are preserved by parsing with the `kdl`
//! document API and serializing the same document.

use kdl::{KdlDocument, KdlNode, KdlValue};
use serde_json::Value as JsonValue;
use ursine_api::{
    ApiError, NiriBindAction, NiriEdgeGesture, NiriEnvVar, NiriGestures, NiriKeybind, NiriOutput,
    NiriSpawn, NiriStruts, NiriValues,
};

pub(super) fn parse_values(doc: &KdlDocument) -> NiriValues {
    NiriValues {
        layout_gaps: integer(doc, &["layout", "gaps"]),
        animations_off: flag(doc, &["animations", "off"]),
        touchpad_tap: flag(doc, &["input", "touchpad", "tap"]),
        touchpad_natural_scroll: flag(doc, &["input", "touchpad", "natural-scroll"]),
        touchpad_dwt: flag(doc, &["input", "touchpad", "dwt"]),
        touchpad_accel_speed: speed(doc, &["input", "touchpad", "accel-speed"]),
        touchpad_accel_profile: string(doc, &["input", "touchpad", "accel-profile"]),
        mouse_accel_speed: speed(doc, &["input", "mouse", "accel-speed"]),
        mouse_accel_profile: string(doc, &["input", "mouse", "accel-profile"]),
        mouse_natural_scroll: flag(doc, &["input", "mouse", "natural-scroll"]),
        cursor_size: integer(doc, &["cursor", "size"]),
        cursor_theme: string(doc, &["cursor", "theme"]),
        keyboard_layout: string(doc, &["input", "keyboard", "xkb", "layout"]),
        keyboard_repeat_delay: integer(doc, &["input", "keyboard", "repeat-delay"]),
        keyboard_repeat_rate: integer(doc, &["input", "keyboard", "repeat-rate"]),
        keyboard_track_layout: string(doc, &["input", "keyboard", "track-layout"]),
        keyboard_numlock: flag(doc, &["input", "keyboard", "numlock"]),
        layout_border_width: integer(doc, &["layout", "border", "width"]),
        layout_focus_ring_width: integer(doc, &["layout", "focus-ring", "width"]),
        layout_struts: struts(doc),
        screenshot_path: string(doc, &["screenshot-path"]),
        hotkey_overlay_skip_at_startup: flag(doc, &["hotkey-overlay", "skip-at-startup"]),
        outputs: outputs(doc),
        environment: environment(doc),
        spawn_at_startup: spawn_at_startup(doc),
        keybinds: keybinds(doc),
        gestures: gestures(doc),
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
            reject_unknown(
                object,
                &[
                    "gaps",
                    "border_width",
                    "focus_ring_width",
                    "strut_left",
                    "strut_right",
                    "strut_top",
                    "strut_bottom",
                ],
            )?;
            if let Some(gaps) = object.get("gaps") {
                let gaps = number(gaps, "layout gaps", 256)?;
                set_integer(doc, "layout", "gaps", gaps);
                applied = true;
            }
            for (key, parent, node_name, label) in [
                ("border_width", "border", "width", "border width"),
                (
                    "focus_ring_width",
                    "focus-ring",
                    "width",
                    "focus ring width",
                ),
            ] {
                if let Some(value) = object.get(key) {
                    let value = number(value, label, 64)?;
                    set_nested_integer(doc, "layout", parent, node_name, value);
                    applied = true;
                }
            }
            for (key, node_name, label) in [
                ("strut_left", "left", "left strut"),
                ("strut_right", "right", "right strut"),
                ("strut_top", "top", "top strut"),
                ("strut_bottom", "bottom", "bottom strut"),
            ] {
                if let Some(value) = object.get(key) {
                    let value = number(value, label, 1024)?;
                    set_strut(doc, node_name, value);
                    applied = true;
                }
            }
        }
        "input.keyboard" => {
            reject_unknown(
                object,
                &[
                    "layout",
                    "repeat_delay",
                    "repeat_rate",
                    "track_layout",
                    "numlock",
                ],
            )?;
            if let Some(layout) = object.get("layout") {
                let layout = layout
                    .as_str()
                    .filter(|value| value.len() <= 128)
                    .ok_or_else(|| ApiError::invalid("invalid keyboard layout"))?;
                if layout.is_empty() {
                    remove_keyboard_layout(doc);
                } else {
                    set_keyboard_layout(doc, layout);
                }
                applied = true;
            }
            for (key, node_name, label) in [
                ("repeat_delay", "repeat-delay", "repeat delay"),
                ("repeat_rate", "repeat-rate", "repeat rate"),
            ] {
                if let Some(value) = object.get(key) {
                    let value = number(value, label, 5000)?;
                    set_nested_integer(doc, "input", "keyboard", node_name, value);
                    applied = true;
                }
            }
            if let Some(value) = object.get("track_layout") {
                let value = value
                    .as_str()
                    .filter(|value| matches!(*value, "global" | "window"))
                    .ok_or_else(|| ApiError::invalid("track layout must be global or window"))?;
                set_nested_string(doc, "input", "keyboard", "track-layout", value);
                applied = true;
            }
            if let Some(value) = object.get("numlock") {
                let value = value
                    .as_bool()
                    .ok_or_else(|| ApiError::invalid("numlock must be boolean"))?;
                set_nested_flag(doc, "input", "keyboard", "numlock", value);
                applied = true;
            }
        }
        "screenshot" => {
            reject_unknown(object, &["path"])?;
            if let Some(path) = object.get("path") {
                let path = path
                    .as_str()
                    .filter(|value| value.len() <= 4096)
                    .ok_or_else(|| ApiError::invalid("invalid screenshot path"))?;
                if path.is_empty() {
                    remove_top(doc, "screenshot-path");
                } else {
                    set_top_string(doc, "screenshot-path", path);
                }
                applied = true;
            }
        }
        "hotkey-overlay" => {
            reject_unknown(object, &["skip_at_startup"])?;
            if let Some(on) = object.get("skip_at_startup") {
                let on = on
                    .as_bool()
                    .ok_or_else(|| ApiError::invalid("hotkey overlay flag must be boolean"))?;
                set_top_nested_flag(doc, "hotkey-overlay", "skip-at-startup", on);
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
            reject_unknown(
                object,
                &[
                    "tap",
                    "natural_scroll",
                    "dwt",
                    "accel_speed",
                    "accel_profile",
                ],
            )?;
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
            if let Some(value) = object.get("accel_speed") {
                let value = speed_value(value, "touchpad acceleration")?;
                set_nested_speed(doc, "input", "touchpad", "accel-speed", value);
                applied = true;
            }
            if let Some(value) = object.get("accel_profile") {
                let value = profile(value, "touchpad acceleration profile")?;
                set_nested_string(doc, "input", "touchpad", "accel-profile", value);
                applied = true;
            }
        }
        "input.mouse" => {
            reject_unknown(object, &["accel_speed", "accel_profile", "natural_scroll"])?;
            if let Some(value) = object.get("accel_speed") {
                let value = speed_value(value, "mouse acceleration")?;
                set_nested_speed(doc, "input", "mouse", "accel-speed", value);
                applied = true;
            }
            if let Some(value) = object.get("accel_profile") {
                let value = profile(value, "mouse acceleration profile")?;
                set_nested_string(doc, "input", "mouse", "accel-profile", value);
                applied = true;
            }
            if let Some(value) = object.get("natural_scroll") {
                let value = value
                    .as_bool()
                    .ok_or_else(|| ApiError::invalid("mouse natural scroll must be boolean"))?;
                set_nested_flag(doc, "input", "mouse", "natural-scroll", value);
                applied = true;
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
        "keybinds" => {
            reject_unknown(object, &["binds"])?;
            let binds = object
                .get("binds")
                .and_then(JsonValue::as_array)
                .ok_or_else(|| ApiError::invalid("keybinds must be an array"))?;
            apply_keybinds(doc, binds)?;
            applied = true;
        }
        "gestures" => {
            apply_gestures(doc, object)?;
            applied = true;
        }
        "spawn" => {
            reject_unknown(object, &["commands"])?;
            let commands = object
                .get("commands")
                .and_then(JsonValue::as_array)
                .ok_or_else(|| ApiError::invalid("spawn commands must be an array"))?;
            apply_spawn(doc, commands)?;
            applied = true;
        }
        "environment" => {
            reject_unknown(object, &["vars"])?;
            let vars = object
                .get("vars")
                .and_then(JsonValue::as_array)
                .ok_or_else(|| ApiError::invalid("environment vars must be an array"))?;
            apply_environment(doc, vars)?;
            applied = true;
        }
        "outputs" => {
            reject_unknown(object, &["outputs"])?;
            let outputs = object
                .get("outputs")
                .and_then(JsonValue::as_array)
                .ok_or_else(|| ApiError::invalid("outputs must be an array"))?;
            for output in outputs {
                apply_output(doc, output)?;
            }
            applied = true;
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
        ["layout", "border", "width"] => nested_child(doc, "layout", "border", "width"),
        ["layout", "focus-ring", "width"] => nested_child(doc, "layout", "focus-ring", "width"),
        ["input", "keyboard", "repeat-delay"] => {
            nested_child(doc, "input", "keyboard", "repeat-delay")
        }
        ["input", "keyboard", "repeat-rate"] => {
            nested_child(doc, "input", "keyboard", "repeat-rate")
        }
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
        ["screenshot-path"] => node(doc, "screenshot-path"),
        ["input", "keyboard", "xkb", "layout"] => node(doc, "input")?
            .children()?
            .get("keyboard")?
            .children()?
            .get("xkb")?
            .children()?
            .get("layout"),
        ["input", "keyboard", "track-layout"] => {
            nested_child(doc, "input", "keyboard", "track-layout")
        }
        ["input", "touchpad", "accel-profile"] => {
            nested_child(doc, "input", "touchpad", "accel-profile")
        }
        ["input", "mouse", "accel-profile"] => nested_child(doc, "input", "mouse", "accel-profile"),
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
        ["input", "keyboard", "numlock"] => nested_child(doc, "input", "keyboard", "numlock"),
        ["input", "mouse", "natural-scroll"] => {
            nested_child(doc, "input", "mouse", "natural-scroll")
        }
        ["hotkey-overlay", "skip-at-startup"] => {
            child_node(doc, "hotkey-overlay", "skip-at-startup")
        }
        _ => None,
    }?;
    Some(
        node.entries()
            .first()
            .and_then(|entry| entry.value().as_bool())
            .unwrap_or(true),
    )
}

#[allow(clippy::cast_possible_truncation)]
fn keybinds(doc: &KdlDocument) -> Vec<NiriKeybind> {
    doc.get("binds")
        .and_then(|node| node.children())
        .map(|children| {
            children
                .nodes()
                .iter()
                .map(|node| NiriKeybind {
                    key: node.name().value().to_string(),
                    actions: node
                        .children()
                        .map(|actions| {
                            actions
                                .nodes()
                                .iter()
                                .map(|action| NiriBindAction {
                                    name: action.name().value().to_string(),
                                    args: action
                                        .entries()
                                        .iter()
                                        .filter_map(|entry| {
                                            entry.value().as_string().map(str::to_string)
                                        })
                                        .collect(),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn apply_keybinds(doc: &mut KdlDocument, binds: &[JsonValue]) -> Result<(), ApiError> {
    let mut desired = Vec::with_capacity(binds.len());
    for value in binds {
        let object = value
            .as_object()
            .ok_or_else(|| ApiError::invalid("keybind must be an object"))?;
        reject_unknown(object, &["key", "actions"])?;
        let key = object
            .get("key")
            .and_then(JsonValue::as_str)
            .filter(|key| valid_key(key))
            .ok_or_else(|| ApiError::invalid("invalid key combination"))?;
        let actions = object
            .get("actions")
            .and_then(JsonValue::as_array)
            .filter(|actions| !actions.is_empty())
            .ok_or_else(|| ApiError::invalid("keybind needs at least one action"))?;
        let mut parsed = Vec::with_capacity(actions.len());
        for action in actions {
            let action = action
                .as_object()
                .ok_or_else(|| ApiError::invalid("bind action must be an object"))?;
            reject_unknown(action, &["name", "args"])?;
            let name = action
                .get("name")
                .and_then(JsonValue::as_str)
                .filter(|name| valid_action(name))
                .ok_or_else(|| ApiError::invalid("invalid bind action"))?;
            let args = action
                .get("args")
                .and_then(JsonValue::as_array)
                .map_or_else(Vec::new, |args| args.iter().collect::<Vec<_>>());
            if args.len() > 64 {
                return Err(ApiError::invalid("too many bind action arguments"));
            }
            let mut parsed_args = Vec::with_capacity(args.len());
            for arg in args {
                let arg = arg
                    .as_str()
                    .filter(|arg| arg.len() <= 4096)
                    .ok_or_else(|| ApiError::invalid("invalid bind action argument"))?;
                parsed_args.push(arg.to_string());
            }
            parsed.push(NiriBindAction {
                name: name.to_string(),
                args: parsed_args,
            });
        }
        desired.push(NiriKeybind {
            key: key.to_string(),
            actions: parsed,
        });
    }
    let binds_node = ensure_node(doc, "binds").ensure_children();
    binds_node.nodes_mut().clear();
    for bind in desired {
        let mut node = KdlNode::new(bind.key);
        let mut actions = KdlDocument::new();
        for action in bind.actions {
            let mut action_node = KdlNode::new(action.name);
            for arg in action.args {
                action_node.push(KdlValue::String(arg));
            }
            action_node.autoformat();
            actions.nodes_mut().push(action_node);
        }
        node.set_children(actions);
        node.autoformat();
        binds_node.nodes_mut().push(node);
    }
    Ok(())
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '_' | ':' | ' '))
}

fn valid_action(action: &str) -> bool {
    !action.is_empty()
        && action.len() <= 128
        && action
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

fn gestures(doc: &KdlDocument) -> NiriGestures {
    let Some(gestures) = doc.get("gestures").and_then(|node| node.children()) else {
        return NiriGestures::default();
    };
    let hot = gestures.get("hot-corners").and_then(|node| node.children());
    let hot_corners = hot
        .map(|children| {
            children
                .nodes()
                .iter()
                .map(|node| node.name().value().to_string())
                .filter(|name| name != "off")
                .collect()
        })
        .unwrap_or_default();
    let hot_corners_off = hot.is_some_and(|children| children.get("off").is_some());
    NiriGestures {
        hot_corners,
        hot_corners_off,
        dnd_edge_view_scroll: edge_gesture(gestures, "dnd-edge-view-scroll"),
        dnd_edge_workspace_switch: edge_gesture(gestures, "dnd-edge-workspace-switch"),
    }
}

fn edge_gesture(doc: &KdlDocument, name: &str) -> NiriEdgeGesture {
    let Some(children) = doc.get(name).and_then(|node| node.children()) else {
        return NiriEdgeGesture::default();
    };
    NiriEdgeGesture {
        trigger_width: integer_child(children, "trigger-width"),
        trigger_height: integer_child(children, "trigger-height"),
        delay_ms: integer_child(children, "delay-ms"),
        max_speed: integer_child(children, "max-speed"),
    }
}

fn apply_gestures(
    doc: &mut KdlDocument,
    object: &serde_json::Map<String, JsonValue>,
) -> Result<(), ApiError> {
    reject_unknown(
        object,
        &[
            "hot_corners",
            "hot_corners_off",
            "dnd_edge_view_scroll",
            "dnd_edge_workspace_switch",
        ],
    )?;
    let gestures = ensure_node(doc, "gestures").ensure_children();
    if let Some(corners) = object.get("hot_corners") {
        let corners = corners
            .as_array()
            .ok_or_else(|| ApiError::invalid("hot_corners must be an array"))?;
        let mut names = Vec::with_capacity(corners.len());
        for corner in corners {
            let corner = corner
                .as_str()
                .filter(|corner| {
                    matches!(
                        *corner,
                        "top-left" | "top-right" | "bottom-left" | "bottom-right"
                    )
                })
                .ok_or_else(|| ApiError::invalid("invalid hot corner"))?;
            names.push(corner.to_string());
        }
        if gestures.get("hot-corners").is_none() {
            gestures.nodes_mut().push(KdlNode::new("hot-corners"));
        }
        if let Some(hot) = gestures
            .get_mut("hot-corners")
            .and_then(|node| node.children_mut().as_mut())
        {
            hot.nodes_mut().clear();
            if object
                .get("hot_corners_off")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false)
            {
                hot.nodes_mut().push(KdlNode::new("off"));
            } else {
                for name in names {
                    hot.nodes_mut().push(KdlNode::new(name));
                }
            }
        }
    }
    for (key, name) in [
        ("dnd_edge_view_scroll", "dnd-edge-view-scroll"),
        ("dnd_edge_workspace_switch", "dnd-edge-workspace-switch"),
    ] {
        if let Some(value) = object.get(key) {
            let value = value
                .as_object()
                .ok_or_else(|| ApiError::invalid("edge gesture must be an object"))?;
            reject_unknown(
                value,
                &["trigger_width", "trigger_height", "delay_ms", "max_speed"],
            )?;
            if gestures.get(name).is_none() {
                gestures.nodes_mut().push(KdlNode::new(name));
            }
            if let Some(children) = gestures
                .get_mut(name)
                .and_then(|node| node.children_mut().as_mut())
            {
                for (field, node_name) in [
                    ("trigger_width", "trigger-width"),
                    ("trigger_height", "trigger-height"),
                    ("delay_ms", "delay-ms"),
                    ("max_speed", "max-speed"),
                ] {
                    if let Some(value) = value.get(field) {
                        let value = number(value, field, 100_000)?;
                        if children.get(node_name).is_none() {
                            children.nodes_mut().push(KdlNode::new(node_name));
                        }
                        if let Some(node) = children.get_mut(node_name) {
                            set_arg(node, KdlValue::Integer(i128::from(value)));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn spawn_at_startup(doc: &KdlDocument) -> Vec<NiriSpawn> {
    doc.nodes()
        .iter()
        .filter(|node| node.name().value() == "spawn-at-startup")
        .filter_map(|node| {
            let command: Vec<String> = node
                .entries()
                .iter()
                .filter_map(|entry| entry.value().as_string().map(str::to_string))
                .collect();
            (!command.is_empty()).then_some(NiriSpawn { command })
        })
        .collect()
}

fn apply_spawn(doc: &mut KdlDocument, commands: &[JsonValue]) -> Result<(), ApiError> {
    let mut desired = Vec::with_capacity(commands.len());
    for value in commands {
        let command = value
            .as_array()
            .ok_or_else(|| ApiError::invalid("spawn command must be an array"))?;
        if command.is_empty() || command.len() > 64 {
            return Err(ApiError::invalid("spawn command must have 1..64 arguments"));
        }
        let mut args = Vec::with_capacity(command.len());
        for arg in command {
            let arg = arg
                .as_str()
                .filter(|arg| arg.len() <= 4096 && !arg.contains('\0'))
                .ok_or_else(|| ApiError::invalid("invalid spawn argument"))?;
            if args.is_empty() && arg.is_empty() {
                return Err(ApiError::invalid("spawn program cannot be empty"));
            }
            args.push(arg.to_string());
        }
        desired.push(args);
    }
    doc.nodes_mut()
        .retain(|node| node.name().value() != "spawn-at-startup");
    for command in desired {
        let mut node = KdlNode::new("spawn-at-startup");
        for arg in command {
            node.push(KdlValue::String(arg));
        }
        node.autoformat();
        doc.nodes_mut().push(node);
    }
    Ok(())
}

fn environment(doc: &KdlDocument) -> Vec<NiriEnvVar> {
    doc.get("environment")
        .and_then(|node| node.children())
        .map(|children| {
            children
                .nodes()
                .iter()
                .map(|node| NiriEnvVar {
                    name: node.name().value().to_string(),
                    value: node
                        .entries()
                        .first()
                        .and_then(|entry| entry.value().as_string())
                        .unwrap_or_default()
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn apply_environment(doc: &mut KdlDocument, vars: &[JsonValue]) -> Result<(), ApiError> {
    let mut desired = Vec::with_capacity(vars.len());
    for value in vars {
        let object = value
            .as_object()
            .ok_or_else(|| ApiError::invalid("environment variable must be an object"))?;
        reject_unknown(object, &["name", "value"])?;
        let name = object
            .get("name")
            .and_then(JsonValue::as_str)
            .filter(|name| {
                !name.is_empty()
                    && name.len() <= 128
                    && name
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
            .ok_or_else(|| ApiError::invalid("invalid environment variable name"))?;
        let value = object
            .get("value")
            .and_then(JsonValue::as_str)
            .filter(|value| value.len() <= 4096)
            .ok_or_else(|| ApiError::invalid("invalid environment variable value"))?;
        if desired
            .iter()
            .any(|(known, _): &(String, String)| known == name)
        {
            return Err(ApiError::invalid("duplicate environment variable"));
        }
        desired.push((name.to_string(), value.to_string()));
    }

    if desired.is_empty() {
        remove_top(doc, "environment");
        return Ok(());
    }

    let environment = ensure_node(doc, "environment").ensure_children();
    environment
        .nodes_mut()
        .retain(|node| desired.iter().any(|(name, _)| name == node.name().value()));
    for (name, value) in desired {
        if environment.get(&name).is_none() {
            environment.nodes_mut().push(KdlNode::new(name.clone()));
        }
        if let Some(node) = environment.get_mut(&name) {
            set_arg(node, KdlValue::String(value));
            node.autoformat();
        }
    }
    Ok(())
}

#[allow(clippy::cast_possible_truncation)]
fn outputs(doc: &KdlDocument) -> Vec<NiriOutput> {
    doc.nodes()
        .iter()
        .filter(|node| node.name().value() == "output")
        .filter_map(|node| {
            let name = node.entries().first()?.value().as_string()?.to_string();
            Some(NiriOutput {
                name,
                scale: node
                    .children()
                    .and_then(|children| children.get("scale"))
                    .and_then(|node| node.entries().first())
                    .and_then(|entry| match entry.value() {
                        KdlValue::Float(value) => {
                            u32::try_from((value * 100.0).round() as i64).ok()
                        }
                        KdlValue::Integer(value) => u32::try_from(*value).ok(),
                        _ => None,
                    }),
                transform: node
                    .children()
                    .and_then(|children| children.get("transform"))
                    .and_then(|node| node.entries().first()?.value().as_string())
                    .map(str::to_string),
                variable_refresh_rate: node
                    .children()
                    .and_then(|children| children.get("variable-refresh-rate"))
                    .map(|_| true),
            })
        })
        .collect()
}

fn apply_output(doc: &mut KdlDocument, value: &JsonValue) -> Result<(), ApiError> {
    let object = value
        .as_object()
        .ok_or_else(|| ApiError::invalid("output must be an object"))?;
    reject_unknown(
        object,
        &["name", "scale", "transform", "variable_refresh_rate"],
    )?;
    let name = object
        .get("name")
        .and_then(JsonValue::as_str)
        .filter(|name| !name.is_empty() && name.len() <= 128)
        .ok_or_else(|| ApiError::invalid("output name is required"))?;
    let index = doc.nodes().iter().position(|node| {
        node.name().value() == "output"
            && node
                .entries()
                .first()
                .and_then(|entry| entry.value().as_string())
                == Some(name)
    });
    let index = index.unwrap_or_else(|| {
        let mut node = KdlNode::new("output");
        node.push(KdlValue::String(name.to_string()));
        doc.nodes_mut().push(node);
        doc.nodes().len() - 1
    });
    let Some(node) = doc.nodes_mut().get_mut(index) else {
        return Err(ApiError::internal("output node index disappeared"));
    };
    node.autoformat();
    let children = node.ensure_children();
    if let Some(scale) = object.get("scale") {
        let scale = scale
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| (25..=400).contains(value))
            .ok_or_else(|| ApiError::invalid("output scale must be 25..400 percent"))?;
        set_child(children, "scale", KdlValue::Float(f64::from(scale) / 100.0));
    }
    if let Some(transform) = object.get("transform") {
        let transform = transform
            .as_str()
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 32
                    && value
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            })
            .ok_or_else(|| ApiError::invalid("invalid output transform"))?;
        set_child(
            children,
            "transform",
            KdlValue::String(transform.to_string()),
        );
    }
    if let Some(vrr) = object.get("variable_refresh_rate") {
        let vrr = vrr
            .as_bool()
            .ok_or_else(|| ApiError::invalid("variable refresh rate must be boolean"))?;
        if vrr {
            if children.get("variable-refresh-rate").is_none() {
                children
                    .nodes_mut()
                    .push(KdlNode::new("variable-refresh-rate"));
            }
        } else {
            children
                .nodes_mut()
                .retain(|node| node.name().value() != "variable-refresh-rate");
        }
    }
    Ok(())
}

fn set_child(doc: &mut KdlDocument, name: &str, value: KdlValue) {
    if doc.get(name).is_none() {
        doc.nodes_mut().push(KdlNode::new(name));
    }
    if let Some(node) = doc.get_mut(name) {
        set_arg(node, value);
    }
}

fn struts(doc: &KdlDocument) -> NiriStruts {
    let Some(struts) = child_node(doc, "layout", "struts") else {
        return NiriStruts::default();
    };
    NiriStruts {
        left: struts
            .children()
            .and_then(|children| integer_child(children, "left")),
        right: struts
            .children()
            .and_then(|children| integer_child(children, "right")),
        top: struts
            .children()
            .and_then(|children| integer_child(children, "top")),
        bottom: struts
            .children()
            .and_then(|children| integer_child(children, "bottom")),
    }
}

fn integer_child(doc: &KdlDocument, name: &str) -> Option<u32> {
    doc.get(name)?
        .entries()
        .first()?
        .value()
        .as_integer()
        .and_then(|value| u32::try_from(value).ok())
}

fn number(value: &JsonValue, label: &str, max: u32) -> Result<u32, ApiError> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value <= max)
        .ok_or_else(|| ApiError::invalid(format!("{label} must be 0..{max}")))
}

fn ensure_keyboard_xkb(doc: &mut KdlDocument) -> &mut KdlDocument {
    let input = ensure_node(doc, "input").ensure_children();
    if input.get("keyboard").is_none() {
        input.nodes_mut().push(KdlNode::new("keyboard"));
    }
    let Some(keyboard) = input.get_mut("keyboard") else {
        unreachable!("keyboard was just inserted")
    };
    let keyboard = keyboard.ensure_children();
    if keyboard.get("xkb").is_none() {
        keyboard.nodes_mut().push(KdlNode::new("xkb"));
    }
    let Some(xkb) = keyboard.get_mut("xkb") else {
        unreachable!("xkb was just inserted")
    };
    xkb.ensure_children()
}

fn set_keyboard_layout(doc: &mut KdlDocument, layout: &str) {
    let xkb = ensure_keyboard_xkb(doc);
    if xkb.get("layout").is_none() {
        xkb.nodes_mut().push(KdlNode::new("layout"));
    }
    if let Some(node) = xkb.get_mut("layout") {
        set_arg(node, KdlValue::String(layout.to_string()));
    }
}

fn remove_keyboard_layout(doc: &mut KdlDocument) {
    let Some(input) = doc
        .get_mut("input")
        .and_then(|node| node.children_mut().as_mut())
    else {
        return;
    };
    let Some(keyboard) = input
        .get_mut("keyboard")
        .and_then(|node| node.children_mut().as_mut())
    else {
        return;
    };
    if let Some(xkb) = keyboard
        .get_mut("xkb")
        .and_then(|node| node.children_mut().as_mut())
    {
        xkb.nodes_mut()
            .retain(|node| node.name().value() != "layout");
    }
}

#[allow(clippy::cast_possible_truncation)]
fn speed(doc: &KdlDocument, path: &[&str]) -> Option<i32> {
    let node = match path {
        ["input", "touchpad", "accel-speed"] => {
            nested_child(doc, "input", "touchpad", "accel-speed")
        }
        ["input", "mouse", "accel-speed"] => nested_child(doc, "input", "mouse", "accel-speed"),
        _ => None,
    }?;
    match node.entries().first()?.value() {
        KdlValue::Float(value) => Some((value * 100.0).round() as i32),
        KdlValue::Integer(value) => i32::try_from(*value).ok(),
        _ => None,
    }
}

fn speed_value(value: &JsonValue, label: &str) -> Result<i32, ApiError> {
    value
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .filter(|value| (-100..=100).contains(value))
        .ok_or_else(|| ApiError::invalid(format!("{label} must be -100..100")))
}

fn profile<'a>(value: &'a JsonValue, label: &str) -> Result<&'a str, ApiError> {
    value
        .as_str()
        .filter(|value| matches!(*value, "adaptive" | "flat"))
        .ok_or_else(|| ApiError::invalid(format!("{label} must be adaptive or flat")))
}

fn set_nested_speed(doc: &mut KdlDocument, section: &str, parent: &str, key: &str, value: i32) {
    set_arg(
        ensure_nested_child(doc, section, parent, key),
        KdlValue::Float(f64::from(value) / 100.0),
    );
}

fn set_nested_string(doc: &mut KdlDocument, section: &str, parent: &str, key: &str, value: &str) {
    set_arg(
        ensure_nested_child(doc, section, parent, key),
        KdlValue::String(value.to_string()),
    );
}

fn set_nested_integer(doc: &mut KdlDocument, section: &str, parent: &str, key: &str, value: u32) {
    set_arg(
        ensure_nested_child(doc, section, parent, key),
        KdlValue::Integer(i128::from(value)),
    );
}

fn set_strut(doc: &mut KdlDocument, key: &str, value: u32) {
    let layout = ensure_node(doc, "layout").ensure_children();
    if layout.get("struts").is_none() {
        layout.nodes_mut().push(KdlNode::new("struts"));
    }
    let Some(struts) = layout.get_mut("struts") else {
        unreachable!("struts was just inserted")
    };
    let children = struts.ensure_children();
    if children.get(key).is_none() {
        children.nodes_mut().push(KdlNode::new(key));
    }
    if let Some(node) = children.get_mut(key) {
        set_arg(node, KdlValue::Integer(i128::from(value)));
    }
}

fn set_top_string(doc: &mut KdlDocument, name: &str, value: &str) {
    set_arg(ensure_node(doc, name), KdlValue::String(value.to_string()));
}

fn set_top_nested_flag(doc: &mut KdlDocument, section: &str, key: &str, on: bool) {
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

fn remove_top(doc: &mut KdlDocument, name: &str) {
    doc.nodes_mut().retain(|node| node.name().value() != name);
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
    fn parses_keyboard_layout_and_misc() {
        let sample = r#"input {
    keyboard {
        xkb { layout "it" }
        repeat-delay 600
        repeat-rate 25
    }
}
layout {
    border { width 4 }
    focus-ring { width 2 }
    struts { left 8; right 8; top 4; bottom 4; }
}
screenshot-path "~/Pictures/%Y.png"
hotkey-overlay { skip-at-startup; }
"#;
        let doc: KdlDocument = sample.parse().unwrap();
        let values = parse_values(&doc);
        assert_eq!(values.keyboard_layout.as_deref(), Some("it"));
        assert_eq!(values.keyboard_repeat_delay, Some(600));
        assert_eq!(values.keyboard_repeat_rate, Some(25));
        assert_eq!(values.layout_border_width, Some(4));
        assert_eq!(values.layout_focus_ring_width, Some(2));
        assert_eq!(values.layout_struts.left, Some(8));
        assert_eq!(values.layout_struts.bottom, Some(4));
        assert_eq!(values.screenshot_path.as_deref(), Some("~/Pictures/%Y.png"));
        assert_eq!(values.hotkey_overlay_skip_at_startup, Some(true));
    }

    #[test]
    fn parses_and_updates_outputs() {
        let mut doc: KdlDocument = r#"output "eDP-1" {
    scale 1.5
    transform "normal"
    variable-refresh-rate
}
"#
        .parse()
        .unwrap();
        let values = parse_values(&doc);
        assert_eq!(values.outputs.len(), 1);
        assert_eq!(values.outputs[0].name, "eDP-1");
        assert_eq!(values.outputs[0].scale, Some(150));
        assert_eq!(values.outputs[0].transform.as_deref(), Some("normal"));
        apply_section(
            &mut doc,
            "outputs",
            &serde_json::json!({"outputs": [{"name": "DP-2", "scale": 125, "transform": "90"}]}),
        )
        .unwrap();
        let output = doc.to_string();
        assert!(output.contains("output DP-2"));
        assert!(output.contains("scale 1.25"));
        assert!(output.contains("transform \"90\""));
    }

    #[test]
    fn parses_and_updates_environment() {
        let mut doc: KdlDocument = r#"environment {
    FOO "bar"
    KEEP "1"
}
"#
        .parse()
        .unwrap();
        let values = parse_values(&doc);
        assert_eq!(values.environment.len(), 2);
        apply_section(
            &mut doc,
            "environment",
            &serde_json::json!({"vars": [{"name": "KEEP", "value": "2"}, {"name": "NEW", "value": "x"}]}),
        )
        .unwrap();
        let output = doc.to_string();
        assert!(!output.contains("FOO"));
        assert!(output.contains("KEEP \"2\""));
        assert!(output.contains("NEW x"));
    }

    #[test]
    fn parses_and_updates_spawn() {
        let mut doc: KdlDocument = "spawn-at-startup \"foo\" \"bar\"\n".parse().unwrap();
        let values = parse_values(&doc);
        assert_eq!(values.spawn_at_startup.len(), 1);
        assert_eq!(values.spawn_at_startup[0].command, vec!["foo", "bar"]);
        apply_section(
            &mut doc,
            "spawn",
            &serde_json::json!({"commands": [["sh", "-c", "echo hi"]]}),
        )
        .unwrap();
        let output = doc.to_string();
        assert!(output.contains("spawn-at-startup"));
        assert!(output.contains("echo hi"));
        assert!(!output.contains("\"foo\""));
    }

    #[test]
    fn parses_pointer_settings() {
        let mut doc: KdlDocument = r#"input {
    keyboard { track-layout "window"; numlock; }
    touchpad { accel-speed 0.25; accel-profile "flat"; }
    mouse { accel-speed -0.5; accel-profile "adaptive"; natural-scroll; }
}
"#
        .parse()
        .unwrap();
        let values = parse_values(&doc);
        assert_eq!(values.keyboard_track_layout.as_deref(), Some("window"));
        assert_eq!(values.keyboard_numlock, Some(true));
        assert_eq!(values.touchpad_accel_speed, Some(25));
        assert_eq!(values.touchpad_accel_profile.as_deref(), Some("flat"));
        assert_eq!(values.mouse_accel_speed, Some(-50));
        assert_eq!(values.mouse_natural_scroll, Some(true));
        apply_section(
            &mut doc,
            "input.mouse",
            &serde_json::json!({"accel_speed": 10, "natural_scroll": false}),
        )
        .unwrap();
        let output = doc.to_string();
        assert!(output.contains("accel-speed 0.1"));
    }

    #[test]
    fn parses_and_updates_keybinds_and_gestures() {
        let mut doc: KdlDocument = r#"binds {
    Mod+Return { spawn "foot"; }
    Mod+Q { close-window; }
}
gestures {
    hot-corners { top-right; bottom-right; }
    dnd-edge-view-scroll { trigger-width 30; delay-ms 100; max-speed 1500; }
}
"#
        .parse()
        .unwrap();
        let values = parse_values(&doc);
        assert_eq!(values.keybinds.len(), 2);
        assert_eq!(values.keybinds[0].key, "Mod+Return");
        assert_eq!(values.keybinds[0].actions[0].name, "spawn");
        assert_eq!(values.keybinds[0].actions[0].args, vec!["foot"]);
        assert_eq!(values.gestures.hot_corners.len(), 2);
        assert_eq!(values.gestures.dnd_edge_view_scroll.trigger_width, Some(30));
        apply_section(
            &mut doc,
            "keybinds",
            &serde_json::json!({"binds": [{"key": "Mod+T", "actions": [{"name": "spawn", "args": ["foot"]}]}]}),
        )
        .unwrap();
        apply_section(
            &mut doc,
            "gestures",
            &serde_json::json!({"hot_corners": ["top-left"], "hot_corners_off": false}),
        )
        .unwrap();
        let output = doc.to_string();
        assert!(output.contains("Mod+T"));
        assert!(output.contains("top-left"));
        assert!(!output.contains("bottom-right"));
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
