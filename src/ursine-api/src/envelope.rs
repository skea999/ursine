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
pub const PROTOCOL_VERSION: u16 = 1;

const fn wire_version() -> u16 {
    PROTOCOL_VERSION
}

/// Client -> daemon message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequestBody {
    /// One-time handshake: credentials verified by the daemon via PAM.
    Auth { user: String, password: String },
    /// Authenticated call: requires `Authorization: Bearer <token>`.
    Call {
        module: ModuleId,
        action: Action,
        /// Target user for per-user operations; defaults to the principal.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        as_user: Option<String>,
    },
}

/// Daemon -> client message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    #[serde(default = "wire_version")]
    pub v: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(flatten)]
    pub body: ResponseBody,
}

/// Tagged body of a response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseBody {
    /// Handshake succeeded.
    AuthOk {
        token: SessionToken,
        expires_in_secs: u64,
    },
    /// Action succeeded.
    Ok { result: serde_json::Value },
    /// Anything failed.
    Err { error: ApiError },
}

impl Response {
    pub fn auth_ok(id: Option<u64>, token: SessionToken, expires_in_secs: u64) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id,
            body: ResponseBody::AuthOk {
                token,
                expires_in_secs,
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
            },
        };
        let line = serde_json::to_string(&r).unwrap();
        assert!(line.contains("\"type\":\"auth\""));
        let back: Request = serde_json::from_str(&line).unwrap();
        assert_eq!(r, back);
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

        let auth = Response::auth_ok(Some(1), SessionToken::new("tok"), 600);
        let s = serde_json::to_string(&auth).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&s).unwrap(), auth);
    }
}
