//! User crontab management (SPEC-crontab): read and replace the session
//! user's crontab through the `crontab` tool (busybox, cronie and vixie/
//! Debian all provide it — the tool abstracts the spool layout).

use std::path::{Path, PathBuf};

use ursine_api::{Action, ApiError, CrontabView};

use crate::modules::packages_nix::exec::{run_as_user_argv, run_as_user_argv_bounded};
use crate::modules::theming::{target_identity, write_user_file};

/// Trusted absolute path for the tool: never a bare name resolved through
/// the target user's login-shell PATH (a user-writable PATH could plant a
/// fake `crontab` and mask the real one).
fn crontab_tool() -> Result<&'static str, ApiError> {
    for candidate in ["/usr/bin/crontab", "/bin/crontab", "/usr/local/bin/crontab"] {
        if Path::new(candidate).is_file() {
            return Ok(candidate);
        }
    }
    Err(ApiError::unavailable(
        "crontab is not installed",
        "apk add dcron  # or: apt install cron",
    ))
}

/// Payload validation (pure, unit tested): size cap, NUL rejection and
/// trailing-newline normalization (all crontab implementations accept a
/// final newline; some require it).
fn validate_content(content: &str) -> Result<String, ApiError> {
    if content.len() > ursine_api::max_crontab_bytes() {
        return Err(ApiError::invalid("crontab too large (256 KiB max)"));
    }
    if content.contains('\0') {
        return Err(ApiError::invalid("crontab contains NUL bytes"));
    }
    if content.is_empty() || content.ends_with('\n') {
        Ok(content.to_string())
    } else {
        Ok(format!("{content}\n"))
    }
}

/// Staging-file identity: root stages in its passwd home (uid/gid 0) —
/// the normal-login-user guard in `target_identity` exists for cross-user
/// writes, not for root editing its own crontab.
fn staging_identity(user: &str) -> Result<(PathBuf, u32, u32), ApiError> {
    if user != "root" {
        return target_identity(user);
    }
    let passwd = std::fs::read_to_string("/etc/passwd")
        .map_err(|e| ApiError::internal(format!("read /etc/passwd: {e}")))?;
    for line in passwd.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 7 && fields.first() == Some(&"root") {
            let uid: u32 = fields
                .get(2)
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| ApiError::internal("bad root uid"))?;
            let gid: u32 = fields
                .get(3)
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| ApiError::internal("bad root gid"))?;
            let Some(home) = fields.get(5).map(|v| PathBuf::from(*v)) else {
                continue;
            };
            if uid == 0 && home.is_absolute() {
                return Ok((home, uid, gid));
            }
        }
    }
    Err(ApiError::internal("no root entry in /etc/passwd"))
}

/// Route a crontab action (async exec helpers run inline, like niri's).
pub async fn handle(
    action: &Action,
    ctx: &crate::modules::ModuleCtx<'_>,
) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::CrontabGet => {
            let view = get_crontab(ctx.as_user).await?;
            serde_json::to_value(view)
                .map_err(|e| ApiError::internal(format!("encode crontab: {e}")))
        }
        Action::CrontabSet { content } => {
            let view = set_crontab(ctx, content).await?;
            serde_json::to_value(view)
                .map_err(|e| ApiError::internal(format!("encode crontab: {e}")))
        }
        _ => Err(ApiError::invalid(format!(
            "action {} is not valid for module crontab",
            crate::modules::action_name(action)
        ))),
    }
}

/// Current crontab of `user`; "no crontab" maps to `None` (not an error).
/// Reads are bounded: a crontab over the cap is an error, never a silent
/// truncation (a truncated read + save would destroy the tail).
async fn get_crontab(user: &str) -> Result<CrontabView, ApiError> {
    let tool = crontab_tool()?;
    let outcome = if user == "root" {
        // Root is the calling process (`ursinectl`): no `su`, direct exec.
        crate::modules::packages_nix::exec::run_root_argv_bounded(
            tool,
            &["-l"],
            std::time::Duration::from_secs(10),
            ursine_api::max_crontab_bytes() + 1024,
        )
        .await
    } else {
        run_as_user_argv_bounded(
            user,
            tool,
            &["-l"],
            std::time::Duration::from_secs(10),
            ursine_api::max_crontab_bytes() + 1024,
        )
        .await
    };
    view_from_outcome(outcome)
}

