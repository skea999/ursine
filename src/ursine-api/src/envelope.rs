//! Wire envelope: newline-delimited JSON over the unix socket.
//!
//! One request or response per line. `v` carries the protocol version;
//! `id` echoes back for request/response pairing (frontends may pipeline).

use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::error::ApiError;
use crate::module::ModuleId;
use crate::token::SessionToken;

/// Current protocol version. Bump on breaking wire changes.
///
/// The version is not advisory: `Daemon::handle` compares `Request::v` with
/// this constant and refuses any other value with an `invalid` error naming
/// both numbers, before authentication or dispatch. That check is the only
/// enforcement of the rule; `docs/specs/SPEC-protocol.md` states it for
/// client authors, and a test keeps the documented number in step with this
/// constant.
///
/// v3: package mutations return `job_id` (poll `job_output`); no more inline
/// mutation output.
/// v4: `list_apk_repositories` rows are `{url, tag}` objects (was a list
/// of raw strings).
/// v5: no D-Bus — `set_bluetooth_pairing_pin` removed (pairing is
/// bluetoothctl's own agent), `BluetoothView.pairing_pin_set` removed,
/// capability `accountsservice` renamed to `shadow` (user CLI probe).
/// v6: Nix repository scopes mirror the `FeralOS` model — `system: bool`
/// selects the installation on `list_nix_repositories`, `add_nix_repository`
/// and `remove_nix_repository`, and `remove` names the row's source
/// (`registry` | `channel`) instead of the old `scope` string.
/// v7: `refresh_apk` removed — the update check refreshes the indexes first,
/// and it was the only package action outside the job path.
pub const PROTOCOL_VERSION: u16 = 7;

const fn wire_version() -> u16 {
    PROTOCOL_VERSION
}

/// Client -> daemon message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    #[serde(default = "wire_version")]
    pub v: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// Session token for `call` requests (absent in `auth`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<SessionToken>,
    #[serde(flatten)]
    pub body: RequestBody,
}

/// Tagged body of a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequestBody {
    /// One-time handshake: credentials checked against the shadow file.
    /// It is the only unauthenticated request.
    Auth {
        user: String,
        password: String,
        /// Ask for a read-only credential: mutating actions on this session
        /// are refused. Absent on the wire means read-write, so a client that
        /// predates the field keeps exactly the rights it had.
        #[serde(default)]
        read_only: bool,
        /// Frontend that opened the session (`ursinectl`, `ursine-gui`).
        /// Recorded in the audit log as `on_behalf_of`; the daemon does not
        /// authorize on it, so omitting it is legal.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client: Option<String>,
    },
    /// Authenticated call: the session token travels in `token` on the
    /// request line, not in a header.
    Call {
        module: ModuleId,
        action: Action,
        /// Target user for per-user operations; defaults to the principal.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        as_user: Option<String>,
    },
}

/// Daemon -> client message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    #[serde(default = "wire_version")]
    pub v: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(flatten)]
    pub body: ResponseBody,
}

/// Tagged body of a response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseBody {
    /// Handshake succeeded.
    AuthOk {
        token: SessionToken,
        expires_in_secs: u64,
        /// The scope the daemon granted: the token carries it, so the client
        /// can tell read-only from read-write without inspecting requests.
        #[serde(default)]
        read_only: bool,
        /// Echo of the accepted client id, so a client can confirm the name
        /// the audit log will show.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client: Option<String>,
    },
    /// Action succeeded.
    Ok { result: serde_json::Value },
    /// Anything failed.
    Err { error: ApiError },
}

