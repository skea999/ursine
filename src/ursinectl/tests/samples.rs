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

#[test]
fn every_sample_parses_and_classifies_destructive() {
    for s in common::SAMPLES {
        let pairs: Vec<(String, String)> = s
            .pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let action = ursinectl::cli::build_action_with(s.action, &pairs, || Ok("x".to_string()))
            .unwrap_or_else(|e| panic!("{}: {e}", s.action));
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
