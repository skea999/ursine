//! Blocking Unix-socket client for `ursined`.
//!
//! Transport only: build one request line, read one response line, hand the
//! payload back. All policy (auth, capability gates, asUser scope) lives in
//! the daemon; nothing here decides what is allowed.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use ursine_api::{
    installable_from_attr, timeouts, Action, ApiError, ApkPackage, ApkUpdate, BluetoothOp,
    BluetoothView, BtrbkConfigView, BtrbkSettings, Disk, FirewallStatus, FlatpakPackage,
    FlatpakRemote, InstalledPackage, ModuleId, ModulesConfig, NetworkView, NiriConfigView,
    PrintJob, Printer, Request, RequestBody, Response, ResponseBody, ScannerConfigView,
    SearchResult, Service, ServiceOp, SessionToken, Snapshot, SnapshotStats, TriState, UserSummary,
    PROTOCOL_VERSION,
};

/// A stuck daemon must never freeze the UI.
const TIMEOUT: Duration = Duration::from_secs(5);
/// Daemon work budget plus its socket write timeout plus margin.
const REPLY_MARGIN_SECS: u64 = 10;
const fn daemon_budget(work_secs: u64) -> Duration {
    Duration::from_secs(work_secs + timeouts::WRITE_SECS + REPLY_MARGIN_SECS)
}
/// PAM is bounded at 30 s in the daemon; leave the same reply margin.
const AUTH_TIMEOUT: Duration = daemon_budget(30);
/// `nix profile` reads.
pub const PROFILE_TIMEOUT: Duration = daemon_budget(timeouts::PROFILE_SECS);
/// `nix search`.
pub const SEARCH_TIMEOUT: Duration = daemon_budget(timeouts::SEARCH_SECS);
/// Profile mutations.
pub const MUTATE_TIMEOUT: Duration = daemon_budget(timeouts::MUTATE_SECS);
/// Prune reads history then deletes under one request.
pub const PRUNE_TIMEOUT: Duration = daemon_budget(timeouts::HISTORY_SECS + timeouts::MUTATE_SECS);
/// Store-wide GC.
pub const GC_TIMEOUT: Duration = daemon_budget(timeouts::GC_SECS);

/// Hard upper bound for one daemon reply line (8 MiB).
const MAX_REPLY_BYTES: usize = 8 * 1024 * 1024;

/// Socket path: `URSINE_SOCKET` override, else the system default.
pub fn socket_path() -> PathBuf {
    std::env::var_os("URSINE_SOCKET")
        .map_or_else(|| PathBuf::from("/run/ursine.sock"), PathBuf::from)
}

/// A live session: the token and its lifetime, both issued by the daemon.
#[derive(Debug, Clone)]
pub struct AuthSession {
    pub token: SessionToken,
    pub expires_in_secs: u64,
}

/// Current configurable daemon state plus the path it was loaded from.
#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    pub modules: ModulesConfig,
    pub path: String,
    /// Whether the configuration file exists on disk right now. `None` when an
    /// older daemon omits the flag (ambiguous; do not offer Create).
    pub exists: Option<bool>,
}

