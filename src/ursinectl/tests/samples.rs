//! One test per functionality layer over the shared sample table:
//! serde-tag reachability, --help coverage, sample coverage (exactly one
//! sample per wire action) and parse/destructive classification.

#![allow(clippy::unwrap_used)] // tests
#![allow(clippy::panic)] // test failure reporting

pub mod common;

#[test]
fn every_wire_action_is_a_valid_serde_tag() {
    // The CLI parses actions through the wire serde tag: every name in
    // the registry must resolve to a known variant (missing fields are
    // fine — an unknown variant is not).
    for name in common::ALL_ACTIONS {
        match serde_json::from_value::<ursine_api::Action>(serde_json::json!({
            "action": name
        })) {
            // All-optional payload: parses from the tag alone.
            Ok(_) => {}
            // Required fields: the ONLY acceptable failure is a missing
            // field — an unknown variant would mean the CLI cannot
            // reach that action at all.
            Err(err) => {
                let msg = err.to_string();
                assert!(
                    !msg.contains("unknown variant"),
                    "action tag {name:?} is not reachable from the CLI: {msg}"
                );
                assert!(
                    msg.contains("missing field"),
                    "action tag {name:?} failed for an unexpected reason: {msg}"
                );
            }
        }
    }
    let err = serde_json::from_value::<ursine_api::Action>(serde_json::json!({
        "action": "frobnicate"
    }))
    .unwrap_err();
    assert!(err.to_string().contains("unknown variant"));
}

#[test]
fn usage_documents_every_wire_action() {
    let tokens: Vec<&str> = ursinectl::cli::USAGE
        .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
        .filter(|t| !t.is_empty())
        .collect();
    for name in common::ALL_ACTIONS {
        assert!(
            tokens.contains(name),
            "action {name:?} missing from ursinectl --help"
        );
    }
}

#[test]
fn every_wire_action_has_exactly_one_sample() {
    let mut sample_names: Vec<&str> = common::SAMPLES.iter().map(|s| s.action).collect();
    sample_names.sort_unstable();
    let mut all: Vec<&str> = common::ALL_ACTIONS.to_vec();
    all.sort_unstable();
    assert_eq!(
        sample_names, all,
        "SAMPLES must cover every wire action exactly once"
    );
    // No duplicate module/action pair.
    let mut pairs: Vec<(&str, &str)> = common::SAMPLES
        .iter()
        .map(|s| (s.module.as_str(), s.action))
        .collect();
    pairs.sort_unstable();
    let before = pairs.len();
    pairs.dedup();
    assert_eq!(pairs.len(), before, "duplicate (module, action) sample");
}

/// Build the action one sample row describes: every test that walks the
/// catalogue needs the same parse, so the six lines live here once.
fn action_of(s: &common::Sample) -> ursine_api::Action {
    let pairs: Vec<(String, String)> = s
        .pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    ursinectl::cli::build_action_with(s.action, &pairs, || Ok("x".to_string()))
        .unwrap_or_else(|e| panic!("{}: {e}", s.action))
}

#[test]
fn every_sample_parses_and_classifies_destructive() {
    for s in common::SAMPLES {
        let action = action_of(s);
        // The tag the CLI parsed must be the tag core audits.
        assert_eq!(
            ursine_core::modules::action_name(&action),
            s.action,
            "{}",
            s.action
        );
        assert_eq!(
            ursinectl::cli::is_destructive(&action),
            s.destructive,
            "destructive expectation for {}",
            s.action
        );
    }
}

#[test]
fn catalogue_covers_every_wire_action() {
    // The printed catalogue is a client contract: a wire action that has no
    // row would be invisible to every frontend that is not the GUI.
    let mut catalogue: Vec<&str> = ursine_api::catalogue::ACTIONS
        .iter()
        .map(|spec| spec.name)
        .collect();
    catalogue.sort_unstable();
    let mut registry: Vec<&str> = common::ALL_ACTIONS.to_vec();
    registry.sort_unstable();
    assert_eq!(
        catalogue, registry,
        "ursine-api catalogue and the wire action registry disagree"
    );
}

#[test]
fn every_catalogue_row_is_reachable_from_its_action() {
    for s in common::SAMPLES {
        let action = action_of(s);
        let spec = action.spec();
        assert_eq!(spec.name, s.action, "wire name of {}", s.action);
        assert_eq!(spec.module, s.module, "module of {}", s.action);
        // The row this action resolves to is the row the catalogue prints:
        // a variant that is not in `ACTIONS` never reaches a client.
        let printed = ursine_api::catalogue::ACTIONS
            .iter()
            .find(|row| row.name == s.action)
            .unwrap_or_else(|| panic!("{} is not in the printed catalogue", s.action));
        assert_eq!(*printed, *spec, "printed row of {}", s.action);
        // `required` is derived from the serde attributes: the sample proves
        // every parameter the wire demands is really in the payload.
        for param in spec.params {
            if param.required {
                assert!(
                    s.pairs.iter().any(|(key, _)| *key == param.name),
                    "{}: required parameter {:?} missing from the sample",
                    s.action,
                    param.name
                );
            }
        }
        // A read-only request never needs the destructive confirmation.
        if action.is_read_only() {
            assert!(
                !action.is_destructive(),
                "{} reads only and must not be gated",
                s.action
            );
        }
    }
}

#[test]
fn catalogue_document_is_the_documented_shape() {
    let doc = ursine_api::catalogue::json_document();
    assert_eq!(doc["schema"], "ursine.actions/1");
    assert_eq!(doc["count"], ursine_api::catalogue::ACTIONS.len());
    let actions = doc["actions"].as_array().unwrap();
    assert_eq!(actions.len(), ursine_api::catalogue::ACTIONS.len());
    // Every row carries the six documented keys and nothing else.
    for row in actions {
        let mut keys: Vec<&str> = row
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "destructive",
                "effect",
                "module",
                "name",
                "params",
                "previewable"
            ],
            "row {row}"
        );
        assert!(matches!(
            row["effect"].as_str(),
            Some("read_only" | "mutating")
        ));
        assert!(matches!(
            row["destructive"].as_str(),
            Some("never" | "always" | "request")
        ));
    }
}