impl Response {
    pub fn auth_ok(
        id: Option<u64>,
        token: SessionToken,
        expires_in_secs: u64,
        read_only: bool,
        client: Option<String>,
    ) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id,
            body: ResponseBody::AuthOk {
                token,
                expires_in_secs,
                read_only,
                client,
            },
        }
    }

    pub fn ok(id: Option<u64>, result: serde_json::Value) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id,
            body: ResponseBody::Ok { result },
        }
    }

    pub fn err(id: Option<u64>, error: ApiError) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id,
            body: ResponseBody::Err { error },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_request_roundtrip() {
        let r = Request {
            v: PROTOCOL_VERSION,
            id: Some(1),
            token: None,
            body: RequestBody::Auth {
                user: "skea999".into(),
                password: "pw1".into(),
                read_only: false,
                client: None,
            },
        };
        let line = serde_json::to_string(&r).unwrap();
        assert!(line.contains("\"type\":\"auth\""));
        let back: Request = serde_json::from_str(&line).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn auth_request_client_and_scope_roundtrip() {
        let r = Request {
            v: PROTOCOL_VERSION,
            id: Some(2),
            token: None,
            body: RequestBody::Auth {
                user: "skea999".into(),
                password: "pw1".into(),
                read_only: true,
                client: Some("ursinectl".into()),
            },
        };
        let line = serde_json::to_string(&r).unwrap();
        assert!(line.contains("\"read_only\":true"));
        assert!(line.contains("\"client\":\"ursinectl\""));
        let back: Request = serde_json::from_str(&line).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn auth_request_without_scope_fields_means_read_write() {
        // A client from before the scoped-credential change sends only the
        // credentials; it must keep the read-write behaviour it had.
        let line = r#"{"type":"auth","user":"skea999","password":"pw1"}"#;
        let r: Request = serde_json::from_str(line).unwrap();
        assert_eq!(
            r.body,
            RequestBody::Auth {
                user: "skea999".into(),
                password: "pw1".into(),
                read_only: false,
                client: None,
            }
        );
    }

    #[test]
    fn auth_ok_without_scope_fields_still_parses() {
        let line = r#"{"type":"auth_ok","token":"abc","expires_in_secs":600}"#;
        let r: Response = serde_json::from_str(line).unwrap();
        let ResponseBody::AuthOk {
            read_only, client, ..
        } = r.body
        else {
            panic!("want auth_ok");
        };
        assert!(!read_only);
        assert!(client.is_none());
    }

    #[test]
    fn call_request_omits_as_user_when_none() {
        let r = Request {
            v: PROTOCOL_VERSION,
            id: None,
            token: None,
            body: RequestBody::Call {
                module: ModuleId::Capabilities,
                action: Action::List,
                as_user: None,
            },
        };
        let line = serde_json::to_string(&r).unwrap();
        assert!(!line.contains("as_user"));
        let back: Request = serde_json::from_str(&line).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn older_envelope_missing_v_still_parses() {
        let line = r#"{"type":"call","module":"system","action":{"action":"status"}}"#;
        let r: Request = serde_json::from_str(line).unwrap();
        assert_eq!(r.v, PROTOCOL_VERSION);
        assert!(r.id.is_none());
    }

    #[test]
    fn responses_roundtrip() {
        let ok = Response::ok(Some(7), serde_json::json!({"answer": 42}));
        let s = serde_json::to_string(&ok).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&s).unwrap(), ok);

        let err = Response::err(None, ApiError::not_implemented("snapshots.list"));
        let s = serde_json::to_string(&err).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&s).unwrap(), err);

        let auth = Response::auth_ok(Some(1), SessionToken::new("tok"), 600, true, None);
        let s = serde_json::to_string(&auth).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&s).unwrap(), auth);
        assert!(s.contains("\"read_only\":true"));
        assert!(!s.contains("client"), "absent client is not serialized");

        let auth = Response::auth_ok(
            Some(2),
            SessionToken::new("tok2"),
            600,
            false,
            Some("ursine-gui".into()),
        );
        let s = serde_json::to_string(&auth).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&s).unwrap(), auth);
        assert!(s.contains("\"client\":\"ursine-gui\""));
    }

    #[test]
    fn spec_protocol_states_the_current_wire_version() {
        // The specification is the only protocol reference a client author
        // has, so it must name the version this crate sends. Reading it at
        // compile time keeps the check free of IO and makes a stale sentence
        // a test failure instead of a support question.
        const SPEC: &str = include_str!("../../../docs/specs/SPEC-protocol.md");
        let stated = format!("(currently {PROTOCOL_VERSION};");
        let mut sentences = 0;
        for line in SPEC.lines().filter(|line| line.contains("(currently ")) {
            sentences += 1;
            assert!(
                line.contains(&stated),
                "docs/specs/SPEC-protocol.md must state `v: u16` {stated}, \
                found: {line}"
            );
        }
        assert_eq!(
            sentences, 1,
            "the specification carries exactly one `v: u16` version sentence"
        );
    }
}
