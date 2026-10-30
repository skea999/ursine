//! `ursinectl` — the root CLI.
//!
//! In-process host of `ursine-core`: builds the same `Daemon` the system
//! service uses and drives `Daemon::handle` directly, so every gate,
//! validation and audit line is identical to the GUI path. Works inside a
//! `FeralOS` chroot (no dinit, no socket): every path is absolute and resolves
//! against the chroot root. Hard root gate — the CLI never runs as non-root.

#![allow(clippy::print_stdout, clippy::print_stderr)] // CLI output is the interface

pub mod cli;
pub mod daemon;
pub mod root;

use std::io::Write;

use ursine_api::{Action, ModuleId, Request, RequestBody, ResponseBody, SessionToken};
use ursine_core::dispatch::Daemon;

/// Exit code: usage error, root gate, missing confirmation.
pub const EXIT_USAGE: u8 = 2;

/// Issue an in-process session token for the root principal.
///
/// Root is superuser: the principal carries `wheel` unconditionally so
/// config actions are not hostage to `/etc/group` wording on minimal chroot
/// installs. This grants nothing the euid-0 gate has not already granted.
pub fn issue_token(daemon: &Daemon) -> SessionToken {
    let principal = ursine_api::Principal::local("root", vec!["wheel".to_string()]);
    let (token, _expires) = daemon.sessions.issue(principal);
    token
}

/// Root peer identity for in-process calls (euid 0 was verified by the gate).
pub fn root_peer() -> ursine_core::auth::PeerInfo {
    ursine_core::auth::PeerInfo {
        uid: 0,
        user: "root".to_string(),
    }
}

/// One authenticated in-process call through the shared dispatch path.
pub async fn call(
    daemon: &Daemon,
    token: &SessionToken,
    peer: &ursine_core::auth::PeerInfo,
    module: ModuleId,
    action: Action,
    as_user: Option<String>,
) -> Result<serde_json::Value, ursine_api::ApiError> {
    let req = Request {
        v: ursine_api::PROTOCOL_VERSION,
        id: Some(1),
        token: Some(token.clone()),
        body: RequestBody::Call {
            module,
            action,
            as_user,
        },
    };
    match daemon.handle(req, peer).await.body {
        ResponseBody::Ok { result } => Ok(result),
        ResponseBody::Err { error } => Err(error),
        ResponseBody::AuthOk { .. } => Err(ursine_api::ApiError::internal(
            "unexpected auth response for an in-process call",
        )),
    }
}

/// Stream a package-mutation job to `out` (terminal text, never a bar).
///
/// Polls `job_output` through the same dispatch path until the job is
/// closed, then returns the job exit code (`1` when the worker died without
/// a status). The daemon's own registry is untouched: `ursinectl` keeps its
/// jobs under `/run/ursinectl/jobs`.
pub async fn stream_job(
    daemon: &Daemon,
    token: &SessionToken,
    peer: &ursine_core::auth::PeerInfo,
    job_id: &str,
    out: &mut impl Write,
) -> Result<i32, ursine_api::ApiError> {
    let mut offset = 0u64;
    loop {
        let result = call(
            daemon,
            token,
            peer,
            ModuleId::System,
            Action::JobOutput {
                job_id: job_id.to_string(),
                offset,
            },
            None,
        )
        .await?;
        let poll: ursine_api::JobOutput = serde_json::from_value(result)
            .map_err(|e| ursine_api::ApiError::internal(format!("decode job output: {e}")))?;
        if !poll.lines.is_empty() {
            out.write_all(poll.lines.as_bytes())
                .map_err(|e| ursine_api::ApiError::internal(format!("write output: {e}")))?;
            let _ = out.flush();
        }
        offset = poll.offset;
        match poll.status {
            ursine_api::JobStatus::Running => {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
            ursine_api::JobStatus::Done => {
                if poll.truncated {
                    eprintln!("note: job output truncated at the log size cap");
                }
                return Ok(poll.exit_code.unwrap_or(1));
            }
        }
    }
}