/// `system.status` payload: daemon build and protocol facts.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SystemStatus {
    pub version: String,
    pub protocol_version: u16,
    pub auth: String,
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
        match roundtrip(&request, AUTH_TIMEOUT)? {
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
pub async fn call_with_timeout(
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

/// Daemon build, protocol and auth mode.
pub async fn system_status(token: SessionToken) -> Result<SystemStatus, ApiError> {
    let value = call(token, ModuleId::System, Action::Status, None).await?;
    serde_json::from_value(value)
        .map_err(|error| ApiError::internal(format!("cannot read daemon status: {error}")))
}

/// Read the daemon's configurable module switches.
pub async fn modules_config(token: SessionToken) -> Result<ConfigSnapshot, ApiError> {
    let value = call(token, ModuleId::Config, Action::GetConfig, None).await?;
    parse_config_snapshot(value)
}

/// Write the commented default config when the daemon has none yet.
pub async fn init_config(token: SessionToken) -> Result<ConfigSnapshot, ApiError> {
    let value = call(token, ModuleId::Config, Action::Init, None).await?;
    parse_config_snapshot(value)
}

/// Persist one module switch; the daemon applies it to its live configuration.
pub async fn set_module_switch(
    token: SessionToken,
    module: ModuleId,
    value: TriState,
) -> Result<ConfigSnapshot, ApiError> {
    let value = call(
        token,
        ModuleId::Config,
        Action::SetModuleSwitch { module, value },
        None,
    )
    .await?;
    parse_config_snapshot(value)
}

fn parse_config_snapshot(value: serde_json::Value) -> Result<ConfigSnapshot, ApiError> {
    #[derive(serde::Deserialize)]
    struct Payload {
        modules: ModulesConfig,
        // v0.3.1 omitted `exists`; `written` is only present when Init wrote.
        #[serde(default)]
        exists: Option<bool>,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        written: Option<String>,
    }
    let parsed: Payload = serde_json::from_value(value).map_err(|error| {
        ApiError::internal(format!("cannot read daemon configuration: {error}"))
    })?;
    let exists = parsed
        .exists
        .or_else(|| parsed.written.as_ref().map(|_| true));
    let path = parsed
        .path
        .or(parsed.written)
        .ok_or_else(|| ApiError::internal("daemon configuration response has no path"))?;
    Ok(ConfigSnapshot {
        modules: parsed.modules,
        path,
        exists,
    })
}

/// Snapshots, stats and btrbk config in one refresh.
#[derive(Debug, Clone, Default)]
pub struct SnapshotPage {
    pub snapshots: Vec<Snapshot>,
    pub stats: Vec<SnapshotStats>,
    pub config: BtrbkConfigView,
}

pub async fn snapshot_page(token: SessionToken) -> Result<SnapshotPage, ApiError> {
    let snapshots = call(
        token.clone(),
        ModuleId::Snapshots,
        Action::ListSnapshots,
        None,
    )
    .await?;
    let stats = call(token.clone(), ModuleId::Snapshots, Action::Stats, None).await?;
    let config = call(token, ModuleId::Snapshots, Action::GetBtrbkConfig, None).await?;
    let snapshots = snapshots
        .get("snapshots")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon snapshots response has no list"))?;
    let stats = stats
        .get("stats")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon snapshots response has no stats"))?;
    Ok(SnapshotPage {
        snapshots: serde_json::from_value(snapshots)
            .map_err(|e| ApiError::internal(format!("cannot read snapshots: {e}")))?,
        stats: serde_json::from_value(stats)
            .map_err(|e| ApiError::internal(format!("cannot read snapshot stats: {e}")))?,
        config: serde_json::from_value(config)
            .map_err(|e| ApiError::internal(format!("cannot read btrbk config: {e}")))?,
    })
}

pub async fn create_snapshot(
    token: SessionToken,
    section: Option<String>,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Snapshots,
        Action::CreateSnapshot { section },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn restore_snapshot(
    token: SessionToken,
    snapshot: String,
    target: String,
    dry_run: bool,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Snapshots,
        Action::RestoreSnapshot {
            snapshot,
            target,
            dry_run,
        },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn set_btrbk_config(
    token: SessionToken,
    settings: BtrbkSettings,
) -> Result<BtrbkConfigView, ApiError> {
    let value = call(
        token,
        ModuleId::Snapshots,
        Action::SetBtrbkConfig {
            settings: Box::new(settings),
        },
        None,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read btrbk config: {e}")))
}

pub async fn delete_snapshot(
    token: SessionToken,
    snapshot: String,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Snapshots,
        Action::DeleteSnapshot { snapshot },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn init_btrbk_config(token: SessionToken) -> Result<BtrbkConfigView, ApiError> {
    let value = call(token, ModuleId::Snapshots, Action::InitBtrbkConfig, None).await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read btrbk config: {e}")))
}

/// Bounded search page returned by the daemon.
#[derive(Debug, Clone, Default)]
pub struct SearchPage {
    pub results: Vec<SearchResult>,
    pub truncated: bool,
}

/// New daemons return `{results, truncated}`; v0.3.1 returned a bare array.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum SearchPayload {
    Page {
        results: Vec<SearchResult>,
        #[serde(default)]
        truncated: bool,
    },
    Legacy(Vec<SearchResult>),
}

fn parse_search_payload(value: serde_json::Value) -> Result<SearchPage, ApiError> {
    let page = serde_json::from_value::<SearchPayload>(value)
        .map_err(|error| ApiError::internal(format!("cannot read search results: {error}")))?;
    Ok(match page {
        SearchPayload::Page { results, truncated } => SearchPage { results, truncated },
        SearchPayload::Legacy(mut results) => {
            // Older daemons may omit `installable`; derive it from the attr so
            // install-from-search never sends the raw attr layout.
            for result in &mut results {
                if result.installable.is_none() {
                    result.installable = Some(installable_from_attr(&result.attr));
                }
            }
            SearchPage {
                results,
                truncated: false,
            }
        }
    })
}

/// Search nixpkgs. Slow by design; the daemon allows 300 s.
pub async fn search(
    token: SessionToken,
    query: String,
    as_user: Option<String>,
) -> Result<SearchPage, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::PackagesNix,
        Action::Search { query },
        as_user,
        SEARCH_TIMEOUT,
    )
    .await?;
    parse_search_payload(value)
}

/// Run one `packages_nix` action with the timeout that action needs.
pub async fn package_call(
    token: SessionToken,
    action: Action,
    as_user: Option<String>,
    timeout: Duration,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(token, ModuleId::PackagesNix, action, as_user, timeout).await
}

/// Installed packages and generation history for the target user's profile.
pub async fn store_snapshot(
    token: SessionToken,
    as_user: Option<String>,
) -> Result<StoreSnapshot, ApiError> {
    let packages = installed(token.clone(), as_user.clone()).await?;
    let generations = generations(token, as_user).await?;
    Ok(StoreSnapshot {
        packages,
        generations,
    })
}

async fn installed(
    token: SessionToken,
    as_user: Option<String>,
) -> Result<Vec<InstalledPackage>, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::PackagesNix,
        Action::ListInstalled,
        as_user,
        PROFILE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|error| ApiError::internal(format!("cannot read installed packages: {error}")))
}

async fn generations(
    token: SessionToken,
    as_user: Option<String>,
) -> Result<Vec<String>, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::PackagesNix,
        Action::Generations,
        as_user,
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
    // One deadline for the whole round trip: write time counts against it.
    let deadline = Instant::now() + timeout;

    let mut stream = connect_before(&path, deadline, timeout)?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(reply_timeout(timeout));
    }
    stream
        .set_write_timeout(Some(remaining))
        .map_err(|error| ApiError::internal(error.to_string()))?;

    stream
        .write_all(format!("{line}\n").as_bytes())
        .map_err(|error| ApiError::internal(format!("cannot send request: {error}")))?;

    let reply = read_reply_bounded(&mut stream, deadline, timeout, MAX_REPLY_BYTES)?;
    let response: Response = serde_json::from_str(reply.trim()).map_err(|error| {
        ApiError::internal(format!("ursined sent an unreadable reply: {error}"))
    })?;
    Ok(response.body)
}

