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
    pub config: ursine_api::UrsineConfig,
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
    pub fn handle(&self, req: Request, peer: &PeerInfo) -> Response {
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
            } => self.handle_call(id, req.token.as_ref(), module, action, as_user),
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

    fn handle_call(
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
        let action_name = modules::action_name(&action);
        let ctx = modules::ModuleCtx {
            caps: &self.caps,
            config: &self.config,
            config_path: &self.config_path,
            as_user: &target_user,
        };

        let result = modules::handle(module, &action, &ctx);

        let outcome = Outcome::from_result(&result);
        self.audit
            .record(&principal, Some(module), action_name, as_user, outcome);

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
            config: cfg,
            clock: clock.clone(),
        };
        (clock, d)
    }

    fn tmp_dir(name: &str) -> PathBuf {
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

    #[test]
    fn happy_path_auth_then_call() {
        let (_clock, d) = daemon(&tmp_dir("happy"));
        let resp = d.handle(auth_request(1), &peer());
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
        let resp = d.handle(call, &peer());
        match resp.body {
            ResponseBody::Ok { result } => {
                assert!(result.get("version").is_some());
            }
            other => panic!("want ok, got {other:?}"),
        }
    }

    #[test]
    fn call_without_token_is_unauthorized() {
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
        match d.handle(call, &peer()).body {
            ResponseBody::Err { error } => assert_eq!(error.code, ErrorCode::Unauthorized),
            other => panic!("want err, got {other:?}"),
        }
    }

    #[test]
    fn unknown_token_is_unauthorized() {
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
        match d.handle(call, &peer()).body {
            ResponseBody::Err { error } => assert_eq!(error.code, ErrorCode::Unauthorized),
            other => panic!("want err, got {other:?}"),
        }
    }

    #[test]
    fn stub_rejects_empty_credentials_generically() {
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
        match d.handle(req, &peer()).body {
            ResponseBody::Err { error } => assert_eq!(error.code, ErrorCode::Unauthorized),
            other => panic!("want err, got {other:?}"),
        }
    }

    #[test]
    fn rate_limit_blocks_after_five_failures() {
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
            last = Some(d.handle(req, &peer()).body);
        }
        match last {
            Some(ResponseBody::Err { error }) => assert_eq!(error.code, ErrorCode::RateLimited),
            other => panic!("want rate_limited, got {other:?}"),
        }
    }

    #[test]
    fn wrong_protocol_version_is_rejected() {
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
        match d.handle(req, &peer()).body {
            ResponseBody::Err { error } => assert_eq!(error.code, ErrorCode::Invalid),
            other => panic!("want err, got {other:?}"),
        }
    }

    #[test]
    fn module_stubs_return_not_implemented_with_gating() {
        let (_clock, d) = daemon(&tmp_dir("stub"));
        let resp = d.handle(auth_request(1), &peer());
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
        match d.handle(call, &peer()).body {
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
