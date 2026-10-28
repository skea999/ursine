//! Request dispatch: auth handshake, token check, module routing, audit.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ursine_api::{
    Action, ApiError, ErrorCode, ModuleId, Principal, Request, RequestBody, Response,
};

use crate::audit::{AuditLog, Outcome};
use crate::auth::{AuthError, Authenticator, PeerInfo};
use crate::capabilities::Registry;
use crate::jobs::JobRegistry;
use crate::modules;
use crate::ratelimit::RateLimiter;
use crate::session::TokenStore;

/// PAM input caps: bound work before any audit or authentication call.
const MAX_AUTH_USER_BYTES: usize = 256;
const MAX_AUTH_PASSWORD_BYTES: usize = 1024;
/// A hung PAM/NSS backend must not pin a connection slot forever.
const AUTH_TIMEOUT: Duration = Duration::from_secs(30);
/// Bounded PAM concurrency: a timed-out task keeps its permit until PAM
/// returns, so hung backends cannot accumulate blocking-pool threads.
pub const MAX_INFLIGHT_AUTH: usize = 8;
/// A profile target user is a local account name, never a path or a blob.
const MAX_AS_USER_BYTES: usize = 64;

/// Everything the daemon owns. Built once at startup, shared via `Arc`.
pub struct Daemon {
    /// Live daemon configuration. Module switches can be changed while running;
    /// startup-critical settings (socket, audit, token TTL) require a restart.
    pub config: tokio::sync::RwLock<ursine_api::UrsineConfig>,
    /// Path the daemon was started with (`config.init` writes here).
    pub config_path: PathBuf,
    pub caps: Registry,
    pub auth: Arc<dyn Authenticator>,
    pub sessions: TokenStore,
    pub ratelimit: RateLimiter,
    pub auth_permits: Arc<tokio::sync::Semaphore>,
    pub audit: Arc<AuditLog>,
    /// Package job registry (shared with module handlers).
    pub jobs: JobRegistry,
}

impl std::fmt::Debug for Daemon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Daemon")
            .field("config_path", &self.config_path)
            .finish_non_exhaustive()
    }
}

impl Daemon {
    /// Handle one request. Never panics on request content.
    pub async fn handle(&self, req: Request, peer: &PeerInfo) -> Response {
        let id = req.id;
        if req.v != ursine_api::PROTOCOL_VERSION {
            return Response::err(
                id,
                ApiError::invalid(format!(
                    "protocol version {} not supported (want {})",
                    req.v,
                    ursine_api::PROTOCOL_VERSION
                )),
            );
        }

        match req.body {
            RequestBody::Auth { user, password } => {
                self.handle_auth(id, peer, &user, &password).await
            }
            RequestBody::Call {
                module,
                action,
                as_user,
            } => {
                self.handle_call(id, req.token.as_ref(), module, action, as_user)
                    .await
            }
        }
    }

    /// Read or change daemon configuration. Wheel is checked locally so the
    /// authorization invariant does not depend on there being only one token
    /// issuer. Config actions never honor `as_user`: they affect the daemon.
    async fn handle_config_action(
        &self,
        principal: &Principal,
        action: &Action,
    ) -> Result<serde_json::Value, ApiError> {
        if !principal.in_group("wheel") {
            return Err(ApiError::forbidden());
        }

        match action {
            Action::GetConfig => {
                let config = self.config.read().await;
                Ok(serde_json::json!({
                    "modules": config.modules,
                    "path": self.config_path.display().to_string(),
                    "exists": self.config_path.exists(),
                }))
            }
            Action::SetModuleSwitch { module, value } => {
                if crate::config::configurable_module(*module).is_none() {
                    return Err(ApiError::invalid(format!(
                        "module {} has no configurable switch",
                        module.as_str()
                    )));
                }
                let mut config = self.config.write().await;
                let path = self.config_path.clone();
                let module = *module;
                let value = *value;
                let result = tokio::task::spawn_blocking(move || {
                    crate::config::set_module_switch(&path, module, value)
                })
                .await
                .map_err(|e| ApiError::internal(format!("configuration task failed: {e}")))?
                .map_err(|e| ApiError::internal(e.to_string()))?;
                config.modules = result.clone();
                Ok(serde_json::json!({
                    "modules": result,
                    "path": self.config_path.display().to_string(),
                    "exists": self.config_path.exists(),
                }))
            }
            Action::Init => {
                let mut config = self.config.write().await;
                if self.config_path.exists() {
                    return Err(ApiError::new(
                        ErrorCode::Conflict,
                        "configuration already exists; use module switches instead",
                    ));
                }
                let caps = self.caps.reports();
                let path = self.config_path.clone();
                let modules = tokio::task::spawn_blocking(move || {
                    crate::config::write_template(&path, &caps)?;
                    Ok::<_, crate::error::CoreError>(crate::config::load(&path)?.modules)
                })
                .await
                .map_err(|e| ApiError::internal(format!("configuration task failed: {e}")))?
                .map_err(|e| ApiError::internal(e.to_string()))?;
                config.modules = modules.clone();
                Ok(serde_json::json!({
                    "modules": modules,
                    "written": self.config_path.display().to_string(),
                    "exists": true,
                }))
            }
            _ => Err(ApiError::invalid("configuration action required")),
        }
    }