/// A timed-out connect thread can stay blocked in the kernel. Cap the number
/// of outstanding helpers so repeated retries cannot grow threads without bound.
const MAX_CONNECTING: usize = 4;
static CONNECTING: AtomicUsize = AtomicUsize::new(0);

struct ConnectGuard;

impl ConnectGuard {
    fn acquire() -> Option<Self> {
        let previous = CONNECTING.fetch_add(1, Ordering::AcqRel);
        if previous >= MAX_CONNECTING {
            CONNECTING.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Self)
    }
}

impl Drop for ConnectGuard {
    fn drop(&mut self) {
        CONNECTING.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Connect without letting a saturated socket backlog block past the deadline.
/// The helper thread may outlive a timeout; it drops the stream when it returns.
fn connect_before(
    path: &Path,
    deadline: Instant,
    timeout: Duration,
) -> Result<UnixStream, ApiError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(reply_timeout(timeout));
    }
    let Some(guard) = ConnectGuard::acquire() else {
        return Err(ApiError::internal(
            "daemon busy: too many pending connection attempts",
        ));
    };
    let display = path.display().to_string();
    let path = path.to_path_buf();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _guard = guard;
        let _ = sender.send(UnixStream::connect(&path));
    });
    match receiver.recv_timeout(remaining) {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(error)) => Err(ApiError::internal(format!(
            "ursined not reachable at {display}: {error}"
        ))),
        Err(_) => Err(reply_timeout(timeout)),
    }
}

