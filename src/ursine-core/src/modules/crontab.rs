//! User crontab management (SPEC-crontab): read and replace a user's
//! crontab through the `crontab` tool (busybox, cronie and vixie/Debian
//! all provide it — the tool abstracts the spool layout).
//!
//! The daemon/CLI always run as root, so EVERY target goes through the
//! portable `crontab -u <user>` form (user decision 2026-11-02): no `su`
//! impersonation and no suid dependency on the tool.

use std::path::{Path, PathBuf};

use ursine_api::{Action, ApiError, CrontabView};

use crate::modules::packages_nix::exec::run_root_argv_bounded;

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

/// The target must EXIST and be root or a normal-login account (uid
/// 1000..60000) — system accounts never get a managed crontab.
fn validate_crontab_target(user: &str) -> Result<(), ApiError> {
    let passwd = std::fs::read_to_string("/etc/passwd")
        .map_err(|e| ApiError::internal(format!("read /etc/passwd: {e}")))?;
    for line in passwd.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3 && fields.first() == Some(&user) {
            let uid: u32 = fields
                .get(2)
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| ApiError::internal(format!("bad uid for {user}")))?;
            return if uid == 0 || (1000..60000).contains(&uid) {
                Ok(())
            } else {
                Err(ApiError::invalid("target is a system user"))
            };
        }
    }
    Err(ApiError::new(
        ursine_api::ErrorCode::NotFound,
        format!("user {user} does not exist"),
    ))
}

/// Root-owned staging file under `/run` (tmpfs: cleared at boot). The
/// name embeds the process id so concurrent sets never race on a path
/// nor hand the wrong payload to `crontab -u`.
fn staging_path() -> Result<PathBuf, ApiError> {
    let dir = Path::new("/run/ursine");
    std::fs::create_dir_all(dir)
        .map_err(|e| ApiError::internal(format!("create {}: {e}", dir.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(dir.join(format!("crontab-{}.tmp", std::process::id())))
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
    validate_crontab_target(user)?;
    let tool = crontab_tool()?;
    // Portable root form: `crontab -u <user> -l` (busybox, cronie, vixie).
    let outcome = run_root_argv_bounded(
        tool,
        &["-u", user, "-l"],
        std::time::Duration::from_secs(10),
        ursine_api::max_crontab_bytes() + 1024,
    )
    .await;
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
        // vixie/cronie: "no crontab for <user>".
        Err(error)
            if error.message.trim().ends_with("no crontab for user")
                || error.message.contains("no crontab for") =>
        {
            Ok(CrontabView { crontab: None })
        }
        // busybox list for a user who never had a crontab: the spool file
        // does not exist yet ("can't open '<user>': No such file …").
        // Permission problems keep their own strerror and stay errors.
        Err(error)
            if error.message.contains("can't open")
                && error.message.contains("No such file or directory") =>
        {
            Ok(CrontabView { crontab: None })
        }
        Err(error) => Err(error),
    }
}

/// Directory holding crontab backups: the live crontab lives in the cron
/// spool (owned by the cron daemon), so previous versions are archived
/// here with the uniform `.ursine-backup[.N]` slot rotation.
fn backup_dir() -> &'static Path {
    Path::new("/var/backups/ursine")
}

/// Archive `previous` as the newest backup of `user`'s crontab (rotation
/// runs between the `crontab -l` read and `crontab -u` — a concurrent
/// editor's version landing in that window is not captured; accepted,
/// there is no shared lock across cron clients). Runs on the
/// blocking pool; a failure aborts the replacement (fail-closed — the
/// current crontab is never replaced without a recoverable copy).
async fn backup_previous(user: &str, previous: &str, copies: usize) -> Result<(), ApiError> {
    let dir = backup_dir().to_path_buf();
    let target = dir.join(format!("crontab.{user}"));
    let bytes = previous.as_bytes().to_vec();
    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        std::fs::create_dir_all(&dir)
            .map_err(|e| ApiError::internal(format!("create {}: {e}", dir.display())))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| ApiError::internal(format!("harden {}: {e}", dir.display())))?;
        }
        crate::backup_configs::rotate_store(&target, copies, &bytes, 0o600)
    })
    .await
    .map_err(|e| ApiError::internal(format!("crontab backup task failed: {e}")))?
}