    async fn handle_auth(
        &self,
        id: Option<u64>,
        peer: &PeerInfo,
        user: &str,
        password: &str,
    ) -> Response {
        // Charge the rate limiter before the size check so oversized input
        // cannot bypass login throttling. A limited request must not write an
        // fsynced audit line per attempt: a local user could otherwise flood
        // the audit disk without authenticating.
        if !self.ratelimit.allow(peer.uid) {
            tracing::warn!(uid = peer.uid, "auth attempt rate limited");
            return Response::err(id, ApiError::rate_limited());
        }

        // Bound the work before any audit or PAM call. On rejection, audit the
        // kernel peer identity so a huge claimed name cannot grow the log. The
        // rate limiter above caps these writes per uid.
        if user.len() > MAX_AUTH_USER_BYTES || password.len() > MAX_AUTH_PASSWORD_BYTES {
            let peer_principal = Principal::local(peer.user.clone(), Vec::new());
            self.audit.record(
                &peer_principal,
                None,
                "auth",
                None,
                Outcome::Err {
                    code: ErrorCode::Unauthorized,
                    message: "invalid credentials".into(),
                },
            );
            return Response::err(id, ApiError::unauthorized());
        }

        let anonymous = Principal::local(user, Vec::new());

        // PAM blocks; keep it off the async worker threads. Everything the
        // closure needs is owned so the task is 'static. A timed-out blocking
        // task keeps its permit until PAM returns, which bounds thread growth.
        let Ok(permit) = self.auth_permits.clone().try_acquire_owned() else {
            tracing::warn!(uid = peer.uid, "authenticator busy");
            self.audit.record(
                &anonymous,
                None,
                "auth",
                None,
                Outcome::Err {
                    code: ErrorCode::Internal,
                    message: "authenticator busy".into(),
                },
            );
            return Response::err(id, ApiError::internal("authenticator failure"));
        };
        let auth = self.auth.clone();
        let peer_for_auth = peer.clone();
        let user = user.to_owned();
        let password = password.to_owned();
        let attempt = tokio::time::timeout(
            AUTH_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                auth.authenticate(&peer_for_auth, &user, &password)
            }),
        )
        .await;
        let principal = match attempt {
            Err(_) => {
                tracing::error!(uid = peer.uid, "authenticator timed out");
                self.audit.record(
                    &anonymous,
                    None,
                    "auth",
                    None,
                    Outcome::Err {
                        code: ErrorCode::Internal,
                        message: "authenticator timed out".into(),
                    },
                );
                return Response::err(id, ApiError::internal("authenticator failure"));
            }
            Ok(Ok(Ok(p))) => p,
            Ok(Ok(Err(AuthError::Invalid))) => {
                // Generic error: no user-exists oracle.
                self.audit.record(
                    &anonymous,
                    None,
                    "auth",
                    None,
                    Outcome::Err {
                        code: ErrorCode::Unauthorized,
                        message: "invalid credentials".into(),
                    },
                );
                return Response::err(id, ApiError::unauthorized());
            }
            Ok(Ok(Err(AuthError::Internal(msg)))) => {
                tracing::error!(error = %msg, "authenticator failure");
                self.audit.record(
                    &anonymous,
                    None,
                    "auth",
                    None,
                    Outcome::Err {
                        code: ErrorCode::Internal,
                        message: "authenticator failure".into(),
                    },
                );
                return Response::err(id, ApiError::internal("authenticator failure"));
            }
            Ok(Err(error)) => {
                tracing::error!(error = %error, "authenticator task failed");
                self.audit.record(
                    &anonymous,
                    None,
                    "auth",
                    None,
                    Outcome::Err {
                        code: ErrorCode::Internal,
                        message: "authenticator task failed".into(),
                    },
                );
                return Response::err(id, ApiError::internal("authenticator failure"));
            }
        };

        if !principal.in_group("wheel") {
            self.audit.record(
                &principal,
                None,
                "auth",
                None,
                Outcome::Err {
                    code: ErrorCode::Forbidden,
                    message: "not in wheel".into(),
                },
            );
            return Response::err(id, ApiError::forbidden());
        }

        self.ratelimit.reset(peer.uid);
        let (token, ttl) = self.sessions.issue(principal.clone());
        self.audit
            .record(&principal, None, "auth", None, Outcome::Ok);
        Response::auth_ok(id, token, ttl)
    }

    async fn handle_call(
        &self,
        id: Option<u64>,
        token: Option<&ursine_api::SessionToken>,
        module: ModuleId,
        action: Action,
        as_user: Option<String>,
    ) -> Response {
        let Some(token) = token else {
            return Response::err(id, ApiError::unauthorized());
        };
        let Some(principal) = self.sessions.validate(token) else {
            return Response::err(id, ApiError::unauthorized());
        };

        // `asUser` is meaningful for packages actions that read or mutate one
        // profile and for theming (writes land in that user's home).
        // Everything else (StoreGc, system, capabilities, config) acts on the
        // daemon or the whole store and must not carry a target.
        let profile_scoped = module == ModuleId::PackagesNix && uses_profile(&action);
        let user_scoped = profile_scoped || module == ModuleId::Theming;
        // Theming writes config files: only the caller's own home (no
        // cross-user theming from the wire — the GUI never sends a target).
        let theming_foreign_target = module == ModuleId::Theming
            && as_user
                .as_ref()
                .is_some_and(|target| target != &principal.user);
        let as_user_error = if as_user.is_some() && !user_scoped {
            Some(ApiError::invalid(
                "as_user is only valid for profile-scoped packages_nix or theming actions",
            ))
        } else if theming_foreign_target {
            Some(ApiError::invalid(
                "theming only applies to your own account",
            ))
        } else if as_user
            .as_ref()
            .is_some_and(|target| target.len() > MAX_AS_USER_BYTES)
        {
            Some(ApiError::invalid(format!(
                "as_user exceeds {MAX_AS_USER_BYTES} bytes"
            )))
        } else {
            None
        };
        let as_user_valid = as_user
            .as_ref()
            .is_none_or(|target| target.len() <= MAX_AS_USER_BYTES);
        // Never audit a target the action did not honor.
        let audited_as_user = if user_scoped && as_user_valid {
            as_user.clone()
        } else {
            None
        };
        let target_user = if user_scoped {
            as_user.clone().unwrap_or_else(|| principal.user.clone())
        } else {
            principal.user.clone()
        };
        let action_name = match &action {
            Action::SetModuleSwitch { module, value } => {
                format!("set_module_switch {}={}", module.as_str(), value.as_str())
            }
            _ => modules::action_name(&action).to_string(),
        };
        let config = self.config.read().await.clone();
        let ctx = modules::ModuleCtx {
            caps: &self.caps,
            config,
            as_user: &target_user,
            jobs: &self.jobs,
            principal: &principal,
        };

        let result = match as_user_error {
            Some(error) => Err(error),
            None => match (module, &action) {
                (
                    ModuleId::Config,
                    Action::GetConfig | Action::SetModuleSwitch { .. } | Action::Init,
                ) => self.handle_config_action(&principal, &action).await,
                _ => modules::handle(module, &action, &ctx).await,
            },
        };

        // `job_output` polls are read-only and frequent; auditing them would
        // flood the log. Start and completion are audited (completion by the
        // registry when the worker exits).
        if !matches!(&action, Action::JobOutput { .. }) {
            let outcome = Outcome::from_result(&result);
            self.audit.record(
                &principal,
                Some(module),
                &action_name,
                audited_as_user,
                outcome,
            );
        }

        match result {
            Ok(value) => Response::ok(id, value),
            Err(e) => Response::err(id, e),
        }
    }
}

