//! Core error type (daemon-internal; mapped to `ApiError` at the dispatch edge).

/// Internal failures that never cross the wire as-is.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("config: {0}")]
    Config(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("auth: {0}")]
    Auth(String),
    #[error("socket: {0}")]
    Socket(String),
}

impl CoreError {
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth(msg.into())
    }

    pub fn socket(msg: impl Into<String>) -> Self {
        Self::Socket(msg.into())
    }
}