/// Read one newline-terminated reply with a byte cap and one absolute deadline.
///
/// Each read gets only the time left before `deadline`, so a slow trickle
/// cannot keep the client waiting past `timeout`. Bytes after the first
/// newline belong to no response this client asks for and are ignored.
fn read_reply_bounded(
    stream: &mut UnixStream,
    deadline: Instant,
    timeout: Duration,
    max_bytes: usize,
) -> Result<String, ApiError> {
    let mut reply = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(reply_timeout(timeout));
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|error| ApiError::internal(error.to_string()))?;

        match stream.read(&mut chunk) {
            Ok(0) => {
                if reply.is_empty() {
                    return Err(ApiError::internal(
                        "daemon closed the connection without a reply",
                    ));
                }
                break;
            }
            Ok(n) => {
                let bytes = chunk.get(..n).unwrap_or_default();
                if let Some(newline) = bytes.iter().position(|byte| *byte == b'\n') {
                    if reply.len() + newline > max_bytes {
                        return Err(reply_too_large(max_bytes));
                    }
                    reply.extend_from_slice(bytes.get(..newline).unwrap_or_default());
                    break;
                }
                if reply.len() + n > max_bytes {
                    return Err(reply_too_large(max_bytes));
                }
                reply.extend_from_slice(bytes);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                // An early timeout or signal: retry with the time left.
            }
            Err(error) => {
                return Err(ApiError::internal(format!("cannot read reply: {error}")));
            }
        }
    }

    String::from_utf8(reply)
        .map_err(|error| ApiError::internal(format!("ursined sent invalid UTF-8: {error}")))
}

fn reply_timeout(timeout: Duration) -> ApiError {
    ApiError::internal(format!(
        "daemon did not reply within {} s; the operation may still be running",
        timeout.as_secs()
    ))
}

fn reply_too_large(max_bytes: usize) -> ApiError {
    ApiError::internal(format!("daemon reply exceeds the {max_bytes} byte limit"))
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
    fn config_snapshot_parses_module_switches() {
        let snapshot = parse_config_snapshot(serde_json::json!({
            "modules": {"packages_nix": true, "snapshots": "auto"},
            "path": "/etc/ursine/config.toml",
            "exists": true,
        }))
        .unwrap();
        assert_eq!(snapshot.modules.packages_nix, TriState::On);
        assert_eq!(snapshot.path, "/etc/ursine/config.toml");
        assert_eq!(snapshot.exists, Some(true));
    }

    #[test]
    fn init_response_uses_the_written_path() {
        let snapshot = parse_config_snapshot(serde_json::json!({
            "modules": {"packages_nix": "auto"},
            "written": "/etc/ursine/config.toml",
            "exists": true,
        }))
        .unwrap();
        assert_eq!(snapshot.path, "/etc/ursine/config.toml");
        assert_eq!(snapshot.exists, Some(true));
    }

    #[test]
    fn config_snapshot_defaults_exists_for_old_daemons() {
        // v0.3.1 GetConfig omitted `exists`; a missing file is the safe read.
        let snapshot = parse_config_snapshot(serde_json::json!({
            "modules": {"packages_nix": "auto"},
            "path": "/etc/ursine/config.toml",
        }))
        .unwrap();
        assert_eq!(snapshot.exists, None);

        // Init wrote the file, so `written` implies existence.
        let initialized = parse_config_snapshot(serde_json::json!({
            "modules": {"packages_nix": "auto"},
            "written": "/etc/ursine/config.toml",
        }))
        .unwrap();
        assert_eq!(initialized.exists, Some(true));
    }

    #[test]
    fn search_payload_reads_page_and_legacy_shapes() {
        let page = parse_search_payload(serde_json::json!({
            "results": [{"attr": "legacyPackages.x86_64-linux.hello", "installable": "hello"}],
            "truncated": true,
        }))
        .unwrap();
        assert_eq!(page.results.len(), 1);
        assert!(page.truncated);

        // v0.3.1 daemons returned a bare array and never flagged truncation.
        let legacy = parse_search_payload(serde_json::json!([
            {"attr": "legacyPackages.x86_64-linux.hello"}
        ]))
        .unwrap();
        assert_eq!(legacy.results.len(), 1);
        assert!(!legacy.truncated);
        assert_eq!(
            legacy.results[0].installable.as_deref(),
            Some("hello"),
            "missing installable is derived from the attr"
        );
    }

    #[test]
    fn system_status_parses_the_daemon_payload() {
        let status: SystemStatus = serde_json::from_value(serde_json::json!({
            "version": "0.3.1",
            "protocol_version": 1,
            "auth": "pam",
        }))
        .unwrap();
        assert_eq!(status.version, "0.3.1");
        assert_eq!(status.protocol_version, 1);
        assert_eq!(status.auth, "pam");
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

    #[test]
    fn reply_reader_accepts_one_line() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let writer = std::thread::spawn(move || {
            server.write_all(b"{\"type\":\"ok\"}\n").unwrap();
        });
        let reply = read_reply_bounded(
            &mut client,
            Instant::now() + Duration::from_secs(5),
            Duration::from_secs(5),
            1024,
        )
        .unwrap();
        writer.join().unwrap();
        assert_eq!(reply, "{\"type\":\"ok\"}");
    }

    #[test]
    fn reply_reader_reports_eof_without_a_reply() {
        let (mut client, server) = UnixStream::pair().unwrap();
        drop(server);
        let error = read_reply_bounded(
            &mut client,
            Instant::now() + Duration::from_secs(5),
            Duration::from_secs(5),
            1024,
        )
        .unwrap_err();
        assert!(error
            .message
            .contains("daemon closed the connection without a reply"));
    }

    #[test]
    fn reply_reader_rejects_an_oversized_reply() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let writer = std::thread::spawn(move || {
            let _ = server.write_all(&[b'a'; 256]);
        });
        let error = read_reply_bounded(
            &mut client,
            Instant::now() + Duration::from_secs(5),
            Duration::from_secs(5),
            64,
        )
        .unwrap_err();
        drop(client);
        writer.join().unwrap();
        assert!(error.message.contains("exceeds"));
    }
}