/// Actions that operate on exactly one nix profile.
fn uses_profile(action: &Action) -> bool {
    matches!(
        action,
        Action::ListInstalled
            | Action::Search { .. }
            | Action::Install { .. }
            | Action::Remove { .. }
            | Action::RemoveAll
            | Action::RemoveRegex { .. }
            | Action::Upgrade { .. }
            | Action::UpgradeRegex { .. }
            | Action::Generations
            | Action::Rollback { .. }
            | Action::WipeHistory { .. }
            | Action::DiffClosures { .. }
            | Action::PruneGenerations { .. }
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir
    use super::*;
    use crate::audit::AuditLog;
    use crate::auth::stub::StubAuthenticator;
    use crate::clock::ManualClock;
    use std::time::Duration;
    use ursine_api::{ResponseBody, SessionToken, PROTOCOL_VERSION};

    fn daemon(dir: &std::path::Path) -> (Arc<ManualClock>, Daemon) {
        let clock = ManualClock::new(0);
        let cfg = ursine_api::UrsineConfig {
            socket_path: dir.join("sock"),
            audit_path: dir.join("audit.jsonl"),
            token_ttl_min: 10,
            login_rate_limit_per_min: 5,
            modules: ursine_api::ModulesConfig::default(),
        };
        let audit = Arc::new(AuditLog::open(cfg.audit_path.clone(), clock.clone()).unwrap());
        let jobs = JobRegistry::new(dir.join("jobs"), audit.clone(), clock.clone()).unwrap();
        let d = Daemon {
            caps: Registry::system(),
            config_path: dir.join("config.toml"),
            auth: Arc::new(StubAuthenticator),
            sessions: TokenStore::new(Duration::from_secs(600), clock.clone()),
            ratelimit: RateLimiter::new(cfg.login_rate_limit_per_min, clock.clone()),
            auth_permits: Arc::new(tokio::sync::Semaphore::new(MAX_INFLIGHT_AUTH)),
            audit,
            jobs,
            config: tokio::sync::RwLock::new(cfg),
        };
        (clock, d)
    }

    fn tmp_dir(name: &str) -> PathBuf {
        // nosemgrep: rust.lang.security.temp-dir.temp-dir — tests only; paths are container-local and non-security-sensitive
        let d = std::env::temp_dir().join(format!("ursine-dispatch-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn peer() -> PeerInfo {
        PeerInfo {
            uid: 1000,
            user: "skea999".into(),
        }
    }

    fn auth_request(id: u64) -> Request {
        Request {
            v: PROTOCOL_VERSION,
            id: Some(id),
            token: None,
            body: RequestBody::Auth {
                user: "skea999".into(),
                password: "pw1".into(),
            },
        }
    }

    async fn token(daemon: &Daemon) -> SessionToken {
        match daemon.handle(auth_request(1), &peer()).await.body {
            ResponseBody::AuthOk { token, .. } => token,
            other => panic!("want auth_ok, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn happy_path_auth_then_call() {
        let (_clock, d) = daemon(&tmp_dir("happy"));
        let resp = d.handle(auth_request(1), &peer()).await;
        let token = match resp.body {
            ResponseBody::AuthOk {
                token,
                expires_in_secs,
            } => {
                assert_eq!(expires_in_secs, 600);
                token
            }
            other => panic!("want auth_ok, got {other:?}"),
        };

        let call = Request {
            v: PROTOCOL_VERSION,
            id: Some(2),
            token: Some(token),
            body: RequestBody::Call {
                module: ModuleId::System,
                action: Action::Status,
                as_user: None,
            },
        };
        let resp = d.handle(call, &peer()).await;
        match resp.body {
            ResponseBody::Ok { result } => {
                assert!(result.get("version").is_some());
            }
            other => panic!("want ok, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn job_output_poll_validates_id_and_is_not_audited() {
        let (_clock, d) = daemon(&tmp_dir("job-output"));
        let token = token(&d).await;
        for (job_id, want) in [
            ("../oops", ErrorCode::Invalid),
            ("0000000000000000", ErrorCode::NotFound),
        ] {
            let call = Request {
                v: PROTOCOL_VERSION,
                id: Some(3),
                token: Some(token.clone()),
                body: RequestBody::Call {
                    module: ModuleId::System,
                    action: Action::JobOutput {
                        job_id: job_id.into(),
                        offset: 0,
                    },
                    as_user: None,
                },
            };
            let resp = d.handle(call, &peer()).await;
            match resp.body {
                ResponseBody::Err { error } => assert_eq!(error.code, want, "{job_id}"),
                other => panic!("want err, got {other:?}"),
            }
        }
        // Polls are read-only and frequent: they must not reach the audit log.
        let audit = std::fs::read_to_string(d.audit.path()).unwrap_or_default();
        assert!(
            !audit.contains("job_output"),
            "poll leaked into audit:\n{audit}"
        );
    }

    #[tokio::test]
    async fn call_without_token_is_unauthorized() {
        let (_clock, d) = daemon(&tmp_dir("no-token"));
        let call = Request {
            v: PROTOCOL_VERSION,
            id: None,
            token: None,
            body: RequestBody::Call {
                module: ModuleId::System,
                action: Action::Status,
                as_user: None,
            },
        };
        match d.handle(call, &peer()).await.body {
            ResponseBody::Err { error } => assert_eq!(error.code, ErrorCode::Unauthorized),
            other => panic!("want err, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_token_is_unauthorized() {
        let (_clock, d) = daemon(&tmp_dir("bad-token"));
        let call = Request {
            v: PROTOCOL_VERSION,
            id: None,
            token: Some(SessionToken::new("nope")),
            body: RequestBody::Call {
                module: ModuleId::System,
                action: Action::Status,
                as_user: None,
            },
        };
        match d.handle(call, &peer()).await.body {
            ResponseBody::Err { error } => assert_eq!(error.code, ErrorCode::Unauthorized),
            other => panic!("want err, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stub_rejects_empty_credentials_generically() {
        let (_clock, d) = daemon(&tmp_dir("empty-creds"));
        let req = Request {
            v: PROTOCOL_VERSION,
            id: None,
            token: None,
            body: RequestBody::Auth {
                user: String::new(),
                password: String::new(),
            },
        };
        match d.handle(req, &peer()).await.body {
            ResponseBody::Err { error } => assert_eq!(error.code, ErrorCode::Unauthorized),
            other => panic!("want err, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rate_limit_blocks_after_five_failures() {
        let (_clock, d) = daemon(&tmp_dir("rate"));
        let mut last = None;
        for _ in 0..7 {
            let req = Request {
                v: PROTOCOL_VERSION,
                id: None,
                token: None,
                body: RequestBody::Auth {
                    user: "x".into(),
                    password: String::new(),
                },
            };
            last = Some(d.handle(req, &peer()).await.body);
        }
        match last {
            Some(ResponseBody::Err { error }) => assert_eq!(error.code, ErrorCode::RateLimited),
            other => panic!("want rate_limited, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn oversized_auth_input_is_rejected_generically() {
        let (_clock, d) = daemon(&tmp_dir("auth-bounds"));
        let long_user = "a".repeat(MAX_AUTH_USER_BYTES + 1);
        let req = Request {
            v: PROTOCOL_VERSION,
            id: None,
            token: None,
            body: RequestBody::Auth {
                user: long_user.clone(),
                password: "pw".into(),
            },
        };
        match d.handle(req, &peer()).await.body {
            ResponseBody::Err { error } => assert_eq!(error.code, ErrorCode::Unauthorized),
            other => panic!("want unauthorized, got {other:?}"),
        }

        let long_password = "p".repeat(MAX_AUTH_PASSWORD_BYTES + 1);
        let req = Request {
            v: PROTOCOL_VERSION,
            id: None,
            token: None,
            body: RequestBody::Auth {
                user: "skea999".into(),
                password: long_password.clone(),
            },
        };
        match d.handle(req, &peer()).await.body {
            ResponseBody::Err { error } => assert_eq!(error.code, ErrorCode::Unauthorized),
            other => panic!("want unauthorized, got {other:?}"),
        }

        let audit = std::fs::read_to_string(d.audit.path()).unwrap();
        assert!(audit.contains("\"principal\":\"skea999\""));
        assert!(!audit.contains(&long_user));
        assert!(!audit.contains(&long_password));
    }

    #[tokio::test]
    async fn system_with_as_user_is_invalid() {
        let (_clock, d) = daemon(&tmp_dir("as-user-system"));
        let token = token(&d).await;
        let call = Request {
            v: PROTOCOL_VERSION,
            id: Some(2),
            token: Some(token),
            body: RequestBody::Call {
                module: ModuleId::System,
                action: Action::Status,
                as_user: Some("other".into()),
            },
        };
        match d.handle(call, &peer()).await.body {
            ResponseBody::Err { error } => assert_eq!(error.code, ErrorCode::Invalid),
            other => panic!("want invalid, got {other:?}"),
        }
        let audit = std::fs::read_to_string(d.audit.path()).unwrap();
        assert!(!audit.lines().last().unwrap().contains("\"as_user\""));
    }

    #[tokio::test]
    async fn store_gc_with_as_user_is_invalid() {
        let (_clock, d) = daemon(&tmp_dir("as-user-gc"));
        let token = token(&d).await;
        let call = Request {
            v: PROTOCOL_VERSION,
            id: Some(2),
            token: Some(token),
            body: RequestBody::Call {
                module: ModuleId::PackagesNix,
                action: Action::StoreGc,
                as_user: Some("other".into()),
            },
        };
        match d.handle(call, &peer()).await.body {
            ResponseBody::Err { error } => assert_eq!(error.code, ErrorCode::Invalid),
            other => panic!("want invalid, got {other:?}"),
        }
        let audit = std::fs::read_to_string(d.audit.path()).unwrap();
        assert!(!audit.lines().last().unwrap().contains("\"as_user\""));
    }

    #[tokio::test]
    async fn wrong_protocol_version_is_rejected() {
        let (_clock, d) = daemon(&tmp_dir("version"));
        let req = Request {
            v: 99,
            id: None,
            token: None,
            body: RequestBody::Auth {
                user: "a".into(),
                password: "b".into(),
            },
        };
        match d.handle(req, &peer()).await.body {
            ResponseBody::Err { error } => assert_eq!(error.code, ErrorCode::Invalid),
            other => panic!("want err, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn config_switch_persists_and_applies_live() {
        let (_clock, d) = daemon(&tmp_dir("config-switch"));
        let resp = d.handle(auth_request(1), &peer()).await;
        let token = match resp.body {
            ResponseBody::AuthOk { token, .. } => token,
            other => panic!("want auth_ok, got {other:?}"),
        };

        let set = Request {
            v: PROTOCOL_VERSION,
            id: Some(2),
            token: Some(token.clone()),
            body: RequestBody::Call {
                module: ModuleId::Config,
                action: Action::SetModuleSwitch {
                    module: ModuleId::PackagesNix,
                    value: ursine_api::TriState::Off,
                },
                as_user: None,
            },
        };
        match d.handle(set, &peer()).await.body {
            ResponseBody::Ok { result } => {
                assert_eq!(result["modules"]["packages_nix"], false);
                assert_eq!(result["path"], d.config_path.display().to_string());
                assert_eq!(result["exists"], true);
            }
            other => panic!("want ok, got {other:?}"),
        }
        assert_eq!(
            d.config.read().await.modules.packages_nix,
            ursine_api::TriState::Off
        );
        assert!(d.config_path.exists());
        assert!(!d.config_path.with_extension("toml.tmp").exists());

        let get = Request {
            v: PROTOCOL_VERSION,
            id: Some(3),
            token: Some(token),
            body: RequestBody::Call {
                module: ModuleId::Config,
                action: Action::GetConfig,
                as_user: None,
            },
        };
        match d.handle(get, &peer()).await.body {
            ResponseBody::Ok { result } => {
                assert_eq!(result["modules"]["packages_nix"], false);
                assert_eq!(result["exists"], true);
            }
            other => panic!("want ok, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn config_get_reports_a_missing_file() {
        let (_clock, d) = daemon(&tmp_dir("config-missing"));
        let resp = d.handle(auth_request(1), &peer()).await;
        let token = match resp.body {
            ResponseBody::AuthOk { token, .. } => token,
            other => panic!("want auth_ok, got {other:?}"),
        };
        let get = Request {
            v: PROTOCOL_VERSION,
            id: Some(2),
            token: Some(token),
            body: RequestBody::Call {
                module: ModuleId::Config,
                action: Action::GetConfig,
                as_user: None,
            },
        };
        match d.handle(get, &peer()).await.body {
            ResponseBody::Ok { result } => {
                assert_eq!(result["exists"], false);
            }
            other => panic!("want ok, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn config_init_refuses_to_overwrite_existing_file() {
        let (_clock, d) = daemon(&tmp_dir("config-init"));
        let resp = d.handle(auth_request(1), &peer()).await;
        let token = match resp.body {
            ResponseBody::AuthOk { token, .. } => token,
            other => panic!("want auth_ok, got {other:?}"),
        };

        let init = |id| Request {
            v: PROTOCOL_VERSION,
            id: Some(id),
            token: Some(token.clone()),
            body: RequestBody::Call {
                module: ModuleId::Config,
                action: Action::Init,
                as_user: None,
            },
        };
        match d.handle(init(2), &peer()).await.body {
            ResponseBody::Ok { result } => {
                assert_eq!(result["written"], d.config_path.display().to_string());
                assert_eq!(result["exists"], true);
            }
            other => panic!("want ok, got {other:?}"),
        }
        assert!(d.config_path.exists());
        match d.handle(init(3), &peer()).await.body {
            ResponseBody::Err { error } => assert_eq!(error.code, ErrorCode::Conflict),
            other => panic!("want conflict, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_configurable_module_switch_is_invalid() {
        let (_clock, d) = daemon(&tmp_dir("config-invalid"));
        let resp = d.handle(auth_request(1), &peer()).await;
        let token = match resp.body {
            ResponseBody::AuthOk { token, .. } => token,
            other => panic!("want auth_ok, got {other:?}"),
        };
        let req = Request {
            v: PROTOCOL_VERSION,
            id: Some(2),
            token: Some(token),
            body: RequestBody::Call {
                module: ModuleId::Config,
                action: Action::SetModuleSwitch {
                    module: ModuleId::Capabilities,
                    value: ursine_api::TriState::On,
                },
                as_user: None,
            },
        };
        match d.handle(req, &peer()).await.body {
            ResponseBody::Err { error } => assert_eq!(error.code, ErrorCode::Invalid),
            other => panic!("want invalid, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn module_stubs_return_not_implemented_with_gating() {
        let (_clock, d) = daemon(&tmp_dir("stub"));
        let resp = d.handle(auth_request(1), &peer()).await;
        let token = match resp.body {
            ResponseBody::AuthOk { token, .. } => token,
            other => panic!("want auth_ok, got {other:?}"),
        };
        let call = Request {
            v: PROTOCOL_VERSION,
            id: None,
            token: Some(token),
            body: RequestBody::Call {
                module: ModuleId::PackagesNix,
                action: Action::ListInstalled,
                as_user: None,
            },
        };
        match d.handle(call, &peer()).await.body {
            ResponseBody::Err { error } => {
                // Either gating (unavailable) or NotImplemented — both acceptable
                // on machines without nix; must NEVER be Ok.
                assert!(matches!(
                    error.code,
                    ErrorCode::Unavailable | ErrorCode::NotImplemented
                ));
            }
            other => panic!("stub must not succeed: {other:?}"),
        }
    }
}
