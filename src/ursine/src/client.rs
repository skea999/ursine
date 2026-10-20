//! Blocking Unix-socket client for `ursined`.
//!
//! Transport only: build one request line, read one response line, hand the
//! payload back. All policy (auth, capability gates, asUser scope) lives in
//! the daemon; nothing here decides what is allowed.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use ursine_api::{
    Action, ApiError, InstalledPackage, ModuleId, Request, RequestBody, Response, ResponseBody,
    SessionToken, PROTOCOL_VERSION,
};

/// A stuck daemon must never freeze the UI.
const TIMEOUT: Duration = Duration::from_secs(5);
/// `nix profile` actions can take up to 60 s in the daemon; leave margin.
const PROFILE_TIMEOUT: Duration = Duration::from_secs(75);

/// Socket path: `URSINE_SOCKET` override, else the system default.
pub fn socket_path() -> PathBuf {
    std::env::var_os("URSINE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/run/ursine.sock"))
}

/// A live session: the token and its lifetime, both issued by the daemon.
#[derive(Debug, Clone)]
pub struct AuthSession {
    pub token: SessionToken,
    pub expires_in_secs: u64,
}

/// Read-only view of the target user's nix profile.
#[derive(Debug, Clone, Default)]
pub struct StoreSnapshot {
    pub packages: Vec<InstalledPackage>,
    pub generations: Vec<String>,
}

/// Handshake: verify credentials and obtain a RAM-only session token.
pub async fn auth(user: String, password: String) -> Result<AuthSession, ApiError> {
    off_thread(move || {
        let request = Request {
            v: PROTOCOL_VERSION,
            id: Some(1),
            token: None,
            body: RequestBody::Auth { user, password },
        };
        match roundtrip(&request, TIMEOUT)? {
            ResponseBody::AuthOk {
                token,
                expires_in_secs,
            } => Ok(AuthSession {
                token,
                expires_in_secs,
            }),
            ResponseBody::Err { error } => Err(error),
            ResponseBody::Ok { .. } => Err(ApiError::internal(
                "daemon answered a handshake with a result",
            )),
        }
    })
    .await
}

/// Authenticated call. The daemon re-checks token, capability and scope.
pub async fn call(
    token: SessionToken,
    module: ModuleId,
    action: Action,
    as_user: Option<String>,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(token, module, action, as_user, TIMEOUT).await
}

/// Authenticated call with an explicit socket timeout.
async fn call_with_timeout(
    token: SessionToken,
    module: ModuleId,
    action: Action,
    as_user: Option<String>,
    timeout: Duration,
) -> Result<serde_json::Value, ApiError> {
    off_thread(move || {
        let request = Request {
            v: PROTOCOL_VERSION,
            id: Some(1),
            token: Some(token),
            body: RequestBody::Call {
                module,
                action,
                as_user,
            },
        };
        match roundtrip(&request, timeout)? {
            ResponseBody::Ok { result } => Ok(result),
            ResponseBody::Err { error } => Err(error),
            ResponseBody::AuthOk { .. } => {
                Err(ApiError::internal("daemon answered a call with a session"))
            }
        }
    })
    .await
}

/// Installed packages and generation history for the target user's profile.
pub async fn store_snapshot(token: SessionToken) -> Result<StoreSnapshot, ApiError> {
    let packages = installed(token.clone()).await?;
    let generations = generations(token).await?;
    Ok(StoreSnapshot {
        packages,
        generations,
    })
}

async fn installed(token: SessionToken) -> Result<Vec<InstalledPackage>, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::PackagesNix,
        Action::ListInstalled,
        None,
        PROFILE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|error| ApiError::internal(format!("cannot read installed packages: {error}")))
}

async fn generations(token: SessionToken) -> Result<Vec<String>, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::PackagesNix,
        Action::Generations,
        None,
        PROFILE_TIMEOUT,
    )
    .await?;
    parse_generations(value)
}

fn parse_generations(value: serde_json::Value) -> Result<Vec<String>, ApiError> {
    #[derive(serde::Deserialize)]
    struct Lines {
        lines: Vec<String>,
    }
    let parsed: Lines = serde_json::from_value(value)
        .map_err(|error| ApiError::internal(format!("cannot read profile generations: {error}")))?;
    Ok(parsed.lines)
}

/// Run a blocking round-trip off the UI thread.
async fn off_thread<T, F>(work: F) -> Result<T, ApiError>
where
    F: FnOnce() -> Result<T, ApiError> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result,
        Err(error) => Err(ApiError::internal(format!("client task failed: {error}"))),
    }
}

fn roundtrip(request: &Request, timeout: Duration) -> Result<ResponseBody, ApiError> {
    let path = socket_path();
    let line = serde_json::to_string(request)
        .map_err(|error| ApiError::internal(format!("cannot encode request: {error}")))?;

    let mut stream = UnixStream::connect(&path).map_err(|error| {
        ApiError::internal(format!(
            "ursined not reachable at {}: {error}",
            path.display()
        ))
    })?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| ApiError::internal(error.to_string()))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| ApiError::internal(error.to_string()))?;

    stream
        .write_all(format!("{line}\n").as_bytes())
        .map_err(|error| ApiError::internal(format!("cannot send request: {error}")))?;

    let mut reply = String::new();
    BufReader::new(stream)
        .read_line(&mut reply)
        .map_err(|error| ApiError::internal(format!("cannot read reply: {error}")))?;

    let response: Response = serde_json::from_str(reply.trim()).map_err(|error| {
        ApiError::internal(format!("ursined sent an unreadable reply: {error}"))
    })?;
    Ok(response.body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_is_absolute() {
        assert!(socket_path().is_absolute());
    }

    #[test]
    fn auth_request_roundtrips_on_the_wire() {
        let request = Request {
            v: PROTOCOL_VERSION,
            id: Some(1),
            token: None,
            body: RequestBody::Auth {
                user: "skea999".into(),
                password: "pw1".into(),
            },
        };
        let line = serde_json::to_string(&request).unwrap();
        let back: Request = serde_json::from_str(&line).unwrap();
        assert_eq!(request, back);
    }

    #[test]
    fn generation_lines_are_parsed() {
        let payload = serde_json::json!({
            "lines": ["Version 1", "Version 2 (current)"],
        });
        assert_eq!(
            parse_generations(payload).unwrap(),
            vec!["Version 1", "Version 2 (current)"]
        );
        assert!(parse_generations(serde_json::json!({"other": []})).is_err());
    }
}