/// Users and system groups in one refresh.
#[derive(Debug, Clone, Default)]
pub struct UsersPage {
    pub users: Vec<UserSummary>,
    pub groups: Vec<String>,
}

pub async fn users_page(token: SessionToken) -> Result<UsersPage, ApiError> {
    let users = call(token.clone(), ModuleId::Users, Action::ListUsers, None).await?;
    let groups = call(token, ModuleId::Users, Action::ListGroups, None).await?;
    let users = users
        .get("users")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon users response has no list"))?;
    let groups = groups
        .get("groups")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon users response has no groups"))?;
    Ok(UsersPage {
        users: serde_json::from_value(users)
            .map_err(|e| ApiError::internal(format!("cannot read users: {e}")))?,
        groups: serde_json::from_value(groups)
            .map_err(|e| ApiError::internal(format!("cannot read groups: {e}")))?,
    })
}

pub async fn create_user(
    token: SessionToken,
    name: String,
    fullname: String,
    admin: bool,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Users,
        Action::CreateUser {
            name,
            fullname,
            admin,
        },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn delete_user(
    token: SessionToken,
    uid: u32,
    remove_files: bool,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Users,
        Action::DeleteUser { uid, remove_files },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn set_password(
    token: SessionToken,
    uid: u32,
    new_password: String,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Users,
        Action::SetPassword { uid, new_password },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn set_groups(
    token: SessionToken,
    uid: u32,
    groups: Vec<String>,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Users,
        Action::SetGroups { uid, groups },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn get_niri_config(token: SessionToken) -> Result<NiriConfigView, ApiError> {
    let value = call(token, ModuleId::Niri, Action::GetNiriConfig, None).await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read niri config: {e}")))
}

pub async fn validate_niri_config(
    token: SessionToken,
    proposed: Option<String>,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Niri,
        Action::ValidateNiriConfig { proposed },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn set_niri_section(
    token: SessionToken,
    section: String,
    value: serde_json::Value,
) -> Result<NiriConfigView, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Niri,
        Action::SetNiriSection { section, value },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read niri config: {e}")))
}

pub async fn apk_list(token: SessionToken) -> Result<Vec<ApkPackage>, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::PackagesApk,
        Action::ListApk,
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    let rows = value
        .get("packages")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon apk response has no packages"))?;
    serde_json::from_value(rows)
        .map_err(|e| ApiError::internal(format!("cannot read apk packages: {e}")))
}

pub async fn apk_search(token: SessionToken, query: String) -> Result<Vec<ApkPackage>, ApiError> {
    apk_packages(token, Action::SearchApk { query }, MUTATE_TIMEOUT).await
}

pub async fn apk_refresh(token: SessionToken) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::PackagesApk,
        Action::RefreshApk,
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn apk_repositories(token: SessionToken) -> Result<Vec<String>, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::PackagesApk,
        Action::ListApkRepositories,
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    let repositories = value.get("repositories").cloned().ok_or_else(|| {
        ApiError::internal("daemon apk repositories response has no repositories")
    })?;
    serde_json::from_value(repositories)
        .map_err(|e| ApiError::internal(format!("cannot read apk repositories: {e}")))
}

pub async fn apk_updates(token: SessionToken) -> Result<Vec<ApkUpdate>, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::PackagesApk,
        Action::ListApkUpdates,
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    let updates = value
        .get("updates")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon apk updates response has no updates"))?;
    serde_json::from_value(updates)
        .map_err(|e| ApiError::internal(format!("cannot read apk updates: {e}")))
}

pub async fn apk_preview(token: SessionToken, package: String) -> Result<String, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::PackagesApk,
        Action::PreviewApkInstall { package },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    Ok(value
        .get("output")
        .and_then(|value| value.as_str())
        .unwrap_or("no preview")
        .to_string())
}

