//! User crontab management (SPEC-crontab): read and replace the session
//! user's crontab through the `crontab` tool (busybox, cronie and vixie/
//! Debian all provide it — the tool abstracts the spool layout).

use std::path::PathBuf;

use ursine_api::{Action, ApiError, CrontabView};

use crate::modules::packages_nix::exec::run_as_user_argv;
use crate::modules::theming::{target_identity, write_user_file};

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
async fn get_crontab(user: &str) -> Result<CrontabView, ApiError> {
    match run_as_user_argv(user, "crontab", &["-l"], std::time::Duration::from_secs(10)).await {
        Ok(outcome) => Ok(CrontabView {
            crontab: Some(outcome.stdout),
        }),
        Err(error) if error.message.to_lowercase().contains("no crontab") => {
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
    if content.len() > ursine_api::max_crontab_bytes() {
        return Err(ApiError::invalid("crontab too large (256 KiB max)"));
    }
    if content.contains('\0') {
        return Err(ApiError::invalid("crontab contains NUL bytes"));
    }
    let normalized = if content.is_empty() || content.ends_with('\n') {
        content.to_string()
    } else {
        format!("{content}\n")
    };
    let (home, uid, gid) = target_identity(ctx.as_user)?;
    let staging: PathBuf = home.join(".config/ursine-crontab.tmp");
    write_user_file(&home, &staging, normalized.as_bytes(), uid, gid)?;
    let staged = staging.to_string_lossy().into_owned();
    let result = run_as_user_argv(
        ctx.as_user,
        "crontab",
        &[&staged],
        std::time::Duration::from_secs(15),
    )
    .await;
    let _ = std::fs::remove_file(&staging);
    result?;
    Ok(CrontabView {
        crontab: Some(normalized),
    })
}