/// Pure mapping: `crontab -l` result → view ("no crontab" is not an error).
fn view_from_outcome(
    outcome: Result<ursine_api::CommandOutcome, ApiError>,
) -> Result<CrontabView, ApiError> {
    match outcome {
        // Busybox keeps an emptied crontab readable with empty output and
        // exit 0: an empty read is "no crontab", same as the stderr mapping.
        Ok(outcome) if outcome.stdout.trim().is_empty() => Ok(CrontabView { crontab: None }),
        Ok(outcome) => Ok(CrontabView {
            crontab: Some(outcome.stdout),
        }),
        // Every implementation words it "no crontab for …".
        Err(error)
            if error.message.trim().ends_with("no crontab for user")
                || error.message.contains("no crontab for") =>
        {
            Ok(CrontabView { crontab: None })
        }
        Err(error) => Err(error),
    }
}

/// Replace `user`'s crontab with `content` (empty clears it). The payload is
/// staged in the user's home (owned by them) and handed to `crontab <file>`
/// — the portable form supported by busybox, cronie and vixie alike.
async fn set_crontab(
    ctx: &crate::modules::ModuleCtx<'_>,
    content: &str,
) -> Result<CrontabView, ApiError> {
    let normalized = validate_content(content)?;
    let user = ctx.as_user.to_string();
    let payload = normalized.clone();
    // Identity lookup + atomic write off the async executor (same policy
    // as the theming writers).
    // Root stages in its own home (the normal-login-user guard is for
    // cross-user writes, not for root editing its own crontab).
    let staging: PathBuf = tokio::task::spawn_blocking(move || -> Result<PathBuf, ApiError> {
        let (home, uid, gid) = staging_identity(&user)?;
        let staging: PathBuf = home.join(".config/ursine-crontab.tmp");
        write_user_file(&home, &staging, payload.as_bytes(), uid, gid)?;
        Ok(staging)
    })
    .await
    .map_err(|e| ApiError::internal(format!("crontab staging failed: {e}")))??;
    let staged = staging.to_string_lossy().into_owned();
    let tool = crontab_tool()?;
    let result = if ctx.as_user == "root" {
        crate::modules::packages_nix::exec::run_root_argv_bounded(
            tool,
            &[&staged],
            std::time::Duration::from_secs(15),
            ursine_api::max_crontab_bytes() + 1024,
        )
        .await
    } else {
        run_as_user_argv(
            ctx.as_user,
            tool,
            &[&staged],
            std::time::Duration::from_secs(15),
        )
        .await
    };
    let _ = std::fs::remove_file(&staging);
    result?;
    Ok(CrontabView {
        crontab: Some(normalized),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_validation_caps_rejects_nul_and_normalizes_newline() {
        // Cap: one byte over is refused.
        let big = "x".repeat(ursine_api::max_crontab_bytes() + 1);
        assert!(validate_content(&big).is_err());
        // NUL rejected.
        assert!(validate_content("a\0b").is_err());
        // Trailing newline normalization.
        assert_eq!(
            validate_content("* * * * * true").unwrap(),
            "* * * * * true\n"
        );
        assert_eq!(
            validate_content("* * * * * true\n").unwrap(),
            "* * * * * true\n"
        );
        // Empty clears and stays empty.
        assert_eq!(validate_content("").unwrap(), "");
    }

    #[test]
    fn no_crontab_maps_to_none_and_real_errors_propagate() {
        let absent: Result<ursine_api::CommandOutcome, ApiError> =
            Err(ApiError::internal("command failed: no crontab for gab"));
        let view = view_from_outcome(absent).unwrap();
        assert!(view.crontab.is_none());

        let boom: Result<ursine_api::CommandOutcome, ApiError> =
            Err(ApiError::internal("command failed: crontab: not found"));
        assert!(view_from_outcome(boom).is_err());

        let ok: Result<ursine_api::CommandOutcome, ApiError> = Ok(ursine_api::CommandOutcome {
            stdout: "* * * * * true\n".into(),
            stderr: String::new(),
        });
        let view = view_from_outcome(ok).unwrap();
        assert_eq!(view.crontab.as_deref(), Some("* * * * * true\n"));
    }
}