pub async fn apk_install(
    token: SessionToken,
    package: String,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::PackagesApk,
        Action::InstallApk { package },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn apk_remove(
    token: SessionToken,
    package: String,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::PackagesApk,
        Action::RemoveApk { package },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn apk_upgrade_package(
    token: SessionToken,
    package: String,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::PackagesApk,
        Action::UpgradeApkPackage { package },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn apk_upgrade(token: SessionToken) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::PackagesApk,
        Action::UpgradeApk,
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

async fn apk_packages(
    token: SessionToken,
    action: Action,
    timeout: Duration,
) -> Result<Vec<ApkPackage>, ApiError> {
    let value = call_with_timeout(token, ModuleId::PackagesApk, action, None, timeout).await?;
    let rows = value
        .get("packages")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon apk response has no packages"))?;
    serde_json::from_value(rows)
        .map_err(|e| ApiError::internal(format!("cannot read apk packages: {e}")))
}

pub async fn flatpak_list(
    token: SessionToken,
    system: bool,
) -> Result<Vec<FlatpakPackage>, ApiError> {
    flatpak_packages(token, Action::ListFlatpak { system }, MUTATE_TIMEOUT).await
}

pub async fn flatpak_remotes(token: SessionToken) -> Result<Vec<FlatpakRemote>, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::PackagesFlatpak,
        Action::ListFlatpakRemotes,
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    let remotes = value
        .get("remotes")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon flatpak remotes response has no remotes"))?;
    serde_json::from_value(remotes)
        .map_err(|e| ApiError::internal(format!("cannot read flatpak remotes: {e}")))
}

pub async fn flatpak_search(
    token: SessionToken,
    query: String,
    system: bool,
) -> Result<Vec<FlatpakPackage>, ApiError> {
    flatpak_packages(
        token,
        Action::SearchFlatpak { query, system },
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn flatpak_install(
    token: SessionToken,
    app_id: String,
    system: bool,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::PackagesFlatpak,
        Action::InstallFlatpak { app_id, system },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn flatpak_update_app(
    token: SessionToken,
    app_id: String,
    system: bool,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::PackagesFlatpak,
        Action::UpdateFlatpakApp { app_id, system },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn flatpak_remove(
    token: SessionToken,
    app_id: String,
    system: bool,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::PackagesFlatpak,
        Action::RemoveFlatpak { app_id, system },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn flatpak_remove_orphans(
    token: SessionToken,
    system: bool,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::PackagesFlatpak,
        Action::RemoveFlatpakOrphans { system },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn flatpak_update(
    token: SessionToken,
    system: bool,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::PackagesFlatpak,
        Action::UpdateFlatpak { system },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

async fn flatpak_packages(
    token: SessionToken,
    action: Action,
    timeout: Duration,
) -> Result<Vec<FlatpakPackage>, ApiError> {
    let value = call_with_timeout(token, ModuleId::PackagesFlatpak, action, None, timeout).await?;
    let rows = value
        .get("packages")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon flatpak response has no packages"))?;
    serde_json::from_value(rows)
        .map_err(|e| ApiError::internal(format!("cannot read flatpak packages: {e}")))
}

pub async fn printer_page(token: SessionToken) -> Result<(Vec<Printer>, Vec<PrintJob>), ApiError> {
    let printers = call_with_timeout(
        token.clone(),
        ModuleId::Printers,
        Action::ListPrinters,
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    let jobs = call_with_timeout(
        token,
        ModuleId::Printers,
        Action::PrinterJobs { printer: None },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    let printers = printers
        .get("printers")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon printer response has no printers"))?;
    let jobs = jobs
        .get("jobs")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon printer response has no jobs"))?;
    Ok((
        serde_json::from_value(printers)
            .map_err(|e| ApiError::internal(format!("cannot read printers: {e}")))?,
        serde_json::from_value(jobs)
            .map_err(|e| ApiError::internal(format!("cannot read print jobs: {e}")))?,
    ))
}

pub async fn add_printer(
    token: SessionToken,
    name: String,
    uri: String,
    description: String,
    location: String,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Printers,
        Action::AddPrinter {
            name,
            uri,
            description,
            location,
        },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn add_printer_manual(
    token: SessionToken,
    name: String,
    uri: String,
    driver: String,
    ppd: String,
    description: String,
    location: String,
    options: Vec<String>,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Printers,
        Action::AddPrinterManual {
            name,
            uri,
            driver,
            ppd,
            description,
            location,
            options,
        },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn remove_printer(
    token: SessionToken,
    name: String,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Printers,
        Action::RemovePrinter { name },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn set_default_printer(
    token: SessionToken,
    name: String,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Printers,
        Action::SetDefaultPrinter { name },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn set_printer_options(
    token: SessionToken,
    name: String,
    options: Vec<String>,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Printers,
        Action::SetPrinterOptions { name, options },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn pause_printer(
    token: SessionToken,
    name: String,
    pause: bool,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Printers,
        Action::PausePrinter { name, pause },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn cancel_printer_jobs(
    token: SessionToken,
    printer: Option<String>,
) -> Result<serde_json::Value, ApiError> {
    call_with_timeout(
        token,
        ModuleId::Printers,
        Action::CancelPrinterJobs { printer },
        None,
        MUTATE_TIMEOUT,
    )
    .await
}

pub async fn scanner_config(token: SessionToken) -> Result<ScannerConfigView, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Scanners,
        Action::GetScannerConfig,
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read scanner config: {e}")))
}

pub async fn set_scanner_device(
    token: SessionToken,
    name: String,
    address: String,
    protocol: String,
) -> Result<ScannerConfigView, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Scanners,
        Action::SetScannerDevice {
            name,
            address,
            protocol,
        },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read scanner config: {e}")))
}

pub async fn remove_scanner_device(
    token: SessionToken,
    name: String,
) -> Result<ScannerConfigView, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Scanners,
        Action::RemoveScannerDevice { name },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read scanner config: {e}")))
}

pub async fn set_scanner_discovery(
    token: SessionToken,
    enabled: bool,
) -> Result<ScannerConfigView, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Scanners,
        Action::SetScannerDiscovery { enabled },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read scanner config: {e}")))
}

pub async fn firewall_status(token: SessionToken) -> Result<FirewallStatus, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Firewall,
        Action::GetFirewall,
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read firewall: {e}")))
}

pub async fn firewall_set_default(
    token: SessionToken,
    zone: String,
) -> Result<FirewallStatus, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Firewall,
        Action::SetDefaultFirewallZone { zone },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read firewall: {e}")))
}

pub async fn firewall_add_service(
    token: SessionToken,
    zone: String,
    service: String,
) -> Result<FirewallStatus, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Firewall,
        Action::AddFirewallService { zone, service },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read firewall: {e}")))
}

pub async fn firewall_remove_service(
    token: SessionToken,
    zone: String,
    service: String,
) -> Result<FirewallStatus, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Firewall,
        Action::RemoveFirewallService { zone, service },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read firewall: {e}")))
}

pub async fn firewall_add_port(
    token: SessionToken,
    zone: String,
    port: String,
) -> Result<FirewallStatus, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Firewall,
        Action::AddFirewallPort { zone, port },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read firewall: {e}")))
}

pub async fn firewall_remove_port(
    token: SessionToken,
    zone: String,
    port: String,
) -> Result<FirewallStatus, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Firewall,
        Action::RemoveFirewallPort { zone, port },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read firewall: {e}")))
}

pub async fn firewall_reload(token: SessionToken) -> Result<FirewallStatus, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Firewall,
        Action::ReloadFirewall,
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read firewall: {e}")))
}

pub async fn service_list(token: SessionToken) -> Result<Vec<Service>, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Services,
        Action::ListServices,
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    let services = value
        .get("services")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon services response has no services"))?;
    serde_json::from_value(services)
        .map_err(|e| ApiError::internal(format!("cannot read services: {e}")))
}

pub async fn service_action(
    token: SessionToken,
    name: String,
    op: ServiceOp,
) -> Result<Vec<Service>, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Services,
        Action::ServiceAction { name, op },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    let services = value
        .get("services")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon services response has no services"))?;
    serde_json::from_value(services)
        .map_err(|e| ApiError::internal(format!("cannot read services: {e}")))
}

pub async fn disk_list(token: SessionToken) -> Result<Vec<Disk>, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Disks,
        Action::ListDisks,
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    let disks = value
        .get("disks")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon disks response has no disks"))?;
    serde_json::from_value(disks).map_err(|e| ApiError::internal(format!("cannot read disks: {e}")))
}

pub async fn disk_mount(token: SessionToken, device: String) -> Result<Vec<Disk>, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Disks,
        Action::MountDisk { device },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    let disks = value
        .get("disks")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon disks response has no disks"))?;
    serde_json::from_value(disks).map_err(|e| ApiError::internal(format!("cannot read disks: {e}")))
}

pub async fn disk_unmount(token: SessionToken, device: String) -> Result<Vec<Disk>, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Disks,
        Action::UnmountDisk { device },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    let disks = value
        .get("disks")
        .cloned()
        .ok_or_else(|| ApiError::internal("daemon disks response has no disks"))?;
    serde_json::from_value(disks).map_err(|e| ApiError::internal(format!("cannot read disks: {e}")))
}

pub async fn bluetooth_view(token: SessionToken) -> Result<BluetoothView, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Bluetooth,
        Action::GetBluetooth,
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read bluetooth: {e}")))
}

