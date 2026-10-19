//! Integration surface of `ursine-api` — the exact wire shapes frontends see.
//!
//! Lives under the repo-root `tests/` tree (layout rule), wired via `[[test]]`.

use ursine_api::{
    Action, ApiError, Capability, CapabilityReport, ModuleId, Request, RequestBody, Response,
    ResponseBody, SessionToken, TriState, UrsineConfig, PROTOCOL_VERSION,
};

#[test]
fn capability_list_call_roundtrips_as_one_line() {
    let req = Request {
        v: PROTOCOL_VERSION,
        id: Some(42),
        token: None,
        body: RequestBody::Call {
            module: ModuleId::Capabilities,
            action: Action::List,
            as_user: None,
        },
    };
    let line = serde_json::to_string(&req).unwrap();
    assert!(!line.contains('\n'), "protocol frames must be single lines");
    let parsed: Request = serde_json::from_str(&line).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn auth_response_contract() {
    let resp = Response::auth_ok(Some(1), SessionToken::new("tok"), 600);
    let line = serde_json::to_string(&resp).unwrap();
    let parsed: Response = serde_json::from_str(&line).unwrap();
    match parsed.body {
        ResponseBody::AuthOk {
            expires_in_secs,
            token,
        } => {
            assert_eq!(expires_in_secs, 600);
            assert_eq!(token.as_str(), "tok");
        }
        other => panic!("unexpected body: {other:?}"),
    }
}

#[test]
fn error_envelope_is_actionable() {
    let err = Response::err(
        None,
        ApiError::unavailable("btrbk not installed", "apk add btrbk"),
    );
    let v = serde_json::to_value(&err).unwrap();
    assert_eq!(v["type"], "err");
    assert_eq!(v["error"]["code"], "unavailable");
    assert_eq!(v["error"]["fix"], "apk add btrbk");
}

#[test]
fn tri_state_full_matrix() {
    for (text, expected) in [
        ("\"auto\"", TriState::Auto),
        ("true", TriState::On),
        ("false", TriState::Off),
    ] {
        assert_eq!(serde_json::from_str::<TriState>(text).unwrap(), expected);
        assert_eq!(serde_json::to_string(&expected).unwrap(), text);
    }
}

#[test]
fn config_parses_like_the_spec_template() {
    let toml_src = r#"
token_ttl_min = 10
socket_path = "/run/ursine.sock"

[modules]
packages_nix = "auto"
snapshots = "auto"
users = false
niri = true
"#;
    let cfg: UrsineConfig = toml::from_str(toml_src).unwrap();
    assert_eq!(cfg.token_ttl_min, 10);
    assert!(cfg.modules.packages_nix.is_auto());
    assert_eq!(cfg.modules.users, TriState::Off);
    assert_eq!(cfg.modules.niri, TriState::On);
}

#[test]
fn capability_report_gates_panels() {
    let report =
        CapabilityReport::unavailable(Capability::Btrbk, "btrbk not installed", "apk add btrbk");
    let v = serde_json::to_value(&report).unwrap();
    assert_eq!(v["available"], false);
    assert_eq!(v["fix"], "apk add btrbk");
    assert_eq!(v["capability"], "btrbk");
}
