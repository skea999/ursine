//! niri — schema-driven editing of managed `config.kdl` sections.
//!
//! All file I/O and validation run as the target user through `su`. The root
//! daemon never opens, chowns, validates or replaces a user-controlled path
//! itself; a compromised home cannot become a root write or read primitive.

mod kdl;

use std::path::{Path, PathBuf};
use std::time::Duration;

use ::kdl::KdlDocument;
use serde_json::json;
use ursine_api::{Action, ApiError, NiriConfigView, NiriValues};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const MAX_CONFIG: usize = 1024 * 1024;
static NIRI_MUTATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const NIRI_CANDIDATES: [&str; 4] = [
    "/usr/bin/niri",
    "/usr/local/bin/niri",
    "/run/current-system/sw/bin/niri",
    "/nix/var/nix/profiles/default/bin/niri",
];

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::GetNiriConfig => Ok(json!(read_view(ctx).await?)),
        Action::ValidateNiriConfig { proposed } => {
            let path = config_path(ctx)?;
            let content = match proposed {
                Some(content) => content.clone(),
                None => read_raw(ctx.as_user, &path).await?.1,
            };
            let (valid, stdout, stderr) = exec::run_as_user_validate(
                ctx.as_user,
                &path.to_string_lossy(),
                niri_path()?,
                &content,
            )
            .await?;
            Ok(json!({"valid": valid, "stdout": stdout, "stderr": stderr}))
        }
        Action::SetNiriSection { section, value } => {
            let _guard = NIRI_MUTATION.lock().await;
            let path = config_path(ctx)?;
            let current = read_raw(ctx.as_user, &path).await?.1;
            let mut doc: KdlDocument = current
                .parse()
                .map_err(|_| ApiError::invalid("niri config could not be parsed"))?;
            kdl::apply_section(&mut doc, section, value)?;
            let updated = doc.to_string();
            if updated.len() > MAX_CONFIG {
                return Err(ApiError::invalid("niri config would exceed the size limit"));
            }
            let write_path = resolve_write_path(&path)?;
            exec::run_as_user_write(
                ctx.as_user,
                &write_path.to_string_lossy(),
                niri_path()?,
                &updated,
            )
            .await?;
            Ok(json!(read_view(ctx).await?))
        }
        _ => Err(ApiError::invalid("action is not valid for module niri")),
    }
}

async fn read_view(ctx: &ModuleCtx<'_>) -> Result<NiriConfigView, ApiError> {
    let path = config_path(ctx)?;
    let (exists, content) = read_raw(ctx.as_user, &path).await?;
    let values = if exists {
        content
            .parse::<KdlDocument>()
            .map_or_else(|_| NiriValues::default(), |doc| kdl::parse_values(&doc))
    } else {
        NiriValues::default()
    };
    let version = match niri_path() {
        Ok(niri) => {
            exec::run_as_user_argv(ctx.as_user, niri, &["--version"], Duration::from_secs(10))
                .await
                .ok()
                .map(|out| out.stdout.trim().to_string())
                .filter(|version| !version.is_empty())
        }
        Err(_) => None,
    };
    Ok(NiriConfigView {
        path: path.display().to_string(),
        exists,
        content,
        version,
        values,
    })
}

/// Returns `(exists, content)`. The first byte is a marker emitted by the
/// user-side reader so callers can distinguish a missing file from an empty one.
async fn read_raw(user: &str, path: &Path) -> Result<(bool, String), ApiError> {
    let outcome = exec::run_as_user_read_file(user, &path.to_string_lossy()).await?;
    if outcome.stdout.len() > MAX_CONFIG + 1 {
        return Err(ApiError::invalid("niri config is too large"));
    }
    let mut chars = outcome.stdout.chars();
    match chars.next() {
        Some('1') => Ok((true, chars.collect())),
        Some('0') if chars.next().is_none() => Ok((false, String::new())),
        _ => Err(ApiError::internal("unexpected target-user read response")),
    }
}

fn resolve_write_path(path: &Path) -> Result<PathBuf, ApiError> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(path.to_path_buf()),
        Err(error) => {
            return Err(ApiError::internal(format!(
                "inspect niri config path: {error}"
            )))
        }
    };
    if !meta.file_type().is_symlink() {
        return Ok(path.to_path_buf());
    }
    let target = std::fs::canonicalize(path)
        .map_err(|_| ApiError::invalid("niri config symlink target is missing"))?;
    let target_meta = std::fs::metadata(&target)
        .map_err(|_| ApiError::invalid("niri config symlink target is unreadable"))?;
    if !target_meta.is_file() {
        return Err(ApiError::invalid(
            "niri config symlink target is not a file",
        ));
    }
    Ok(target)
}

fn config_path(ctx: &ModuleCtx<'_>) -> Result<PathBuf, ApiError> {
    if let Some(path) = std::env::var_os("URSINE_NIRI_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    Ok(home_for_user(ctx.as_user)?.join(".config/niri/config.kdl"))
}

fn home_for_user(user: &str) -> Result<PathBuf, ApiError> {
    let raw = std::fs::read_to_string("/etc/passwd")
        .map_err(|e| ApiError::internal(format!("read /etc/passwd: {e}")))?;
    let home = raw.lines().find_map(|line| {
        let mut fields = line.split(':');
        if fields.next()? != user {
            return None;
        }
        let _password = fields.next()?;
        let _uid = fields.next()?;
        let _gid = fields.next()?;
        let _gecos = fields.next()?;
        fields.next().map(PathBuf::from)
    });
    let home = home.ok_or_else(|| {
        ApiError::new(
            ursine_api::ErrorCode::NotFound,
            format!("no password entry for {user}"),
        )
    })?;
    if !home.is_absolute() {
        return Err(ApiError::invalid("user home is not absolute"));
    }
    Ok(home)
}

fn niri_path() -> Result<&'static str, ApiError> {
    NIRI_CANDIDATES
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| ApiError::unavailable("niri not installed", "apk add niri"))
}