pub async fn bluetooth_power(token: SessionToken, on: bool) -> Result<BluetoothView, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Bluetooth,
        Action::SetBluetoothPower { on },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read bluetooth: {e}")))
}

pub async fn bluetooth_scan(token: SessionToken) -> Result<BluetoothView, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Bluetooth,
        Action::ScanBluetooth,
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read bluetooth: {e}")))
}

pub async fn bluetooth_action(
    token: SessionToken,
    address: String,
    op: BluetoothOp,
) -> Result<BluetoothView, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Bluetooth,
        Action::BluetoothAction { address, op },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read bluetooth: {e}")))
}

pub async fn network_view(token: SessionToken) -> Result<NetworkView, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Network,
        Action::GetNetwork,
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read network: {e}")))
}

pub async fn wifi_enabled(token: SessionToken, enabled: bool) -> Result<NetworkView, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Network,
        Action::SetWifiEnabled { enabled },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read network: {e}")))
}

pub async fn connect_wifi(
    token: SessionToken,
    ssid: String,
    password: String,
) -> Result<NetworkView, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Network,
        Action::ConnectWifi { ssid, password },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read network: {e}")))
}

pub async fn connect_network(token: SessionToken, device: String) -> Result<NetworkView, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Network,
        Action::ConnectNetwork { device },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read network: {e}")))
}

pub async fn disconnect_network(
    token: SessionToken,
    device: String,
) -> Result<NetworkView, ApiError> {
    let value = call_with_timeout(
        token,
        ModuleId::Network,
        Action::DisconnectNetwork { device },
        None,
        MUTATE_TIMEOUT,
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|e| ApiError::internal(format!("cannot read network: {e}")))
}