/// Replace `user`'s crontab with `content` (empty clears it). The payload
/// is staged as root under `/run` and handed to `crontab -u <user> <file>`
/// — the portable replace form supported by busybox, cronie and vixie.
/// The previous crontab is archived first (fail-closed).
async fn set_crontab(
    ctx: &crate::modules::ModuleCtx<'_>,
    content: &str,
) -> Result<CrontabView, ApiError> {
    let normalized = validate_content(content)?;
    let user = ctx.as_user.to_string();
    // Archive the current crontab before anything touches it; "no
    // crontab" has nothing to save.
    if let Some(previous) = get_crontab(&user).await?.crontab {
        backup_previous(
            &user,
            &previous,
            crate::backup_configs::copies_from(&ctx.config),
        )
        .await?;
    }
    let payload = normalized.clone();
    let stage_user = user.clone();
    // Target validation + staging write off the async executor (same
    // policy as the theming writers).
    let staging: PathBuf = tokio::task::spawn_blocking(move || -> Result<PathBuf, ApiError> {
        validate_crontab_target(&stage_user)?;
        let staging = staging_path()?;
        std::fs::write(&staging, payload.as_bytes())
            .map_err(|e| ApiError::internal(format!("stage {}: {e}", staging.display())))?;
        Ok(staging)
    })
    .await
    .map_err(|e| ApiError::internal(format!("crontab staging failed: {e}")))??;
    let staged = staging.to_string_lossy().into_owned();
    let tool = crontab_tool()?;
    // Portable root form: `crontab -u <user> <file>`.
    let result = run_root_argv_bounded(
        tool,
        &["-u", &user, &staged],
        std::time::Duration::from_secs(15),
        ursine_api::max_crontab_bytes() + 1024,
    )
    .await;
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
    fn staging_path_is_per_process_under_run() {
        // Two concurrent sets must stage to different files: the name
        // embeds this process id and lives under /run (tmpfs, root-owned).
        let path = staging_path().unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.contains(&std::process::id().to_string()), "{name}");
        assert!(path.starts_with("/run/ursine"), "{}", path.display());
    }

    #[test]
    fn absent_crontab_wordings_map_to_none() {
        use ursine_api::CommandOutcome;
        // vixie/cronie wording.
        let vixie: Result<CommandOutcome, ApiError> =
            Err(ApiError::internal("command failed: no crontab for gab"));
        assert!(view_from_outcome(vixie).unwrap().crontab.is_none());
        // busybox wording for a user who never had a crontab.
        let busybox: Result<CommandOutcome, ApiError> = Err(ApiError::internal(
            "command failed: crontab: can't open 'gab': No such file or directory",
        ));
        assert!(view_from_outcome(busybox).unwrap().crontab.is_none());
        // A permission failure must NOT be swallowed as "no crontab".
        let denied: Result<CommandOutcome, ApiError> = Err(ApiError::internal(
            "command failed: crontab: can't open 'gab': Permission denied",
        ));
        assert!(view_from_outcome(denied).is_err());
    }

    #[test]
    fn crontab_target_validation_covers_root_unknown_and_system_users() {
        // Root is a valid crontab target (its own account).
        validate_crontab_target("root").unwrap();
        // Unknown users are a typed NotFound naming the user.
        let err = validate_crontab_target("definitely-not-a-user").unwrap_err();
        assert_eq!(err.code, ursine_api::ErrorCode::NotFound);
        assert!(err.message.contains("does not exist"), "{err:?}");
        // System accounts are rejected (uid < 1000 — `daemon` is uid 1
        // on every distro we target).
        let err = validate_crontab_target("daemon").unwrap_err();
        assert!(err.message.contains("system user"), "{err:?}");
    }

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
