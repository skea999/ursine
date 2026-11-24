//! Wire error type shared by daemon and frontends.

use serde::{Deserialize, Serialize};

/// Stable error codes frontends can switch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Module action exists in the protocol but is not implemented yet.
    NotImplemented,
    /// Missing or invalid session token.
    Unauthorized,
    /// Authenticated but not allowed (e.g. not in wheel).
    Forbidden,
    /// Too many failed login attempts.
    RateLimited,
    /// Capability missing on this machine (config `auto`, backend absent).
    Unavailable,
    /// Malformed request or invalid input.
    Invalid,
    /// Target not found (user, package, snapshot).
    NotFound,
    /// Conflicting state (already exists, busy).
    Conflict,
    /// Unexpected internal failure.
    Internal,
}

impl ErrorCode {
    /// The stable wire name, byte for byte the string serde emits.
    ///
    /// Frontends that print the code (the CLI's `error[not_found]: …` form)
    /// use this instead of `Debug`, which spells `RateLimited` the variant way.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotImplemented => "not_implemented",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::RateLimited => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::Invalid => "invalid",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::Internal => "internal",
        }
    }
}

/// Error payload carried by `Response::Err`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    /// Actionable hint for the UI (e.g. "apk add btrbk").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            fix: None,
        }
    }

    #[must_use]
    pub fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }

    pub fn not_implemented(what: &str) -> Self {
        Self::new(
            ErrorCode::NotImplemented,
            format!("{what} is not implemented yet"),
        )
    }

    pub fn unauthorized() -> Self {
        Self::new(ErrorCode::Unauthorized, "authentication required")
    }

    pub fn forbidden() -> Self {
        Self::new(ErrorCode::Forbidden, "not allowed for this principal")
    }

    pub fn rate_limited() -> Self {
        Self::new(ErrorCode::RateLimited, "too many attempts; try again later")
    }

    pub fn unavailable(reason: impl Into<String>, fix: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unavailable, reason).with_fix(fix)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Invalid, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_snake_case_on_the_wire() {
        let json = serde_json::to_string(&ErrorCode::NotImplemented).unwrap();
        assert_eq!(json, "\"not_implemented\"");
        let json = serde_json::to_string(&ErrorCode::RateLimited).unwrap();
        assert_eq!(json, "\"rate_limited\"");
    }

    #[test]
    fn as_str_matches_the_serialized_name() {
        // The CLI prints `as_str`; the daemon sends serde. They must not drift.
        for code in [
            ErrorCode::NotImplemented,
            ErrorCode::Unauthorized,
            ErrorCode::Forbidden,
            ErrorCode::RateLimited,
            ErrorCode::Unavailable,
            ErrorCode::Invalid,
            ErrorCode::NotFound,
            ErrorCode::Conflict,
            ErrorCode::Internal,
        ] {
            let wire = serde_json::to_value(code).unwrap();
            assert_eq!(wire, code.as_str(), "{code:?}");
        }
    }

    #[test]
    fn fix_field_is_omitted_when_none() {
        let err = ApiError::unauthorized();
        let json = serde_json::to_value(&err).unwrap();
        assert!(!json.as_object().unwrap().contains_key("fix"));
    }

    #[test]
    fn roundtrip() {
        let err = ApiError::unavailable("btrbk not installed", "apk add btrbk");
        let s = serde_json::to_string(&err).unwrap();
        let back: ApiError = serde_json::from_str(&s).unwrap();
        assert_eq!(err, back);
    }
}
