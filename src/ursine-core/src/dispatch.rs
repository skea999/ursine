//! Request dispatch: auth handshake, token check, module routing, audit.

use std::path::PathBuf;
use std::sync::Arc;

use ursine_api::{
    Action, ApiError, ErrorCode, ModuleId, Principal, Request, RequestBody, Response,
};

use crate::audit::{AuditLog, Outcome};
use crate::auth::{AuthError, Authenticator, PeerInfo};
use crate::capabilities::Registry;
use crate::clock::Clock;
use crate::modules;
use crate::ratelimit::RateLimiter;
use crate::session::TokenStore;

/// Everything the daemon owns. Built once at startup, shared via `Arc`.
pub struct Daemon {
    /// Live daemon configuration. Module switches can be changed while running;
    /// startup-critical settings (socket, audit, token TTL) require a restart.
    pub config: tokio::sync::RwLock<ursine_api::UrsineConfig>,
    /// Path the daemon was started with (`config.init` writes here).
    pub config_path: PathBuf,
    pub caps: Registry,
    pub auth: Box<dyn Authenticator>,
    pub sessions: TokenStore,
    pub ratelimit: RateLimiter,
    pub audit: AuditLog,
    #[allow(dead_code)]
    pub clock: Arc<dyn Clock>,
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
            RequestBody::Auth { user, password } => self.handle_auth(id, peer, &user, &password),
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
                }))
            }
            _ => Err(ApiError::invalid("configuration action required")),
        }
    }

    fn handle_auth(
        &self,
        id: Option<u64>,
        peer: &PeerInfo,
        user: &str,
        password: &str,
    ) -> Response {
        let anonymous = Principal::local(user, Vec::new());

        if !self.ratelimit.allow(peer.uid) {
            self.audit.record(
                &anonymous,
                None,
                "auth",
                None,
                Outcome::Err {
                    code: ErrorCode::RateLimited,
                    message: "rate limited".into(),
                },
            );
            return Response::err(id, ApiError::rate_limited());
        }

        let principal = match self.auth.authenticate(peer, user, password) {
            Ok(p) => p,
            Err(AuthError::Invalid) => {
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
            Err(AuthError::Internal(msg)) => {
                tracing::error!(error = %msg, "authenticator failure");
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

        let target_user = as_user.clone().unwrap_or_else(|| principal.user.clone());
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
        };

        let result = match (module, &action) {
            (ModuleId::Config, Action::GetConfig)
            | (ModuleId::Config, Action::SetModuleSwitch { .. })
            | (ModuleId::Config, Action::Init) => {
                if as_user.is_some() {
                    Err(ApiError::invalid("as_user is not valid for module config"))
                } else {
                    self.handle_config_action(&principal, &action).await
                }
            }
            _ => modules::handle(module, &action, &ctx).await,
        };

        let outcome = Outcome::from_result(&result);
        self.audit
            .record(&principal, Some(module), &action_name, as_user, outcome);

        match result {
            Ok(value) => Response::ok(id, value),
            Err(e) => Response::err(id, e),
        }
    }
}

#[cfg(test)]
mod tests {
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
            modules: Default::default(),
        };
        let audit = AuditLog::open(cfg.audit_path.clone(), clock.clone()).unwrap();
        let d = Daemon {
            caps: crate::capabilities::Registry::system(),
            config_path: dir.join("config.toml"),
            auth: Box::new(StubAuthenticator),
            sessions: TokenStore::new(Duration::from_secs(600), clock.clone()),
            ratelimit: RateLimiter::new(cfg.login_rate_limit_per_min, clock.clone()),
            audit,
            config: tokio::sync::RwLock::new(cfg),
            clock: clock.clone(),
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
                user: "".into(),
                password: "".into(),
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
                    password: "".into(),
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
