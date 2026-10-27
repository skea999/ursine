//! `ursined` — the root daemon.
//!
//! Serves the newline-delimited JSON protocol on a unix socket, authenticates
//! principals via the PAM handshake (dev builds: loud stub), and dispatches to
//! module handlers. Stateless: nothing survives a restart but the audit log.

#![allow(clippy::print_stdout, clippy::print_stderr)] // CLI output is the interface

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ursine_core::audit::AuditLog;
use ursine_core::auth::Authenticator;
use ursine_core::capabilities::Registry;
use ursine_core::clock::{Clock, SystemClock};
use ursine_core::dispatch::Daemon;
use ursine_core::jobs::JobRegistry;
use ursine_core::ratelimit::RateLimiter;
use ursine_core::session::TokenStore;
use ursine_core::socket;

const USAGE: &str = "\
ursined — Ursine control daemon

USAGE: ursined [--config <path>]

OPTIONS:
  --config <path>   Configuration file (default: /etc/ursine/config.toml)
  --version         Print version and exit
  --help            Print this help and exit
";

fn main() -> std::process::ExitCode {
    // nosemgrep: rust.lang.security.args.args — CLI args parsed strictly (unknown rejected); no shell, no eval
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut config_path = PathBuf::from("/etc/ursine/config.toml");

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--config" => {
                let Some(p) = it.next() else {
                    eprintln!("error: --config needs a path");
                    return std::process::ExitCode::FAILURE;
                };
                config_path = PathBuf::from(p);
            }
            "--version" => {
                let auth = if cfg!(feature = "pam") { "pam" } else { "stub" };
                println!("ursined {} (auth: {auth})", env!("CARGO_PKG_VERSION"));
                return std::process::ExitCode::SUCCESS;
            }
            "--help" | "-h" => {
                print!("{USAGE}");
                return std::process::ExitCode::SUCCESS;
            }
            other => {
                eprintln!("error: unknown argument {other:?}\n\n{USAGE}");
                return std::process::ExitCode::FAILURE;
            }
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let Ok(runtime) = tokio::runtime::Runtime::new() else {
        eprintln!("error: cannot start the async runtime");
        return std::process::ExitCode::FAILURE;
    };
    runtime.block_on(async_main(config_path))
}

async fn async_main(config_path: PathBuf) -> std::process::ExitCode {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());

    let config = match ursine_core::config::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "cannot load configuration");
            return std::process::ExitCode::FAILURE;
        }
    };
    if !config_path.exists() {
        tracing::warn!(path = %config_path.display(), "config file missing — using defaults (config.init writes one)");
    }

    let socket_path = config.socket_path.clone();
    let token_ttl_min = config.token_ttl_min;
    let audit = match AuditLog::open(config.audit_path.clone(), clock.clone()) {
        Ok(a) => Arc::new(a),
        Err(e) => {
            tracing::error!(error = %e, "cannot open audit log");
            return std::process::ExitCode::FAILURE;
        }
    };
    tracing::info!(path = %audit.path().display(), "audit log ready");

    let jobs_dir = PathBuf::from("/run/ursine/jobs");
    // Fail closed on purpose: protocol v3 mutation replies return `job_id`,
    // so a daemon without the registry would answer every mutation wrong.
    let jobs = match JobRegistry::new(jobs_dir.clone(), audit.clone(), clock.clone()) {
        Ok(j) => j,
        Err(e) => {
            tracing::error!(error = %e, "cannot open job registry");
            return std::process::ExitCode::FAILURE;
        }
    };
    tracing::info!(dir = %jobs_dir.display(), "job registry ready");

    let auth: Arc<dyn Authenticator> = build_authenticator();

    let daemon = Arc::new(Daemon {
        caps: Registry::system(),
        config_path: config_path.clone(),
        sessions: TokenStore::new(
            Duration::from_secs(u64::from(config.token_ttl_min) * 60),
            clock.clone(),
        ),
        ratelimit: RateLimiter::new(config.login_rate_limit_per_min, clock.clone()),
        auth_permits: Arc::new(tokio::sync::Semaphore::new(
            ursine_core::dispatch::MAX_INFLIGHT_AUTH,
        )),
        audit,
        jobs,
        auth,
        config: tokio::sync::RwLock::new(config),
    });

    tracing::info!(
        socket = %socket_path.display(),
        ttl_min = token_ttl_min,
        "starting ursined {}",
        env!("CARGO_PKG_VERSION")
    );

    let result = socket::serve(daemon.clone(), &socket_path, shutdown_signal()).await;
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "server stopped with error");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "pam")]
fn build_authenticator() -> Arc<dyn Authenticator> {
    tracing::info!("authentication: PAM (service \"ursine\")");
    Arc::new(ursine_core::auth::pam::PamAuthenticator)
}

#[cfg(not(feature = "pam"))]
fn build_authenticator() -> Arc<dyn Authenticator> {
    tracing::warn!("authentication: STUB — DEVELOPMENT BUILD, never ship this binary");
    Arc::new(ursine_core::auth::stub::StubAuthenticator)
}

/// SIGTERM or Ctrl-C ends the accept loop cleanly.
async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            tracing::warn!("Ctrl-C handler unavailable; waiting for SIGTERM only");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::warn!(error = %error, "SIGTERM handler unavailable");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = term => {},
    }
    tracing::info!("signal received");
}
