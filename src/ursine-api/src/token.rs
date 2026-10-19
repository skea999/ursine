//! Opaque RAM-only session token.
//!
//! Tokens live in daemon memory and frontend process memory only —
//! never on disk (see `docs/architecture/overview.md`, auth model).

use serde::{Deserialize, Serialize};

/// Bearer token issued after a successful handshake.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionToken(String);

impl SessionToken {
    /// Wrap a token string (duplicated only where unavoidable).
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SessionToken {
    /// Never print the secret value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionToken(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_leaks_the_value() {
        let t = SessionToken::new("super-secret-value");
        assert_eq!(format!("{t:?}"), "SessionToken(<redacted>)");
    }

    #[test]
    fn wire_is_a_bare_string_and_roundtrips() {
        let t = SessionToken::new("abc");
        let s = serde_json::to_string(&t).unwrap();
        assert_eq!(s, "\"abc\"");
        assert_eq!(serde_json::from_str::<SessionToken>(&s).unwrap(), t);
    }
}
